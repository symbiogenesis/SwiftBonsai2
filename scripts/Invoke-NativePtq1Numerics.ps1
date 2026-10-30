#requires -Version 7.6
<# .SYNOPSIS
Captures a fixed two-token PTQ1 native/Rust numerical comparison on owner CI.
.DESCRIPTION
Loads only the exact current-weight export and approved adapter. The C++ reference
uses the unchanged released DLLs and exact pinned public headers; ordinary serving
remains Rust. No timings, quality or authoring-basis qualification is inferred.
PlanOnly starts no process. Actual model execution requires this repository main CI.
#>
[CmdletBinding()]
[OutputType([pscustomobject])]
param([Parameter(Mandatory)][string]$DataDirectory, [switch]$PlanOnly, [string]$ToolPath)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1')
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Tools.psm1')
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Native.psm1')
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$pins = Get-Content -LiteralPath (Join-Path $repo 'tools/native-reference/pins.json') -Raw | ConvertFrom-Json -DateKind String
$plan = [pscustomobject]@{
    Status = 'plan-only'; ModelProcessStarted = $false; SuppliedTokens = @(248045, 872)
    Representations = @('current-repacked-PTQ1_0'); AdapterArms = @('none', 'zero', 'scale-1.3')
    RustBackends = @('scalar', 'auto', 'neon'); NativeSamples = 3; RustSamples = 9
    ContextTokens = 8192; JobBytes = 12348030976; AdmissionBytes = 16642998272
    PhysicalReserveBytes = 4294967296; PerSampleSeconds = 600; InitialComputeSeconds = 7200
    EstimatedSnapshotBytes = 3793413516; RequiredFreeDiskBytes = 8589934592; NativeSource = $pins.nativeSource
    RustSource = $pins.rustSource; WriterBasisQualified = $false; DefaultMayChange = $false
}
if ($PlanOnly) { return $plan }
function Get-Ptq1ComputeAllowance {
    <# .SYNOPSIS
    Bounds remaining model/comparison work by the declared initial-compute budget.
    #>
    [CmdletBinding()]
    [OutputType([int])]
    param([Parameter(Mandatory)][Diagnostics.Stopwatch]$Clock)
    $remaining = [int][Math]::Floor(7200 - $Clock.Elapsed.TotalSeconds)
    if ($remaining -lt 1) { throw 'The two-hour PTQ1 initial-compute budget is exhausted.' }
    return $remaining
}
if (-not $IsWindows -or [Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture -ne 'Arm64' -or
    [Runtime.InteropServices.RuntimeInformation]::OSArchitecture -ne 'Arm64' -or
    $env:GITHUB_ACTIONS -cne 'true' -or $env:GITHUB_REF -cne 'refs/heads/main' -or
    $env:GITHUB_REPOSITORY -cne 'symbiogenesis/SwiftBonsai2Runner' -or $env:RUNNER_ARCH -cne 'ARM64' -or
    $env:GITHUB_RUN_ID -cnotmatch '^[0-9]+$' -or $env:GITHUB_SHA -cnotmatch '^[a-f0-9]{40}$' -or
    [string]::IsNullOrWhiteSpace($env:RUNNER_NAME)) { throw 'PTQ1 numerical capture requires this repository main native Windows ARM64 owner CI.' }
foreach ($name in @('ACTIONS_RUNTIME_TOKEN', 'ACTIONS_ID_TOKEN_REQUEST_TOKEN', 'GITHUB_TOKEN', 'GH_TOKEN', 'ACTIONS_RUNNER_INPUT_TOKEN')) {
    if (Test-Path -LiteralPath ('Env:' + $name)) { throw 'PTQ1 numerical capture requires CI credentials omitted from the process tree.' }
}
$processor = @(Get-CimInstance -ClassName Win32_Processor -Property Name -OperationTimeoutSec 10)
if ($processor.Count -ne 1 -or $processor[0].Name -notmatch 'Snapdragon.*X.*Elite') { throw 'Expected one native Snapdragon X/X2 Elite processor.' }
$data = Resolve-ExternalPath $DataDirectory
$evidence = Resolve-ExternalPath (Join-Path ([IO.Path]::GetTempPath()) ('Bonsai-NativePtq1Numerics-' + [guid]::NewGuid().ToString('N')))
[void][IO.Directory]::CreateDirectory($evidence)
if ([IO.DriveInfo]::new([IO.Path]::GetPathRoot($evidence)).AvailableFreeSpace -lt $plan.RequiredFreeDiskBytes) { throw 'PTQ1 numerical capture requires at least 8 GiB external disk headroom.' }
if ([string]::IsNullOrWhiteSpace($env:GITHUB_ENV)) { throw 'Expected the CI environment file.' }
"BONSAI_PTQ1_NUMERICS_EVIDENCE=$evidence" | Add-Content -LiteralPath (Resolve-ExternalPath $env:GITHUB_ENV) -Encoding utf8
$reportPath = Join-Path $evidence 'numerics.json'
$report = [ordered]@{ Status = 'preparing'; Plan = $plan; Revision = $env:GITHUB_SHA
    RunUrl = "https://github.com/symbiogenesis/SwiftBonsai2Runner/actions/runs/$env:GITHUB_RUN_ID"
    RunAttempt = $env:GITHUB_RUN_ATTEMPT; ProcessorName = $processor[0].Name
    ModelTrialsRequested = $false; WriterBasisQualified = $false; DefaultMayChange = $false
    Samples = [Collections.Generic.List[object]]::new(); Comparisons = [Collections.Generic.List[object]]::new()
    IdentityControls = [Collections.Generic.List[object]]::new() }
try {
    $report | ConvertTo-Json -Depth 30 | Set-Content -LiteralPath $reportPath -Encoding utf8
    foreach ($source in @($pins.nativeSource, $pins.rustSource)) {
        $archive = Join-Path $evidence ($source.commit + '.zip')
        Receive-LockedArtifact -Uri $source.url -Path $archive -Bytes $source.bytes -Sha256 $source.sha256 | Out-Null
        Expand-Archive -LiteralPath $archive -DestinationPath (Join-Path $evidence $source.commit)
    }
    $native = Join-Path $evidence ($pins.nativeSource.commit + '/llama.cpp-' + $pins.nativeSource.commit)
    $rust = Join-Path $evidence ($pins.rustSource.commit + '/prism-inference-' + $pins.rustSource.commit)
    $header = Join-Path $native 'include/llama.h'
    if ((Get-FileHash -LiteralPath $header -Algorithm SHA256).Hash.ToLowerInvariant() -cne $pins.nativeSource.headerSha256) { throw 'Pinned native public header identity changed.' }
    $clang = Join-Path $data 'tools/llvm-23.1.4-arm64/clang+llvm-23.1.4-aarch64-pc-windows-msvc/bin/clang-cl.exe'
    if (-not [IO.File]::Exists($clang)) { throw 'Provision the owner-selected native ARM64 LLVM 23.1.4 compiler first.' }
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
    $vs = (& $vswhere -latest -products '*' -version '[18.0,19.0)' -requires Microsoft.VisualStudio.Component.VC.Tools.ARM64 -property installationPath | Out-String).Trim()
    if (-not $vs) { throw 'Visual Studio 2026 ARM64 headers/libraries are required.' }
    $version = (Get-Content -LiteralPath (Join-Path $vs 'VC/Auxiliary/Build/Microsoft.VCToolsVersion.default.txt') -Raw).Trim()
    $vc = Join-Path $vs ('VC/Tools/MSVC/' + $version)
    $sdk = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits/10'
    $savedInclude = $env:INCLUDE; $savedLib = $env:LIB
    $collector = Join-Path $evidence 'swift-ptq1-reference.exe'
    try {
        $env:INCLUDE = "$vc/include;$sdk/Include/10.0.26100.0/ucrt;$sdk/Include/10.0.26100.0/shared;$sdk/Include/10.0.26100.0/um"
        $env:LIB = "$vc/lib/arm64;$sdk/Lib/10.0.26100.0/ucrt/arm64;$sdk/Lib/10.0.26100.0/um/arm64"
        $built = Invoke-BonsaiNativeProcess -Executable $clang -WorkingDirectory $evidence -TimeoutSeconds 120 -LogPath (Join-Path $evidence 'native-build.txt') -ArgumentList @(
            '/nologo', '/std:c++17', '/MD', '/W4', '/WX', '/O2', '/EHsc', '/I', (Join-Path $native 'include'), '/I', (Join-Path $native 'ggml/include'),
            (Join-Path $repo 'tools/native-reference/swift_ptq1_reference.cpp'), "/Fe:$collector", ('/Fo:' + (Join-Path $evidence 'reference.obj')), '/link', '/incremental:no')
        if ($built.ExitCode -ne 0) { throw 'Native reference collector build failed.' }
    } finally { $env:INCLUDE = $savedInclude; $env:LIB = $savedLib }
    $cargo = @(Get-Command cargo -CommandType Application -ErrorAction Stop)[0].Source
    $target = Join-Path $evidence 'rust-build'
    $built = Invoke-BonsaiNativeProcess -Executable $cargo -WorkingDirectory $evidence -TimeoutSeconds 1200 -LogPath (Join-Path $evidence 'rust-build.txt') -ArgumentList @(
        '+nightly-2026-10-22-aarch64-pc-windows-msvc', 'build', '--locked', '--release', '--manifest-path', (Join-Path $rust 'Cargo.toml'),
        '-p', 'prism-inference', '--example', 'swift_model_check', '--target-dir', $target)
    if ($built.ExitCode -ne 0) { throw 'Pinned Rust correctness checker build failed.' }
    $qualifier = Join-Path $target 'release/examples/swift_model_check.exe'
    Assert-Arm64PE $collector
    Assert-Arm64PE $qualifier
    $lock = Read-BonsaiLock (Join-Path $repo 'config/swift-assessment.lock.json')
    $runtime = & (Join-Path $PSScriptRoot 'Get-Engine.ps1') -DataDirectory $data -LockPath (Join-Path $repo 'config/swift-assessment.lock.json') -VerifyOnly -ToolPath $ToolPath
    if ($runtime.RuntimeFiles.Count -ne 63) { throw 'Expected the complete pinned native package inventory.' }
    $artifacts = @($runtime.RuntimeFiles | ForEach-Object { @{ path = $_.Path; bytes = [long]$_.Bytes; sha256 = $_.Sha256 } })
    $artifacts += @{ path = $runtime.ArchivePath; bytes = [long]$lock.runtime.asset.bytes; sha256 = $lock.runtime.asset.sha256 }
    $report['BuildIdentity'] = @{ NativeSource = $pins.nativeSource; RustSource = $pins.rustSource
        RustToolchain = 'nightly-2026-10-22-aarch64-pc-windows-msvc'; VisualCppToolsVersion = $version; WindowsSdkVersion = '10.0.26100.0'
        CompilerSha256 = (Get-FileHash -LiteralPath $clang -Algorithm SHA256).Hash.ToLowerInvariant()
        CollectorSha256 = (Get-FileHash -LiteralPath $collector -Algorithm SHA256).Hash.ToLowerInvariant()
        QualifierSha256 = (Get-FileHash -LiteralPath $qualifier -Algorithm SHA256).Hash.ToLowerInvariant() }
    $preflightLane = Join-Path $evidence 'abi-preflight'; [void][IO.Directory]::CreateDirectory($preflightLane)
    $collectorGuard = @{ path = $collector; bytes = (Get-Item -LiteralPath $collector).Length; sha256 = $report.BuildIdentity.CollectorSha256 }
    $report['AbiPreflight'] = Invoke-BonsaiTool -Command completion -ToolPath $ToolPath -Request @{
        schemaVersion = 1; executable = $collector; arguments = @('--abi-probe', $runtime.Runtime.runtimeDirectory)
        workingDirectory = $preflightLane; evidenceDirectory = $preflightLane; artifacts = @($artifacts) + @($collectorGuard)
        memoryLimitBytes = 536870912; requiredAvailableBytes = $plan.AdmissionBytes; physicalReserveBytes = $plan.PhysicalReserveBytes
        timeoutSeconds = 10; stdoutLimitBytes = 65536; stderrLimitBytes = 65536; requireNonemptyStdout = $true; ciOnly = $false
    }
    $probe = Get-Content -LiteralPath (Join-Path $preflightLane 'stdout.txt') -Raw | ConvertFrom-Json -DateKind String
    if ($probe.status -cne 'native-default-parameter-abi-probe-passed' -or $probe.modelLoaded -or
        $probe.modelParamsBytes -ne 80 -or $probe.contextParamsBytes -ne 168 -or $probe.batchBytes -ne 56) { throw 'Exact native public-header ABI preflight failed.' }
    $report['AbiProbe'] = $probe
    $admission = & (Join-Path $PSScriptRoot 'Invoke-CpuCheck.ps1') -DataDirectory $data -Profile swift-pq2-assessment -UseAbliterateLora -UseRepackedPtq1 -CheckOnly -ToolPath $ToolPath
    if ($admission.Status -cne 'admission-passed') { throw 'Shared Swift memory admission failed.' }
    $adapter = & (Join-Path $PSScriptRoot 'Get-Adapter.ps1') -DataDirectory $data -VerifyOnly
    $modelPin = Get-Content -LiteralPath (Join-Path $repo 'config/swift.ptq1-export.json') -Raw | ConvertFrom-Json -DateKind String
    $artifacts += @{ path = $admission.ModelPath; bytes = [long]$modelPin.OutputBytes; sha256 = $modelPin.OutputSha256 }
    $artifacts += @{ path = $adapter.AdapterPath; bytes = 9682464; sha256 = $adapter.Sha256 }
    $report.ModelTrialsRequested = $true
    $compute = [Diagnostics.Stopwatch]::StartNew()
    foreach ($backend in @('native', 'scalar', 'auto', 'neon')) {
        foreach ($arm in @('none', 'zero', 'scale-1.3')) {
            $sampleLane = Join-Path $evidence ($backend + '-' + $arm); [void][IO.Directory]::CreateDirectory($sampleLane)
            $scale = if ($arm -ceq 'scale-1.3') { '1.3' } else { '0' }
            if ($backend -ceq 'native') {
                $executable = $collector
                $selectedAdapter = if ($arm -ceq 'none') { 'none' } else { $adapter.AdapterPath }
                $arguments = @('capture', $runtime.Runtime.runtimeDirectory, $admission.ModelPath, $sampleLane, $selectedAdapter, $scale)
            } else {
                $executable = $qualifier
                $arguments = @('--use-repacked-ptq1')
                if ($arm -cne 'none') { $arguments += @('--rank-one-adapter', $adapter.AdapterPath, $scale) }
                $arguments += @('forward', $admission.ModelPath, $sampleLane, $backend, '248045,872')
            }
            $guard = @{ path = $executable; bytes = (Get-Item -LiteralPath $executable).Length; sha256 = (Get-FileHash -LiteralPath $executable -Algorithm SHA256).Hash.ToLowerInvariant() }
            $arguments | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $sampleLane 'arguments.json') -Encoding utf8
            @($artifacts) + @($guard) | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath (Join-Path $sampleLane 'artifacts.json') -Encoding utf8
            $remaining = Get-Ptq1ComputeAllowance -Clock $compute
            $supervision = Invoke-BonsaiTool -Command completion -ToolPath $ToolPath -TimeoutSeconds $remaining -Request @{
                schemaVersion = 1; executable = $executable; arguments = $arguments; workingDirectory = $sampleLane; evidenceDirectory = $sampleLane
                artifacts = @($artifacts) + @($guard); memoryLimitBytes = $plan.JobBytes; requiredAvailableBytes = $plan.AdmissionBytes
                physicalReserveBytes = $plan.PhysicalReserveBytes; timeoutSeconds = [int][Math]::Min($plan.PerSampleSeconds, $remaining)
                stdoutLimitBytes = 4194304; stderrLimitBytes = 67108864; requireNonemptyStdout = $true; ciOnly = $false
            }
            $report.Samples.Add(@{ Backend = $backend; Arm = $arm; Supervision = $supervision })
            $report | ConvertTo-Json -Depth 40 | Set-Content -LiteralPath $reportPath -Encoding utf8
        }
    }
    $python = @(Get-Command python -CommandType Application -ErrorAction Stop)[0].Source
    foreach ($backend in @('native', 'scalar', 'auto', 'neon')) {
        $baseline = Join-Path $evidence ($backend + '-none')
        $zero = Join-Path $evidence ($backend + '-zero')
        $nonzero = Join-Path $evidence ($backend + '-scale-1.3')
        $payloads = @(Get-ChildItem -LiteralPath $baseline -File -Recurse | Where-Object Extension -CIn @('.f32le', '.native-seq'))
        if (-not $payloads.Count) { throw 'Missing complete zero-scale control payloads.' }
        foreach ($file in $payloads) {
            $null = Get-Ptq1ComputeAllowance -Clock $compute
            $relative = [IO.Path]::GetRelativePath($baseline, $file.FullName)
            if ((Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash -cne
                (Get-FileHash -LiteralPath (Join-Path $zero $relative) -Algorithm SHA256).Hash) { throw "Loaded-at-zero logits/state identity failed: $backend / $relative" }
        }
        $changed = @(0, 1 | Where-Object {
            (Get-FileHash -LiteralPath (Join-Path $baseline ("logits-$_.f32le")) -Algorithm SHA256).Hash -cne
                (Get-FileHash -LiteralPath (Join-Path $nonzero ("logits-$_.f32le")) -Algorithm SHA256).Hash
        }).Count -gt 0
        if (-not $changed) { throw 'Nonzero adapter failed to change native/Rust logits.' }
        $report.IdentityControls.Add(@{ Backend = $backend; LoadedZeroLogitsAndStateByteExact = $true; NonzeroLogitsChanged = $true; PayloadFiles = $payloads.Count })
    }
    foreach ($backend in @('scalar', 'auto', 'neon')) {
        foreach ($arm in @('none', 'zero', 'scale-1.3')) {
            $output = Join-Path $evidence ($backend + '-' + $arm + '-comparison.json')
            $remaining = Get-Ptq1ComputeAllowance -Clock $compute
            $compared = Invoke-BonsaiNativeProcess -Executable $python -WorkingDirectory $evidence -TimeoutSeconds ([int][Math]::Min(300, $remaining)) -LogPath (Join-Path $evidence ($backend + '-' + $arm + '-comparison.txt')) -ArgumentList @(
                '-B', (Join-Path $repo 'tools/native-reference/compare.py'), '--native', (Join-Path $evidence ('native-' + $arm)), '--rust', (Join-Path $evidence ($backend + '-' + $arm)), '--backend', $backend, '--output', $output)
            if ($compared.ExitCode -ne 0) { throw 'Same-format native/Rust numerical comparison failed.' }
            $report.Comparisons.Add((Get-Content -LiteralPath $output -Raw | ConvertFrom-Json -AsHashtable -DateKind String))
        }
    }
    $report.Status = 'completed-bounded-two-token-comparisons'
    $report['InitialComputeElapsedSeconds'] = $compute.Elapsed.TotalSeconds
    $report | ConvertTo-Json -Depth 40 | Set-Content -LiteralPath $reportPath -Encoding utf8
    [pscustomobject]@{ Status = $report.Status; EvidenceDirectory = $evidence; WriterBasisQualified = $false; DefaultMayChange = $false }
} catch {
    $report.Status = 'failed'; $report['Failure'] = $_.Exception.Message
    $report | ConvertTo-Json -Depth 40 | Set-Content -LiteralPath $reportPath -Encoding utf8
    Write-Warning "Native PTQ1 numerical evidence retained at $evidence"
    throw
}
