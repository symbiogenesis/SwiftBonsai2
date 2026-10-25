#requires -Version 7.6
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1')
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Inference.psm1')
$script:InferenceSources = @{}

function Resolve-BonsaiInferenceSource {
    <# .SYNOPSIS
    Materializes the pinned external runtime source for explicit conformance builds.
    .DESCRIPTION
    This resolver is used by test infrastructure only. Ordinary binary installation
    never fetches runtime source or invokes Cargo. One verified source extraction
    is retained in a unique system-temp lane for this module session.
    #>
    [CmdletBinding()]
    [OutputType([string])]
    param([string]$LockPath = (Join-Path $PSScriptRoot '../config/prism-inference.lock.json'))
    $lock = Read-BonsaiInferenceLock -Path $LockPath
    if (-not $lock.Contains('Source') -or $lock.Source -isnot [Collections.IDictionary]) {
        throw 'Explicit runtime source checks require the inference Source lock.'
    }
    $source = $lock.Source
    if ($source.Repository -cne $lock.repository -or $source.Commit -cne $lock.commit -or
        $source.Commit -cnotmatch '^[a-f0-9]{40}$' -or
        $source.Url -cne "$($source.Repository)/archive/$($source.Commit).zip" -or
        ($source.Bytes -isnot [long] -and $source.Bytes -isnot [int]) -or
        $source.Bytes -le 0 -or $source.Bytes -gt 268435456 -or
        $source.Sha256 -cnotmatch '^[a-f0-9]{64}$') {
        throw 'Invalid immutable Prism Inference source archive identity.'
    }
    $identity = $source.Commit + ':' + $source.Sha256
    if ($script:InferenceSources.ContainsKey($identity)) { return $script:InferenceSources[$identity] }
    $lane = Resolve-ExternalPath -Path (Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-InferenceSource-' + [guid]::NewGuid().ToString('N')))
    [void][IO.Directory]::CreateDirectory($lane)
    $archivePath = Join-Path $lane ($source.Commit + '.zip')
    Receive-LockedArtifact -Uri $source.Url -Path $archivePath -Bytes $source.Bytes -Sha256 $source.Sha256 | Out-Null
    $root = Join-Path $lane ('prism-inference-' + $source.Commit)
    $rootPrefix = $root + [IO.Path]::DirectorySeparatorChar
    $archivePrefix = 'prism-inference-' + $source.Commit + '/'
    $stream = [IO.FileStream]::new($archivePath, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
    try {
        if ($stream.Length -ne $source.Bytes -or
            [Convert]::ToHexStringLower([Security.Cryptography.SHA256]::HashData($stream)) -cne $source.Sha256) {
            throw 'Inference source archive changed before extraction.'
        }
        $stream.Position = 0
        $archive = [IO.Compression.ZipArchive]::new($stream, [IO.Compression.ZipArchiveMode]::Read, $true)
        try {
            if ($archive.Entries.Count -gt 20000) { throw 'Inference source archive exceeds its entry bound.' }
            $names = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
            [long]$totalBytes = 0
            foreach ($entry in $archive.Entries) {
                if (-not $entry.FullName.StartsWith($archivePrefix, [StringComparison]::Ordinal) -or
                    -not $names.Add($entry.FullName) -or
                    (($entry.ExternalAttributes -shr 16) -band 0xF000) -eq 0xA000) {
                    throw 'Inference source archive has an unexpected root, duplicate or linked entry.'
                }
                $relative = $entry.FullName.Substring($archivePrefix.Length)
                if (-not $relative) { continue }
                $isDirectory = $relative.EndsWith('/', [StringComparison]::Ordinal)
                $segments = $relative.TrimEnd('/').Split('/')
                if (@($segments | Where-Object {
                    $_ -in @('', '.', '..', '.git') -or $_ -match '[<>:"\\|?*\x00-\x1f]' -or $_ -match '[. ]$' -or
                    $_ -match '^(?i:CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])(?:\.|$)'
                }).Count) { throw 'Inference source archive has an unsafe path.' }
                $path = [IO.Path]::GetFullPath((Join-Path $root $relative))
                if (-not $path.StartsWith($rootPrefix, [StringComparison]::OrdinalIgnoreCase)) {
                    throw 'Inference source archive path escapes its extraction root.'
                }
                if ($isDirectory) {
                    if ($entry.Length -ne 0) { throw 'Inference source directory contains file bytes.' }
                    [void][IO.Directory]::CreateDirectory($path)
                    continue
                }
                $totalBytes += $entry.Length
                if ($entry.Length -gt 268435456 -or $totalBytes -gt 1073741824) {
                    throw 'Inference source archive exceeds its expansion bound.'
                }
                [void][IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($path))
                $inputStream = $entry.Open()
                try {
                    $output = [IO.FileStream]::new($path, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
                    try {
                        $buffer = [byte[]]::new(65536)
                        while (($count = $inputStream.Read($buffer, 0, $buffer.Length)) -gt 0) {
                            if ($output.Position + $count -gt $entry.Length) { throw 'Inference source entry exceeded its declared size.' }
                            $output.Write($buffer, 0, $count)
                        }
                        if ($output.Position -ne $entry.Length) { throw 'Inference source entry is truncated.' }
                    } finally { $output.Dispose() }
                } finally { $inputStream.Dispose() }
            }
        } finally { $archive.Dispose() }
    } finally { $stream.Dispose() }
    if (-not [IO.File]::Exists((Join-Path $root 'Cargo.toml')) -or
        -not [IO.File]::Exists((Join-Path $root 'tests/upstream_prism/harness.rs'))) {
        throw 'Inference source archive lacks the runtime manifest or conformance harness.'
    }
    $script:InferenceSources[$identity] = $root
    return $root
}

Export-ModuleMember -Function Resolve-BonsaiInferenceSource
