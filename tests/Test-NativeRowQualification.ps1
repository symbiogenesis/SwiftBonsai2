#requires -Version 7.6
<# .SYNOPSIS
Checks row qualification plans and execution guards without models or compilers.
#>
[CmdletBinding()]
[OutputType([string])]
param([Parameter(Mandatory)][ValidateSet('HostedGuard', 'OwnerGuard', 'Plan')][string]$Case)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.TestSupport.psm1')
$builder = Join-Path $PSScriptRoot '../scripts/Build-NativeRowCaptureTools.ps1'
$qualifier = Join-Path $PSScriptRoot '../scripts/Invoke-NativeRowQualification.ps1'
$output = Join-Path ([IO.Path]::GetTempPath()) ('Bonsai-RowGuard-' + [guid]::NewGuid().ToString('N'))
if ($Case -ceq 'Plan') {
    $plan = & $qualifier -DataDirectory $output -PlanOnly
    Assert ($plan.Status -ceq 'plan-only' -and -not $plan.ModelProcessStarted) 'Row plan claimed model execution.'
    Assert ($plan.Representation -ceq 'PQ2_0' -and $plan.AdapterScale -eq 1.3 -and $plan.RustBackend -ceq 'auto') 'Row plan changed the intended app artifacts/settings.'
    Assert ($plan.SuppliedTokens.Count -eq 1 -and $plan.SuppliedTokens[0] -eq 248045) 'Row plan expanded the supplied token workload.'
    Assert ($plan.Conditions.Count -eq 2 -and $plan.Conditions[0].Name -ceq 'serial' -and $plan.Conditions[0].RowWorkers -eq 1 -and
        $plan.Conditions[1].Name -ceq 'parallel' -and $plan.Conditions[1].RowWorkers -eq 4) 'Row plan changed ordering or worker budgets.'
    Assert ($plan.WorkBudgetSeconds -eq 240 -and $plan.WholeJobMinutes -eq 5 -and
        $plan.JobBytes -eq 12348030976 -and $plan.AdmissionBytes -eq 16642998272 -and
        $plan.PhysicalReserveBytes -eq 4294967296) 'Row plan changed time or memory limits.'
    Assert (-not $plan.AppTpsQualified -and -not $plan.WriterBasisQualified -and -not $plan.DefaultMayChange) 'Row plan granted an independent qualification gate.'
    Assert (-not (Test-Path -LiteralPath $output)) 'Row plan created storage.'
    "Focused $Case row qualification checks passed."
    return
}
$savedActions = $env:GITHUB_ACTIONS
$savedEnvironment = $env:RUNNER_ENVIRONMENT
try {
    if ($Case -ceq 'OwnerGuard') {
        $env:GITHUB_ACTIONS = 'false'
        $failure = Assert-Failure -PassThru { & $qualifier -DataDirectory $output }
        Assert ($failure -like '*native Windows ARM64 owner CI*') 'Row qualification accepted local model execution.'
        Assert (-not (Test-Path -LiteralPath $output)) 'Rejected owner capture created data storage.'
    } else { foreach ($environment in @('local', 'self-hosted')) {
        $env:GITHUB_ACTIONS = if ($environment -ceq 'local') { 'false' } else { 'true' }
        $env:RUNNER_ENVIRONMENT = if ($environment -ceq 'local') { 'github-hosted' } else { 'self-hosted' }
        $failure = Assert-Failure -PassThru { & $builder -OutputDirectory $output }
        Assert ($failure -like '*hosted native Windows ARM64 CI*') 'Row builder accepted a non-hosted environment.'
        Assert (-not (Test-Path -LiteralPath $output)) 'Rejected row builder created output storage.'
    } }
} finally {
    [Environment]::SetEnvironmentVariable('GITHUB_ACTIONS', $savedActions)
    [Environment]::SetEnvironmentVariable('RUNNER_ENVIRONMENT', $savedEnvironment)
}
"Focused $Case row qualification checks passed."
