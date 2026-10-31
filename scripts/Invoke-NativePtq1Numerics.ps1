#requires -Version 7.6
<# .SYNOPSIS
Runs one bounded native/Rust Auto PTQ1 correctness smoke on owner CI.
.DESCRIPTION
One supplied token and approved adapter scale 1.3 only. All preparation, loading
and comparison share 240 seconds within the owning job's five-minute hard cap.
Requires hosted-prepared, source/hash-bound capture tools; never builds locally.
No zero, scalar, explicit NEON, writer-basis, quality or performance gate passes.
#>
[CmdletBinding()]
[OutputType([pscustomobject])]
param([Parameter(Mandatory)][string]$DataDirectory, [switch]$PlanOnly,
      [string]$CaptureToolsDirectory, [string]$ToolPath)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
$workClock = [Diagnostics.Stopwatch]::StartNew()
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1')
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Tools.psm1')
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Native.psm1')
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Ptq1Capture.psm1')
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$pins = Get-Content -LiteralPath (Join-Path $repo 'tools/native-reference/pins.json') -Raw | ConvertFrom-Json -DateKind String
$pending = @('no-adapter', 'loaded-zero-identity', 'nonzero-change', 'scalar', 'explicit-neon',
    'writer-authoring-basis', 'quality', 'performance', 'full-context', 'service')
