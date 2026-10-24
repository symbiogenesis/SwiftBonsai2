#requires -Version 7.6
# Pure completion policy and argument construction; importing never launches a process.
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true

function Get-CpuCheckPolicy {
    <# .SYNOPSIS
    Returns the fixed CPU check limits and source-based memory accounting.
    #>
    [CmdletBinding()]
    [OutputType([pscustomobject])]
    param(
        [Alias('Profile')][ValidateSet('reference-cpu', 'swift-pq2-assessment')][string]$ProfileName = 'reference-cpu',
        [switch]$UseAbliterateLora,
        [switch]$UseRepackedPtq1
    )
    if ($UseAbliterateLora -and $ProfileName -ne 'swift-pq2-assessment') { throw 'The pinned adapter assessment requires the Swift profile.' }
    if ($UseRepackedPtq1 -and $ProfileName -ne 'swift-pq2-assessment') { throw 'The current-weight PTQ1 export requires the Swift profile.' }
    [long]$ceiling = 18GB
    [long]$weights = 13146393504
    [long]$transforms = 0
    [long]$transformTemporary = 0
    $lockPath = '../config/runtime.lock.json'
    $batch = 256
    $microbatch = 128
    if ($ProfileName -eq 'swift-pq2-assessment') {
        $ceiling = 12GB
        $weights = 7206168928
        # One shared 1024-square F32 rotation; three shared F32 sign vectors and
        # retained int32 sign metadata. Construction also briefly stages 4 MiB.
        $transforms = 4L * 1024 * 1024 + 8L * (5120 + 6144 + 17408)
        $transformTemporary = 4MB
        $lockPath = '../config/swift-assessment.lock.json'
        $batch = 128
        $microbatch = 64
    }
    [long]$kv = 8192L * 16 * 4 * 256 * 2 * 2
    [long]$recurrent = 48L * 4 * (128 * 6144 + 3 * (6144 + 2 * 16 * 128))
    [long]$adapterBytes = if ($UseAbliterateLora) { 9682464 } else { 0 }
    if ($UseRepackedPtq1) { $weights = 5946648928 }
    [long]$known = $weights + $kv + $recurrent + $transforms + $adapterBytes
    [pscustomobject]@{
        Name = $ProfileName; LockRelativePath = $lockPath
        ServiceCeilingBytes = $ceiling; AvailableMemoryFloorBytes = 4GB
        RequiredAvailableBytes = $ceiling + 4GB
        ContextTokens = 8192; BatchTokens = $batch; MicrobatchTokens = $microbatch
        TokenLimit = 16; TimeoutSeconds = 600
        EstimatedWeightBytes = $weights; EstimatedKvBytes = $kv
        EstimatedRecurrentBytes = $recurrent; EstimatedTransformBytes = $transforms
        EstimatedAdapterBytes = $adapterBytes
        WeightRepresentation = if ($UseRepackedPtq1) { 'current-repacked-PTQ1_0' } elseif ($ProfileName -eq 'swift-pq2-assessment') { 'PQ2_0' } else { 'reference-mixed' }
        EstimatedTransformConstructionTemporaryBytes = $transformTemporary
        EstimatedKnownSubtotalBytes = $known; EstimatedRuntimeHeadroomBytes = $ceiling - $known
        EstimateScope = 'Whole file plus persistent caches/transforms; temporary construction, tokenizer, graph/scratch and allocation overhead use remaining allowance. Source arithmetic, not measured peak.'
    }
}
function Get-CpuCheckArgument {
    <# .SYNOPSIS
    Builds the bounded CPU check argv without probing artifacts or starting tools.
    #>
    [CmdletBinding()]
    [OutputType([string], [array])]
    param(
        [Parameter(Mandatory)][string]$ModelPath,
        [Alias('Profile')][ValidateSet('reference-cpu', 'swift-pq2-assessment')][string]$ProfileName = 'reference-cpu',
        [switch]$UseAbliterateLora,
        [string]$AdapterPath
    )
    $policy = Get-CpuCheckPolicy -Profile $ProfileName -UseAbliterateLora:$UseAbliterateLora
    if ($UseAbliterateLora.IsPresent -ne (-not [string]::IsNullOrWhiteSpace($AdapterPath))) { throw 'Adapter arguments require the explicit switch and verified adapter path together.' }
    $logVerbosity = if ($ProfileName -eq 'swift-pq2-assessment') { '3' } else { '2' }
    $arguments = @(Get-BonsaiCompletionArgument -ModelPath $ModelPath -Profile $ProfileName) + @(
        '--no-perf', '--log-verbosity', $logVerbosity, '--color', 'off', '--simple-io', '--no-display-prompt',
        '--conversation', '--jinja', '--single-turn', '--n-predict', [string]$policy.TokenLimit,
        '--reasoning', 'on', '--reasoning-effort', 'xhigh', '--reasoning-budget', '-1',
        '--temp', '1.0', '--top-p', '0.95', '--top-k', '20', '--min-p', '0.0',
        '--repeat-penalty', '1.0', '--presence-penalty', '0.0', '--frequency-penalty', '0.0')
    if ($UseAbliterateLora) { $arguments += @('--lora', $AdapterPath) }
    # Swift's template requires a user query. Completion formats an explicit
    # system message on its own before appending the user, which throws. Start
    # this synthetic conversation with the user. Preserve the publisher template
    # and request its supported thinking mode rather than adding a system persona.
    # Sampler/reasoning defaults live here, not in the common prefix: the paired
    # LoRA qualification deliberately uses its own immutable greedy controls.
    $arguments + @('--prompt', 'Reply with the word ready.')
}

