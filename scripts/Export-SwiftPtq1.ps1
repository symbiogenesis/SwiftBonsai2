#requires -Version 7.6
<#
.SYNOPSIS
Exports the pinned current Swift PQ2 artifact into experimental weight-lossless PTQ1.
.DESCRIPTION
Streams bounded batches, preserving every raw scale and ternary symbol, all other
tensor bytes, and tokenizer/template/transform metadata. Verifies each converted
batch by byte-identical PQ2 reconstruction. PTQ1's native activation quantization
differs; this does not qualify logits, quality, speed or enable an inference profile.
Creates a unique partial file, then publishes the new output name after validation.
Failed partial files are retained for diagnosis. No model process is started.
#>
[CmdletBinding(SupportsShouldProcess)]
[OutputType([Collections.IDictionary])]
param(
    [Parameter(Mandatory)][string]$ModelPath,
    [Parameter(Mandatory)][string]$OutputPath,
    [ValidateRange(1,3600)][int]$TimeoutSeconds = 1800
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1') -Force
$ModelPath = Resolve-ExternalPath $ModelPath
$OutputPath = Resolve-ExternalPath $OutputPath
if (Test-Path -LiteralPath $OutputPath) { throw 'OutputPath must name a new external GGUF file.' }
if (-not (Test-Path -LiteralPath ([IO.Path]::GetDirectoryName($OutputPath)) -PathType Container)) { throw 'The external output directory must already exist.' }
$lock = Get-Content -Raw -LiteralPath (Join-Path $PSScriptRoot '../config/swift-assessment.lock.json') | ConvertFrom-Json -DateKind String
if (-not $PSCmdlet.ShouldProcess($OutputPath, 'Export pinned Swift weights to experimental PTQ1')) { return }
# This command needs the repacker partial and codec together. Run in a fresh
# PowerShell process if an inspector-only command has already loaded its type.
if ('SwiftBonsai.GgufInspector' -as [type]) { throw 'Use a fresh PowerShell process for this export.' }
Add-Type -Path @((Join-Path $PSScriptRoot 'GgufMetadata.cs'), (Join-Path $PSScriptRoot 'GgufPtq1Repacker.cs'), (Join-Path $PSScriptRoot 'Pq2Ptq1Codec.cs')) -CompilerOptions @('/nullable:enable', '/checked+', '/unsafe-', '/warn:9999', '/warnaserror+', '/langversion:14.0')
$guard = [IO.File]::Open($ModelPath, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
try {
    Test-LockedArtifact $ModelPath $lock.model.bytes $lock.model.sha256
    $partial = $OutputPath + '.partial-' + [guid]::NewGuid().ToString('N')
    $result = [SwiftBonsai.GgufInspector]::RepackPq2ToPtq1($ModelPath, $partial, $TimeoutSeconds)
    $outputGuard = [IO.File]::Open($partial, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
    try {
        Test-LockedArtifact $partial ([long]$result.OutputBytes) $result.OutputSha256
        $source = [SwiftBonsai.GgufInspector]::Read($ModelPath, 'Prism-73bfe6cc485554822f80c473a3c0c4f25e80f41b', $false)
        $export = [SwiftBonsai.GgufInspector]::Read($partial, 'Prism-73bfe6cc485554822f80c473a3c0c4f25e80f41b', $false)
        if ($source.TokenizerMetadataSha256 -cne $export.TokenizerMetadataSha256 -or $source.TensorCount -ne $export.TensorCount) { throw 'Export metadata identity differs.' }
        $result['SourceSha256'] = [string]$lock.model.sha256
        $result['SourceRevision'] = [string]$lock.model.revision
        $result['TokenizerMetadataSha256'] = $export.TokenizerMetadataSha256
    } finally { $outputGuard.Dispose() }
    [IO.File]::Move($partial, $OutputPath, $false)
    $result['OutputPath'] = $OutputPath
    return $result
} finally { $guard.Dispose() }
