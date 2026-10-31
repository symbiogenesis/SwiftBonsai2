#requires -Version 7.6
<# .SYNOPSIS
Checks the exact archived hosted publisher candidate on X2.
.DESCRIPTION
Requires an already extracted candidate and prebuilt maintenance tool. Fixed
version/help and seven argument-only probes use the required headless supervisor.
All setup, identity verification, probes and teardown share one bounded budget;
By default this is model-free startup evidence. Explicit ModelSmoke instead runs
one four-token original-PQ2 completion with the reviewed CPU memory policy; it
does not install the candidate or qualify numerics, adapters, quality or speed.
#>
[CmdletBinding()]
[OutputType([pscustomobject])]
param(
    [Parameter(Mandatory)][string]$CandidateDirectory,
    [Parameter(Mandatory)][string]$ToolPath,
    [Parameter(Mandatory)][string]$OutputDirectory,
    [switch]$ModelSmoke,
    [string]$DataDirectory,
    [ValidateRange(45,180)][int]$WorkBudgetSeconds = 90
)
$workClock = [Diagnostics.Stopwatch]::StartNew()
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
if (-not $IsWindows -or [Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture -ne 'Arm64' -or
    [Runtime.InteropServices.RuntimeInformation]::OSArchitecture -ne 'Arm64') {
    throw 'Candidate startup requires native Windows ARM64 PowerShell.'
}
foreach ($name in @('ACTIONS_RUNTIME_TOKEN', 'ACTIONS_ID_TOKEN_REQUEST_TOKEN', 'GITHUB_TOKEN', 'GH_TOKEN', 'ACTIONS_RUNNER_INPUT_TOKEN')) {
    if (Test-Path -LiteralPath ('Env:' + $name)) { throw 'Run the model-free candidate probe without inherited CI credentials.' }
}
if (@(Get-ChildItem Env: | Where-Object Name -Match '^(LLAMA_|LLAMACPP_|GGML_|AIP_|MTMD_|HF_TOKEN$|HF_ENDPOINT$|MODEL_ENDPOINT$)').Count) {
    throw 'Inherited model/backend overrides are unsupported for candidate startup.'
}
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1')
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Tools.psm1')

function Get-HeadlessStartupAllowance {
    <# .SYNOPSIS
    Reserves reporting time after the native aggregate verification/teardown budget.
    #>
    [CmdletBinding()]
    [OutputType([int])]
    param([Diagnostics.Stopwatch]$Clock, [int]$BudgetSeconds)
    $remaining = [int][Math]::Floor($BudgetSeconds - $Clock.Elapsed.TotalSeconds) - 10
    if ($remaining -lt 24) { throw 'Candidate startup budget is exhausted; remaining probes stay pending.' }
    return $remaining
}

function Get-HeadlessStartupPlan {
    <# .SYNOPSIS
    Allows only fixed model-free probes of the complete pinned hosted bundle.
    #>
    [CmdletBinding()]
    [OutputType([hashtable])]
    param([string]$Bundle, [object[]]$PackageFiles)
    if ($PackageFiles.Count -lt 4 -or $PackageFiles.Count -gt 32) { throw 'Invalid candidate package inventory.' }
    $names = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
    $artifacts = foreach ($file in $PackageFiles) {
        if ($file.Name -cnotmatch '^[a-zA-Z0-9_.-]+$' -or -not $names.Add($file.Name) -or
            $file.Bytes -le 0 -or $file.Sha256 -cnotmatch '^[a-f0-9]{64}$') { throw 'Invalid candidate package identity.' }
        $path = Resolve-ExternalPath (Join-Path $Bundle $file.Name)
        $item = Get-Item -LiteralPath $path -Force
        if ($item.PSIsContainer -or $item.Length -ne $file.Bytes) { throw 'A pinned candidate file is missing or has changed length.' }
        @{ path=$path; bytes=$file.Bytes; sha256=$file.Sha256 }
    }
    $actual = @(Get-ChildItem -LiteralPath $Bundle -Force)
    if ($actual.Count -ne $names.Count -or @($actual | Where-Object { $_.PSIsContainer -or -not $names.Contains($_.Name) }).Count) {
        throw 'Candidate bundle contains unpinned entries.'
    }
    foreach ($required in @('llama-completion.exe', 'llama-server.exe', 'llama-bench.exe', 'test-headless-argv.exe')) {
        if (-not $names.Contains($required)) { throw 'A required model-free candidate executable is not pinned.' }
    }
    $cases = @(
        @{ Name='completion-version'; Executable='llama-completion.exe'; Arguments=@('--version') },
        @{ Name='server-version'; Executable='llama-server.exe'; Arguments=@('--version') },
        @{ Name='bench-help'; Executable='llama-bench.exe'; Arguments=@('--help') }
    )
    $vectors = @('', '日本語 🪴 café', 'C:\notes dir\ "quoted" tail\\', "line1`tline2`nline3", '*.gguf', '  leading and trailing  ', 'slashes\\\"quote')
    for ($index=0; $index -lt $vectors.Count; $index++) {
        $cases += @{ Name=('argv-' + $index); Executable='test-headless-argv.exe'
            Arguments=@('--model', '模型/test.gguf', '--seed', [string]$index, '--prompt', $vectors[$index]) }
    }
    return @{ Artifacts=@($artifacts); Cases=$cases }
}

function Get-HeadlessModelSmokePlan {
    <# .SYNOPSIS
    Bounds the explicit candidate smoke to the pinned original PQ2 CPU control.
    #>
    [CmdletBinding()]
    [OutputType([hashtable])]
    param([Collections.IDictionary]$CpuPlan, [Collections.IDictionary]$ModelLock)
    if ($CpuPlan.Policy.Name -cne 'swift-pq2-assessment' -or $null -ne $CpuPlan.AdapterLock -or
        $CpuPlan.Policy.WeightRepresentation -cne 'PQ2_0' -or
        $CpuPlan.Policy.ServiceCeilingBytes -ne 12348030976 -or
        $CpuPlan.Policy.RequiredAvailableBytes -ne 16642998272 -or
        $CpuPlan.Policy.AvailableMemoryFloorBytes -ne 4294967296 -or
        $CpuPlan.Artifact.Bytes -ne $ModelLock.model.bytes -or
        $CpuPlan.Artifact.Sha256 -cne $ModelLock.model.sha256 -or
        $CpuPlan.Lock.runtime.commit -cne $ModelLock.runtime.commit) {
        throw 'Candidate model smoke requires the exact original PQ2 control and unchanged memory policy.'
    }
    foreach ($field in @('repository', 'revision', 'filename', 'bytes', 'sha256', 'architecture')) {
        if ($CpuPlan.Lock.model[$field] -cne $ModelLock.model[$field]) {
            throw 'The compiled CPU model identity differs from the checked-in control.'
        }
    }
    $arguments = [string[]]$CpuPlan.Arguments.Clone()
    $modelIndices = @(for ($index=0; $index -lt $arguments.Count; $index++) {
        if ($arguments[$index] -ceq '--model') { $index }
    })
    if ($modelIndices.Count -ne 1 -or $modelIndices[0] + 1 -ge $arguments.Count -or
        $arguments[$modelIndices[0] + 1] -cne $CpuPlan.ModelPath -or
        @($arguments | Where-Object { $_ -in @('--lora', '--lora-scaled') }).Count) {
        throw 'Candidate smoke arguments must consume only the verified original model.'
    }
    foreach ($change in @(
        @{ Flag='--n-predict'; Before='16'; After='4' },
        @{ Flag='--reasoning'; Before='on'; After='off' }
    )) {
        $indices = @(for ($index=0; $index -lt $arguments.Count; $index++) {
            if ($arguments[$index] -ceq $change.Flag) { $index }
        })
        if ($indices.Count -ne 1 -or $indices[0] + 1 -ge $arguments.Count -or
            $arguments[$indices[0] + 1] -cne $change.Before) {
            throw 'Review candidate smoke arguments after a compiled CPU policy change.'
        }
        $arguments[$indices[0] + 1] = $change.After
    }
    if (@($arguments | Where-Object { $_ -ceq '--no-perf' }).Count -ne 1 -or
        @($arguments | Where-Object { $_ -ceq '--no-display-prompt' }).Count -ne 1) {
        throw 'Candidate smoke requires hidden prompt echo and one removable counter-disable flag.'
    }
    # Keep the pinned logger's counters to prove evaluation occurred. This
    # single smoke produces no performance comparison or throughput verdict.
    $arguments = @($arguments | Where-Object { $_ -cne '--no-perf' })
    $arguments += '--verbose-prompt'
    return @{
        Name='pq2-four-token-smoke'; Executable='llama-completion.exe'; Arguments=$arguments
        Artifact=@{ path=(Resolve-ExternalPath $CpuPlan.ModelPath); bytes=$ModelLock.model.bytes; sha256=$ModelLock.model.sha256 }
    }
}

function Get-HeadlessModelSmokeObservation {
    <# .SYNOPSIS
    Requires actual prompt/decode/sample counters, independently of process creation.
    #>
    [CmdletBinding()]
    [OutputType([hashtable])]
    param([string]$EvidenceDirectory)
    $texts = @{}
    foreach ($name in @('stdout', 'stderr')) {
        $path = Resolve-ExternalPath (Join-Path $EvidenceDirectory ($name + '.txt'))
        $item = Get-Item -LiteralPath $path -Force
        if ($item.PSIsContainer -or $item.Length -gt 65536) { throw 'Candidate model output exceeds the smoke bound.' }
        $texts[$name] = [Text.UTF8Encoding]::new($false, $true).GetString([IO.File]::ReadAllBytes($path))
    }
    # The exact hosted candidate prepends its elapsed timestamp and severity;
    # retain support for the unprefixed pinned logger used by earlier fixtures.
    $prefix = '(?m)^(?:[0-9]+\.[0-9]{2}\.[0-9]{3}\.[0-9]{3} I )?'
    $prompt = [regex]::Matches($texts.stderr, $prefix + '(?:main|llama_completion): number of tokens in prompt = ([0-9]+)\r?$')
    $sampled = [regex]::Matches($texts.stderr, $prefix + 'common_perf_print:[ \t]+samplers time[ \t]*=[^\r\n]*/[ \t]*([0-9]+) tokens[ \t]*\r?$')
    $decoded = [regex]::Matches($texts.stderr, $prefix + 'common_perf_print:[ \t]+eval time[ \t]*=[^\r\n]*/[ \t]*([0-9]+) runs[ \t]+\([^\r\n]+\)\r?$')
    if ($prompt.Count -ne 1 -or $sampled.Count -ne 1 -or $decoded.Count -ne 1 -or
        [string]::IsNullOrWhiteSpace($texts.stdout)) { throw 'Candidate smoke lacks unique prompt, sampler, decode and output evidence.' }
    $inputTokens = [long]$prompt[0].Groups[1].Value
    $sampledTokens = [long]$sampled[0].Groups[1].Value
    $decodeRuns = [long]$decoded[0].Groups[1].Value
    $generated = $sampledTokens - $inputTokens
    if ($inputTokens -lt 2 -or $inputTokens -gt 8192 -or $generated -lt 1 -or $generated -gt 4 -or
        $decodeRuns -lt 1 -or $decodeRuns -gt 4 -or $generated -ne $decodeRuns + 1) { throw 'Candidate smoke counters do not prove bounded inference.' }
    return @{ PromptTokens=$inputTokens; SampledOutputTokens=$generated; DecodeEvaluationRuns=$decodeRuns
        VisibleOutputBytes=[Text.Encoding]::UTF8.GetByteCount($texts.stdout); PerformanceVerdict='not-evaluated' }
}

function Get-HeadlessModelSmokeDeadline {
    <# .SYNOPSIS
    Reserves model hashing and owned teardown before admitting a short worker.
    #>
    [CmdletBinding()]
    [OutputType([int])]
    param([int]$Allowance)
    if ($Allowance -lt 60) { throw 'Candidate model smoke stays pending without model-verification and owned-teardown reserve.' }
    # The first X2 attempt spent about nineteen seconds hashing before launch.
    # Reserve twenty-four seconds for verification, eleven for teardown and
    # one for the native-call margin;
    # do not promise a worker deadline that makes admission impossible.
    return [Math]::Min(30, $Allowance - 36)
}

$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$pinPath = Join-Path $repo 'docs/evidence/headless-publisher-startup.json'
$pin = Get-Content -LiteralPath $pinPath -Raw | ConvertFrom-Json -DateKind String
$source = Get-Content -LiteralPath (Join-Path $repo 'docs/runtime-candidates/headless-ucrt-argv.json') -Raw | ConvertFrom-Json -DateKind String
if ($pin.schemaVersion -ne 1 -or $pin.kind -cne 'publisher-headless-hosted-model-free-startup' -or
    $pin.results.allProbesPassed -cne $true -or $pin.execution.hosted -cne $true -or
    $pin.source.base.commit -cne $source.base.commit -or $pin.source.patch.sha256 -cne $source.patch.sha256 -or
    $pin.results.packageFiles.Count -ne 14) { throw 'The reviewed hosted candidate evidence is incomplete or disagrees with the source candidate.' }
$candidate = Resolve-ExternalPath $CandidateDirectory
$tool = Resolve-ExternalPath $ToolPath
$output = Resolve-ExternalPath $OutputDirectory
$bundle = Resolve-ExternalPath (Join-Path $candidate 'bundle')
if ($output.Equals($candidate, [StringComparison]::OrdinalIgnoreCase) -or
    $output.StartsWith($candidate.TrimEnd('\') + '\', [StringComparison]::OrdinalIgnoreCase)) {
    throw 'Startup evidence must be separate from the archived candidate.'
}
$plan = Get-HeadlessStartupPlan -Bundle $bundle -PackageFiles $pin.results.packageFiles
if ($ModelSmoke.IsPresent -ne (-not [string]::IsNullOrWhiteSpace($DataDirectory))) {
    throw 'ModelSmoke and an existing external DataDirectory must be supplied together.'
}
if (-not [IO.File]::Exists($tool)) { throw 'Provide an existing prebuilt maintenance tool; owner-device builds are disabled.' }
if (Test-Path -LiteralPath $output) { throw 'Use a fresh external startup evidence directory.' }
[void][IO.Directory]::CreateDirectory($output)
$receiptPath = Join-Path $output 'startup.json'
$receipt = [ordered]@{
    SchemaVersion=1; Status='preparing'; Scope=$(if ($ModelSmoke) { 'x2-hosted-candidate-pq2-model-smoke-only' } else { 'x2-hosted-candidate-model-free-startup-only' })
    HostedRunId=$pin.execution.runId; HostedArtifactSha256=$pin.artifact.sha256
    HostedEvidenceSha256=(Get-FileHash -LiteralPath $pinPath -Algorithm SHA256).Hash.ToLowerInvariant()
    StartupScriptSha256=(Get-FileHash -LiteralPath $PSCommandPath -Algorithm SHA256).Hash.ToLowerInvariant()
    Source=$pin.source; PackageFiles=$pin.results.packageFiles
    SupervisorSha256=$null
    WorkerPolicy='ordinary-required-headless'; WorkBudgetSeconds=$WorkBudgetSeconds
    StartedAtUtc=[DateTime]::UtcNow.ToString('o'); RequestedProbes=@($plan.Cases.Name); Probes=@()
    ModelLoaded=$(if ($ModelSmoke) { $null } else { $false })
    ProductionRuntimePinChanged=$false; NumericalOrPerformanceQualified=$false
}
if ($ModelSmoke) { $receipt.RequestedProbes=@('pq2-four-token-smoke') }
$toolGuard = $null
try {
    # Keep the observed supervisor immutable, including its cached version handshake.
    $toolGuard = [IO.FileStream]::new($tool, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
    $receipt.SupervisorSha256=[Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($toolGuard)).ToLowerInvariant()
    $receipt | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $receiptPath -Encoding utf8NoBOM
    $allowance = Get-HeadlessStartupAllowance -Clock $workClock -BudgetSeconds $WorkBudgetSeconds
    $native = Invoke-BonsaiTool -Command host-cpu -ToolPath $tool -TimeoutSeconds ([Math]::Min(10, $allowance))
    $receipt['HostIdentity']=$native
    if ($native.status -cne 'observed' -or $native.architecture -cne 'aarch64' -or
        $native.host_identity.processor_name -cnotmatch '^Snapdragon.* X2') {
        throw 'This owner-device probe requires an observed native Snapdragon X2 identity.'
    }
    $cpuPlan = $null
    if ($ModelSmoke) {
        $allowance = Get-HeadlessStartupAllowance -Clock $workClock -BudgetSeconds $WorkBudgetSeconds
        $cpuPlan = Invoke-BonsaiTool -Command cpu-check-plan -ToolPath $tool -TimeoutSeconds ([Math]::Min(10, $allowance)) -Request @{
            operation='plan'; dataDirectory=(Resolve-ExternalPath $DataDirectory); profile='swift-pq2-assessment'
            useAbliterateLora=$false; useRepackedPtq1=$false
        }
        $modelLock = Read-BonsaiLock -Path (Join-Path $repo 'config/swift-assessment.lock.json')
        if ($modelLock.runtime.commit -cne $pin.source.base.commit) { throw 'Candidate and model-control base revisions disagree.' }
        $case = Get-HeadlessModelSmokePlan -CpuPlan $cpuPlan -ModelLock $modelLock
        $plan.Cases=@($case)
        $plan.Artifacts += $case.Artifact
        $receipt['ModelIdentity']=$modelLock.model
        $receipt['TokenCap']=4
        $receipt['ThinkingRequestedEnabled']=$false
        $receipt['EffectiveThinkingMode']='unqualified-template-behavior'
        $receipt['ConfiguredMemoryPolicy']=$cpuPlan.Policy
        $receipt['NumericalStateAdapterQualityAndPerformanceGates']='pending'
    } else {
        # A wildcard sentinel makes unintended CRT wildcard expansion observable.
        [IO.File]::WriteAllText((Join-Path $output 'literal-wildcard-fixture.gguf'), 'Synthetic argv sentinel; not a model.')
    }
    $receipt.Status='probing'
    foreach ($case in $plan.Cases) {
        $lane = Join-Path $output $case.Name
        [void][IO.Directory]::CreateDirectory($lane)
        $receipt['PendingProbe']=$case.Name
        $receipt | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $receiptPath -Encoding utf8NoBOM
        $allowance = Get-HeadlessStartupAllowance -Clock $workClock -BudgetSeconds $WorkBudgetSeconds
        $deadline = if ($ModelSmoke) { Get-HeadlessModelSmokeDeadline -Allowance $allowance } else { 10 }
        $result = Invoke-BonsaiTool -Command completion -ToolPath $tool -TimeoutSeconds $allowance -Request @{
            schemaVersion=1; executable=(Join-Path $bundle $case.Executable); arguments=$case.Arguments
            workingDirectory=$output; evidenceDirectory=$lane; artifacts=$plan.Artifacts
            memoryLimitBytes=$(if ($ModelSmoke) { $cpuPlan.Policy.ServiceCeilingBytes } else { 536870912 })
            requiredAvailableBytes=$(if ($ModelSmoke) { $cpuPlan.Policy.RequiredAvailableBytes } else { 536870912 })
            physicalReserveBytes=$(if ($ModelSmoke) { $cpuPlan.Policy.AvailableMemoryFloorBytes } else { 0 })
            timeoutSeconds=$deadline; totalTimeoutSeconds=($allowance - 1)
            stdoutLimitBytes=65536; stderrLimitBytes=65536; requireNonemptyStdout=$ModelSmoke.IsPresent; ciOnly=$false
        }
        $receipt.Probes += @{ Name=$case.Name; Arguments=$case.Arguments; Result=$result
            ModelLoaded=$(if ($ModelSmoke) { $null } else { $false })
            InferenceVerification=$(if ($ModelSmoke) { 'pending' } else { 'not-requested' }) }
        # Preserve native completion even when subsequent evidence parsing fails.
        $receipt | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $receiptPath -Encoding utf8NoBOM
        if ($ModelSmoke) {
            if ($result.Status -cne 'completed' -or $result.ExitCode -ne 0) { throw 'Candidate smoke did not complete owned execution.' }
            $receipt['InferenceObservation']=Get-HeadlessModelSmokeObservation -EvidenceDirectory $lane
            $receipt.Probes[-1].InferenceVerification='passed'
            $receipt.Probes[-1].ModelLoaded=$true
            $receipt.ModelLoaded=$true
        }
    }
    $receipt.Remove('PendingProbe')
    $receipt.Status='passed'
} catch {
    $receipt.Status='failed-or-pending'
    throw
} finally {
    try {
        $receipt['ElapsedMilliseconds']=$workClock.ElapsedMilliseconds
        $receipt | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $receiptPath -Encoding utf8NoBOM
    } finally {
        if ($null -ne $toolGuard) { $toolGuard.Dispose() }
    }
}
[pscustomobject]@{ Status=$receipt.Status; EvidenceDirectory=$output; ModelLoaded=$receipt.ModelLoaded; RuntimePinChanged=$false }
