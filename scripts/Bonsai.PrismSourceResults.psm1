#requires -Version 7.6
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1')
Import-Module (Join-Path $PSScriptRoot 'Bonsai.PrismSourcePartition.psm1')

function Read-PrismSourceArtifact {
    <# .SYNOPSIS
    Reads a bounded ordinary source handoff from external artifact storage.
    #>
    [CmdletBinding()]
    [OutputType([hashtable])]
    param([Parameter(Mandatory)][string]$Path, [Parameter(Mandatory)][long]$MaximumBytes)
    $path = Resolve-ExternalPath $Path
    if ((Get-Item -LiteralPath $path).Length -gt $MaximumBytes) { throw 'Ordinary source artifact exceeds its bound.' }
    return Get-Content -LiteralPath $path -Raw | ConvertFrom-Json -AsHashtable -Depth 100 -DateKind String
}

function Merge-BonsaiPrismSourceResult {
    <# .SYNOPSIS
    Requires all four same-revision ordinary partitions exactly once on both targets.
    .DESCRIPTION
    Each partition verifies and builds its own pinned sources. Partition zero
    supplies the backend bundle; other partitions may have different native
    feature selections and binary digests. Missing, duplicate, foreign or moved
    cases cannot be replaced by a passing result from another partition.
    #>
    [CmdletBinding()]
    [OutputType([hashtable])]
    param(
        [Parameter(Mandatory)][string]$InputDirectory,
        [Parameter(Mandatory)][Collections.IDictionary]$Catalog,
        [Parameter(Mandatory)][string]$ProjectRevision,
        [Parameter(Mandatory)][string]$HarnessSha256,
        [Parameter(Mandatory)][string]$CommonBridgeSha256
    )
    $cases = @(for ($ordinal = 0; $ordinal -lt $Catalog.AdapterIds.Count; $ordinal++) {
        if ($Catalog.AdapterIds[$ordinal] -cne 'tests/test-backend-ops.cpp') {
            [pscustomobject]@{ Id=$Catalog.AdapterIds[$ordinal]; Ordinal=$ordinal }
        }
    })
    $results = @{ cpp=[Collections.Generic.Dictionary[string,object]]::new([StringComparer]::Ordinal)
                  rust=[Collections.Generic.Dictionary[string,object]]::new([StringComparer]::Ordinal) }
    $receipts = [Collections.Generic.List[object]]::new()
    $combinedReceipt = $null
    foreach ($index in 0..3) {
        $directory = Join-Path $InputDirectory "prism-ordinary-results-$index"
        $receiptPath = Join-Path $directory 'receipt.json'
        $receipt = Read-PrismSourceArtifact -Path $receiptPath -MaximumBytes 1MB
        $common = @($receipt.NativeBuilds | Where-Object { $_.Family -ceq 'common' })
        if ($receipt.SchemaVersion -ne 1 -or $receipt.ProjectRevision -cne $ProjectRevision -or
            $receipt.SourceCommit -cne $Catalog.UpstreamCommit -or $receipt.SourceArchiveSha256 -cne $Catalog.ArchiveSha256 -or
            $receipt.Status -cne 'awaiting-backend' -or $receipt.Selection -cne 'All' -or
            ($receipt.SourcePartition -isnot [long] -and $receipt.SourcePartition -isnot [int]) -or
            ($receipt.SourcePartitionCount -isnot [long] -and $receipt.SourcePartitionCount -isnot [int]) -or
            $receipt.SourcePartition -ne $index -or $receipt.SourcePartitionCount -ne 4 -or
            $receipt.NativePlatform -cne 'ARM64' -or $receipt.Architecture -cne 'Arm64' -or
            $receipt.DeferredSource -cne 'tests/test-backend-ops.cpp' -or
            $receipt.HarnessSha256 -cnotmatch '^[a-f0-9]{64}$' -or $common.Count -ne 1 -or
            $common[0].ExecutableSha256 -cnotmatch '^[a-f0-9]{64}$') { throw "Ordinary source partition $index provenance differs." }
        if ($index -eq 0) {
            if ($receipt.HarnessSha256 -cne $HarnessSha256 -or $common[0].ExecutableSha256 -cne $CommonBridgeSha256) {
                throw 'Ordinary partition zero differs from the backend tools.'
            }
            $combinedReceipt = $receipt
        }
        $expected = @(Get-BonsaiPrismSourcePartition -Cases $cases -Index $index)
        foreach ($target in @('cpp','rust')) {
            $report = Read-PrismSourceArtifact -Path (Join-Path $directory "$target-results.json") -MaximumBytes 128MB
            if ($report.SchemaVersion -ne 1 -or $report.UpstreamCommit -cne $Catalog.UpstreamCommit -or
                $report.Cases -isnot [array] -or $report.Cases.Count -ne $expected.Count) {
                throw "Ordinary source partition $index $target result identity or count differs."
            }
            $ids = [Collections.Generic.HashSet[string]]::new([string[]]$expected.Id, [StringComparer]::Ordinal)
            foreach ($case in $report.Cases) {
                if (-not $ids.Remove($case.Id) -or -not $results[$target].TryAdd($case.Id, $case)) {
                    throw "Ordinary source partition $index $target contains a duplicate or misplaced source."
                }
            }
        }
        $receipts.Add(@{
            Index=$index; SourceCount=$expected.Count; HarnessSha256=$receipt.HarnessSha256
            CommonBridgeSha256=$common[0].ExecutableSha256
            ReceiptSha256=(Get-FileHash -LiteralPath $receiptPath -Algorithm SHA256).Hash.ToLowerInvariant()
            CaseRuns=$receipt.CaseRuns
        })
    }
    $combinedReceipt.SourcePartition = -1
    $combinedReceipt.SourcePartitions = $receipts.ToArray()
    $combinedReceipt.CaseRuns = @($receipts | ForEach-Object { $_.CaseRuns })
    $merged = @{ Receipt=$combinedReceipt }
    foreach ($target in @('cpp','rust')) {
        $merged[$target] = @{
            SchemaVersion=1; UpstreamCommit=$Catalog.UpstreamCommit
            Cases=@($cases | ForEach-Object { $results[$target][$_.Id] })
        }
    }
    return $merged
}

Export-ModuleMember -Function Merge-BonsaiPrismSourceResult
