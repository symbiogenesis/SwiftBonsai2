#requires -Version 7.6
<#
.SYNOPSIS
Checks the real PowerShell-to-Rust promotion plan and pending evidence handoff.
.DESCRIPTION
Uses the packaged maintenance tool without loading models or running measurements.
The synthetic review revision never authorizes a serving default.
#>
[CmdletBinding()]
[OutputType([string])]
param([string]$ToolPath)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
$script = Join-Path $PSScriptRoot '../scripts/Get-RuntimePromotion.ps1'
$revision = 'a' * 40
$plan = & $script -RunnerRevision $revision -ToolPath $ToolPath
if ($plan.policy.identities.runnerRevision -cne $revision -or
    $plan.policy.identities.adapterScale -ne 1.3 -or
    $plan.policy.requiredGates.Count -lt 17) {
    throw 'Promotion wrapper did not preserve the complete standard-profile policy.'
}
$scratch = Join-Path ([IO.Path]::GetTempPath()) ('Runtime-Promotion-Bridge-' + [guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($scratch)
$evidence = Join-Path $scratch 'evidence.json'
$plan.evidenceTemplate | ConvertTo-Json -Depth 64 |
    Set-Content -LiteralPath $evidence -Encoding utf8NoBOM
$score = & $script -RunnerRevision $revision -ToolPath $ToolPath -EvidencePath $evidence
if ($score.promotionReady -or $score.status -cne 'pending' -or
    @($score.gates | Where-Object status -CNE 'pending').Count -ne 0) {
    throw 'Uncollected evidence incorrectly qualified the default runtime.'
}
'Runtime promotion wrapper preserves exact policy and pending evidence.'
