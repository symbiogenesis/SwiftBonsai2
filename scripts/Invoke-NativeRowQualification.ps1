#requires -Version 7.6
<# .SYNOPSIS
Captures one exact PQ2 token with one then four Rust row workers in owner CI.
.DESCRIPTION
All setup, verification, loading and comparison share 240 seconds inside a
five-minute owning job. Requires hosted-prepared tools; never builds or downloads
models. Raw graph timing does not qualify app TPS or default promotion.
#>
[CmdletBinding()]
[OutputType([pscustomobject])]
param([Parameter(Mandatory)][string]$DataDirectory, [switch]$PlanOnly,
      [string]$ToolPath, [string]$CaptureToolsDirectory)
$workClock = [Diagnostics.Stopwatch]::StartNew()
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$pinsPath = Join-Path $repo 'tools/row-parallel/pins.json'
$source = $null
if ([IO.File]::Exists($pinsPath)) {
    if ((Get-Item -LiteralPath $pinsPath).Length -gt 65536) { throw 'Source pins exceed their pre-read bound.' }
    $source = (Get-Content -LiteralPath $pinsPath -Raw | ConvertFrom-Json -DateKind String).rustSource
}
$plan = [pscustomobject]@{
    Status = 'plan-only'; ModelProcessStarted = $false; Representation = 'PQ2_0'
    SuppliedTokens = @(248045); AdapterScale = 1.3; RustBackend = 'auto'
    Conditions = @(@{ Name = 'serial'; RowWorkers = 1 }, @{ Name = 'parallel'; RowWorkers = 4 })
    WorkBudgetSeconds = 240; WholeJobMinutes = 5; ContextTokens = 8192
    JobBytes = 12348030976; AdmissionBytes = 16642998272; PhysicalReserveBytes = 4294967296
    EstimatedSnapshotBytes = 316035072; RequiredFreeDiskBytes = 1073741824
    RustSource = $source; SourcePinsAvailable = $null -ne $source
    WriterBasisQualified = $false; AppTpsQualified = $false; DefaultMayChange = $false
}
if ($PlanOnly) { return $plan }
if (-not $IsWindows -or [Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture -ne 'Arm64' -or
    [Runtime.InteropServices.RuntimeInformation]::OSArchitecture -ne 'Arm64' -or
    $env:GITHUB_ACTIONS -cne 'true' -or $env:RUNNER_ENVIRONMENT -cne 'self-hosted' -or
    $env:GITHUB_REF -cne 'refs/heads/main' -or $env:GITHUB_REPOSITORY -cne 'symbiogenesis/SwiftBonsai2Runner' -or
    $env:RUNNER_ARCH -cne 'ARM64' -or $env:GITHUB_RUN_ID -cnotmatch '^[0-9]+$' -or
    $env:GITHUB_SHA -cnotmatch '^[a-f0-9]{40}$' -or [string]::IsNullOrWhiteSpace($env:RUNNER_NAME)) {
    throw 'Row qualification requires this repository main native Windows ARM64 owner CI.'
}
foreach ($name in @('ACTIONS_RUNTIME_TOKEN', 'ACTIONS_ID_TOKEN_REQUEST_TOKEN', 'GITHUB_TOKEN', 'GH_TOKEN', 'ACTIONS_RUNNER_INPUT_TOKEN')) {
    if (Test-Path -LiteralPath ('Env:' + $name)) { throw 'Row qualification requires CI credentials omitted from the process tree.' }
}
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1')
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Tools.psm1')
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Native.psm1')
function Get-RowWorkAllowance {
    <# .SYNOPSIS
    Bounds work by the shared clock, reserving the maintenance version handshake.
    #>
    [CmdletBinding()]
    [OutputType([int])]
    param([switch]$ForTool)
    $remaining = [int][Math]::Floor(240 - $workClock.Elapsed.TotalSeconds)
    if ($ForTool) { $remaining -= 10 }
    if ($remaining -lt 1) { throw 'The complete 240-second row qualification budget is exhausted.' }
    return $remaining
}
function Save-RowReport {
    <# .SYNOPSIS
    Flushes partial evidence before atomically replacing the previous report.
    #>
    [CmdletBinding()]
    param([Parameter(Mandatory)][Collections.IDictionary]$Report, [Parameter(Mandatory)][string]$Path)
    $temporary = $Path + '.' + [guid]::NewGuid().ToString('N') + '.tmp'
    $stream = [IO.FileStream]::new($temporary, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
    try {
        $bytes = [Text.UTF8Encoding]::new($false).GetBytes(($Report | ConvertTo-Json -Depth 40) + "`n")
        $stream.Write($bytes, 0, $bytes.Length)
        $stream.Flush($true)
    } finally { $stream.Dispose() }
    [IO.File]::Move($temporary, $Path, $true)
}
$evidence = Resolve-ExternalPath (Join-Path ([IO.Path]::GetTempPath()) ('Bonsai-RowQualification-' + [guid]::NewGuid().ToString('N')))
[void][IO.Directory]::CreateDirectory($evidence)
if ([string]::IsNullOrWhiteSpace($env:GITHUB_ENV)) { throw 'Expected the CI environment file.' }
"BONSAI_ROW_QUALIFICATION_EVIDENCE=$evidence" | Add-Content -LiteralPath (Resolve-ExternalPath $env:GITHUB_ENV) -Encoding utf8
$reportPath = Join-Path $evidence 'qualification.json'
$report = [ordered]@{
    SchemaVersion = 1; Status = 'preparing'; Plan = $plan; Revision = $env:GITHUB_SHA
    RunUrl = "https://github.com/symbiogenesis/SwiftBonsai2Runner/actions/runs/$env:GITHUB_RUN_ID"
    RunAttempt = $env:GITHUB_RUN_ATTEMPT; ModelTrialsRequested = $false
    WriterBasisQualified = $false; AppTpsQualified = $false; DefaultMayChange = $false
    Samples = [Collections.Generic.List[object]]::new()
}
Save-RowReport -Report $report -Path $reportPath
try {
    if ($null -eq $source -or $source.commit -cnotmatch '^[a-f0-9]{40}$' -or $source.sha256 -cnotmatch '^[a-f0-9]{64}$' -or
        $source.bytes -lt 1 -or $source.bytes -gt 67108864 -or
        $source.url -cne "https://github.com/symbiogenesis/prism-inference/archive/$($source.commit).zip") {
        throw 'Reviewed exact Prism source archive pins are required.'
    }
    if (-not $CaptureToolsDirectory) { throw 'Provide hosted-prepared CaptureToolsDirectory; owner builds are disabled.' }
    if (-not $ToolPath) { $ToolPath = $env:BONSAI_TOOLS_PATH }
    if (-not $ToolPath) { throw 'Provide the hosted-prepared checked maintenance ToolPath; owner builds are disabled.' }
    $data = Resolve-ExternalPath $DataDirectory
    $bundle = Resolve-ExternalPath $CaptureToolsDirectory
    $ToolPath = Resolve-ExternalPath $ToolPath
    $receiptPath = Join-Path $bundle 'capture-tools.json'
    if ((Get-Item -LiteralPath $receiptPath).Length -gt 65536) { throw 'Capture receipt exceeds its pre-read bound.' }
    $receipt = Get-Content -LiteralPath $receiptPath -Raw | ConvertFrom-Json -DateKind String
    if ($receipt.SchemaVersion -ne 1 -or $receipt.RunnerRevision -cne $env:GITHUB_SHA -or
        $receipt.RustToolchain -cne 'nightly-2026-10-22-aarch64-pc-windows-msvc' -or
        $receipt.RustSource.commit -cne $source.commit -or $receipt.RustSource.url -cne $source.url -or
        $receipt.RustSource.bytes -ne $source.bytes -or $receipt.RustSource.sha256 -cne $source.sha256 -or
        $receipt.BuildFeatures -isnot [array] -or $receipt.BuildFeatures.Count -ne 1 -or $receipt.BuildFeatures[0] -cne 'parallel-rows' -or
        $receipt.Files -isnot [array] -or $receipt.Files.Count -ne 1) { throw 'Hosted row checker differs from reviewed source/revision/feature pins.' }
    $entry = $receipt.Files[0]
    if ($entry.Name -cne 'swift_model_check.exe' -or $entry.Bytes -lt 1 -or $entry.Bytes -gt 134217728 -or
        $entry.Sha256 -cnotmatch '^[a-f0-9]{64}$') { throw 'Invalid row checker inventory.' }
    $checker = Join-Path $bundle 'swift_model_check.exe'
    if ((Get-Item -LiteralPath $checker).Length -ne $entry.Bytes -or
        (Get-FileHash -LiteralPath $checker -Algorithm SHA256).Hash.ToLowerInvariant() -cne $entry.Sha256) { throw 'Row checker bytes/hash changed.' }
    Assert-Arm64PE $checker
    $maintenanceReceiptPath = Join-Path ([IO.Path]::GetDirectoryName($ToolPath)) 'receipt.json'
    if ((Get-Item -LiteralPath $maintenanceReceiptPath).Length -gt 65536 -or
        (Get-Item -LiteralPath $ToolPath).Length -gt 134217728) { throw 'Maintenance receipt or executable exceeds its byte bound.' }
    $maintenance = Get-Content -LiteralPath $maintenanceReceiptPath -Raw | ConvertFrom-Json -DateKind String
    if ($maintenance.SchemaVersion -ne 2 -or $maintenance.Revision -cne $env:GITHUB_SHA -or
        $maintenance.Sha256 -cnotmatch '^[a-f0-9]{64}$' -or
        $maintenance.Sha256 -cne (Get-FileHash -LiteralPath $ToolPath -Algorithm SHA256).Hash.ToLowerInvariant()) {
        throw 'Maintenance executable differs from the checked Runner revision.'
    }
    Assert-Arm64PE $ToolPath
    $artifacts = foreach ($path in @($checker, $receiptPath, $ToolPath, $maintenanceReceiptPath)) {
        @{ path = $path; bytes = (Get-Item -LiteralPath $path).Length
            sha256 = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant() }
    }
    Copy-Item -LiteralPath $receiptPath -Destination (Join-Path $evidence 'capture-tools.json')
    Copy-Item -LiteralPath $maintenanceReceiptPath -Destination (Join-Path $evidence 'maintenance-tools.json')
    $processor = @(Get-CimInstance Win32_Processor -Property Name -OperationTimeoutSec ([int][Math]::Min(10, (Get-RowWorkAllowance))))
    if ($processor.Count -ne 1 -or $processor[0].Name -notmatch 'Snapdragon.*X.*Elite') { throw 'Expected one native Snapdragon X/X2 Elite processor.' }
    $report['ProcessorName'] = $processor[0].Name
    if ([IO.DriveInfo]::new([IO.Path]::GetPathRoot($evidence)).AvailableFreeSpace -lt $plan.RequiredFreeDiskBytes) { throw 'Row qualification needs at least 1 GiB external disk headroom.' }
    $checked = Invoke-BonsaiTool -Command cpu-check-report -ToolPath $ToolPath -TimeoutSeconds (Get-RowWorkAllowance -ForTool) -Request @{
        operation = 'plan'; dataDirectory = $data; profile = 'swift-pq2-assessment'; useAbliterateLora = $true; useRepackedPtq1 = $false
    }
    $admission = $checked.Report
    if ($admission.Status -cne 'admission-passed' -or $admission.ServiceCeilingBytes -ne $plan.JobBytes -or
        $admission.RequiredAvailableBytes -ne $plan.AdmissionBytes -or $admission.AvailableMemoryFloorBytes -ne $plan.PhysicalReserveBytes -or
        $checked.Artifact.Bytes -ne 7206168928 -or $checked.Artifact.Sha256 -cne '5912bb739217cf25b4283a098c7b893d9baff36bdf2e72d3dbf4cb999d5633d5') {
        throw 'The unchanged Swift PQ2 artifact/resource admission did not pass.'
    }
    $adapter = & (Join-Path $PSScriptRoot 'Get-Adapter.ps1') -DataDirectory $data -VerifyOnly
    if ($adapter.AdapterPath -cne $admission.AdapterPath -or $adapter.Sha256 -cne 'f1669534803d340a496015f5c45125f3437b4d13ec764f40e34488ce83967f42') { throw 'Approved adapter and compiled CPU plan disagree.' }
    $artifacts += @{ path = $admission.ModelPath; bytes = 7206168928; sha256 = $checked.Artifact.Sha256 }
    $artifacts += @{ path = $adapter.AdapterPath; bytes = 9682464; sha256 = $adapter.Sha256 }
    $report['Admission'] = $admission
    foreach ($condition in $plan.Conditions) {
        $lane = Join-Path $evidence $condition.Name
        [void][IO.Directory]::CreateDirectory($lane)
        $arguments = @('--row-workers', $condition.RowWorkers.ToString(), '--rank-one-adapter', $adapter.AdapterPath,
            '1.3', 'forward', $admission.ModelPath, $lane, 'auto', '248045')
        [ordered]@{ SchemaVersion = 1; RunnerRevision = $env:GITHUB_SHA; RustSource = $source
            Condition = $condition.Name; RowWorkers = $condition.RowWorkers; Representation = 'PQ2_0'
            ModelSha256 = $checked.Artifact.Sha256; AdapterSha256 = $adapter.Sha256; AdapterScale = 1.3
            SuppliedTokens = @(248045); Backend = 'auto'; AppTpsQualified = $false } |
            ConvertTo-Json -Depth 12 | Set-Content -LiteralPath (Join-Path $lane 'capture.json') -Encoding utf8
        $arguments | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $lane 'arguments.json') -Encoding utf8
        $artifacts | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath (Join-Path $lane 'artifacts.json') -Encoding utf8
        $remaining = Get-RowWorkAllowance -ForTool
        $report.ModelTrialsRequested = $true
        $report['ActiveCondition'] = $condition.Name
        Save-RowReport -Report $report -Path $reportPath
        $supervision = Invoke-BonsaiTool -Command completion -ToolPath $ToolPath -TimeoutSeconds $remaining -Request @{
            schemaVersion = 1; executable = $checker; arguments = $arguments; workingDirectory = $lane; evidenceDirectory = $lane
            artifacts = @($artifacts); memoryLimitBytes = $plan.JobBytes; requiredAvailableBytes = $plan.AdmissionBytes
            physicalReserveBytes = $plan.PhysicalReserveBytes; timeoutSeconds = $remaining
            stdoutLimitBytes = 65536; stderrLimitBytes = 67108864; requireNonemptyStdout = $true; ciOnly = $false
        }
        $report.Samples.Add(@{ Condition = $condition.Name; RowWorkers = $condition.RowWorkers; Supervision = $supervision })
        Save-RowReport -Report $report -Path $reportPath
    }
    $python = @(Get-Command python -CommandType Application -ErrorAction Stop)[0].Source
    $comparison = Join-Path $evidence 'comparison.json'
    $compared = Invoke-BonsaiNativeProcess -Executable $python -WorkingDirectory $evidence -TimeoutSeconds (Get-RowWorkAllowance) -LogPath (Join-Path $evidence 'comparison.txt') -ArgumentList @(
        '-B', (Join-Path $repo 'tools/row-parallel/compare.py'), '--serial', (Join-Path $evidence 'serial'),
        '--parallel', (Join-Path $evidence 'parallel'), '--output', $comparison)
    if ($compared.ExitCode -ne 0) { throw 'Serial/parallel row correctness comparison failed.' }
    if ((Get-Item -LiteralPath $comparison).Length -gt 1048576) { throw 'Comparison report exceeds its pre-read bound.' }
    $report['Comparison'] = Get-Content -LiteralPath $comparison -Raw | ConvertFrom-Json -AsHashtable -DateKind String
    $report.Status = 'completed-one-token-row-control'
    $report['WorkElapsedSeconds'] = $workClock.Elapsed.TotalSeconds
    Save-RowReport -Report $report -Path $reportPath
    [pscustomobject]@{ Status = $report.Status; EvidenceDirectory = $evidence; AppTpsQualified = $false; DefaultMayChange = $false }
} catch {
    $report.Status = 'failed'; $report['Failure'] = $_.Exception.Message
    $report['WorkElapsedSeconds'] = $workClock.Elapsed.TotalSeconds
    Save-RowReport -Report $report -Path $reportPath
    Write-Warning "Row qualification evidence retained at $evidence"
    throw
}
