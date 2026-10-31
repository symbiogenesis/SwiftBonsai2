#requires -Version 7.6
<#
.SYNOPSIS
Runs one source-bound native Rust signed-session FD mapping probe in an owned Windows Job.
.DESCRIPTION
Requires a trusted same-run hosted fixture bundle, exact source revision, pinned
runtime files and a pinned signtool. A receipt's HostedBuild assertion alone does
not establish provenance: the caller must verify its CI artifact intake first.
All hashing, catalog verification and native session mapping run under one aggregate
worker deadline with 75 seconds reserved for Job teardown and pipe drainage.
No model, driver installation, trust change or CPU fallback is permitted.
Success establishes one native signed-session FD mapping cycle, never compute or Swift admission.
#>
[CmdletBinding(DefaultParameterSetName = 'Execute')]
[OutputType([Collections.Specialized.OrderedDictionary])]
param(
    [Parameter(Mandatory, ParameterSetName = 'Execute')][string]$PackageDirectory,
    [Parameter(Mandatory, ParameterSetName = 'Execute')][string]$FixtureDirectory,
    [Parameter(Mandatory, ParameterSetName = 'Execute')][string]$OutputDirectory,
    [Parameter(Mandatory, ParameterSetName = 'Execute')][string]$SigntoolPath,
    [Parameter(Mandatory, ParameterSetName = 'Execute')][ValidatePattern('^[a-f0-9]{40}$')][string]$ExpectedRunnerRevision,
    [ValidateRange(1,240)][int]$MaximumSeconds = 120,
    [Parameter(Mandatory, ParameterSetName = 'Plan')][switch]$PlanOnly
)
$clock = [Diagnostics.Stopwatch]::StartNew()
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.HexagonSession.psm1')
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1')
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Native.psm1')
$plan = Get-BonsaiHexagonSessionPlan -MaximumSeconds $MaximumSeconds
if ($PlanOnly) { return $plan }
if (-not $IsWindows -or [Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture -ne 'Arm64' -or
    [Runtime.InteropServices.RuntimeInformation]::OSArchitecture -ne 'Arm64') { throw 'Dispatch requires native Windows ARM64.' }
$PackageDirectory = Resolve-ExternalPath $PackageDirectory
$FixtureDirectory = Resolve-ExternalPath $FixtureDirectory
$SigntoolPath = Resolve-ExternalPath $SigntoolPath
$OutputDirectory = Resolve-ExternalPath $OutputDirectory
if (-not [IO.Directory]::Exists($PackageDirectory) -or -not [IO.Directory]::Exists($FixtureDirectory) -or
    -not [IO.File]::Exists($SigntoolPath) -or [IO.Directory]::Exists($OutputDirectory) -or [IO.File]::Exists($OutputDirectory)) {
    throw 'Use existing checked inputs and a new external evidence directory.'
}
[void][IO.Directory]::CreateDirectory($OutputDirectory)
$reportPath = Join-Path $OutputDirectory 'session.json'
$report = [ordered]@{
    SchemaVersion = 1; Kind = 'runner-hexagon-session'; Outcome = 'pending'; Failure = $null
    ExpectedRunnerRevision = $ExpectedRunnerRevision; MaximumSeconds = $MaximumSeconds; CleanupReserveSeconds = 75
    WorkerAllowanceSeconds = 0; ElapsedMilliseconds = 0; WorkerAttempted = $false; OwnedJobTeardown = 'not-started'
    Worker = $null; MappingCycleVerified = $false; ModelLoaded = $false; SwiftQualified = $false; ProfileEligible = $false
    DriverInstalled = $false; TrustChanged = $false
}
try {
    $allowance = [int][Math]::Floor($MaximumSeconds - $clock.Elapsed.TotalSeconds - 75)
    if ($allowance -lt 1) { $report.Failure = 'insufficient-owned-cleanup-reserve'; return $report }
    $repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
    $request = @{
        PackageDirectory = $PackageDirectory; FixtureDirectory = $FixtureDirectory
        SigntoolPath = $SigntoolPath; ExpectedRunnerRevision = $ExpectedRunnerRevision
        LockPath = Join-Path $repo 'config/hexagon-dispatch.lock.json'
        NativeSourceDirectory = Join-Path $repo 'tools/hexagon/native_rpc'
        ManifestPath = Join-Path $repo 'tools/hexagon/diagnostic.manifest'
        WorkerReceiptPath = Join-Path $OutputDirectory 'worker.json'
    }
    $requestPath = Join-Path $OutputDirectory 'worker-request.json'
    [IO.File]::WriteAllText($requestPath, ($request | ConvertTo-Json -Depth 5))
    $modulePath = Join-Path $PSScriptRoot 'Bonsai.HexagonSession.psm1'
    $command = "`$ErrorActionPreference = 'Stop'; Import-Module '" + $modulePath.Replace("'", "''") + "'; " +
        "`$r = ConvertFrom-Json -InputObject ([IO.File]::ReadAllText('" + $requestPath.Replace("'", "''") + "')) -AsHashtable -NoEnumerate -DateKind String; " +
        'Invoke-BonsaiHexagonSessionWorker -Request $r | ConvertTo-Json -Depth 16 -Compress'
    $report.WorkerAllowanceSeconds = $allowance
    $report.WorkerAttempted = $true
    $report.OwnedJobTeardown = 'unconfirmed'
    # The existing trusted trampoline waits for Job assignment before starting
    # this model-free worker. No uncontained retry is available.
    $owned = Invoke-BonsaiNativeProcess -Executable (Join-Path $PSHOME 'pwsh.exe') -ArgumentList @('-NoLogo', '-NoProfile', '-NonInteractive', '-EncodedCommand',
        [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($command))) -WorkingDirectory $OutputDirectory -TimeoutSeconds $allowance -OwnDescendants
    $report.OwnedJobTeardown = 'confirmed-empty'
    if ($owned.ExitCode -ne 0 -or $owned.StandardOutput.Length -gt 131072 -or $owned.StandardError.Length -gt 65536) {
        throw 'The owned worker failed its bounded output contract.'
    }
    $worker = Get-BonsaiHexagonSessionWorkerResult -Json $owned.StandardOutput -ExpectedRunnerRevision $ExpectedRunnerRevision
    $report.Worker = $worker
    $report.Outcome = $worker.Outcome
    $report.Failure = $worker.Failure
    $report.MappingCycleVerified = $worker.MappingCycleVerified
} catch {
    $report.Outcome = 'pending'
    $report.Failure = 'owned-session-probe-incomplete'
    # A killed worker may already have verified signatures or even completed
    # numeric execution. Keep those observations separate from owned teardown.
    $partialPath = Join-Path $OutputDirectory 'worker.json'
    if ([IO.File]::Exists($partialPath)) {
        $partial = [IO.FileStream]::new($partialPath, 'Open', 'Read', 'Read')
        try {
            if ($partial.Length -le 65536) {
                $buffer = [byte[]]::new([int]$partial.Length)
                $partial.ReadExactly($buffer)
                $report.Worker = Get-BonsaiHexagonSessionWorkerResult -Json ([Text.UTF8Encoding]::new($false, $true).GetString($buffer)) -ExpectedRunnerRevision $ExpectedRunnerRevision
            }
        } catch { $report.Worker = $null } finally { $partial.Dispose() }
    }
} finally {
    $report.ElapsedMilliseconds = $clock.ElapsedMilliseconds
    $bytes = [Text.UTF8Encoding]::new($false).GetBytes(($report | ConvertTo-Json -Depth 20) + "`n")
    $output = [IO.FileStream]::new($reportPath, 'CreateNew', 'Write', 'None')
    try { $output.Write($bytes); $output.Flush($true) } finally { $output.Dispose() }
}
return $report
