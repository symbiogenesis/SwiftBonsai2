#requires -Version 7.6
<# .SYNOPSIS
Requires hosted candidate preparation to reject before writes outside its exact CI context.
#>
[CmdletBinding()]
[OutputType([string])]
param()
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'Bonsai.TestSupport.psm1')
$output = Join-Path ([IO.Path]::GetTempPath()) ('Bonsai-RejectedHeadless-' + [guid]::NewGuid().ToString('N'))
$script = Join-Path $PSScriptRoot '../scripts/Build-HeadlessPublisherCandidate.ps1'
$previousRef = [Environment]::GetEnvironmentVariable('GITHUB_REF')
try {
    $env:GITHUB_REF = 'refs/heads/model-free-guard-fixture'
    $message = Assert-Failure -PassThru -Action { & $script -OutputDirectory $output }
    Assert ($message -ceq 'Headless publisher preparation requires this repository main on hosted native Windows ARM64 CI.') 'The guard did not reject before native preparation.'
    Assert (-not (Test-Path -LiteralPath $output)) 'Rejected preparation created an output directory.'
} finally {
    if ($null -eq $previousRef) { Remove-Item -LiteralPath Env:GITHUB_REF -ErrorAction SilentlyContinue }
    else { $env:GITHUB_REF = $previousRef }
}
Write-Output 'Hosted headless candidate guard rejected before writes, builds, probes or model access.'
