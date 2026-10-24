#requires -Version 7.6
# Nonlaunching M1 policy preparation; this module is not an HTTP or OS boundary.
# Literature and pre-implementation sanity checks: docs/implementation-research.md.
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1')

function Assert-BonsaiServerEnvironment {
    <#
    .SYNOPSIS
    Rejects inherited model/backend overrides without exposing their values.
    #>
    [CmdletBinding()]
    [OutputType([void])]
    param([Collections.IDictionary]$Variables = [Environment]::GetEnvironmentVariables())
    # AIP and MTMD are read outside the runtime's usual LLAMA/GGML namespaces.
    $overrides = @($Variables.Keys | Where-Object {
        $_ -match '^(LLAMA_|LLAMACPP_|GGML_|AIP_|MTMD_|HF_TOKEN$|HF_ENDPOINT$|MODEL_ENDPOINT$)'
    } | Sort-Object)
    if ($overrides.Count) {
        # Names only: values can contain credentials, prompts, or commands.
        throw ('Inherited server/model overrides are unsupported: ' + ($overrides -join ', '))
    }
}

function New-BonsaiServerPlan {
    <#
    .SYNOPSIS
    Returns a fixed, nonlaunching development server plan after policy validation.
    #>
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseShouldProcessForStateChangingFunctions', '', Justification = 'Returns an in-memory plan; no filesystem, process, or network state is changed.')]
    [CmdletBinding()]
    [OutputType([pscustomobject])]
    param(
        [Parameter(Mandatory)][string]$DataDirectory,
        [ValidateRange(1024,65535)][int]$Port = 8080,
        [Alias('Profile')][Collections.IDictionary]$ServerProfile = (Get-Content -Raw -LiteralPath (Join-Path $PSScriptRoot '../config/profiles/reference-cpu.json') | ConvertFrom-Json -DateKind String -AsHashtable)
    )
    Assert-BonsaiServerEnvironment
    # Reject UNC before filesystem probing; this plan promises local/offline paths.
    if ($DataDirectory -notmatch '^[A-Za-z]:[\\/]') { throw 'Use an absolute local drive data directory.' }
    $drive = [IO.DriveInfo]::new($DataDirectory.Substring(0,3))
    if ($drive.DriveType -notin @([IO.DriveType]::Fixed, [IO.DriveType]::Removable, [IO.DriveType]::Ram)) {
        throw 'Data directory must be on a recognized local disk, not a mapped network or unavailable drive.'
    }
    $data = Resolve-ExternalPath $DataDirectory
    if ($data.TrimEnd('\','/') -eq [IO.Path]::GetPathRoot($data).TrimEnd('\','/')) {
        throw 'Choose a dedicated data directory, not a drive root.'
    }
    $lock = Read-BonsaiLock
    # Any profile drift requires an explicit policy review rather than coercion or
    # silently accepting new launch/extra-argument fields.
    $expected = @{
        schemaVersion = 1; name = 'reference-cpu'; status = 'development-only-unqualified'
        backend = 'cpu'; contextTokens = 8192; parallel = 1
        batchTokens = 256; microbatchTokens = 128; cacheTypeK = 'f16'; cacheTypeV = 'f16'
        contextCheckpoints = 0; promptCacheMiB = 0; cachePrompt = $false
        speculation = 'none'; serviceCeilingBytes = 18GB; estimatedWeightBytes = $lock.model.bytes
        estimatedKvBytes = 536870912; estimatedRecurrentBytes = 156893184
        estimatedRuntimeHeadroomBytes = 5487195232; launchEnabled = $false
    }
    $allowed = @($expected.Keys) + @('estimateScope','remainingGates')
    if ($ServerProfile.Count -ne $allowed.Count -or @($ServerProfile.Keys | Where-Object { $_ -cnotin $allowed }).Count) {
        throw 'Reference profile has missing or unreviewed fields.'
    }
    foreach ($name in $expected.Keys) {
        $actual = $ServerProfile[$name]; $wanted = $expected[$name]
        $valid = if ($wanted -is [bool]) {
            $actual -is [bool] -and $actual -eq $wanted
        } elseif ($wanted -is [string]) {
            $actual -is [string] -and $actual -ceq $wanted
        } else {
            ($actual -is [int] -or $actual -is [long]) -and $actual -eq $wanted
        }
        if (-not $valid) { throw "Unreviewed reference profile value/type: $name" }
    }
    if ($ServerProfile.estimateScope -isnot [string] -or [string]::IsNullOrWhiteSpace($ServerProfile.estimateScope) -or
        $ServerProfile.remainingGates -isnot [array] -or $ServerProfile.remainingGates.Count -eq 0 -or
        @($ServerProfile.remainingGates | Where-Object { $_ -isnot [string] -or [string]::IsNullOrWhiteSpace($_) }).Count) {
        throw 'Reference profile needs explicit estimate scope and remaining gates.'
    }
    $model = Resolve-ExternalPath (Join-Path $data -ChildPath 'models', $lock.model.revision, $lock.model.filename)
    $keyFile = Resolve-ExternalPath (Join-Path $data 'private/server.key')
    $origin = 'http://127.0.0.1:' + $Port.ToString([Globalization.CultureInfo]::InvariantCulture)
    # Values are separate tokens, never a shell command. This is a reviewable
    # candidate only: no key creation, executable invocation, or socket binding.
    $arguments = @('--model', $model, '--host', '127.0.0.1', '--port', [string]$Port,
        '--api-key-file', $keyFile, '--cors-origins', $origin,
        '--cors-methods', 'GET,POST,OPTIONS', '--cors-headers', 'Authorization,Content-Type', '--no-cors-credentials',
        '--no-agent', '--no-ui-mcp-proxy', '--no-ui', '--no-slots', '--no-mmproj',
        '--ctx-size', '8192', '--parallel', '1', '--batch-size', '256', '--ubatch-size', '128',
        '--cache-type-k', 'f16', '--cache-type-v', 'f16', '--ctx-checkpoints', '0',
        '--cache-ram', '0', '--no-cache-prompt', '--no-cache-idle-slots', '--no-context-shift',
        '--spec-type', 'none', '--gpu-layers', '0', '--device', 'none', '--no-kv-offload', '--no-op-offload',
        '--fit', 'off', '--load-mode', 'mmap', '--no-repack', '--offline', '--no-warmup',
        '--threads', '4', '--threads-batch', '4', '--poll', '0', '--poll-batch', '0',
        '--jinja', '--reasoning', 'on', '--reasoning-effort', 'xhigh', '--reasoning-budget', '-1',
        '--temp', '1.0', '--top-p', '0.95', '--top-k', '20',
        '--min-p', '0.0', '--presence-penalty', '0.0', '--frequency-penalty', '0.0', '--repeat-penalty', '1.0',
        '--n-predict', '4096', '--timeout', '60', '--no-perf', '--log-disable')
    [pscustomobject]@{
        SchemaVersion = 1; Status = 'preflight-only-launch-disabled'; LaunchEnabled = $false
        ModelProcessStarted = $false; Profile = $ServerProfile.name; RuntimeCommit = $lock.runtime.commit
        ModelRevision = $lock.model.revision; ModelPath = $model; PlannedKeyFile = $keyFile
        Origin = $origin; Arguments = [string[]]$arguments
        ServiceCeilingBytes = $ServerProfile.serviceCeilingBytes; RequiredAvailableBytes = 22GB
        ArtifactVerification = 'not-performed'; RuntimeHelpVerification = 'not-performed'
        SecurityQualification = 'not-qualified'
        RemainingGates = @(
            'CPU loading/decode with memory admission; current watchdog is not an OS limit',
            'Owner-private unpredictable key creation and authenticated lifecycle/readiness/cancellation',
            'Host/Origin rejection, route allowlist and backend bypass prevention; CORS is not enforcement',
            'Hard request/token/queue/duration limits; n-predict and timeout are defaults, not those limits',
            'Restricted inference identity/filesystem and denied egress; offline is not an OS network boundary',
            'Pinned/sanitized offline UI assets, publisher-template behavior and creative/comfort CI qualification'
        )
    }
}

function Assert-BonsaiServerHelp {
    <#
    .SYNOPSIS
    Verifies the exact planned switches are advertised by pinned server help.
    #>
    [CmdletBinding()]
    [OutputType([void])]
    param([Parameter(Mandatory)][string]$HelpText, [Parameter(Mandatory)][string[]]$Arguments)
    $flags = @($Arguments | Where-Object { $_ -cmatch '^--[a-z]' } | Select-Object -Unique)
    if ($flags.Count -eq 0) { throw 'No server switches to verify.' }
    foreach ($flag in $flags) {
        if ($HelpText -cnotmatch ('(?<![\w-])' + [regex]::Escape($flag) + '(?![\w-])')) {
            throw "Pinned server help does not advertise required switch: $flag"
        }
    }
}

Export-ModuleMember -Function Assert-BonsaiServerEnvironment, New-BonsaiServerPlan, Assert-BonsaiServerHelp
