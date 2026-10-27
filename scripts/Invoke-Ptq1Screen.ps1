#requires -Version 7.6
<#
.SYNOPSIS
Runs the small current-weight PTQ1 decision screen on the target CI runner.
.DESCRIPTION
PlanOnly never starts a model. Actual captures require main-branch GitHub Actions
on the native Snapdragon runner. The screen compares 64-token prefixes; it cannot
qualify complete-answer quality, adapter basis, logits or a serving profile.
#>
[CmdletBinding()]
[OutputType([pscustomobject])]
param([Parameter(Mandatory)][string]$DataDirectory, [switch]$PlanOnly, [string]$ToolPath)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1')
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Tools.psm1')
$data = Resolve-ExternalPath $DataDirectory
if ($PlanOnly) {
    return [pscustomobject](Invoke-BonsaiTool -Command qualification -ToolPath $ToolPath -Request @{ operation = 'screen-plan' })
}
if (-not $IsWindows -or [Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture -ne 'Arm64' -or
    [Runtime.InteropServices.RuntimeInformation]::OSArchitecture -ne 'Arm64' -or
    $env:GITHUB_ACTIONS -cne 'true' -or $env:GITHUB_REF -cne 'refs/heads/main' -or
    $env:GITHUB_REPOSITORY -cne 'symbiogenesis/SwiftBonsai2Runner' -or $env:RUNNER_ARCH -cne 'ARM64' -or
    $env:GITHUB_RUN_ID -cnotmatch '^[0-9]+$' -or [string]::IsNullOrWhiteSpace($env:RUNNER_NAME)) {
    throw 'PTQ1 timing captures require this repository main and the native Windows ARM64 Snapdragon CI runner.'
}
$processors = @(Get-CimInstance Win32_Processor)
if ($processors.Count -ne 1 -or $processors[0].Name -notmatch 'Snapdragon.*X.*Elite') {
    throw 'The required Snapdragon X Elite device was not identified.'
}
$pq2 = & (Join-Path $PSScriptRoot 'Invoke-CpuCheck.ps1') -DataDirectory $data -Profile swift-pq2-assessment -UseAbliterateLora -CheckOnly -ToolPath $ToolPath
$ptq1 = & (Join-Path $PSScriptRoot 'Invoke-CpuCheck.ps1') -DataDirectory $data -Profile swift-pq2-assessment -UseAbliterateLora -UseRepackedPtq1 -CheckOnly -ToolPath $ToolPath
if ($pq2.Status -cne 'admission-passed' -or $ptq1.Status -cne 'admission-passed') {
    throw 'Both representations must meet the shared 15.75 GiB available-memory admission requirement.'
}
$lockPath = Join-Path $PSScriptRoot '../config/swift-assessment.lock.json'
$lock = Read-BonsaiLock $lockPath
$runtime = & (Join-Path $PSScriptRoot 'Get-Engine.ps1') -DataDirectory $data -LockPath $lockPath -VerifyOnly -ToolPath $ToolPath
$adapter = & (Join-Path $PSScriptRoot 'Get-Adapter.ps1') -DataDirectory $data -VerifyOnly
$scratch = Resolve-ExternalPath (Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-Ptq1Screen-' + [guid]::NewGuid().ToString('N')))
[void][IO.Directory]::CreateDirectory($scratch)
if ([string]::IsNullOrWhiteSpace($env:GITHUB_ENV) -or $scratch.Contains("`n") -or $scratch.Contains("`r")) {
    throw 'Expected the CI environment file and a single-line evidence path.'
}
$environmentFile = Resolve-ExternalPath $env:GITHUB_ENV
"BONSAI_PTQ1_EVIDENCE=$scratch" | Add-Content -LiteralPath $environmentFile -Encoding utf8
try {
    [pscustomobject](Invoke-BonsaiTool -Command qualification -ToolPath $ToolPath -TimeoutSeconds 0 -Request @{
        operation = 'screen-capture'; dataDirectory = $data
        pq2Admission = $pq2; ptq1Admission = $ptq1; lock = $lock; runtime = $runtime; adapter = $adapter
        evidenceDirectory = $scratch; processorName = $processors[0].Name
        runId = [guid]::NewGuid().ToString('N'); startedUtc = [DateTime]::UtcNow.ToString('o')
    })
} catch {
    Write-Warning "PTQ1 screen failure evidence retained at $scratch"
    throw
}
