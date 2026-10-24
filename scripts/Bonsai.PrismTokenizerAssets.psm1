#requires -Version 7.6
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1')

function Save-BonsaiPrismTokenizerRepository {
    <#
    .SYNOPSIS
    Materializes all eleven original external vocabulary/corpus triples.
    #>
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseShouldProcessForStateChangingFunctions', '', Justification = 'Explicit Prism setup writes only hash-verified external input artifacts.')]
    [CmdletBinding()]
    [OutputType([hashtable])]
    param([Parameter(Mandatory)][string]$Repository,
          [Parameter(Mandatory)][string]$Directory)
    $root = Resolve-ExternalPath $Directory
    $lockPath = Join-Path $Repository 'src/prism-runtime/examples/upstream_prism/tokenizer-repository-assets.json'
    $lock = Get-Content -LiteralPath $lockPath -Raw | ConvertFrom-Json -AsHashtable
    $revision = '5160d4e134084aea7d5cf615b95bb84e1739285a'
    if ($lock.Repository -cne 'ggml-org/vocabs' -or $lock.Revision -cne $revision -or
        $lock.AssetPrefix -cne 'tokenizer-repository/' -or $lock.Files.Count -ne 33) {
        throw 'The tokenizer corpus repository lock differs from its source identity.'
    }
    $paths = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
    $receipts = [Collections.Generic.List[object]]::new()
    [long]$total = 0
    foreach ($entry in $lock.Files) {
        $relative = $entry.Path
        if ($relative -isnot [string] -or $relative -cnotmatch '^(BPE|SPM|HybridDNA|PLaMo2|PLaMo3|RWKV|UGM|WPM)/ggml-vocab-[a-z0-9-]+\.gguf(?:\.(?:inp|out))?$' -or
            -not $paths.Add($relative) -or $entry.Sha256 -cnotmatch '^[a-f0-9]{64}$' -or
            $entry.GitBlobSha1 -cnotmatch '^[a-f0-9]{40}$' -or
            $entry.Bytes -le 0 -or $entry.Bytes -gt 20000000 -or
            $entry.Url -cne "https://huggingface.co/ggml-org/vocabs/resolve/$revision/$relative") {
            throw 'An external tokenizer corpus input has an invalid or unpinned identity.'
        }
        $total += $entry.Bytes
        if ($total -gt 61141284) { throw 'External tokenizer corpus extent exceeds its lock.' }
        $path = Resolve-ExternalPath (Join-Path $root ($lock.AssetPrefix + $relative))
        $null = Receive-LockedArtifact -Uri $entry.Url -Path $path -Bytes $entry.Bytes -Sha256 $entry.Sha256 -TimeoutSeconds 600
        $receipts.Add(@{ Path=$path; Bytes=$entry.Bytes; Sha256=$entry.Sha256; Url=$entry.Url })
    }
    if ($total -ne 61141284) { throw 'External tokenizer corpus extent is incomplete.' }
    foreach ($relative in $paths) {
        if ($relative.EndsWith('.gguf', [StringComparison]::Ordinal) -and
            (-not $paths.Contains($relative + '.inp') -or -not $paths.Contains($relative + '.out'))) {
            throw 'An external vocabulary has no complete input/output corpus.'
        }
    }
    return @{ Revision=$revision; TotalBytes=$total; Files=$receipts.ToArray(); CorpusCount=11 }
}

Export-ModuleMember -Function Save-BonsaiPrismTokenizerRepository
