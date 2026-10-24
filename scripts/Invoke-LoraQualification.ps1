#requires -Version 7.6
<#
.SYNOPSIS
Captures the mandatory fixed paired Swift LoRA evaluation on the target CI runner.
.DESCRIPTION
Benchmarks run only on GitHub Actions main on the Snapdragon X Elite. Artifacts
must already be provisioned. PlanOnly reads policy without starting a model or
inference process. Every sample uses a fresh owned process, 12 GiB Job commit ceiling,
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
$plan = Get-BonsaiLoraQualificationPlan -ToolPath $ToolPath -WeightRepresentation $representation
$contract = $plan.Contract
$policy = [pscustomobject]$plan.CpuPolicy
if ($PlanOnly) {
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
    $env:GITHUB_REPOSITORY -cne 'symbiogenesis/SwiftBonsai2' -or $env:RUNNER_ARCH -cne 'ARM64' -or
    $env:GITHUB_RUN_ID -cnotmatch '^[0-9]+$' -or [string]::IsNullOrWhiteSpace($env:RUNNER_NAME)) {
    throw 'Model evaluations are CI-only, on this repository main and the native Windows ARM64 Snapdragon runner.'
}
$processors = @(Get-CimInstance Win32_Processor)
if ($processors.Count -ne 1 -or $processors[0].Name -notmatch 'Snapdragon.*X.*Elite') { throw 'The required Snapdragon X Elite device was not identified.' }
$overrides = @(Get-ChildItem Env: | Where-Object Name -Match '^(LLAMA_|LLAMACPP_|GGML_|AIP_|MTMD_|HF_TOKEN$|HF_ENDPOINT$|MODEL_ENDPOINT$)')
if ($overrides.Count) { throw ('Inherited model/backend overrides are unsupported: ' + ($overrides.Name -join ', ')) }
$admission = & (Join-Path $PSScriptRoot 'Invoke-CpuCheck.ps1') -DataDirectory $data -Profile swift-pq2-assessment -UseAbliterateLora -UseRepackedPtq1:$UseRepackedPtq1 -CheckOnly -ToolPath $ToolPath
if ($admission.Status -cne 'admission-passed') { throw 'The unchanged 16 GiB available-memory admission requirement was not met.' }
$lockPath = Join-Path $PSScriptRoot '../config/swift-assessment.lock.json'
$lock = Read-BonsaiLock $lockPath
$adapterLock = Get-Content -Raw -LiteralPath (Join-Path $PSScriptRoot '../config/swift-lora.lock.json') | ConvertFrom-Json -AsHashtable -DateKind String
if ($admission.ModelRevision -cne $lock.model.revision -or $admission.RuntimeCommit -cne $lock.runtime.commit) { throw 'CPU admission and qualification artifact pins disagree.' }
$runtime = & (Join-Path $PSScriptRoot 'Get-Engine.ps1') -DataDirectory $data -LockPath $lockPath -VerifyOnly -ToolPath $ToolPath
$adapter = & (Join-Path $PSScriptRoot 'Get-Adapter.ps1') -DataDirectory $data -VerifyOnly
$runtimeDirectory = Resolve-ExternalPath ([IO.Path]::GetDirectoryName($runtime.ServerPath))
$executable = Resolve-ExternalPath (Join-Path $runtimeDirectory 'llama-completion.exe')
Assert-Arm64PE $executable
$modelBytes = $admission.EstimatedWeightBytes
$modelPath = $admission.ModelPath
$controls = @{}
foreach ($key in $contract.controls.Keys) { $controls[$key] = $contract.controls[$key] }
$controls.modelSha256 = $contract.representations[$representation]
$controls.weightRepresentation = $representation
if ($admission.ArtifactSha256 -cne $controls.modelSha256) { throw 'Selected artifact and qualification controls disagree.' }
$invocations = Invoke-BonsaiTool -Command qualification -ToolPath $ToolPath -Request @{
    operation = 'invocations'; controls = $controls; modelPath = $modelPath
}
$controlDigest = $invocations.ControlsSha256
if ($lock.runtime.commit -cne $controls.runtimeCommit -or $adapterLock.adapter.sha256 -cne $controls.adapterSha256) { throw 'Runtime/adapter and qualification controls disagree.' }
$scratch = Resolve-ExternalPath (Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-LoraQualification-' + [guid]::NewGuid().ToString('N')))
[void][IO.Directory]::CreateDirectory($scratch)
if ([string]::IsNullOrWhiteSpace($env:GITHUB_ENV) -or $scratch.Contains("`n") -or $scratch.Contains("`r")) { throw 'Expected the CI environment file and a single-line evidence path.' }
$environmentFile = Resolve-ExternalPath $env:GITHUB_ENV
"BONSAI_LORA_EVIDENCE=$scratch" | Add-Content -LiteralPath $environmentFile -Encoding utf8
$evidence = [ordered]@{
    schemaVersion = 1; runId = [guid]::NewGuid().ToString('N'); contractSha256 = $plan.ContractSha256
    execution = @{ githubActions = $true; runUrl = "https://github.com/$env:GITHUB_REPOSITORY/actions/runs/$env:GITHUB_RUN_ID"
        runnerArchitecture = 'Arm64'; deviceModel = 'Snapdragon X Elite'; runnerName = $env:RUNNER_NAME }
    controls = $controls
    basis = @{ status = 'pending'; evidenceSha256 = $null; reviewerId = $null; note = 'The artifact does not declare the source SSM basis; residual-writer verification remains required.' }
    samples = @(); zeroScaleChecks = @()
    nonzeroAdapterCheck = @{ status = 'pending'; scale = 1; adapterSha256 = $controls.adapterSha256
        logitsChanged = $null; evidenceSha256 = $null }
}
$receipt = [ordered]@{ Status = 'capturing'; StartedUtc = [DateTime]::UtcNow.ToString('o')
    EvidenceDirectory = $scratch; ProcessorName = $processors[0].Name; RuntimePackage = $lock.runtime.asset
    RuntimeFiles = $runtime.RuntimeFiles; ModelBytes = $modelBytes; SourceRevision = $env:GITHUB_SHA
    TimingScope = 'native single-token eval runs/ms, excluding prompt evaluation; not exact sampled token count or first-token latency'
    QualificationStatus = 'pending'; ModelProcessStarted = $false }
function Save-CaptureEvidence {
    [CmdletBinding()]
    [OutputType([void])]
    param()
    $evidence | ConvertTo-Json -Depth 16 | Set-Content -LiteralPath (Join-Path $scratch 'evidence.json') -Encoding utf8
    $receipt | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath (Join-Path $scratch 'receipt.json') -Encoding utf8
}
function Read-PairedSample {
    [CmdletBinding()]
    [OutputType([pscustomobject])]
    param([string]$Id)
    $lane = Resolve-ExternalPath (Join-Path $scratch $Id)
    $supervisionPath = Join-Path $lane 'supervision.json'
    if (-not [IO.File]::Exists($supervisionPath)) { return $null }
    $supervision = Get-Content -Raw -LiteralPath $supervisionPath | ConvertFrom-Json -DateKind String
    if ($supervision.Status -cne 'completed') { return $null }
    # The completed receipt is published only after Rust has durably saved the
    # exact UTF-8 response identity and diagnostics. Failed/missing lanes remain
    # unavailable during partial-batch recovery, even if they have raw stdout.
    $captured = Get-Content -Raw -LiteralPath (Join-Path $lane 'capture.json') | ConvertFrom-Json -DateKind String
    if ($captured.SchemaVersion -ne 1) { throw 'Unsupported paired native capture schema.' }
    return [pscustomobject]@{ Text = $captured.Text; Sha256 = $captured.Sha256; Diagnostics = $captured.Diagnostics
        Timing = $supervision.Timing; PeakJobMemoryBytes = [long]$supervision.PeakJobMemoryBytes; MinimumAvailablePhysicalBytes = [long]$supervision.MinimumAvailablePhysicalBytes }

}
try {
    # The Rust supervisor verifies and holds the model and this single scratch
    # copy once for the complete batch. It preserves the relative FNAME:SCALE
    # adapter argument required by the pinned Windows parser.
    $copyPath = Join-Path $scratch 'bonsai-abliterate-lora.gguf'
    [IO.File]::Copy($adapter.AdapterPath, $copyPath, $false)
    # Rust re-verifies the complete runtime and retains its guards across every sample.
    Save-CaptureEvidence
    $nativeSamples = @(
        foreach ($sample in $invocations.Samples) {
            $lane = Resolve-ExternalPath (Join-Path $scratch $sample.id)
            [void][IO.Directory]::CreateDirectory($lane)
            $arguments = [string[]]$sample.arguments
            $arguments | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $lane 'arguments.json') -Encoding utf8
            @{ id = $sample.id; arguments = $arguments; evidenceDirectory = $lane }
        }
    )
    $captureFailure = $null
    try {
        $supervision = Invoke-BonsaiTool -Command completion -ToolPath $ToolPath -Request @{
            schemaVersion = 1; executable = $executable; arguments = @(); samples = $nativeSamples
            workingDirectory = $scratch; evidenceDirectory = $scratch; runtime = $runtime.Runtime
            artifacts = @(
                @{ path = $modelPath; bytes = [long]$modelBytes; sha256 = $controls.modelSha256 },
                @{ path = $copyPath; bytes = [long]$adapterLock.adapter.bytes; sha256 = $controls.adapterSha256 }
            )
            memoryLimitBytes = [long]$policy.ServiceCeilingBytes; requiredAvailableBytes = [long]$policy.RequiredAvailableBytes
            physicalReserveBytes = [long]$policy.AvailableMemoryFloorBytes
            timeoutSeconds = [int]$policy.TimeoutSeconds; stdoutLimitBytes = 4MB; stderrLimitBytes = 64MB
            requireNonemptyStdout = $false; ciOnly = $true
        } -TimeoutSeconds 0
        $receipt.ModelProcessStarted = $supervision.ModelProcessStarted
        $receipt.RuntimeFiles = $supervision.RuntimeFiles
    } catch {
        $captureFailure = $_
        $supervisionPath = Join-Path $scratch 'supervision.json'
        if ([IO.File]::Exists($supervisionPath)) {
            $supervision = Get-Content -Raw -LiteralPath $supervisionPath | ConvertFrom-Json -DateKind String
            $receipt.ModelProcessStarted = $supervision.ModelProcessStarted
        }
    }
    foreach ($scheduled in $plan.Schedule) {
        $captured = Read-PairedSample -Id $scheduled.sampleId
        if ($null -eq $captured) { if ($captureFailure) { break }; throw 'Completed paired sample evidence is missing.' }
        $sample = [ordered]@{}
        foreach ($key in $scheduled.Keys) { $sample[$key] = $scheduled[$key] }
        $sample.controlsSha256 = $controlDigest; $sample.decodeTokens = $captured.Timing.DecodeTokens
        $sample.decodeMilliseconds = $captured.Timing.DecodeMilliseconds; $sample.peakJobMemoryBytes = $captured.PeakJobMemoryBytes
        $sample.minimumAvailablePhysicalBytes = $captured.MinimumAvailablePhysicalBytes; $sample.completed = $true
        $sample.responseText = $captured.Text; $sample.responseSha256 = $captured.Sha256
        $sample.diagnostics = $captured.Diagnostics
        $evidence.samples += $sample
        Save-CaptureEvidence
    }
    foreach ($prompt in $contract.prompts) {
        $captured = Read-PairedSample -Id ('zero-' + $prompt.id)
        if ($null -eq $captured) { if ($captureFailure) { break }; throw 'Completed zero-scale sample evidence is missing.' }
        $baseline = @($evidence.samples | Where-Object { $_.promptId -ceq $prompt.id -and $_.arm -ceq 'baseline' -and $_.repeat -eq 1 })[0]
        $evidence.zeroScaleChecks += @{ promptId = $prompt.id; seed = 42
            baselineResponseSha256 = $baseline.responseSha256; zeroScaleResponseSha256 = $captured.Sha256
            baselineLogitsSha256 = $null; zeroScaleLogitsSha256 = $null }
        Save-CaptureEvidence
    }
    if ($captureFailure) { throw $captureFailure }
    $qualification = Test-BonsaiLoraQualification -Evidence $evidence -ToolPath $ToolPath
    $qualification | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath (Join-Path $scratch 'qualification.json') -Encoding utf8
    $receipt.Status = 'capture-completed'; $receipt.QualificationStatus = $qualification.Status
    Save-CaptureEvidence
    return [pscustomobject]$receipt
} catch {
    $receipt.Status = 'failed'; $receipt['Failure'] = $_.Exception.Message
    Save-CaptureEvidence
    Write-Warning "Paired evaluation failure evidence retained at $scratch"
    throw
}
