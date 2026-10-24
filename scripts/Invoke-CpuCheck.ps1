#requires -Version 7.6
<#
.SYNOPSIS
Runs one synthetic, trusted-input CPU load/decode check; never enables chat serving.
.DESCRIPTION
Vetted against the pinned tools/completion source and its actual --help. The modern
llama-cli launches a server, so this check uses the in-process llama-completion.
The embedded template's thinking mode and xhigh effort are requested explicitly,
with the publisher sampler. Sixteen generated tokens establish a bounded decode
check, not answer quality or performance.
Memory admission reserves the selected fixed service allowance plus 4 GiB available
headroom. The reference remains 18 GiB; the explicit Swift assessment uses 12 GiB.
A Windows Job enforces job-wide committed memory, one active process,
and kill-on-close ownership from creation. The additional one-second physical
memory watchdog is not a peak measurement. This provides no filesystem, token,
or egress isolation. No applications are closed or system settings changed.
.LINK
https://github.com/ggml-org/llama.cpp/blob/620d2225cc436ab0b001e3200e590e3b286ef9ab/tools/completion/completion.cpp
.LINK
https://learn.microsoft.com/en-us/windows/win32/api/sysinfoapi/nf-sysinfoapi-globalmemorystatusex
#>
[CmdletBinding()]
[OutputType([pscustomobject])]
param(
    [Parameter(Mandatory)][string]$DataDirectory,
    [Alias('Profile')][ValidateSet('reference-cpu', 'swift-pq2-assessment')][string]$ProfileName = 'reference-cpu',
    [switch]$UseAbliterateLora,
    [switch]$UseRepackedPtq1,
    [switch]$CheckOnly,
    [string]$ToolPath
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Tools.psm1')
if (-not $IsWindows -or [Runtime.InteropServices.RuntimeInformation]::OSArchitecture -ne 'Arm64' -or
    [Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture -ne 'Arm64') {
    throw 'Use native ARM64 PowerShell on Windows ARM64.'
}
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1') -Force
$data = Resolve-ExternalPath $DataDirectory
$overrides = @(Get-ChildItem Env: | Where-Object Name -Match '^(LLAMA_|LLAMACPP_|GGML_|AIP_|MTMD_|HF_TOKEN$|HF_ENDPOINT$|MODEL_ENDPOINT$)')
if ($overrides.Count) { throw ('Inherited model/backend overrides are unsupported: ' + ($overrides.Name -join ', ')) }
$cpuPlan = Invoke-BonsaiTool -Command cpu-check-plan -ToolPath $ToolPath -Request @{
    operation = 'plan'; dataDirectory = $data; profile = $ProfileName
    useAbliterateLora = $UseAbliterateLora.IsPresent; useRepackedPtq1 = $UseRepackedPtq1.IsPresent
}
$policy = [pscustomobject]$cpuPlan.Policy
$lockPath = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot $policy.LockRelativePath))
$lock = $cpuPlan.Lock
if ($ProfileName -eq 'reference-cpu') {
    $serverProfile = Get-Content -LiteralPath (Join-Path $PSScriptRoot '../config/profiles/reference-cpu.json') -Raw | ConvertFrom-Json -DateKind String
    if ($serverProfile.serviceCeilingBytes -ne $policy.ServiceCeilingBytes) { throw 'Review this check before changing the reference service allowance.' }
}
$artifact = $cpuPlan.Artifact
$artifactBytes = $artifact.Bytes
$artifactSha256 = $artifact.Sha256
$modelPath = $cpuPlan.ModelPath
$adapterLock = $cpuPlan.AdapterLock
$adapterPath = $cpuPlan.AdapterPath
[long]$ceiling = $policy.ServiceCeilingBytes
[long]$reserve = $policy.AvailableMemoryFloorBytes
[long]$available = $cpuPlan.AvailablePhysicalBytes
$report = [ordered]@{
    SchemaVersion = 1; RecordedAtUtc = [DateTime]::UtcNow.ToString('o'); Profile = $policy.Name
    Status = 'admission-passed'; AvailablePhysicalBytesAtAdmission = $available
    RequiredAvailableBytes = $policy.RequiredAvailableBytes; ServiceCeilingBytes = $ceiling
    AvailableMemoryFloorBytes = $reserve; TokenLimit = $policy.TokenLimit; TimeoutSeconds = $policy.TimeoutSeconds
    ContextTokens = $policy.ContextTokens; BatchTokens = $policy.BatchTokens; MicrobatchTokens = $policy.MicrobatchTokens
    EstimatedWeightBytes = $policy.EstimatedWeightBytes; EstimatedKvBytes = $policy.EstimatedKvBytes
    EstimatedRecurrentBytes = $policy.EstimatedRecurrentBytes; EstimatedTransformBytes = $policy.EstimatedTransformBytes
    EstimatedAdapterBytes = $policy.EstimatedAdapterBytes; AdapterRequested = $UseAbliterateLora.IsPresent
    WeightRepresentation = $policy.WeightRepresentation; ArtifactSha256 = $artifactSha256
    EstimatedTransformConstructionTemporaryBytes = $policy.EstimatedTransformConstructionTemporaryBytes
    EstimatedKnownSubtotalBytes = $policy.EstimatedKnownSubtotalBytes
    EstimatedRuntimeHeadroomBytes = $policy.EstimatedRuntimeHeadroomBytes; EstimateScope = $policy.EstimateScope
    ProcessOwnership = 'windows-job-at-creation-kill-on-close'; JobCommittedMemoryLimitBytes = $ceiling
    ActiveProcessLimit = 1; BreakawayAllowed = $false
    ModelRevision = $lock.model.revision; RuntimeCommit = $lock.runtime.commit
    ModelPath = $modelPath; AdapterPath = $adapterPath; EffectiveArguments = [string[]]$cpuPlan.Arguments
    ModelProcessStarted = $false; Qualification = 'trusted-input-development-only; no daily security or performance acceptance'
}
if ($UseAbliterateLora) {
    $report['AdapterRevision'] = $adapterLock.adapter.revision
    $report['AdapterSha256'] = $adapterLock.adapter.sha256
    $report['AdapterScale'] = 1.0
    $report['AdapterQualification'] = 'experimental; native loading/decode is not behavioral, creative quality or performance acceptance'
}
if ($available -lt $report.RequiredAvailableBytes) {
    $report.Status = 'not-run-insufficient-available-memory'
    $report['Action'] = 'Free memory by closing applications you choose, then rerun. This check will not close them.'
}
if ($CheckOnly -or $report.Status -ne 'admission-passed') { return [pscustomobject]$report }

