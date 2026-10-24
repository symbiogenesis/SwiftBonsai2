#requires -Version 7.6
<#
.SYNOPSIS
Runs one focused regression group for the imported Prism test baseline gate.
#>
[CmdletBinding()]
[OutputType([string])]
param([Parameter(Mandatory)][ValidateSet('Catalog', 'Ratchet', 'Evidence')][string]$Case)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.TestSupport.psm1')
Import-Module (Join-Path $PSScriptRoot '../scripts/Bonsai.PrismBaseline.psm1') -Force
$scratch = [IO.Path]::GetFullPath((Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-PrismBaselineTest-' + [guid]::NewGuid().ToString('N'))))
[void][IO.Directory]::CreateDirectory($scratch)
$catalogPath = Join-Path $scratch 'catalog.json'
$baselinePath = Join-Path $scratch 'baseline.json'
$commit = 'a' * 40
$ids = @('tests/test-one.cpp', 'tests/test-two.cpp', 'tests/nested/test-one.cpp')
$catalog = [ordered]@{
    SchemaVersion = 1; UpstreamCommit = $commit; ArchiveSha256 = 'b' * 64; TestCMakeSourceSha256 = 'd' * 64
    Measure = 'upstream-source-files'; AdapterIds = @($ids[0])
    Cases = @($ids | ForEach-Object { [ordered]@{ Id = $_; SourceSha256 = 'c' * 64 } })
}
$baseline = [ordered]@{
    SchemaVersion = 1; UpstreamCommit = $commit; Measure = 'upstream-source-files'; Total = 3
    MinimumPassed = 1; RequiredPassedIds = @($ids[0])
}
$catalog | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $catalogPath -Encoding utf8
$catalog = Import-BonsaiPrismCatalog -Path $catalogPath
$baseline.CatalogSha256 = $catalog.CatalogSha256
$baseline | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $baselinePath -Encoding utf8

function New-TestResults {
    <# .SYNOPSIS
    Creates a sparse runner report for the selected synthetic source IDs.
    #>
    [CmdletBinding()]
    [OutputType([Collections.IDictionary])]
    param([string[]]$Ids = @(), [string[]]$Statuses = @())
    return [ordered]@{
        SchemaVersion = 1; UpstreamCommit = $commit
        Cases = @(for ($i = 0; $i -lt $Ids.Count; $i++) { [ordered]@{ Id = $Ids[$i]; Status = $Statuses[$i]; Detail = 'Synthetic gate regression.' } })
    }
}

switch ($Case) {
    'Catalog' {
        $imported = Import-BonsaiPrismCatalog -Path $catalogPath
        Assert ($imported.Cases.Count -eq 3) 'Catalog IDs must preserve nested paths.'
        $real = Import-BonsaiPrismCatalog -Path (Join-Path $PSScriptRoot '../tools/prism/upstream/catalog.json')
        Assert ($real.Cases.Count -eq 71) 'Pinned upstream inventory must include all 71 test source files.'
        Assert (@($real.Cases | Where-Object { $_.Id -ceq 'tests/test-tokenizer-0.cpp' }).Count -eq 1) 'C++ tokenizer ID is absent.'
        Assert (@($real.Cases | Where-Object { $_.Id -ceq 'tests/test-tokenizer-0.py' }).Count -eq 1) 'Python tokenizer ID must remain separate.'
        Assert (@($real.Cases | Where-Object { $_.Id -ceq 'tests/peg-parser/test-unicode.cpp' }).Count -eq 1) 'Nested parser source was omitted.'
        $sourceRoot = Join-Path $scratch 'source'
        [void][IO.Directory]::CreateDirectory((Join-Path $sourceRoot 'tests/nested'))
        foreach ($id in $ids) {
            [IO.File]::WriteAllText((Join-Path $sourceRoot $id), 'synthetic upstream source')
        }
        $cmakePath = Join-Path $sourceRoot 'tests/CMakeLists.txt'
        [IO.File]::WriteAllText($cmakePath, 'synthetic upstream targets')
        $catalog.TestCMakeSourceSha256 = (Get-FileHash -LiteralPath $cmakePath -Algorithm SHA256).Hash.ToLowerInvariant()
        foreach ($source in $catalog.Cases) { $source.SourceSha256 = (Get-FileHash -LiteralPath (Join-Path $sourceRoot $source.Id) -Algorithm SHA256).Hash.ToLowerInvariant() }
        $catalog | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $catalogPath -Encoding utf8
        [void](Import-BonsaiPrismCatalog -Path $catalogPath -SourceDirectory $sourceRoot)
        $extraPath = Join-Path $sourceRoot 'tests/test-unlisted.cpp'
        [IO.File]::WriteAllText($extraPath, 'unlisted')
        Assert-Failure { Import-BonsaiPrismCatalog -Path $catalogPath -SourceDirectory $sourceRoot } 'A source omitted from the inventory was accepted.'
        [IO.File]::Delete($extraPath)
        [IO.File]::Delete((Join-Path $sourceRoot $ids[2]))
        Assert-Failure { Import-BonsaiPrismCatalog -Path $catalogPath -SourceDirectory $sourceRoot } 'A missing upstream source was accepted.'
        [IO.File]::WriteAllText((Join-Path $sourceRoot $ids[2]), 'changed upstream source')
        Assert-Failure { Import-BonsaiPrismCatalog -Path $catalogPath -SourceDirectory $sourceRoot } 'A modified upstream source was accepted.'
        [IO.File]::WriteAllText((Join-Path $sourceRoot $ids[2]), 'synthetic upstream source')
        [IO.File]::WriteAllText($cmakePath, 'changed upstream targets')
        Assert-Failure { Import-BonsaiPrismCatalog -Path $catalogPath -SourceDirectory $sourceRoot } 'Changed upstream test registrations were accepted.'
        $catalog.Cases += $catalog.Cases[0]
        $catalog | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $catalogPath -Encoding utf8
        Assert-Failure { Import-BonsaiPrismCatalog -Path $catalogPath } 'Duplicate catalog ID was accepted.'
        $catalog.Cases = $catalog.Cases[0..2]
        $catalog.Cases[0].Id = '../tests/test-escape.cpp'
        $catalog | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $catalogPath -Encoding utf8
        Assert-Failure { Import-BonsaiPrismCatalog -Path $catalogPath } 'Escaping source path was accepted.'
    }
    'Ratchet' {
        $target = New-TestResults -Ids @($ids[0]) -Statuses @('pass')
        $reference = New-TestResults -Ids $ids -Statuses @('pass', 'pass', 'pass')
        $report = New-BonsaiPrismReport -Catalog $catalog -TargetResults $target -ReferenceResults $reference
        $gate = Test-BonsaiPrismBaseline -Catalog $catalog -Report $report -BaselinePath $baselinePath
        Assert ($gate.Passed -and $gate.Summary.Total -eq 3 -and $gate.Summary.Passed -eq 1 -and $gate.Summary.Unsupported -eq 2) 'Sparse adapters must retain unported entries as red.'
        Assert ($gate.Summary.GreenRatio -eq (1 / 3)) 'The denominator must include all upstream files.'
        $target = New-TestResults -Ids @($ids[0], $ids[1]) -Statuses @('fail', 'pass')
        $report = New-BonsaiPrismReport -Catalog $catalog -TargetResults $target -ReferenceResults $reference
        $gate = Test-BonsaiPrismBaseline -Catalog $catalog -Report $report -BaselinePath $baselinePath
        Assert (-not $gate.Passed) 'A different new green case must not replace a previously green case.'
        foreach ($status in @('fail', 'skip')) {
            $target = New-TestResults -Ids @($ids[0]) -Statuses @($status)
            $report = New-BonsaiPrismReport -Catalog $catalog -TargetResults $target -ReferenceResults $reference
            $report.Summary.Passed = 3
            $gate = Test-BonsaiPrismBaseline -Catalog $catalog -Report $report -BaselinePath $baselinePath
            Assert (-not $gate.Passed -and $gate.Summary.Passed -eq 0) "Red status $status or a forged presentation summary bypassed the minimum."
            Assert (@($gate.Issues | Where-Object { $_ -like 'Green ratio fell below*' }).Count -eq 1) 'The ratio floor must reject a regression independently.'
        }
        $target = New-TestResults -Ids @($ids[0], $ids[1]) -Statuses @('pass', 'pass')
        $report = New-BonsaiPrismReport -Catalog $catalog -TargetResults $target -ReferenceResults $reference
        Assert ((Test-BonsaiPrismBaseline -Catalog $catalog -Report $report -BaselinePath $baselinePath).Passed) 'New green cases must be accepted without rewriting the policy.'
        $reference = New-TestResults -Ids @($ids[0]) -Statuses @('fail')
        $report = New-BonsaiPrismReport -Catalog $catalog -TargetResults $target -ReferenceResults $reference
        $gate = Test-BonsaiPrismBaseline -Catalog $catalog -Report $report -BaselinePath $baselinePath
        Assert (-not $gate.Passed -and $gate.Summary.ReferenceFailureIds.Count -eq 2) 'Failed and missing C++ reference results must both reject a green target.'
    }
    'Evidence' {
        $valid = New-TestResults -Ids @($ids[0]) -Statuses @('pass')
        $empty = New-TestResults
        Assert-Failure { New-BonsaiPrismReport -Catalog $catalog -TargetResults $empty -ReferenceResults $valid } 'A missing implemented target adapter was accepted.'
        Assert-Failure { New-BonsaiPrismReport -Catalog $catalog -TargetResults $valid -ReferenceResults $empty } 'A missing implemented C++ adapter was accepted.'
        $invalid = New-TestResults -Ids @($ids[0]) -Statuses @('unsupported')
        Assert-Failure { New-BonsaiPrismReport -Catalog $catalog -TargetResults $invalid -ReferenceResults $valid } 'An implemented adapter was allowed to disappear as unsupported.'
        $invalid = New-TestResults -Ids @($ids[0], $ids[0]) -Statuses @('pass', 'fail')
        Assert-Failure { New-BonsaiPrismReport -Catalog $catalog -TargetResults $invalid -ReferenceResults $valid } 'Duplicate target IDs were accepted.'
        Assert-Failure { New-BonsaiPrismReport -Catalog $catalog -TargetResults $valid -ReferenceResults $invalid } 'Duplicate reference IDs were accepted.'
        $invalid = New-TestResults -Ids @('tests/test-unknown.cpp') -Statuses @('pass')
        Assert-Failure { New-BonsaiPrismReport -Catalog $catalog -TargetResults $invalid -ReferenceResults $valid } 'Unknown target ID was accepted.'
        Assert-Failure { New-BonsaiPrismReport -Catalog $catalog -TargetResults $valid -ReferenceResults $invalid } 'Unknown reference ID was accepted.'
        $invalid = New-TestResults -Ids @($ids[0]) -Statuses @('PASS')
        Assert-Failure { New-BonsaiPrismReport -Catalog $catalog -TargetResults $invalid -ReferenceResults $valid } 'Wrong-case status was accepted.'
        $invalid = New-TestResults -Ids @($ids[0]) -Statuses @('pass')
        $invalid.UpstreamCommit = 'd' * 40
        Assert-Failure { New-BonsaiPrismReport -Catalog $catalog -TargetResults $invalid -ReferenceResults $valid } 'Mismatched upstream pin was accepted.'
        $invalid = New-TestResults -Ids @($ids[0]) -Statuses @('pass')
        $invalid.SchemaVersion = '1'
        Assert-Failure { New-BonsaiPrismReport -Catalog $catalog -TargetResults $invalid -ReferenceResults $valid } 'A string schema version was accepted.'
        $invalid = New-TestResults -Ids @($ids[0]) -Statuses @('pass')
        $invalid.Remove('Cases')
        Assert-Failure { New-BonsaiPrismReport -Catalog $catalog -TargetResults $invalid -ReferenceResults $valid } 'A missing Cases array was accepted.'
        $report = New-BonsaiPrismReport -Catalog $catalog -TargetResults $valid -ReferenceResults $valid
        $report.Cases = @($report.Cases[0], $report.Cases[0], $report.Cases[2])
        Assert-Failure { Test-BonsaiPrismBaseline -Catalog $catalog -Report $report -BaselinePath $baselinePath } 'Duplicate full-report records were accepted.'
        $report = New-BonsaiPrismReport -Catalog $catalog -TargetResults $valid -ReferenceResults $valid
        $report.Cases = @($report.Cases[0])
        Assert-Failure { Test-BonsaiPrismBaseline -Catalog $catalog -Report $report -BaselinePath $baselinePath } 'Missing full-report records were accepted.'
        $report = New-BonsaiPrismReport -Catalog $catalog -TargetResults $valid -ReferenceResults $valid
        $baseline.RequiredPassedIds = @($ids[0], $ids[0])
        $baseline | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $baselinePath -Encoding utf8
        Assert-Failure { Test-BonsaiPrismBaseline -Catalog $catalog -Report $report -BaselinePath $baselinePath } 'Duplicate baseline green IDs were accepted.'
        $baseline.RequiredPassedIds = @($ids[0])
        $baseline.Total = 2
        $baseline | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $baselinePath -Encoding utf8
        Assert-Failure { Test-BonsaiPrismBaseline -Catalog $catalog -Report $report -BaselinePath $baselinePath } 'An altered denominator was accepted.'
        $baseline.Total = 3
        $baseline.CatalogSha256 = '0' * 64
        $baseline | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $baselinePath -Encoding utf8
        Assert-Failure { Test-BonsaiPrismBaseline -Catalog $catalog -Report $report -BaselinePath $baselinePath } 'Baseline policy for a different catalog was accepted.'
    }
}
Write-Output "Focused Prism baseline $Case checks passed. Evidence: $scratch"
