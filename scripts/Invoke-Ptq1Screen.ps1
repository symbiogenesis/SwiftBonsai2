#requires -Version 7.6
<#
.SYNOPSIS
Runs the small current-weight PTQ1 decision screen on the target CI runner.
.DESCRIPTION
PlanOnly never starts a model. Actual captures require main-branch GitHub Actions
on the native Snapdragon runner. The screen compares 64-token prefixes; it cannot
qualify complete-answer quality, adapter basis, logits or a serving profile.
The verified completion executable must first pass a model-free --version check
under the unchanged headless worker policy. Failures retain evidence and stop
before model admission or artifact verification.
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
foreach ($name in @('ACTIONS_RUNTIME_TOKEN', 'ACTIONS_ID_TOKEN_REQUEST_TOKEN', 'GITHUB_TOKEN', 'GH_TOKEN', 'ACTIONS_RUNNER_INPUT_TOKEN')) {
    if (Test-Path -LiteralPath ('Env:' + $name)) { throw 'PTQ1 screening requires CI credentials omitted from the model process environment.' }
}
$lockPath = Join-Path $PSScriptRoot '../config/swift-assessment.lock.json'
$lock = Read-BonsaiLock $lockPath
$runtime = & (Join-Path $PSScriptRoot 'Get-Engine.ps1') -DataDirectory $data -LockPath $lockPath -VerifyOnly -ToolPath $ToolPath
$cpu = Invoke-BonsaiTool -Command cpu-check-plan -ToolPath $ToolPath -Request @{
    operation = 'plan'; dataDirectory = $data; profile = 'swift-pq2-assessment'
    useAbliterateLora = $true; useRepackedPtq1 = $false
}
$scratch = Resolve-ExternalPath (Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-Ptq1Screen-' + [guid]::NewGuid().ToString('N')))
[void][IO.Directory]::CreateDirectory($scratch)
if ([string]::IsNullOrWhiteSpace($env:GITHUB_ENV) -or $scratch.Contains("`n") -or $scratch.Contains("`r")) {
    throw 'Expected the CI environment file and a single-line evidence path.'
}
$environmentFile = Resolve-ExternalPath $env:GITHUB_ENV
"BONSAI_PTQ1_EVIDENCE=$scratch" | Add-Content -LiteralPath $environmentFile -Encoding utf8
$preflightPath = Join-Path $scratch 'worker-preflight.json'
$preflightReport = [ordered]@{
    SchemaVersion = 1; Kind = 'model-free-worker-startup'; Status = 'pending'
    Revision = $env:GITHUB_SHA; RunUrl = "https://github.com/symbiogenesis/SwiftBonsai2Runner/actions/runs/$env:GITHUB_RUN_ID"
    RunAttempt = $env:GITHUB_RUN_ATTEMPT; Runner = $env:RUNNER_NAME; ProcessorName = $processors[0].Name
    RuntimeCommit = $runtime.Commit; Arguments = @('--version'); ModelLoaded = $false
    ModelTrialsRequested = $false; WorkerPolicy = 'required-headless'; DefaultMayChange = $false
}
try {
    $preflightLane = Join-Path $scratch 'worker-preflight'
    [void][IO.Directory]::CreateDirectory($preflightLane)
    $preflightReport.Status = 'running'
    $preflightReport | ConvertTo-Json -Depth 32 | Set-Content -LiteralPath $preflightPath -Encoding utf8
    # ciOnly is false only to omit model timing parsing for this fixed command.
    # Main/device CI was checked above; completion still uses the ordinary worker.
    $preflight = Invoke-BonsaiTool -Command completion -ToolPath $ToolPath -Request @{
        schemaVersion = 1; executable = Join-Path $runtime.Runtime.runtimeDirectory 'llama-completion.exe'
        arguments = @('--version'); workingDirectory = $preflightLane; evidenceDirectory = $preflightLane
        runtime = $runtime.Runtime; artifacts = @()
        memoryLimitBytes = 536870912
        requiredAvailableBytes = [long]$cpu.Policy.RequiredAvailableBytes
        physicalReserveBytes = [long]$cpu.Policy.AvailableMemoryFloorBytes
        timeoutSeconds = 10; stdoutLimitBytes = 65536; stderrLimitBytes = 65536
        requireNonemptyStdout = $false; ciOnly = $false
    }
    $preflightReport.Status = 'passed'
    $preflightReport['Supervision'] = $preflight
    $preflightReport | ConvertTo-Json -Depth 32 | Set-Content -LiteralPath $preflightPath -Encoding utf8
    $pq2 = & (Join-Path $PSScriptRoot 'Invoke-CpuCheck.ps1') -DataDirectory $data -Profile swift-pq2-assessment -UseAbliterateLora -CheckOnly -ToolPath $ToolPath
    $ptq1 = & (Join-Path $PSScriptRoot 'Invoke-CpuCheck.ps1') -DataDirectory $data -Profile swift-pq2-assessment -UseAbliterateLora -UseRepackedPtq1 -CheckOnly -ToolPath $ToolPath
    if ($pq2.Status -cne 'admission-passed' -or $ptq1.Status -cne 'admission-passed') {
        throw 'Both representations must meet the shared 15.5 GiB available-memory admission requirement.'
    }
    $adapter = & (Join-Path $PSScriptRoot 'Get-Adapter.ps1') -DataDirectory $data -VerifyOnly
    $preflightReport.ModelTrialsRequested = $true
    $preflightReport | ConvertTo-Json -Depth 32 | Set-Content -LiteralPath $preflightPath -Encoding utf8
    [pscustomobject](Invoke-BonsaiTool -Command qualification -ToolPath $ToolPath -TimeoutSeconds 0 -Request @{
        operation = 'screen-capture'; dataDirectory = $data
        pq2Admission = $pq2; ptq1Admission = $ptq1; lock = $lock; runtime = $runtime; adapter = $adapter
        evidenceDirectory = $scratch; processorName = $processors[0].Name
        runId = [guid]::NewGuid().ToString('N'); startedUtc = [DateTime]::UtcNow.ToString('o')
    })
} catch {
    if ($preflightReport.Status -ceq 'running') { $preflightReport.Status = 'failed' }
    $preflightReport['Failure'] = $_.Exception.Message
    $preflightReport | ConvertTo-Json -Depth 32 | Set-Content -LiteralPath $preflightPath -Encoding utf8
    Write-Warning "PTQ1 screen failure evidence retained at $scratch"
    throw
}
