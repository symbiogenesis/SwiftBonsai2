#requires -Version 7.6
<#
.SYNOPSIS
Verify a local artifact, inspect GGUF descriptors, and optionally count Prism PQ2 symbols.
.DESCRIPTION
Defaults to the repository's pinned model size and SHA-256. Explicit ExpectedBytes and
ExpectedSha256 permit synthetic fixtures or another independently pinned artifact.
TokenizerMetadataSha256 hashes the concatenation, in file order, of complete serialized
tokenizer.* KV records (key length, key UTF-8, value type, value). Template digests hash
only the embedded string UTF-8 bytes, without normalization or a length prefix.
This is a bounded little-endian v3 inspector, not a model loader or general GGUF validator.
Artifact verification streams the full file. Metadata inspection never reads tensor data;
CensusPq2 additionally streams only PQ2 payloads in a bounded buffer without loading a model.
Prism IDs are accepted only with the explicit pinned Dialect. BITCOS candidate byte counts
exclude format/index/alignment overhead and do not establish RAM or throughput savings.
#>
[CmdletBinding()]
[OutputType([Collections.IDictionary])]
param(
    [Parameter(Mandatory)][string]$ModelPath,
    [string]$OutputPath,
    [ValidateRange(1,[long]::MaxValue)][long]$ExpectedBytes,
    [ValidatePattern('^[a-fA-F0-9]{64}$')][string]$ExpectedSha256,
    [ValidateSet('Stock','Prism-73bfe6cc485554822f80c473a3c0c4f25e80f41b')][string]$Dialect = 'Stock',
    [switch]$CensusPq2,
    [string]$GgufToolPath
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1') -Force
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Gguf.psm1')
$ModelPath = Resolve-ExternalPath $ModelPath
if ($CensusPq2 -and $Dialect -eq 'Stock') { throw 'CensusPq2 requires the explicit pinned Prism Dialect.' }
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
$arguments = @('inspect', '--model', $ModelPath, '--expected-bytes', $ExpectedBytes.ToString([Globalization.CultureInfo]::InvariantCulture), '--expected-sha256', $ExpectedSha256, '--dialect', $Dialect)
if ($CensusPq2) { $arguments += '--census-pq2' }
if ($OutputPath) { $arguments += @('--output', $OutputPath) }
return Invoke-GgufTool -Arguments $arguments -GgufToolPath $GgufToolPath
