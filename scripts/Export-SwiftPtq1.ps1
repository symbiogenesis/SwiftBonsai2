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
    [ValidateRange(1,3600)][int]$TimeoutSeconds = 1800,
    [string]$GgufToolPath
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1') -Force
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Gguf.psm1')
$ModelPath = Resolve-ExternalPath $ModelPath
$OutputPath = Resolve-ExternalPath $OutputPath
if (Test-Path -LiteralPath $OutputPath) { throw 'OutputPath must name a new external GGUF file.' }
if (-not (Test-Path -LiteralPath ([IO.Path]::GetDirectoryName($OutputPath)) -PathType Container)) { throw 'The external output directory must already exist.' }
$lock = Get-Content -Raw -LiteralPath (Join-Path $PSScriptRoot '../config/swift-assessment.lock.json') | ConvertFrom-Json -DateKind String
if (-not $PSCmdlet.ShouldProcess($OutputPath, 'Export pinned Swift weights to experimental PTQ1')) { return }
$arguments = @('export-ptq1', '--model', $ModelPath, '--output', $OutputPath, '--expected-bytes', ([long]$lock.model.bytes).ToString([Globalization.CultureInfo]::InvariantCulture), '--expected-sha256', [string]$lock.model.sha256, '--timeout-seconds', $TimeoutSeconds.ToString([Globalization.CultureInfo]::InvariantCulture))
$result = Invoke-GgufTool -Arguments $arguments -GgufToolPath $GgufToolPath
$result['SourceRevision'] = [string]$lock.model.revision
return $result