$runtime = & (Join-Path $PSScriptRoot 'Get-Engine.ps1') -DataDirectory $data -LockPath $lockPath -VerifyOnly -ToolPath $ToolPath
if ($runtime.Commit -cne $lock.runtime.commit) { throw 'Installed runtime and compiled CPU plan disagree.' }
if ($UseAbliterateLora) {
    $adapter = & (Join-Path $PSScriptRoot 'Get-Adapter.ps1') -DataDirectory $data -VerifyOnly
    if ($adapter.AdapterPath -cne $adapterPath -or $adapter.Sha256 -cne $adapterLock.adapter.sha256) { throw 'Verified adapter and compiled CPU plan disagree.' }
}
$executable = Resolve-ExternalPath (Join-Path ([IO.Path]::GetDirectoryName($runtime.ServerPath)) 'llama-completion.exe')
Assert-Arm64PE $executable
$scratch = Resolve-ExternalPath (Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-CpuCheck-' + [Guid]::NewGuid().ToString('N')))
[void][IO.Directory]::CreateDirectory($scratch)
$arguments = [string[]]$cpuPlan.Arguments
$artifacts = @(@{ path = $modelPath; bytes = [long]$artifactBytes; sha256 = $artifactSha256 })
if ($UseAbliterateLora) {
    $artifacts += @{ path = $adapterPath; bytes = [long]$adapterLock.adapter.bytes; sha256 = $adapterLock.adapter.sha256 }
}
$report['EvidenceDirectory'] = $scratch
try {
    $supervision = Invoke-BonsaiTool -Command completion -ToolPath $ToolPath -Request @{
        schemaVersion = 1; executable = $executable; arguments = $arguments
        workingDirectory = $scratch; evidenceDirectory = $scratch; artifacts = $artifacts; runtime = $runtime.Runtime
        memoryLimitBytes = $ceiling; requiredAvailableBytes = [long]$report.RequiredAvailableBytes
        physicalReserveBytes = $reserve; timeoutSeconds = [int]$report.TimeoutSeconds
        stdoutLimitBytes = $null; stderrLimitBytes = $null; requireNonemptyStdout = $true; ciOnly = $false
    }
    $report.ModelProcessStarted = $supervision.ModelProcessStarted
    $report['ExitCode'] = $supervision.ExitCode
    $report['WindowsReportedPeakJobMemoryBytes'] = $supervision.PeakJobMemoryBytes
    $report.Status = 'bounded-load-and-decode-completed'
    $report | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $scratch 'result.json')
    return [pscustomobject]$report
} catch {
    $failure = $_
    $report.Status = 'failed'
    $report['Failure'] = $failure.Exception.Message
    $supervisionPath = Join-Path $scratch 'supervision.json'
    if ([IO.File]::Exists($supervisionPath)) {
        $supervision = Get-Content -Raw -LiteralPath $supervisionPath | ConvertFrom-Json -AsHashtable -DateKind String
        $report.ModelProcessStarted = $supervision.ModelProcessStarted
        if ($supervision.ContainsKey('PeakJobMemoryBytes')) { $report['WindowsReportedPeakJobMemoryBytes'] = $supervision.PeakJobMemoryBytes }
        if ($supervision.ContainsKey('CancellationFailure')) { $report['CancellationFailure'] = $supervision.CancellationFailure }
    }
    $report | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $scratch 'result.json')
    Write-Warning "CPU check failure evidence retained at $scratch"
    throw $failure
}
