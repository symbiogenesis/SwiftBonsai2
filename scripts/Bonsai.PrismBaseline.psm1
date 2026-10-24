#requires -Version 7.6
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true

function Assert-PrismObject {
    <# .SYNOPSIS
    Checks required keys before accessing untrusted JSON properties.
    #>
    [CmdletBinding()]
    [OutputType([void])]
    param([AllowNull()][object]$Value, [string[]]$Required, [string]$Context)
    if ($Value -isnot [Collections.IDictionary]) { throw "$Context must be a JSON object." }
    foreach ($key in $Required) {
        if (-not $Value.Contains($key)) { throw "$Context is missing $key." }
    }
}

function Assert-PrismInteger {
    <# .SYNOPSIS
    Rejects coerced strings, fractions and negative counts.
    #>
    [CmdletBinding()]
    [OutputType([void])]
    param([AllowNull()][object]$Value, [string]$Context, [int]$Minimum = 0)
    if (($Value -isnot [int] -and $Value -isnot [long]) -or $Value -lt $Minimum -or $Value -gt 1000000) {
        throw "$Context must be an integer from $Minimum through 1000000."
    }
}

function Get-PrismCatalogIndex {
    <# .SYNOPSIS
    Validates the complete source-file catalog and returns an ordinal ID index.
    #>
    [CmdletBinding()]
    [OutputType([Collections.Generic.Dictionary[string, object]])]
    param([Parameter(Mandatory)][Collections.IDictionary]$Catalog)
    Assert-PrismObject -Value $Catalog -Required @('SchemaVersion', 'UpstreamCommit', 'ArchiveSha256', 'TestCMakeSourceSha256', 'Measure', 'AdapterIds', 'Cases') -Context 'Catalog'
    Assert-PrismInteger -Value $Catalog.SchemaVersion -Context 'Catalog.SchemaVersion' -Minimum 1
    if ($Catalog.SchemaVersion -ne 1 -or $Catalog.Measure -isnot [string] -or $Catalog.Measure -cne 'upstream-source-files' -or
        $Catalog.UpstreamCommit -isnot [string] -or $Catalog.UpstreamCommit -cnotmatch '^[a-f0-9]{40}$' -or
        $Catalog.ArchiveSha256 -isnot [string] -or $Catalog.ArchiveSha256 -cnotmatch '^[a-f0-9]{64}$' -or
        $Catalog.TestCMakeSourceSha256 -isnot [string] -or $Catalog.TestCMakeSourceSha256 -cnotmatch '^[a-f0-9]{64}$' -or
        $Catalog.AdapterIds -isnot [array] -or
        $Catalog.Cases -isnot [array] -or $Catalog.Cases.Count -eq 0) { throw 'Invalid Prism catalog contract.' }
    $index = [Collections.Generic.Dictionary[string, object]]::new([StringComparer]::Ordinal)
    foreach ($case in $Catalog.Cases) {
        Assert-PrismObject -Value $case -Required @('Id', 'SourceSha256') -Context 'Catalog case'
        if ($case.Id -isnot [string] -or $case.Id -cnotmatch '^tests/(?:[A-Za-z0-9_-]+/)*test-[A-Za-z0-9_.-]+\.(?:cpp|c|py|sh)$' -or
            $case.SourceSha256 -isnot [string] -or $case.SourceSha256 -cnotmatch '^[a-f0-9]{64}$') { throw 'Invalid Prism catalog case identity or source digest.' }
        if (-not $index.TryAdd($case.Id, $case)) { throw "Duplicate catalog ID: $($case.Id)" }
    }
    $adapters = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    foreach ($id in $Catalog.AdapterIds) {
        if ($id -isnot [string] -or -not $index.ContainsKey($id) -or -not $adapters.Add($id)) {
            throw 'Catalog has an unknown or duplicate adapter ID.'
        }
    }
    return $index
}

