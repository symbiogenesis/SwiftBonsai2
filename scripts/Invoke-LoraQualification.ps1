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
Import-Module (Join-Path $PSScriptRoot 'Bonsai.LoraEvaluation.psm1') -Force
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Completion.psm1')
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Tools.psm1')
$data = Resolve-ExternalPath $DataDirectory
$plan = Get-BonsaiLoraQualificationPlan
$contract = $plan.Contract
$representation = if ($UseRepackedPtq1) { 'current-repacked-PTQ1_0' } else { 'PQ2_0' }
if ($PlanOnly) {
    return [pscustomobject]@{ Status = 'plan-only'; ModelProcessStarted = $false
        ContractSha256 = $plan.ContractSha256; WeightRepresentation = $representation
        PairedSamples = $plan.Schedule.Count; ZeroScaleSamples = $contract.prompts.Count
        ServiceCeilingBytes = 12GB; PhysicalReserveBytes = 4GB; PerSampleTimeoutSeconds = 600
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
$runtime = & (Join-Path $PSScriptRoot 'Get-Engine.ps1') -DataDirectory $data -LockPath $lockPath -VerifyOnly
$adapter = & (Join-Path $PSScriptRoot 'Get-Adapter.ps1') -DataDirectory $data -VerifyOnly
$runtimeDirectory = Resolve-ExternalPath ([IO.Path]::GetDirectoryName($runtime.ServerPath))
$executable = Resolve-ExternalPath (Join-Path $runtimeDirectory 'llama-completion.exe')
Assert-Arm64PE $executable
$policy = Get-CpuCheckPolicy -Profile swift-pq2-assessment -UseAbliterateLora -UseRepackedPtq1:$UseRepackedPtq1
$derived = $null
if ($UseRepackedPtq1) {
    $derived = Get-Content -Raw -LiteralPath (Join-Path $PSScriptRoot '../config/swift.ptq1-export.json') | ConvertFrom-Json -AsHashtable -DateKind String
}
$artifact = Get-BonsaiCompletionArtifact -Lock $lock -Policy $policy -Derived $derived
$modelBytes = $artifact.Bytes
$modelPath = Resolve-ExternalPath (Join-Path $data -ChildPath 'models', $lock.model.revision, $artifact.Filename)
$controls = @{}
foreach ($key in $contract.controls.Keys) { $controls[$key] = $contract.controls[$key] }
$controls.modelSha256 = $contract.representations[$representation]
$controls.weightRepresentation = $representation
if ($artifact.Sha256 -cne $controls.modelSha256) { throw 'Selected artifact and qualification controls disagree.' }
$controlDigest = Get-BonsaiLoraControlDigest $controls
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
$guards = [Collections.Generic.List[IO.FileStream]]::new()
$receipt = [ordered]@{ Status = 'capturing'; StartedUtc = [DateTime]::UtcNow.ToString('o')
    EvidenceDirectory = $scratch; ProcessorName = $processors[0].Name; RuntimePackage = $lock.runtime.asset
    RuntimeFiles = @(); ModelBytes = $modelBytes; SourceRevision = $env:GITHUB_SHA
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
    $stdout = Join-Path $lane 'stdout.txt'; $stderr = Join-Path $lane 'stderr.txt'
    $encoding = [Text.UTF8Encoding]::new($false, $true)
    # GetString preserves a leading U+FEFF if generated; ReadAllText would
    # consume its UTF-8 bytes as an encoding preamble and change the capture.
    $text = $encoding.GetString([IO.File]::ReadAllBytes($stdout))
    if ([string]::IsNullOrWhiteSpace($text)) { throw 'Paired native sample returned no captured response.' }
    $logText = [IO.File]::ReadAllText($stderr, $encoding)
    $timing = Read-BonsaiCompletionTiming $logText
    $counts = Read-BonsaiCompletionTokenCount -Text $logText -Timing $timing
    $diagnostics = [ordered]@{ WallMilliseconds = $supervision.WallMilliseconds; PromptTokens = $timing.PromptTokens
        PromptMilliseconds = $timing.PromptMilliseconds; DecodeTokensPerSecond = $timing.DecodeTokensPerSecond
        EogMarkerObserved = $text.Contains(' [end of text]'); FirstTokenLatencyMilliseconds = $null
        PromptTokenCount = $counts.PromptTokenCount; SamplerAcceptedTokens = $counts.SamplerAcceptedTokens
        GeneratedSampledTokens = $counts.GeneratedSampledTokens
        LogitsAvailable = $false; ResponseScope = 'Exact native stdout including reasoning, end marker and trailing newlines' }
    $diagnostics | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $lane 'diagnostics.json') -Encoding utf8
    return [pscustomobject]@{ Text = $text; Sha256 = Get-BonsaiResponseDigest $text; Diagnostics = $diagnostics
        Timing = $timing; PeakJobMemoryBytes = [long]$supervision.PeakJobMemoryBytes; MinimumAvailablePhysicalBytes = [long]$supervision.MinimumAvailablePhysicalBytes }

}
try {
    # The Rust supervisor verifies and holds the model and this single scratch
    # copy once for the complete batch. It preserves the relative FNAME:SCALE
    # adapter argument required by the pinned Windows parser.
    $copyPath = Join-Path $scratch 'bonsai-abliterate-lora.gguf'
    [IO.File]::Copy($adapter.AdapterPath, $copyPath, $false)
    foreach ($file in (Get-ChildItem -LiteralPath $runtimeDirectory -File -Recurse | Where-Object Extension -In @('.exe', '.dll'))) {
        $guards.Add([IO.File]::Open($file.FullName, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read))
        $receipt.RuntimeFiles += @{ Path = $file.FullName; Sha256 = (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash.ToLowerInvariant() }
    }
    # Reverify package identity after acquiring read-denying guards.
    $null = & (Join-Path $PSScriptRoot 'Get-Engine.ps1') -DataDirectory $data -LockPath $lockPath -VerifyOnly
    Save-CaptureEvidence
    $nativeSamples = @(
        foreach ($scheduled in $plan.Schedule) {
            $prompt = @($contract.prompts | Where-Object { $_.id -ceq $scheduled.promptId })[0]
            $lane = Resolve-ExternalPath (Join-Path $scratch $scheduled.sampleId)
            [void][IO.Directory]::CreateDirectory($lane)
            $arguments = @(Get-BonsaiLoraEvaluationArgument -ModelPath $modelPath -AdapterPath 'bonsai-abliterate-lora.gguf' -Arm $scheduled.arm -Prompt $prompt.text)
            $arguments | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $lane 'arguments.json') -Encoding utf8
            @{ id = $scheduled.sampleId; arguments = $arguments; evidenceDirectory = $lane }
        }
        foreach ($prompt in $contract.prompts) {
            $id = 'zero-' + $prompt.id
            $lane = Resolve-ExternalPath (Join-Path $scratch $id)
            [void][IO.Directory]::CreateDirectory($lane)
            $arguments = @(Get-BonsaiLoraEvaluationArgument -ModelPath $modelPath -AdapterPath 'bonsai-abliterate-lora.gguf' -Arm zero -Prompt $prompt.text)
            $arguments | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $lane 'arguments.json') -Encoding utf8
            @{ id = $id; arguments = $arguments; evidenceDirectory = $lane }
        }
    )
    $captureFailure = $null
    try {
        $supervision = Invoke-BonsaiTool -Command completion -ToolPath $ToolPath -Request @{
            schemaVersion = 1; executable = $executable; arguments = @(); samples = $nativeSamples
            workingDirectory = $scratch; evidenceDirectory = $scratch
            artifacts = @(
                @{ path = $modelPath; bytes = [long]$modelBytes; sha256 = $controls.modelSha256 },
                @{ path = $copyPath; bytes = [long]$adapterLock.adapter.bytes; sha256 = $controls.adapterSha256 }
            )
            memoryLimitBytes = 12GB; requiredAvailableBytes = 16GB; physicalReserveBytes = 4GB
            timeoutSeconds = 600; stdoutLimitBytes = 4MB; stderrLimitBytes = 64MB
            requireNonemptyStdout = $false; ciOnly = $true
        } -TimeoutSeconds 0
        $receipt.ModelProcessStarted = $supervision.ModelProcessStarted
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
    $qualification = Test-BonsaiLoraQualification -Evidence $evidence
    $qualification | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath (Join-Path $scratch 'qualification.json') -Encoding utf8
    $receipt.Status = 'capture-completed'; $receipt.QualificationStatus = $qualification.Status
    Save-CaptureEvidence
    return [pscustomobject]$receipt
} catch {
    $receipt.Status = 'failed'; $receipt['Failure'] = $_.Exception.Message
    Save-CaptureEvidence
    Write-Warning "Paired evaluation failure evidence retained at $scratch"
    throw
} finally { foreach ($guard in $guards) { $guard.Dispose() } }
