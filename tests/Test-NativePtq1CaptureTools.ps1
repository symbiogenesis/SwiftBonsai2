#requires -Version 7.6
<# .SYNOPSIS
Checks that capture-tool preparation rejects local and owner-runner execution.
#>
[CmdletBinding()]
[OutputType([string])]
param([Parameter(Mandatory)][ValidateSet('HostedGuard')][string]$Case)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.TestSupport.psm1')
$script = Join-Path $PSScriptRoot '../scripts/Build-NativePtq1CaptureTools.ps1'
$output = Join-Path ([IO.Path]::GetTempPath()) ('Bonsai-HostedCaptureGuard-' + [guid]::NewGuid().ToString('N'))
$savedActions = $env:GITHUB_ACTIONS
$savedEnvironment = $env:RUNNER_ENVIRONMENT
try {
    foreach ($environment in @('local', 'self-hosted')) {
        $env:GITHUB_ACTIONS = if ($environment -ceq 'local') { 'false' } else { 'true' }
        $env:RUNNER_ENVIRONMENT = if ($environment -ceq 'local') { 'github-hosted' } else { 'self-hosted' }
        $failure = Assert-Failure -PassThru { & $script -OutputDirectory $output }
        Assert ($failure -like '*hosted native Windows ARM64 CI*') 'Capture build did not reject the non-hosted environment.'
        Assert (-not (Test-Path -LiteralPath $output)) 'Rejected capture preparation created an output directory.'
    }
} finally {
    [Environment]::SetEnvironmentVariable('GITHUB_ACTIONS', $savedActions)
    [Environment]::SetEnvironmentVariable('RUNNER_ENVIRONMENT', $savedEnvironment)
}
"Focused $Case capture preparation checks passed."
