#requires -Version 7.6
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true

function Get-BonsaiPrismSourcePartition {
    <#
    .SYNOPSIS
    Selects one deterministic shard of the complete ordinary Prism catalog.
    .DESCRIPTION
    Hosted receipts identify four dominant sources. Isolating tokenizer-random
    and thread-safety, pairing quantize-stats with llguidance, and placing every
    remaining source in the fourth shard bounds the existing catalog's work.
    Unknown future sources stay included. The original objects and catalog order
    survive partitioning; this selector does not execute or alter workloads.
    #>
    [CmdletBinding()]
    [OutputType([object])]
    param(
        [Parameter(Mandatory)][AllowEmptyCollection()][object[]]$Cases,
        [Parameter(Mandatory)][ValidateRange(0, 3)][int]$Index
    )
    $ids = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    $ordinals = [Collections.Generic.HashSet[int]]::new()
    $selected = [Collections.Generic.List[object]]::new()
    foreach ($source in $Cases) {
        if ($null -eq $source -or $source.Id -isnot [string] -or [string]::IsNullOrWhiteSpace($source.Id) -or
            $source.Ordinal -isnot [int] -or $source.Ordinal -lt 0) {
            throw 'Each ordinary source requires an Id and nonnegative integer Ordinal.'
        }
        if (-not $ids.Add($source.Id) -or -not $ordinals.Add($source.Ordinal)) {
            throw 'Ordinary source Ids and Ordinals must be unique before partitioning.'
        }
        $partition = switch -CaseSensitive ($source.Id) {
            'tests/test-tokenizer-random.py' { 0 }
            'tests/test-thread-safety.cpp' { 1 }
            'tests/test-quantize-stats.cpp' { 2 }
            'tests/test-grammar-llguidance.cpp' { 2 }
            default { 3 }
        }
        if ($partition -eq $Index) { $selected.Add($source) }
    }
    return $selected | Sort-Object Ordinal
}

Export-ModuleMember -Function Get-BonsaiPrismSourcePartition
