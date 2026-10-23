#requires -Version 7.4
<#
.SYNOPSIS
Verify a local artifact and inspect GGUF metadata and tensor descriptors without loading weights.
.DESCRIPTION
Defaults to the repository's pinned model size and SHA-256. Explicit ExpectedBytes and
ExpectedSha256 permit synthetic fixtures or another independently pinned artifact.
TokenizerMetadataSha256 hashes the concatenation, in file order, of complete serialized
tokenizer.* KV records (key length, key UTF-8, value type, value). Template digests hash
only the embedded string UTF-8 bytes, without normalization or a length prefix.
This is a bounded little-endian v3 inspector, not a model loader or general GGUF validator.
The artifact verification streams the full file; metadata inspection never reads tensor data.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$ModelPath,
    [string]$OutputPath,
    [ValidateRange(1,[long]::MaxValue)][long]$ExpectedBytes,
    [ValidatePattern('^[a-fA-F0-9]{64}$')][string]$ExpectedSha256
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1') -Force
$ModelPath = Resolve-ExternalPath $ModelPath
if ($PSBoundParameters.ContainsKey('ExpectedBytes') -ne $PSBoundParameters.ContainsKey('ExpectedSha256')) {
    throw 'Specify both ExpectedBytes and ExpectedSha256, or neither.'
}
if (-not $PSBoundParameters.ContainsKey('ExpectedBytes')) {
    $lock = Read-BonsaiLock
    $ExpectedBytes = $lock.model.bytes
    $ExpectedSha256 = $lock.model.sha256
}
if ($OutputPath) {
    $OutputPath = Resolve-ExternalPath $OutputPath
    if (Test-Path -LiteralPath $OutputPath) { throw 'OutputPath must name a new external JSON file.' }
    if (-not (Test-Path -LiteralPath ([IO.Path]::GetDirectoryName($OutputPath)) -PathType Container)) {
        throw 'The external output directory must already exist.'
    }
}
if (-not ('SwiftBonsai.GgufInspector' -as [type])) { Add-Type -Path (Join-Path $PSScriptRoot 'GgufMetadata.cs') }
# Hold a read-only, write-denying handle across verification and inspection on Windows.
$guard = [IO.File]::Open($ModelPath, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
try {
    Test-LockedArtifact $ModelPath $ExpectedBytes $ExpectedSha256
    $report = [SwiftBonsai.GgufInspector]::Read($ModelPath)
    $report['ArtifactSha256'] = $ExpectedSha256.ToLowerInvariant()
    $report['ArtifactVerification'] = 'verified-size-and-sha256'
    if ($OutputPath) {
        $bytes = [Text.UTF8Encoding]::new($false).GetBytes(($report | ConvertTo-Json -Depth 20) + [Environment]::NewLine)
        $output = [IO.File]::Open($OutputPath, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
        try { $output.Write($bytes); $output.Flush($true) } finally { $output.Dispose() }
    }
    return $report
} finally { $guard.Dispose() }
