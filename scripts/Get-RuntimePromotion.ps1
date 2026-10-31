#requires -Version 7.6
<#
.SYNOPSIS
Reports the exact Rust standard-runtime promotion policy or reviews retained evidence.
.DESCRIPTION
Read-only: never starts a model, benchmarks, changes runtime selection or publishes.
RunnerRevision must be the exact settled main revision under review. Without an
EvidencePath, returns the policy and its SHA-256. With evidence, verifies bounded
external report bytes and completeness of reviewer-attested gate records. Missing
gates remain pending. A passed result still requires independent verification of
CI provenance and human identities; this tool does not authenticate attestations.
#>
[CmdletBinding()]
[OutputType([pscustomobject])]
param(
    [Parameter(Mandatory)][ValidatePattern('^[0-9a-f]{40}$')][string]$RunnerRevision,
    [ValidateSet(1, 4)][int]$NativeRowWorkers = 1,
    [string]$EvidencePath,
    [string]$ToolPath
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Tools.psm1')
$request = @{ operation = 'plan'; runnerRevision = $RunnerRevision; nativeRowWorkers = $NativeRowWorkers }
if ($EvidencePath) {
    $request.operation = 'file-score'
    $request.evidencePath = $EvidencePath
}
[pscustomobject](Invoke-BonsaiTool -Command runtime-promotion -Request $request -ToolPath $ToolPath)
