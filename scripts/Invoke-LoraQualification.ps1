#requires -Version 7.6
<#
.SYNOPSIS
Captures the mandatory fixed paired Swift LoRA evaluation on the target CI runner.
.DESCRIPTION
Benchmarks run only on GitHub Actions main on the Snapdragon X Elite. Artifacts
must already be provisioned. PlanOnly reads policy without starting any native
process. Every sample uses a fresh owned process, 12 GiB Job commit ceiling,
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
    [switch]$PlanOnly
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1') -Force
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Qualification.psm1') -Force
Import-Module (Join-Path $PSScriptRoot 'Bonsai.LoraEvaluation.psm1') -Force
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
$admission = & (Join-Path $PSScriptRoot 'Invoke-CpuCheck.ps1') -DataDirectory $data -Profile swift-pq2-assessment -UseAbliterateLora -UseRepackedPtq1:$UseRepackedPtq1 -CheckOnly
if ($admission.Status -cne 'admission-passed') { throw 'The unchanged 16 GiB available-memory admission requirement was not met.' }
$lockPath = Join-Path $PSScriptRoot '../config/swift-assessment.lock.json'
$lock = Read-BonsaiLock $lockPath
$adapterLock = Get-Content -Raw -LiteralPath (Join-Path $PSScriptRoot '../config/swift-lora.lock.json') | ConvertFrom-Json -AsHashtable -DateKind String
$runtime = & (Join-Path $PSScriptRoot 'Get-Engine.ps1') -DataDirectory $data -LockPath $lockPath -VerifyOnly
$adapter = & (Join-Path $PSScriptRoot 'Get-Adapter.ps1') -DataDirectory $data -VerifyOnly
$runtimeDirectory = Resolve-ExternalPath ([IO.Path]::GetDirectoryName($runtime.ServerPath))
$executable = Resolve-ExternalPath (Join-Path $runtimeDirectory 'llama-completion.exe')
Assert-Arm64PE $executable
$modelFilename = $lock.model.filename
$modelBytes = $lock.model.bytes
if ($UseRepackedPtq1) {
    $derived = Get-Content -Raw -LiteralPath (Join-Path $PSScriptRoot '../config/swift.ptq1-export.json') | ConvertFrom-Json -AsHashtable -DateKind String
    if ($derived.SourceSha256 -cne $lock.model.sha256 -or $derived.SourceRevision -cne $lock.model.revision -or
        $derived.CodecCommit -cne $lock.runtime.commit -or -not $derived.ExactPq2ReconstructionVerified) { throw 'Derived weights do not match their locked source/runtime.' }
    $modelFilename = $derived.OutputFilename; $modelBytes = $derived.OutputBytes
}
$modelPath = Resolve-ExternalPath (Join-Path $data -ChildPath 'models', $lock.model.revision, $modelFilename)
$controls = @{}
foreach ($key in $contract.controls.Keys) { $controls[$key] = $contract.controls[$key] }
$controls.modelSha256 = $contract.representations[$representation]
$controls.weightRepresentation = $representation
$controlDigest = Get-BonsaiLoraControlDigest $controls
if ($lock.runtime.commit -cne $controls.runtimeCommit -or $adapterLock.adapter.sha256 -cne $controls.adapterSha256) { throw 'Runtime/adapter and qualification controls disagree.' }
if (-not ('SwiftBonsai.OwnedProcess' -as [type])) {
    Add-Type -Path (Join-Path $PSScriptRoot 'Bonsai.Process.cs') -CompilerOptions @('/nullable:enable', '/checked+', '/unsafe-', '/warn:9999', '/warnaserror+', '/langversion:14.0')
}
$scratch = Resolve-ExternalPath (Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-LoraQualification-' + [guid]::NewGuid().ToString('N')))
[IO.Directory]::CreateDirectory($scratch) | Out-Null
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
function Invoke-PairedSample {
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseShouldProcessForStateChangingFunctions', '', Justification = 'This explicit CI-only evaluation owns and always terminates its bounded child.')]
    [CmdletBinding()]
    [OutputType([pscustomobject])]
    param([string]$Id, [string]$Prompt, [ValidateSet('baseline', 'zero', 'adapter')][string]$Arm)
    $lane = Resolve-ExternalPath (Join-Path $scratch $Id)
    [IO.Directory]::CreateDirectory($lane) | Out-Null
    $owned = $null
    $watch = [Diagnostics.Stopwatch]::StartNew()
    try {
        [long]$minimumAvailable = [SwiftBonsai.PhysicalMemory]::AvailableBytes()
        if ($minimumAvailable -lt 16GB) { throw 'Available physical memory changed before sample admission.' }
        $arguments = @(Get-BonsaiLoraEvaluationArgument -ModelPath $modelPath -AdapterPath 'bonsai-abliterate-lora.gguf' -Arm $Arm -Prompt $Prompt)
        $arguments | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $lane 'arguments.json') -Encoding utf8
        $stdout = Join-Path $lane 'stdout.txt'; $stderr = Join-Path $lane 'stderr.txt'
        $owned = [SwiftBonsai.OwnedProcess]::Start($executable, [string[]]$arguments, $scratch, $stdout, $stderr, 12GB, 1)
        $receipt.ModelProcessStarted = $true
        $process = $owned.Process
        $deadline = [Environment]::TickCount64 + 600000L
        while (-not $process.WaitForExit(1000)) {
            if ([Environment]::TickCount64 -ge $deadline) { throw 'Paired sample exceeded its ten-minute deadline.' }
            $minimumAvailable = [Math]::Min($minimumAvailable, [long][SwiftBonsai.PhysicalMemory]::AvailableBytes())
            if ($minimumAvailable -lt 4GB) { throw 'Paired sample crossed the physical memory reserve.' }
            $process.Refresh()
            if (-not $process.HasExited -and ($process.WorkingSet64 -gt 12GB -or $process.PrivateMemorySize64 -gt 12GB)) { throw 'Paired sample crossed the process memory ceiling.' }
            if ((Get-Item -LiteralPath $stdout).Length -gt 4MB -or (Get-Item -LiteralPath $stderr).Length -gt 64MB) { throw 'Paired sample exceeded the diagnostic output limit.' }
        }
        $watch.Stop()
        if ($process.ExitCode -ne 0) { throw "Paired native sample failed with exit code $($process.ExitCode)." }
        if ((Get-Item -LiteralPath $stdout).Length -gt 4MB -or (Get-Item -LiteralPath $stderr).Length -gt 64MB) { throw 'Paired sample exceeded the diagnostic output limit.' }
        $minimumAvailable = [Math]::Min($minimumAvailable, [long][SwiftBonsai.PhysicalMemory]::AvailableBytes())
        if ($minimumAvailable -lt 4GB) { throw 'Paired sample crossed the physical memory reserve at exit.' }
        $encoding = [Text.UTF8Encoding]::new($false, $true)
        # GetString preserves a leading U+FEFF if generated; ReadAllText would
        # consume its UTF-8 bytes as an encoding preamble and change the capture.
        $text = $encoding.GetString([IO.File]::ReadAllBytes($stdout))
        if ([string]::IsNullOrWhiteSpace($text)) { throw 'Paired native sample returned no captured response.' }
        $logText = [IO.File]::ReadAllText($stderr, $encoding)
        $timing = Read-BonsaiCompletionTiming $logText
        $counts = Read-BonsaiCompletionTokenCount -Text $logText -Timing $timing
        $diagnostics = [ordered]@{ WallMilliseconds = $watch.ElapsedMilliseconds; PromptTokens = $timing.PromptTokens
            PromptMilliseconds = $timing.PromptMilliseconds; DecodeTokensPerSecond = $timing.DecodeTokensPerSecond
            EogMarkerObserved = $text.Contains(' [end of text]'); FirstTokenLatencyMilliseconds = $null
            PromptTokenCount = $counts.PromptTokenCount; SamplerAcceptedTokens = $counts.SamplerAcceptedTokens
            GeneratedSampledTokens = $counts.GeneratedSampledTokens
            LogitsAvailable = $false; ResponseScope = 'Exact native stdout including reasoning, end marker and trailing newlines' }
        $diagnostics | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $lane 'diagnostics.json') -Encoding utf8
        return [pscustomobject]@{ Text = $text; Sha256 = Get-BonsaiResponseDigest $text; Diagnostics = $diagnostics
            Timing = $timing; PeakJobMemoryBytes = [long]$owned.GetPeakJobMemoryBytes(); MinimumAvailablePhysicalBytes = $minimumAvailable }
    } finally {
        try { if ($owned) { $owned.Stop(10000) } } finally {
            if ($owned) { $owned.Dispose() }
        }
    }
}
try {
    $guards.Add([IO.File]::Open($modelPath, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read))
    Test-LockedArtifact $modelPath $modelBytes $controls.modelSha256
    $guards.Add([IO.File]::Open($adapter.AdapterPath, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read))
    Test-LockedArtifact $adapter.AdapterPath $adapterLock.adapter.bytes $controls.adapterSha256
    # One verified scratch copy avoids both the pinned Windows FNAME:SCALE
    # drive-colon parser defect and a separate adapter copy for every sample.
    $copyPath = Join-Path $scratch 'bonsai-abliterate-lora.gguf'
    [IO.File]::Copy($adapter.AdapterPath, $copyPath, $false)
    $guards.Add([IO.File]::Open($copyPath, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read))
    Test-LockedArtifact $copyPath $adapterLock.adapter.bytes $controls.adapterSha256
    foreach ($file in (Get-ChildItem -LiteralPath $runtimeDirectory -File -Recurse | Where-Object Extension -In @('.exe', '.dll'))) {
        $guards.Add([IO.File]::Open($file.FullName, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read))
        $receipt.RuntimeFiles += @{ Path = $file.FullName; Sha256 = (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash.ToLowerInvariant() }
    }
    # Reverify package identity after acquiring read-denying guards.
    $null = & (Join-Path $PSScriptRoot 'Get-Engine.ps1') -DataDirectory $data -LockPath $lockPath -VerifyOnly
    Save-CaptureEvidence
    foreach ($scheduled in $plan.Schedule) {
        $prompt = @($contract.prompts | Where-Object { $_.id -ceq $scheduled.promptId })[0]
        $captured = Invoke-PairedSample -Id $scheduled.sampleId -Prompt $prompt.text -Arm $scheduled.arm
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
        $captured = Invoke-PairedSample -Id ('zero-' + $prompt.id) -Prompt $prompt.text -Arm zero
        $baseline = @($evidence.samples | Where-Object { $_.promptId -ceq $prompt.id -and $_.arm -ceq 'baseline' -and $_.repeat -eq 1 })[0]
        $evidence.zeroScaleChecks += @{ promptId = $prompt.id; seed = 42
            baselineResponseSha256 = $baseline.responseSha256; zeroScaleResponseSha256 = $captured.Sha256
            baselineLogitsSha256 = $null; zeroScaleLogitsSha256 = $null }
        Save-CaptureEvidence
    }
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
