#requires -Version 7.6
<# .SYNOPSIS
Checks the nonlaunching PTQ1 screen bridge and local capture rejection.
#>
[CmdletBinding()]
[OutputType([string])]
param([Parameter(Mandatory)][ValidateSet('Plan', 'LocalRejection', 'All')][string]$Case, [string]$ToolPath)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
if ($Case -ceq 'All' -and $env:GITHUB_ACTIONS -cne 'true') { throw 'Select one focused case locally.' }
Import-Module (Join-Path $PSScriptRoot 'Bonsai.TestSupport.psm1')
$script = Join-Path $PSScriptRoot '../scripts/Invoke-Ptq1Screen.ps1'
if ($Case -cin @('Plan', 'All')) {
    $plan = & $script -DataDirectory ([IO.Path]::GetTempPath()) -PlanOnly -ToolPath $ToolPath
    Assert ($plan.TimedSamples -eq 12 -and $plan.ZeroScaleSamples -eq 2 -and $plan.Schedule.Count -eq 12) 'The small screen sample budget changed.'
    Assert ($plan.Contract.controls.maxOutputTokens -eq 64 -and $plan.Contract.adapterScale -eq 1.3 -and
        $plan.Contract.repeatCount -eq 3) 'The declared screen output cap, adapter scale or repetitions changed.'
    Assert (-not $plan.DefaultMayChange -and -not $plan.StandardProfileMayEnable) 'A screen plan authorized promotion.'
}
if ($Case -cin @('LocalRejection', 'All') -and $env:GITHUB_ACTIONS -cne 'true') {
    Assert-Failure { & $script -DataDirectory ([IO.Path]::GetTempPath()) -ToolPath $ToolPath } 'Local execution reached a model timing capture.'
}
'Focused PTQ1 screen bridge checks passed.'
