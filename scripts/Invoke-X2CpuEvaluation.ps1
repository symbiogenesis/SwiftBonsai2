#requires -Version 7.6
<#
.SYNOPSIS
Captures bounded paired X2 Elite Extreme CPU thread trials on main CI.
.DESCRIPTION
Uses already provisioned pinned Swift PQ2, the approved LoRA at scale 1.3 and
the publisher sampler. A fixed 4/4 decode/prefill control is paired against at
most three eligible thread budgets, with reversed order on the second repeat.
Short and long synthetic prompts retain separate prefill/decode/process timings.
The native supervisor owns fresh processes, artifact handles, caller lifetime,
180-second trial deadlines, 11.5 GiB Job commit limits and a 4 GiB physical reserve.
PlanOnly observes hardware and builds a plan without loading a model. CLI template
defaults differ from chat; results do not promote defaults or qualify quality,
energy, sustained comfort, GPU or NPU execution.
ObserveOutputProgress enables a separate attribution capture using existing
capped-stdout checks. It suppresses all timing ratios and does not identify
startup/prefill/decode phases or first-token latency.
Execution first checks the verified completion executable with a model-free
--version invocation under the required worker policy. A failed check retains
its receipt and stops before model verification or thread trials.
#>
[CmdletBinding()]
[OutputType([pscustomobject])]
param(
    [Parameter(Mandatory)][string]$DataDirectory,
    [string]$ToolPath,
    [switch]$PlanOnly,
    [switch]$ObserveOutputProgress
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1') -Force
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Tools.psm1')
if (-not $IsWindows -or
    [Runtime.InteropServices.RuntimeInformation]::OSArchitecture -ne 'Arm64' -or
    [Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture -ne 'Arm64') {
    throw 'X2 CPU evaluation requires native Windows ARM64 PowerShell.'
}
$data = Resolve-ExternalPath $DataDirectory
$processors = @(Get-CimInstance -ClassName Win32_Processor -Property Name, NumberOfLogicalProcessors -OperationTimeoutSec 10)
$native = Invoke-BonsaiTool -Command host-cpu -ToolPath $ToolPath
if ($processors.Count -ne 1 -or $native.status -cne 'observed' -or
    $native.architecture -cne 'aarch64' -or
    $processors[0].NumberOfLogicalProcessors -ne $native.topology.logical_processors) {
    throw 'CPU evaluation requires matching observed CIM and native CPU topology.'
}
$initialConditions = Invoke-BonsaiTool -Command cpu-evaluation -ToolPath $ToolPath -Request @{
    operation = 'compare-conditions'; before = $native; after = $native
}
if ('host-identity-unavailable' -cin $initialConditions.Unknown -or
    $processors[0].Name.Trim() -cne $native.host_identity.processor_name.Trim()) {
    throw 'CPU evaluation requires complete native host identity and matching CIM/native processor names.'
}
$plan = Invoke-BonsaiTool -Command cpu-evaluation -ToolPath $ToolPath -Request @{
    operation = 'plan'; dataDirectory = $data; processorName = $processors[0].Name
    logicalProcessors = [int]$native.topology.logical_processors
}
$plan['HostIdentity'] = $native.host_identity
$plan['InstrumentedAttribution'] = [bool]$ObserveOutputProgress
if ($PlanOnly) { return [pscustomobject]$plan }
if ($env:GITHUB_ACTIONS -cne 'true' -or $env:GITHUB_REF -cne 'refs/heads/main' -or
    $env:GITHUB_REPOSITORY -cne 'symbiogenesis/SwiftBonsai2Runner' -or
    $env:RUNNER_ARCH -cne 'ARM64' -or $env:GITHUB_RUN_ID -cnotmatch '^[0-9]+$' -or
    [string]::IsNullOrWhiteSpace($env:RUNNER_NAME) -or
    $env:GITHUB_SHA -cnotmatch '^[a-f0-9]{40}$') {
    throw 'All CPU performance trials require this repository main CI on the identified native Snapdragon runner.'
}
$overrides = @(Get-ChildItem Env: | Where-Object Name -Match '^(LLAMA_|LLAMACPP_|GGML_|AIP_|MTMD_|HF_TOKEN$|HF_ENDPOINT$|MODEL_ENDPOINT$)')
if ($overrides.Count) { throw ('Inherited model/backend overrides are unsupported: ' + ($overrides.Name -join ', ')) }
foreach ($name in @('ACTIONS_RUNTIME_TOKEN', 'ACTIONS_ID_TOKEN_REQUEST_TOKEN', 'GITHUB_TOKEN', 'GH_TOKEN', 'ACTIONS_RUNNER_INPUT_TOKEN')) {
    if (Test-Path -LiteralPath ('Env:' + $name)) { throw 'CPU evaluation requires CI credentials omitted from the model process environment.' }
}
$cpu = $plan.CpuPlan
$admission = & (Join-Path $PSScriptRoot 'Invoke-CpuCheck.ps1') -DataDirectory $data -Profile swift-pq2-assessment -UseAbliterateLora -CheckOnly -ToolPath $ToolPath
if ($admission.Status -cne 'admission-passed') { throw 'The shared 15.5 GiB available-memory admission requirement was not met.' }
$runtime = & (Join-Path $PSScriptRoot 'Get-Engine.ps1') -DataDirectory $data -LockPath (Join-Path $PSScriptRoot '../config/swift-assessment.lock.json') -VerifyOnly -ToolPath $ToolPath
$adapter = & (Join-Path $PSScriptRoot 'Get-Adapter.ps1') -DataDirectory $data -VerifyOnly
if ($runtime.Commit -cne $cpu.Lock.runtime.commit -or
    $runtime.Runtime.archiveSha256 -cne $cpu.Lock.runtime.asset.sha256 -or
    $runtime.Runtime.archiveBytes -ne $cpu.Lock.runtime.asset.bytes -or
    $adapter.AdapterPath -cne $cpu.AdapterPath -or
    $adapter.Sha256 -cne $cpu.AdapterLock.adapter.sha256) { throw 'Verified runtime/adapter and compiled evaluation plan disagree.' }
$evidence = Resolve-ExternalPath (Join-Path ([IO.Path]::GetTempPath()) ('Bonsai-X2CpuEvaluation-' + [guid]::NewGuid().ToString('N')))
[void][IO.Directory]::CreateDirectory($evidence)
if ([string]::IsNullOrWhiteSpace($env:GITHUB_ENV) -or $evidence.Contains("`n") -or $evidence.Contains("`r")) {
    throw 'Expected the CI environment file and a single-line evidence path.'
}
"BONSAI_X2_CPU_EVIDENCE=$evidence" | Add-Content -LiteralPath (Resolve-ExternalPath $env:GITHUB_ENV) -Encoding utf8
$reportPath = Join-Path $evidence 'cpu-evaluation.json'
$report = [ordered]@{
    SchemaVersion = 1; Status = 'preparing'; Revision = $env:GITHUB_SHA
    RunUrl = "https://github.com/symbiogenesis/SwiftBonsai2Runner/actions/runs/$env:GITHUB_RUN_ID"
    RunAttempt = $env:GITHUB_RUN_ATTEMPT; Runner = $env:RUNNER_NAME
    ProcessorName = $processors[0].Name; DeviceModel = $plan.DeviceModel
    OperatingSystem = [Environment]::OSVersion.Version.ToString()
    OsArchitecture = [Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
    ProcessArchitecture = [Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture.ToString()
    ProcessIsa = @{
        Source = '.NET System.Runtime.Intrinsics.Arm.AdvSimd/Dp.IsSupported'; Scope = 'current-process'
        AdvSimd = [Runtime.Intrinsics.Arm.AdvSimd]::IsSupported
        DotProd = [Runtime.Intrinsics.Arm.Dp]::IsSupported
    }
    StartedUtc = [DateTime]::UtcNow.ToString('o'); NativeCpu = $native; Plan = $plan
    TimingDefinitions = @{
        PromptMilliseconds = 'Pinned native prompt eval; actual prompt token count retained.'
        DecodeMilliseconds = 'Pinned native eval time; count excludes first sampled token.'
        WallMilliseconds = 'Owned process creation through exit and final supervision; includes model load; excludes artifact verification and CPU/power boundary observations.'
        Memory = 'ProcessMemory retains sampled private/current working-set maxima and OS lifetime peak working set from existing one-second safety queries plus final exit. Optional OutputProgress partitions current samples around the first observed nonempty capped stdout and root exit; these are file/exit observations, not startup/prefill/decode phases or first-token latency. Sampled maxima can miss brief peaks. Working set includes mapped residency; Job commit excludes it. These domains overlap and must not be added. No energy measurement.'
    }
    Qualification = 'screen-only'; DefaultProfileChanged = $false
    InstrumentedAttribution = [bool]$ObserveOutputProgress
    ModelTrialsRequested = $false
    WorkerPreflight = [ordered]@{
        Kind = 'model-free-worker-startup'; Status = 'pending'; Arguments = @('--version')
        ModelLoaded = $false; WorkerPolicy = 'required-headless'
    }
    Records = [Collections.Generic.List[object]]::new()
    Comparisons = [Collections.Generic.List[object]]::new()
}
try {
    $plan | ConvertTo-Json -Depth 64 | Set-Content -LiteralPath (Join-Path $evidence 'plan.json') -Encoding utf8
    $native | ConvertTo-Json -Depth 32 | Set-Content -LiteralPath (Join-Path $evidence 'native-cpu.json') -Encoding utf8
    $report | ConvertTo-Json -Depth 64 | Set-Content -LiteralPath $reportPath -Encoding utf8
    $preflightLane = Join-Path $evidence 'worker-preflight'
    [void][IO.Directory]::CreateDirectory($preflightLane)
    $report.WorkerPreflight.Status = 'running'
    $report | ConvertTo-Json -Depth 64 | Set-Content -LiteralPath $reportPath -Encoding utf8
    # Use the ordinary headless worker, not the relaxed inspection launch.
    # ciOnly is false solely to skip model timing parsing for this fixed command;
    # the enclosing script already enforced main/device CI before reaching here.
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
    $report.WorkerPreflight.Status = 'passed'
    $report.WorkerPreflight['Supervision'] = $preflight
    $report | ConvertTo-Json -Depth 64 | Set-Content -LiteralPath $reportPath -Encoding utf8
    $adapterCopy = Join-Path $evidence 'bonsai-abliterate-lora.gguf'
    [IO.File]::Copy($adapter.AdapterPath, $adapterCopy, $false)
    $samples = [Collections.Generic.List[object]]::new()
    foreach ($trial in $plan.Schedule) {
        $lane = Join-Path $evidence $trial.Id
        [void][IO.Directory]::CreateDirectory($lane)
        $trial.Arguments | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $lane 'arguments.json') -Encoding utf8
        $samples.Add(@{ id = $trial.Id; arguments = $trial.Arguments; evidenceDirectory = $lane })
    }
    $report.Status = 'running'
    $report.ModelTrialsRequested = $true
    $report | ConvertTo-Json -Depth 64 | Set-Content -LiteralPath $reportPath -Encoding utf8
    $supervision = Invoke-BonsaiTool -Command completion -ToolPath $ToolPath -TimeoutSeconds 0 -Request @{
        schemaVersion = 1; executable = Join-Path $runtime.Runtime.runtimeDirectory 'llama-completion.exe'
        arguments = @(); samples = $samples.ToArray(); workingDirectory = $evidence; evidenceDirectory = $evidence
        runtime = $runtime.Runtime
        artifacts = @(
            @{ path = $cpu.ModelPath; bytes = [long]$cpu.Artifact.Bytes; sha256 = $cpu.Artifact.Sha256 },
            @{ path = $adapterCopy; bytes = [long]$cpu.AdapterLock.adapter.bytes; sha256 = $cpu.AdapterLock.adapter.sha256 }
        )
        memoryLimitBytes = [long]$cpu.Policy.ServiceCeilingBytes
        requiredAvailableBytes = [long]$cpu.Policy.RequiredAvailableBytes
        physicalReserveBytes = [long]$cpu.Policy.AvailableMemoryFloorBytes
        timeoutSeconds = [int]$plan.PerSampleTimeoutSeconds
        stdoutLimitBytes = 4194304; stderrLimitBytes = 67108864; requireNonemptyStdout = $true; ciOnly = $true
        observeCpuConditions = $true
        observeOutputProgress = [bool]$ObserveOutputProgress
    }
    foreach ($trial in $plan.Schedule) {
        $sample = @($supervision.Samples | Where-Object Id -CEQ $trial.Id)
        if ($sample.Count -ne 1 -or $sample[0].Status -cne 'completed') { throw 'Completed CPU sample evidence is missing.' }
        $sample = $sample[0]
        $capturePath = Join-Path -Path $evidence -ChildPath $trial.Id -AdditionalChildPath 'capture.json'
        $captured = Get-Content -LiteralPath $capturePath -Raw | ConvertFrom-Json -AsHashtable -DateKind String
        $record = [ordered]@{
            Id = $trial.Id; CandidateId = $trial.CandidateId; Workload = $trial.Workload
            Repetition = $trial.Repetition; Role = $trial.Role; PairPosition = $trial.PairPosition
            DecodeThreads = $trial.DecodeThreads; PrefillThreads = $trial.PrefillThreads
            Timing = $sample.Timing; Counts = $sample.Counts; WallMilliseconds = $sample.WallMilliseconds
            PeakJobMemoryBytes = $sample.PeakJobMemoryBytes; MinimumAvailablePhysicalBytes = $sample.MinimumAvailablePhysicalBytes
            ProcessMemory = $sample.ProcessMemory
            CpuConditions = $sample.CpuConditions
            ResponseSha256 = $captured.Sha256
            ReachedTokenCap = $sample.Counts.GeneratedSampledTokens -eq $plan.GeneratedTokenCap
        }
        $report.Records.Add($record)
    }
    $report['NativeCpuAfter'] = Invoke-BonsaiTool -Command host-cpu -ToolPath $ToolPath
    $report['ExecutionConditions'] = Invoke-BonsaiTool -Command cpu-evaluation -ToolPath $ToolPath -Request @{
        operation = 'compare-conditions'; before = $native; after = $report.NativeCpuAfter
    }
    for ($index = 0; $index -lt $report.Records.Count; $index += 2) {
        $pair = @($report.Records[$index], $report.Records[$index + 1])
        $baseline = @($pair | Where-Object Role -CEQ 'baseline')[0]
        $candidate = @($pair | Where-Object Role -CEQ 'candidate')[0]
        $matchingTokenCounts = $baseline.ReachedTokenCap -and $candidate.ReachedTokenCap -and
            $baseline.Timing.PromptTokens -eq $candidate.Timing.PromptTokens -and
            $baseline.Timing.DecodeTokens -eq $candidate.Timing.DecodeTokens
        $pairConditions = Invoke-BonsaiTool -Command cpu-evaluation -ToolPath $ToolPath -Request @{
            operation = 'compare-sample-conditions'; baseline = $baseline.CpuConditions; candidate = $candidate.CpuConditions
        }
        $comparable = -not $ObserveOutputProgress -and $matchingTokenCounts -and $report.ExecutionConditions.Comparable -and $pairConditions.Comparable
        $report.Comparisons.Add([ordered]@{
            CandidateId = $candidate.CandidateId; Workload = $candidate.Workload; Repetition = $candidate.Repetition
            MatchingTokenCountsAtFixedCap = $matchingTokenCounts
            ExecutionConditionsStatus = $report.ExecutionConditions.Status
            PairExecutionConditions = $pairConditions
            ComparableAtFixedTokenCap = $comparable
            InstrumentedAttribution = [bool]$ObserveOutputProgress
            CandidateOverBaselinePromptMilliseconds = if ($comparable) { $candidate.Timing.PromptMilliseconds / $baseline.Timing.PromptMilliseconds } else { $null }
            CandidateOverBaselineDecodeMilliseconds = if ($comparable) { $candidate.Timing.DecodeMilliseconds / $baseline.Timing.DecodeMilliseconds } else { $null }
            CandidateOverBaselineWallMilliseconds = if ($comparable) { $candidate.WallMilliseconds / $baseline.WallMilliseconds } else { $null }
            ExactResponseMatch = $baseline.ResponseSha256 -ceq $candidate.ResponseSha256
        })
    }
    $report.Status = 'completed'
    $report.CompletedUtc = [DateTime]::UtcNow.ToString('o')
    $report | ConvertTo-Json -Depth 64 | Set-Content -LiteralPath $reportPath -Encoding utf8
    [pscustomobject]@{ Status = $report.Status; ReportPath = $reportPath; EvidenceDirectory = $evidence
        Samples = $report.Records.Count; Qualification = $report.Qualification; DefaultProfileChanged = $false }
} catch {
    $report.Status = 'failed'
    if ($report.WorkerPreflight.Status -ceq 'running') { $report.WorkerPreflight.Status = 'failed' }
    $report['Failure'] = $_.Exception.Message
    $report | ConvertTo-Json -Depth 64 | Set-Content -LiteralPath $reportPath -Encoding utf8
    Write-Warning "X2 CPU evaluation failure evidence retained at $evidence"
    throw
}