$plan = [pscustomobject]@{
    Status = 'plan-only'; ModelProcessStarted = $false; SuppliedTokens = @(248045)
    Representation = 'current-repacked-PTQ1_0'; AdapterScale = 1.3; RustBackend = 'auto'
    NativeSamples = 1; RustSamples = 1; WorkBudgetSeconds = 240; WholeJobMinutes = 5
    EstimatedSnapshotBytes = 315971136; RequiredFreeDiskBytes = 1073741824
    ContextTokens = 8192; JobBytes = 12348030976; AdmissionBytes = 16642998272
    PhysicalReserveBytes = 4294967296; NativeSource = $pins.nativeSource; RustSource = $pins.rustSource
    PendingControls = $pending; WriterBasisQualified = $false; DefaultMayChange = $false
}
if ($PlanOnly) { return $plan }
function Get-Ptq1WorkAllowance {
    <# .SYNOPSIS
    Caps every external operation by the remaining complete smoke budget.
    #>
    [CmdletBinding()]
    [OutputType([int])]
    param([Parameter(Mandatory)][Diagnostics.Stopwatch]$Clock)
    $remaining = [int][Math]::Floor(240 - $Clock.Elapsed.TotalSeconds)
    if ($remaining -lt 1) { throw 'The 240-second total PTQ1 smoke budget is exhausted.' }
    return $remaining
}
if (-not $IsWindows -or [Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture -ne 'Arm64' -or
    [Runtime.InteropServices.RuntimeInformation]::OSArchitecture -ne 'Arm64' -or
    $env:GITHUB_ACTIONS -cne 'true' -or $env:GITHUB_REF -cne 'refs/heads/main' -or
    $env:GITHUB_REPOSITORY -cne 'symbiogenesis/SwiftBonsai2Runner' -or $env:RUNNER_ARCH -cne 'ARM64' -or
    $env:GITHUB_RUN_ID -cnotmatch '^[0-9]+$' -or $env:GITHUB_SHA -cnotmatch '^[a-f0-9]{40}$' -or
    [string]::IsNullOrWhiteSpace($env:RUNNER_NAME)) { throw 'PTQ1 smoke requires this repository main native Windows ARM64 owner CI.' }
foreach ($name in @('ACTIONS_RUNTIME_TOKEN', 'ACTIONS_ID_TOKEN_REQUEST_TOKEN', 'GITHUB_TOKEN', 'GH_TOKEN', 'ACTIONS_RUNNER_INPUT_TOKEN')) {
    if (Test-Path -LiteralPath ('Env:' + $name)) { throw 'PTQ1 smoke requires CI credentials omitted from the process tree.' }
}
if (-not $CaptureToolsDirectory) { throw 'Provide hosted-prepared CaptureToolsDirectory; owner builds are disabled.' }
if (-not $ToolPath) { $ToolPath = $env:BONSAI_TOOLS_PATH }
if (-not $ToolPath) { throw 'Provide the hosted-prepared maintenance ToolPath; owner builds are disabled.' }
$data = Resolve-ExternalPath $DataDirectory
$bundle = Resolve-ExternalPath $CaptureToolsDirectory
$receiptPath = Join-Path $bundle 'capture-tools.json'
$receipt = Read-BonsaiPtq1CaptureReceipt -BundleDirectory $bundle -RepositoryDirectory $repo -Revision $env:GITHUB_SHA -Pins $pins
$toolGuards = @()
foreach ($name in @('swift-ptq1-reference.exe', 'swift_model_check.exe')) {
    $entry = @($receipt.Files | Where-Object Name -CEQ $name)
    if ($entry.Count -ne 1 -or $entry[0].Bytes -lt 1 -or $entry[0].Bytes -gt 134217728 -or $entry[0].Sha256 -cnotmatch '^[a-f0-9]{64}$') { throw 'Invalid capture tool inventory.' }
    $path = Join-Path $bundle $name
    if ((Get-Item -LiteralPath $path).Length -ne $entry[0].Bytes -or (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant() -cne $entry[0].Sha256) { throw 'Capture tool bytes/hash changed.' }
    Assert-Arm64PE $path
    $toolGuards += @{ path = $path; bytes = [long]$entry[0].Bytes; sha256 = $entry[0].Sha256 }
}
$toolGuards += @{ path = $receiptPath; bytes = (Get-Item -LiteralPath $receiptPath).Length; sha256 = (Get-FileHash -LiteralPath $receiptPath -Algorithm SHA256).Hash.ToLowerInvariant() }
$processor = @(Get-CimInstance -ClassName Win32_Processor -Property Name -OperationTimeoutSec ([int][Math]::Min(10, (Get-Ptq1WorkAllowance $workClock))))
if ($processor.Count -ne 1 -or $processor[0].Name -notmatch 'Snapdragon.*X.*Elite') { throw 'Expected one native Snapdragon X/X2 Elite processor.' }
$evidence = Resolve-ExternalPath (Join-Path ([IO.Path]::GetTempPath()) ('Bonsai-NativePtq1Smoke-' + [guid]::NewGuid().ToString('N')))
[void][IO.Directory]::CreateDirectory($evidence)
if ([IO.DriveInfo]::new([IO.Path]::GetPathRoot($evidence)).AvailableFreeSpace -lt $plan.RequiredFreeDiskBytes) { throw 'The smoke needs at least 1 GiB external disk headroom.' }
if ([string]::IsNullOrWhiteSpace($env:GITHUB_ENV)) { throw 'Expected the CI environment file.' }
"BONSAI_PTQ1_NUMERICS_EVIDENCE=$evidence" | Add-Content -LiteralPath (Resolve-ExternalPath $env:GITHUB_ENV) -Encoding utf8
$reportPath = Join-Path $evidence 'numerics.json'
$report = [ordered]@{ Status = 'preparing'; Plan = $plan; Revision = $env:GITHUB_SHA
    RunUrl = "https://github.com/symbiogenesis/SwiftBonsai2Runner/actions/runs/$env:GITHUB_RUN_ID"
    RunAttempt = $env:GITHUB_RUN_ATTEMPT; ProcessorName = $processor[0].Name
    ModelTrialsRequested = $false; PendingControls = $pending; WriterBasisQualified = $false; DefaultMayChange = $false
    Samples = [Collections.Generic.List[object]]::new() }
try {
    Copy-Item -LiteralPath $receiptPath -Destination (Join-Path $evidence 'capture-tools.json')
    $report | ConvertTo-Json -Depth 30 | Set-Content -LiteralPath $reportPath -Encoding utf8
    $lock = Read-BonsaiLock (Join-Path $repo 'config/swift-assessment.lock.json')
    if ($lock.runtime.commit -cne $pins.nativeSource.commit) { throw 'Native reference runtime differs from the reviewed source.' }
    $runtime = Invoke-BonsaiTool -Command runtime-verify -ToolPath $ToolPath -TimeoutSeconds (Get-Ptq1WorkAllowance $workClock) -Request @{
        schemaVersion = 1; commit = $lock.runtime.commit; runtime = @{
            archivePath = Join-Path $data ('downloads/' + $lock.runtime.asset.filename)
            runtimeDirectory = Join-Path $data ('runtime/' + $lock.runtime.commit)
            archiveBytes = [long]$lock.runtime.asset.bytes; archiveSha256 = $lock.runtime.asset.sha256
        }
    }
    if ($runtime.Status -cne 'verified' -or $runtime.RuntimeFiles.Count -ne 63) { throw 'Expected the complete pinned native package inventory.' }
    $artifacts = @($runtime.RuntimeFiles | ForEach-Object { @{ path = $_.Path; bytes = [long]$_.Bytes; sha256 = $_.Sha256 } }) + @($toolGuards)
    $artifacts += @{ path = $runtime.ArchivePath; bytes = [long]$lock.runtime.asset.bytes; sha256 = $lock.runtime.asset.sha256 }
    $report['RuntimeFiles'] = $runtime.RuntimeFiles
    $preflightLane = Join-Path $evidence 'abi-preflight'; [void][IO.Directory]::CreateDirectory($preflightLane)
    $collector = Join-Path $bundle 'swift-ptq1-reference.exe'
    $qualifier = Join-Path $bundle 'swift_model_check.exe'
    $remaining = Get-Ptq1WorkAllowance $workClock
    $report['AbiPreflight'] = Invoke-BonsaiTool -Command completion -ToolPath $ToolPath -TimeoutSeconds $remaining -Request @{
        schemaVersion = 1; executable = $collector; arguments = @('--abi-probe', $runtime.Runtime.runtimeDirectory)
        workingDirectory = $preflightLane; evidenceDirectory = $preflightLane; artifacts = $artifacts
        memoryLimitBytes = 536870912; requiredAvailableBytes = $plan.AdmissionBytes; physicalReserveBytes = $plan.PhysicalReserveBytes
        timeoutSeconds = [int][Math]::Min(10, $remaining); stdoutLimitBytes = 65536; stderrLimitBytes = 65536; requireNonemptyStdout = $true; ciOnly = $false
    }
    $probe = Get-Content -LiteralPath (Join-Path $preflightLane 'stdout.txt') -Raw | ConvertFrom-Json -DateKind String
    if ($probe.status -cne 'native-default-parameter-abi-probe-passed' -or $probe.modelLoaded -or
        $probe.modelParamsBytes -ne 80 -or $probe.contextParamsBytes -ne 168 -or $probe.batchBytes -ne 56) { throw 'Exact native public-header ABI preflight failed.' }
    $abiLane = Join-Path $evidence 'abi-observations'; [void][IO.Directory]::CreateDirectory($abiLane)
    $remaining = Get-Ptq1WorkAllowance $workClock
    $report['AbiObservations'] = Invoke-BonsaiTool -Command completion -ToolPath $ToolPath -TimeoutSeconds $remaining -Request @{
        schemaVersion = 1; executable = $collector; arguments = @('--abi-observations', $runtime.Runtime.runtimeDirectory)
        workingDirectory = $abiLane; evidenceDirectory = $abiLane; artifacts = $artifacts
        memoryLimitBytes = 536870912; requiredAvailableBytes = $plan.AdmissionBytes; physicalReserveBytes = $plan.PhysicalReserveBytes
        timeoutSeconds = [int][Math]::Min(10, $remaining); stdoutLimitBytes = 65536; stderrLimitBytes = 65536; requireNonemptyStdout = $true; ciOnly = $false
    }
    Assert-BonsaiPtq1AbiObservations -ActualPath (Join-Path $abiLane 'stdout.txt') -ExpectedPath (Join-Path $repo 'tools/native-reference/fixtures/collector-abi-v1.txt') -ExpectedSha256 $receipt.AbiControlSha256
    $checked = Invoke-BonsaiTool -Command cpu-check-report -ToolPath $ToolPath -TimeoutSeconds (Get-Ptq1WorkAllowance $workClock) -Request @{
        operation = 'plan'; dataDirectory = $data; profile = 'swift-pq2-assessment'; useAbliterateLora = $true; useRepackedPtq1 = $true
    }
    $admission = $checked.Report
    if ($admission.Status -cne 'admission-passed') { throw 'Shared Swift memory admission failed.' }
    $adapter = & (Join-Path $PSScriptRoot 'Get-Adapter.ps1') -DataDirectory $data -VerifyOnly
    $artifacts += @{ path = $admission.ModelPath; bytes = 5946648928; sha256 = 'e658f0f0736a03a4549535a439f9b07254ebf8ab88ae27cbe6e52d098ca5d1f5' }
    $artifacts += @{ path = $adapter.AdapterPath; bytes = 9682464; sha256 = 'f1669534803d340a496015f5c45125f3437b4d13ec764f40e34488ce83967f42' }
    foreach ($backend in @('native', 'auto')) {
        $sampleLane = Join-Path $evidence $backend; [void][IO.Directory]::CreateDirectory($sampleLane)
        $executable = if ($backend -ceq 'native') { $collector } else { $qualifier }
        $arguments = if ($backend -ceq 'native') {
            @('capture', $runtime.Runtime.runtimeDirectory, $admission.ModelPath, $sampleLane, $adapter.AdapterPath, '1.3')
        } else { @('--use-repacked-ptq1', '--rank-one-adapter', $adapter.AdapterPath, '1.3', 'forward', $admission.ModelPath, $sampleLane, 'auto', '248045') }
        $arguments | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $sampleLane 'arguments.json') -Encoding utf8
        $artifacts | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath (Join-Path $sampleLane 'artifacts.json') -Encoding utf8
        $remaining = Get-Ptq1WorkAllowance $workClock
        $report.ModelTrialsRequested = $true
        $supervision = Invoke-BonsaiTool -Command completion -ToolPath $ToolPath -TimeoutSeconds $remaining -Request @{
            schemaVersion = 1; executable = $executable; arguments = $arguments; workingDirectory = $sampleLane; evidenceDirectory = $sampleLane
            artifacts = $artifacts; memoryLimitBytes = $plan.JobBytes; requiredAvailableBytes = $plan.AdmissionBytes
            physicalReserveBytes = $plan.PhysicalReserveBytes; timeoutSeconds = $remaining
            stdoutLimitBytes = 65536; stderrLimitBytes = 67108864; requireNonemptyStdout = $true; ciOnly = $false
        }
        $report.Samples.Add(@{ Backend = $backend; Supervision = $supervision })
        $report | ConvertTo-Json -Depth 40 | Set-Content -LiteralPath $reportPath -Encoding utf8
    }
    $python = @(Get-Command python -CommandType Application -ErrorAction Stop)[0].Source
    $output = Join-Path $evidence 'comparison.json'
    $compared = Invoke-BonsaiNativeProcess -Executable $python -WorkingDirectory $evidence -TimeoutSeconds (Get-Ptq1WorkAllowance $workClock) -LogPath (Join-Path $evidence 'comparison.txt') -ArgumentList @(
        '-B', (Join-Path $repo 'tools/native-reference/compare.py'), '--native', (Join-Path $evidence 'native'), '--rust', (Join-Path $evidence 'auto'), '--backend', 'auto', '--output', $output)
    if ($compared.ExitCode -ne 0) { throw 'Native/Rust one-token smoke comparison failed.' }
    $report['Comparison'] = Get-Content -LiteralPath $output -Raw | ConvertFrom-Json -AsHashtable -DateKind String
    $report.Status = 'completed-one-token-smoke'; $report['WorkElapsedSeconds'] = $workClock.Elapsed.TotalSeconds
    $report | ConvertTo-Json -Depth 40 | Set-Content -LiteralPath $reportPath -Encoding utf8
    [pscustomobject]@{ Status = $report.Status; EvidenceDirectory = $evidence; PendingControls = $pending; DefaultMayChange = $false }
} catch {
    $report.Status = 'failed'; $report['Failure'] = $_.Exception.Message; $report['WorkElapsedSeconds'] = $workClock.Elapsed.TotalSeconds
    $report | ConvertTo-Json -Depth 40 | Set-Content -LiteralPath $reportPath -Encoding utf8
    Write-Warning "Native PTQ1 smoke evidence retained at $evidence"
    throw
}