function Get-BonsaiCompletionArgument {
    <# .SYNOPSIS
    Builds the common CPU-only completion prefix for checks and paired evaluation.
    #>
    [CmdletBinding()]
    [OutputType([string], [array])]
    param(
        [Parameter(Mandatory)][string]$ModelPath,
        [Alias('Profile')][ValidateSet('reference-cpu', 'swift-pq2-assessment')][string]$ProfileName = 'reference-cpu'
    )
    $policy = Get-CpuCheckPolicy -Profile $ProfileName
    @('--model', $ModelPath, '--ctx-size', [string]$policy.ContextTokens, '--parallel', '1',
        '--cache-type-k', 'f16', '--cache-type-v', 'f16', '--gpu-layers', '0', '--device', 'none',
        '--no-kv-offload', '--no-op-offload', '--fit', 'off', '--threads', '4', '--threads-batch', '4',
        '--poll', '0', '--poll-batch', '0', '--batch-size', [string]$policy.BatchTokens,
        '--ubatch-size', [string]$policy.MicrobatchTokens,
        '--load-mode', 'mmap', '--no-repack', '--no-context-shift', '--offline', '--no-warmup')
}

function Get-BonsaiCompletionArtifact {
    <# .SYNOPSIS
    Selects the locked source or verified PTQ1 identity without reading model bytes.
    #>
    [CmdletBinding()]
    [OutputType([pscustomobject])]
    param(
        [Parameter(Mandatory)][Collections.IDictionary]$Lock,
        [Parameter(Mandatory)][pscustomobject]$Policy,
        [Collections.IDictionary]$Derived
    )
    $artifact = [pscustomobject]@{ Filename = $Lock.model.filename; Bytes = $Lock.model.bytes; Sha256 = $Lock.model.sha256 }
    if ($Policy.WeightRepresentation -ceq 'current-repacked-PTQ1_0') {
        if ($null -eq $Derived -or $Derived.SourceRevision -cne $Lock.model.revision -or
            $Derived.SourceSha256 -cne $Lock.model.sha256 -or $Derived.SourceBytes -ne $Lock.model.bytes -or
            $Derived.CodecCommit -cne $Lock.runtime.commit -or
            $Derived.OutputFilename -cne 'Swift-Bonsai-2-current-repacked-PTQ1_0.gguf' -or
            $Derived.OutputSha256 -cnotmatch '^[a-f0-9]{64}$' -or -not $Derived.ExactPq2ReconstructionVerified) {
            throw 'Derived PTQ1 evidence differs from its pinned source/runtime.'
        }
        $artifact = [pscustomobject]@{ Filename = $Derived.OutputFilename; Bytes = $Derived.OutputBytes; Sha256 = $Derived.OutputSha256 }
    } elseif ($null -ne $Derived) { throw 'Derived weights require the explicit PTQ1 policy.' }
    if ($artifact.Bytes -ne $Policy.EstimatedWeightBytes) { throw 'Review this check before changing the assessment weight estimate.' }
    return $artifact
}

Export-ModuleMember -Function Get-CpuCheckPolicy, Get-CpuCheckArgument, Get-BonsaiCompletionArgument, Get-BonsaiCompletionArtifact