function Get-PrismCatalogDigest {
    <# .SYNOPSIS
    Binds baseline policy to all catalog content independently of JSON whitespace.
    #>
    [CmdletBinding()]
    [OutputType([string])]
    param([Parameter(Mandatory)][Collections.IDictionary]$Catalog)
    $canonical = [ordered]@{
        SchemaVersion = $Catalog.SchemaVersion; UpstreamCommit = $Catalog.UpstreamCommit
        ArchiveSha256 = $Catalog.ArchiveSha256; TestCMakeSourceSha256 = $Catalog.TestCMakeSourceSha256
        Measure = $Catalog.Measure; AdapterIds = @($Catalog.AdapterIds)
        Cases = @($Catalog.Cases | ForEach-Object { [ordered]@{ Id = $_.Id; SourceSha256 = $_.SourceSha256 } })
    } | ConvertTo-Json -Compress -Depth 10
    return [Convert]::ToHexStringLower([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($canonical)))
}

function Import-BonsaiPrismCatalog {
    <# .SYNOPSIS
    Reads the fixed inventory, optionally verifying every pinned upstream source.
    #>
    [CmdletBinding()]
    [OutputType([Collections.IDictionary])]
    param([Parameter(Mandatory)][string]$Path, [string]$SourceDirectory)
    $catalog = Get-Content -LiteralPath $Path -Raw | ConvertFrom-Json -AsHashtable -DateKind String
    $index = Get-PrismCatalogIndex -Catalog $catalog
    if ($SourceDirectory) {
        if (-not [IO.Path]::IsPathFullyQualified($SourceDirectory) -or $SourceDirectory -match '%[^%]+%') {
            throw 'SourceDirectory must be an expanded absolute path.'
        }
        $sourceRoot = [IO.Path]::GetFullPath($SourceDirectory)
        $discovered = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
        foreach ($file in (Get-ChildItem -LiteralPath (Join-Path $sourceRoot 'tests') -Recurse -File)) {
            if ($file.Name -cnotmatch '^test-.*\.(?:cpp|c|py|sh)$') { continue }
            $id = [IO.Path]::GetRelativePath($sourceRoot, $file.FullName).Replace('\', '/')
            if (-not $index.ContainsKey($id)) { throw "Upstream test is absent from the catalog: $id" }
            if ((Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash.ToLowerInvariant() -cne $index[$id].SourceSha256) {
                throw "Upstream source digest differs from the catalog: $id"
            }
            [void]$discovered.Add($id)
        }
        if ($discovered.Count -ne $index.Count) { throw 'The upstream source inventory is incomplete.' }
        if ((Get-FileHash -LiteralPath (Join-Path $sourceRoot 'tests/CMakeLists.txt') -Algorithm SHA256).Hash.ToLowerInvariant() -cne $catalog.TestCMakeSourceSha256) {
            throw 'Upstream tests/CMakeLists.txt differs from its catalog provenance.'
        }
    }
    $catalog.CatalogSha256 = Get-PrismCatalogDigest -Catalog $catalog
    return $catalog
}

function Get-PrismResultIndex {
    <# .SYNOPSIS
    Validates sparse runner results without silently merging duplicate identities.
    #>
    [CmdletBinding()]
    [OutputType([Collections.Generic.Dictionary[string, object]])]
    param([Parameter(Mandatory)][Collections.IDictionary]$Results,
          [Parameter(Mandatory)][Collections.IDictionary]$Catalog,
          [Parameter(Mandatory)][Collections.Generic.Dictionary[string, object]]$CatalogIndex,
          [string]$Context)
    Assert-PrismObject -Value $Results -Required @('SchemaVersion', 'UpstreamCommit', 'Cases') -Context $Context
    Assert-PrismInteger -Value $Results.SchemaVersion -Context "$Context.SchemaVersion" -Minimum 1
    if ($Results.SchemaVersion -ne 1 -or $Results.UpstreamCommit -isnot [string] -or $Results.UpstreamCommit -cne $Catalog.UpstreamCommit -or $Results.Cases -isnot [array]) {
        throw "$Context schema or upstream revision does not match the catalog."
    }
    $index = [Collections.Generic.Dictionary[string, object]]::new([StringComparer]::Ordinal)
    foreach ($case in $Results.Cases) {
        Assert-PrismObject -Value $case -Required @('Id', 'Status', 'Detail') -Context "$Context case"
        if ($case.Id -isnot [string] -or -not $CatalogIndex.ContainsKey($case.Id)) { throw "Unknown $Context case ID: $($case.Id)" }
        if ($case.Status -isnot [string] -or $case.Status -cnotin @('pass', 'fail', 'skip', 'unsupported') -or $case.Detail -isnot [string]) { throw "Invalid $Context result for $($case.Id)." }
        if (-not $index.TryAdd($case.Id, $case)) { throw "Duplicate $Context case ID: $($case.Id)" }
    }
    return $index
}

function Get-PrismReportSummary {
    <# .SYNOPSIS
    Counts every catalog entry, retaining red statuses and reference failures.
    #>
    [CmdletBinding()]
    [OutputType([Collections.Specialized.OrderedDictionary])]
    param([Parameter(Mandatory)][array]$Cases)
    $passed = @($Cases | Where-Object { $_.Status -ceq 'pass' })
    $failed = @($Cases | Where-Object { $_.Status -ceq 'fail' })
    $skipped = @($Cases | Where-Object { $_.Status -ceq 'skip' })
    $unsupported = @($Cases | Where-Object { $_.Status -ceq 'unsupported' })
    $referenceFailures = @($Cases | Where-Object { $_.ReferenceStatus -cin @('fail', 'skip') -or ($_.Status -cne 'unsupported' -and $_.ReferenceStatus -cne 'pass') })
    return [ordered]@{
        Total = $Cases.Count; Passed = $passed.Count; Red = $Cases.Count - $passed.Count
        Failed = $failed.Count; Skipped = $skipped.Count; Unsupported = $unsupported.Count
        GreenRatio = $passed.Count / $Cases.Count; ReferencePassed = ($referenceFailures.Count -eq 0)
        PassedIds = @($passed | ForEach-Object { $_.Id })
        RedIds = @($Cases | Where-Object { $_.Status -cne 'pass' } | ForEach-Object { $_.Id })
        ReferenceFailureIds = @($referenceFailures | ForEach-Object { $_.Id })
    }
}

function New-BonsaiPrismReport {
    <# .SYNOPSIS
    Expands sparse target/reference results into the complete red/green record.
    .DESCRIPTION
    Missing adapters remain unsupported and red. The original C++ reference must
    independently pass each adapted source. A reference failure is retained in
    the report and always rejects the gate, regardless of the target green ratio.
    #>
    [CmdletBinding()]
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseShouldProcessForStateChangingFunctions', '', Justification = 'Constructs only an in-memory evidence report; no external state changes.')]
    [OutputType([Collections.Specialized.OrderedDictionary])]
    param([Parameter(Mandatory)][Collections.IDictionary]$Catalog,
          [Parameter(Mandatory)][Collections.IDictionary]$TargetResults,
          [Parameter(Mandatory)][Collections.IDictionary]$ReferenceResults)
    $catalogIndex = Get-PrismCatalogIndex -Catalog $Catalog
    $targetIndex = Get-PrismResultIndex -Results $TargetResults -Catalog $Catalog -CatalogIndex $catalogIndex -Context 'Target'
    $referenceIndex = Get-PrismResultIndex -Results $ReferenceResults -Catalog $Catalog -CatalogIndex $catalogIndex -Context 'Reference'
    foreach ($id in $Catalog.AdapterIds) {
        if (-not $targetIndex.ContainsKey($id) -or -not $referenceIndex.ContainsKey($id)) {
            throw "An implemented adapter is missing its target or C++ reference result: $id"
        }
        if ($targetIndex[$id].Status -ceq 'unsupported' -or $referenceIndex[$id].Status -ceq 'unsupported') {
            throw "An implemented adapter cannot be reported as unsupported: $id"
        }
    }
    $cases = @(
        foreach ($source in $Catalog.Cases) {
            $target = if ($targetIndex.ContainsKey($source.Id)) { $targetIndex[$source.Id] } else { @{ Status = 'unsupported'; Detail = 'No Rust adapter is implemented for this upstream source.' } }
            $reference = if ($referenceIndex.ContainsKey($source.Id)) { $referenceIndex[$source.Id] } else { @{ Status = 'unsupported'; Detail = 'No C++ reference adapter result was supplied.' } }
            [ordered]@{
                Id = $source.Id; SourceSha256 = $source.SourceSha256
                Status = $target.Status; Detail = $target.Detail
                ReferenceStatus = $reference.Status; ReferenceDetail = $reference.Detail
            }
        }
    )
    return [ordered]@{
        SchemaVersion = 1; UpstreamCommit = $Catalog.UpstreamCommit; CatalogSha256 = Get-PrismCatalogDigest -Catalog $Catalog
        Measure = $Catalog.Measure; Cases = $cases; Summary = Get-PrismReportSummary -Cases $cases
    }
}

function Test-BonsaiPrismBaseline {
    <# .SYNOPSIS
    Requires the recorded target/reference green IDs and minimum target ratio.
    .DESCRIPTION
    Baselines are checked-in policy and are never updated by this function.
    Malformed evidence throws; measured regressions return Passed=false and
    diagnostics. Counts are recomputed from all case records, never trusted from
    the report's presentation summary.
    #>
    [CmdletBinding()]
    [OutputType([Collections.Specialized.OrderedDictionary])]
    param([Parameter(Mandatory)][Collections.IDictionary]$Catalog,
          [Parameter(Mandatory)][Collections.IDictionary]$Report,
          [Parameter(Mandatory)][string]$BaselinePath)
    $catalogIndex = Get-PrismCatalogIndex -Catalog $Catalog
    $catalogDigest = Get-PrismCatalogDigest -Catalog $Catalog
    Assert-PrismObject -Value $Report -Required @('SchemaVersion', 'UpstreamCommit', 'CatalogSha256', 'Measure', 'Cases') -Context 'Report'
    Assert-PrismInteger -Value $Report.SchemaVersion -Context 'Report.SchemaVersion' -Minimum 1
    if ($Report.SchemaVersion -ne 1 -or $Report.UpstreamCommit -isnot [string] -or $Report.UpstreamCommit -cne $Catalog.UpstreamCommit -or
        $Report.CatalogSha256 -isnot [string] -or $Report.CatalogSha256 -cne $catalogDigest -or
        $Report.Measure -isnot [string] -or $Report.Measure -cne $Catalog.Measure -or $Report.Cases -isnot [array]) { throw 'Report does not match the catalog.' }
    $seen = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    foreach ($case in $Report.Cases) {
        Assert-PrismObject -Value $case -Required @('Id', 'SourceSha256', 'Status', 'Detail', 'ReferenceStatus', 'ReferenceDetail') -Context 'Report case'
        if ($case.Id -isnot [string] -or -not $catalogIndex.ContainsKey($case.Id)) { throw 'Report contains an unknown case ID.' }
        if (-not $seen.Add($case.Id)) { throw "Duplicate report case ID: $($case.Id)" }
        if ($case.SourceSha256 -isnot [string] -or $case.SourceSha256 -cne $catalogIndex[$case.Id].SourceSha256 -or
            $case.Status -isnot [string] -or $case.Status -cnotin @('pass', 'fail', 'skip', 'unsupported') -or
            $case.ReferenceStatus -isnot [string] -or $case.ReferenceStatus -cnotin @('pass', 'fail', 'skip', 'unsupported') -or
            $case.Detail -isnot [string] -or $case.ReferenceDetail -isnot [string]) { throw "Invalid full report case: $($case.Id)" }
        if ($case.Id -cin $Catalog.AdapterIds -and ($case.Status -ceq 'unsupported' -or $case.ReferenceStatus -ceq 'unsupported')) {
            throw "Full report lost an implemented adapter: $($case.Id)"
        }
    }
    if ($seen.Count -ne $catalogIndex.Count) { throw 'Full report is missing catalog cases.' }
    $baseline = Get-Content -LiteralPath $BaselinePath -Raw | ConvertFrom-Json -AsHashtable -DateKind String
    Assert-PrismObject -Value $baseline -Required @('SchemaVersion', 'UpstreamCommit', 'CatalogSha256', 'Measure', 'Total', 'MinimumPassed', 'RequiredPassedIds', 'RequiredReferencePassedIds') -Context 'Baseline'
    Assert-PrismInteger -Value $baseline.SchemaVersion -Context 'Baseline.SchemaVersion' -Minimum 1
    Assert-PrismInteger -Value $baseline.Total -Context 'Baseline.Total' -Minimum 1
    Assert-PrismInteger -Value $baseline.MinimumPassed -Context 'Baseline.MinimumPassed'
    if ($baseline.SchemaVersion -ne 1 -or $baseline.UpstreamCommit -isnot [string] -or $baseline.UpstreamCommit -cne $Catalog.UpstreamCommit -or
        $baseline.CatalogSha256 -isnot [string] -or $baseline.CatalogSha256 -cne $catalogDigest -or
        $baseline.Measure -isnot [string] -or $baseline.Measure -cne $Catalog.Measure -or $baseline.Total -ne $catalogIndex.Count -or
        $baseline.MinimumPassed -gt $baseline.Total -or $baseline.RequiredPassedIds -isnot [array] -or
        $baseline.RequiredReferencePassedIds -isnot [array]) {
        throw 'Baseline does not match the catalog or has an invalid minimum.'
    }
    $required = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    foreach ($id in $baseline.RequiredPassedIds) {
        if ($id -isnot [string] -or -not $catalogIndex.ContainsKey($id) -or -not $required.Add($id)) { throw 'Baseline has an unknown or duplicate green ID.' }
    }
    if ($required.Count -ne $baseline.MinimumPassed) { throw 'Baseline minimum must record every measured green ID.' }
    $requiredReferences = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    foreach ($id in $baseline.RequiredReferencePassedIds) {
        if ($id -isnot [string] -or -not $catalogIndex.ContainsKey($id) -or -not $requiredReferences.Add($id)) {
            throw 'Baseline has an unknown or duplicate C++ reference green ID.'
        }
    }
    $summary = Get-PrismReportSummary -Cases $Report.Cases
    $issues = [Collections.Generic.List[string]]::new()
    foreach ($id in $required) {
        $current = @($Report.Cases | Where-Object { $_.Id -ceq $id })[0]
        if ($current.Status -cne 'pass') { $issues.Add("Previously green case regressed: $id ($($current.Status)).") }
    }
    foreach ($id in $requiredReferences) {
        $current = @($Report.Cases | Where-Object { $_.Id -ceq $id })[0]
        if ($current.ReferenceStatus -cne 'pass') {
            $issues.Add("Previously passing C++ reference regressed: $id ($($current.ReferenceStatus)).")
        }
    }
    if ([long]$summary.Passed * $baseline.Total -lt [long]$baseline.MinimumPassed * $summary.Total) {
        $issues.Add("Green ratio fell below $($baseline.MinimumPassed)/$($baseline.Total): $($summary.Passed)/$($summary.Total).")
    }
    foreach ($id in $summary.ReferenceFailureIds) { $issues.Add("C++ reference did not pass for adapted case: $id.") }
    return [ordered]@{
        Passed = ($issues.Count -eq 0); Issues = @($issues.ToArray()); Summary = $summary
        MinimumPassed = $baseline.MinimumPassed; BaselineTotal = $baseline.Total
    }
}

Export-ModuleMember -Function Import-BonsaiPrismCatalog, New-BonsaiPrismReport, Test-BonsaiPrismBaseline
