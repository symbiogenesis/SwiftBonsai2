#requires -Version 7.6
<#
.SYNOPSIS
Captures the mandatory fixed paired Swift LoRA evaluation on the target CI runner.
.DESCRIPTION
Benchmarks run only on GitHub Actions main on the Snapdragon X Elite. Artifacts
must already be provisioned. PlanOnly reads policy without starting a model or
inference process. Every sample uses a fresh owned process, 11.75 GiB Job commit ceiling,
4 GiB physical reserve, fixed controls and a ten-minute deadline. Responses,
timings, arguments and failures remain in unique external system-temp evidence.
This CLI capture cannot establish logits parity or the SSM authoring basis;
those checks and independent human quality review remain mandatory and pending.
#>
[CmdletBinding()]
[OutputType([pscustomobject])]
param(
    [Parameter(Mandatory)][string]$DataDirectory,
    [switch]$UseRepackedPtq1,
    [switch]$PlanOnly,
    [string]$ToolPath
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1') -Force
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Qualification.psm1') -Force
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Tools.psm1')
$data = Resolve-ExternalPath $DataDirectory
$representation = if ($UseRepackedPtq1) { 'current-repacked-PTQ1_0' } else { 'PQ2_0' }
if ($PlanOnly) {
    $plan = Get-BonsaiLoraQualificationPlan -ToolPath $ToolPath -WeightRepresentation $representation
    $contract = $plan.Contract
    $policy = [pscustomobject]$plan.CpuPolicy
    return [pscustomobject]@{ Status = 'plan-only'; ModelProcessStarted = $false
        ContractSha256 = $plan.ContractSha256; WeightRepresentation = $representation
        PairedSamples = $plan.Schedule.Count; ZeroScaleSamples = $contract.prompts.Count
        ServiceCeilingBytes = $policy.ServiceCeilingBytes; PhysicalReserveBytes = $policy.AvailableMemoryFloorBytes
        PerSampleTimeoutSeconds = $policy.TimeoutSeconds
        MandatoryQualityReview = $true; MandatoryLogitsAndBasisVerification = $true }
}
if (-not $IsWindows -or [Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture -ne 'Arm64' -or
    [Runtime.InteropServices.RuntimeInformation]::OSArchitecture -ne 'Arm64' -or
    $env:GITHUB_ACTIONS -cne 'true' -or $env:GITHUB_REF -cne 'refs/heads/main' -or
    $env:GITHUB_REPOSITORY -cne 'symbiogenesis/SwiftBonsai2Runner' -or $env:RUNNER_ARCH -cne 'ARM64' -or
    $env:GITHUB_RUN_ID -cnotmatch '^[0-9]+$' -or [string]::IsNullOrWhiteSpace($env:RUNNER_NAME)) {
    throw 'Model evaluations are CI-only, on this repository main and the native Windows ARM64 Snapdragon runner.'
}
$processors = @(Get-CimInstance Win32_Processor)
if ($processors.Count -ne 1 -or $processors[0].Name -notmatch 'Snapdragon.*X.*Elite') { throw 'The required Snapdragon X Elite device was not identified.' }
$overrides = @(Get-ChildItem Env: | Where-Object Name -Match '^(LLAMA_|LLAMACPP_|GGML_|AIP_|MTMD_|HF_TOKEN$|HF_ENDPOINT$|MODEL_ENDPOINT$)')
if ($overrides.Count) { throw ('Inherited model/backend overrides are unsupported: ' + ($overrides.Name -join ', ')) }
$admission = & (Join-Path $PSScriptRoot 'Invoke-CpuCheck.ps1') -DataDirectory $data -Profile swift-pq2-assessment -UseAbliterateLora -UseRepackedPtq1:$UseRepackedPtq1 -CheckOnly -ToolPath $ToolPath
if ($admission.Status -cne 'admission-passed') { throw 'The shared 15.75 GiB available-memory admission requirement was not met.' }
$lockPath = Join-Path $PSScriptRoot '../config/swift-assessment.lock.json'
$lock = Read-BonsaiLock $lockPath
$runtime = & (Join-Path $PSScriptRoot 'Get-Engine.ps1') -DataDirectory $data -LockPath $lockPath -VerifyOnly -ToolPath $ToolPath
$adapter = & (Join-Path $PSScriptRoot 'Get-Adapter.ps1') -DataDirectory $data -VerifyOnly
$scratch = Resolve-ExternalPath (Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-LoraQualification-' + [guid]::NewGuid().ToString('N')))
[void][IO.Directory]::CreateDirectory($scratch)
if ([string]::IsNullOrWhiteSpace($env:GITHUB_ENV) -or $scratch.Contains("`n") -or $scratch.Contains("`r")) { throw 'Expected the CI environment file and a single-line evidence path.' }
$environmentFile = Resolve-ExternalPath $env:GITHUB_ENV
"BONSAI_LORA_EVIDENCE=$scratch" | Add-Content -LiteralPath $environmentFile -Encoding utf8
try {
    [pscustomobject](Invoke-BonsaiTool -Command qualification -ToolPath $ToolPath -TimeoutSeconds 0 -Request @{
        operation = 'capture'; dataDirectory = $data; weightRepresentation = $representation
        admission = $admission; lock = $lock; runtime = $runtime; adapter = $adapter
        evidenceDirectory = $scratch; processorName = $processors[0].Name
        runId = [guid]::NewGuid().ToString('N'); startedUtc = [DateTime]::UtcNow.ToString('o')
    })
} catch {
    Write-Warning "Paired evaluation failure evidence retained at $scratch"
    throw
}
