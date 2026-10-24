#requires -Version 7.6
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1')

function Save-BonsaiPrismLoraAsset {
    <#
    .SYNOPSIS
    Materializes the exact original model, adapter, tokenizer and text inputs.
    #>
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseShouldProcessForStateChangingFunctions', '', Justification = 'Explicit Prism test setup writes only verified external input artifacts.')]
    [CmdletBinding()]
    [OutputType([hashtable])]
    param([Parameter(Mandatory)][string]$Repository,
          [Parameter(Mandatory)][string]$Directory)
    $root = Resolve-ExternalPath $Directory
    $lockPath = Join-Path $Repository 'src/prism-runtime/examples/upstream_prism/lora-assets.json'
    $lock = Get-Content -LiteralPath $lockPath -Raw | ConvertFrom-Json -AsHashtable
    $revision = 'c26d5fb85b4070a9e9c4e65d132c783b98086890'
    if ($lock.Repository -cne 'ggml-org/lora-tests' -or $lock.Revision -cne $revision -or
        $lock.Source -cne 'tests/test-lora-conversion-inference.sh' -or
        $lock.SourceSha256 -cne '69584616380bdaa7f6ee1e53d165408118fb0897e1280f964eb7bc3b932b9c9a' -or
        $lock.AssetPrefix -cne 'lora-tests/' -or $lock.Files.Count -ne 28) {
        throw 'The original LoRA fixture lock differs from its source identity.'
    }
    $paths = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
    $receipts = [Collections.Generic.List[object]]::new()
    [long]$total = 0
    foreach ($entry in $lock.Files) {
        $relative = $entry.Path
        if ($relative -isnot [string] -or [string]::IsNullOrEmpty($relative) -or
            [IO.Path]::IsPathRooted($relative) -or $relative.Contains('\') -or
            @($relative.Split('/') | Where-Object { $_ -in @('', '.', '..') }).Count -or
            -not $paths.Add($relative) -or $entry.Sha256 -cnotmatch '^[a-f0-9]{64}$' -or
            $entry.Bytes -le 0 -or $entry.Bytes -gt 100000000 -or
            $entry.Url -cne "https://huggingface.co/ggml-org/lora-tests/resolve/$revision/$relative") {
            throw 'An original LoRA input has an invalid or unpinned identity.'
        }
        $total += $entry.Bytes
        if ($total -gt 269404497) { throw 'Original LoRA input extent exceeds its lock.' }
        $path = Resolve-ExternalPath (Join-Path $root ($lock.AssetPrefix + $relative))
        $null = Receive-LockedArtifact -Uri $entry.Url -Path $path -Bytes $entry.Bytes -Sha256 $entry.Sha256 -TimeoutSeconds 600
        $receipts.Add(@{ Path=$path; Bytes=$entry.Bytes; Sha256=$entry.Sha256; Url=$entry.Url })
    }
    if ($total -ne 269404497) { throw 'Original LoRA input extent is incomplete.' }
    return @{ Revision=$revision; TotalBytes=$total; Files=$receipts.ToArray(); ConvertedInputs=$false }
}

Export-ModuleMember -Function Save-BonsaiPrismLoraAsset
