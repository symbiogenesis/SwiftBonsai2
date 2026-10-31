#requires -Version 7.6
<# .SYNOPSIS
Checks source-bound Hexagon dispatch policy with synthetic metadata only.
#>
[CmdletBinding()]
[OutputType([string])]
param([Parameter(Mandatory)][ValidateSet('Plan', 'Build', 'Result', 'Environment', 'Capture')][string]$Case)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.TestSupport.psm1')
Import-Module (Join-Path $PSScriptRoot '../scripts/Bonsai.HexagonDispatch.psm1')
if ($Case -ceq 'Plan') {
    $plan = & (Join-Path $PSScriptRoot '../scripts/Invoke-HexagonDispatch.ps1') -PlanOnly
    Assert ($plan.MaximumSeconds -eq 120 -and $plan.CleanupReserveSeconds -eq 75 -and $plan.OwnedJobRequired) 'Dispatch lost its aggregate ownership budget.'
    Assert ($plan.Cases.Count -eq 2 -and $plan.Cases[0].Batch -eq 1 -and $plan.Cases[1].Batch -eq 32) 'Dispatch fixture shapes changed.'
    Assert (-not $plan.ModelLoaded -and -not $plan.CpuFallbackAllowed -and -not $plan.ProfileEligible -and
        $plan.CatalogMemberVerificationRequired -and $plan.SourceBoundHostedFixtureRequired -and $plan.AsInvokerManifestRequired -and -not $plan.DriverInstallationAllowed) 'Plan authorized unsupported execution or deployment.'
} elseif ($Case -ceq 'Build') {
    $revision = '1' * 40
    $source = '2' * 64
    $lock = '3' * 64
    $manifest = '6' * 64
    $receipt = @{
        SchemaVersion = 1; RunnerRevision = $revision; RustToolchain = 'nightly-2026-10-22-aarch64-pc-windows-msvc'
        SourceSha256 = $source; ManifestSha256 = $manifest; LockSha256 = $lock; HostedBuild = $true; Target = 'aarch64-pc-windows-msvc'
        CompilerIdentitySha256 = '4' * 64
        Executable = @{ Name = 'hexagon-dispatch-fixture.exe'; Bytes = 65536; Sha256 = '5' * 64 }
    }
    Assert-BonsaiHexagonDispatchBuild -Receipt $receipt -ExpectedRunnerRevision $revision -SourceSha256 $source -ManifestSha256 $manifest -LockSha256 $lock
    foreach ($field in @('RunnerRevision', 'SourceSha256', 'ManifestSha256', 'LockSha256', 'HostedBuild', 'Target', 'CompilerIdentitySha256')) {
        $changed = $receipt.Clone()
        $changed[$field] = if ($field -ceq 'HostedBuild') { $false } else { 'changed' }
        Assert-Failure { Assert-BonsaiHexagonDispatchBuild -Receipt $changed -ExpectedRunnerRevision $revision -SourceSha256 $source -ManifestSha256 $manifest -LockSha256 $lock } 'Changed hosted/source identity admitted.'
    }
    foreach ($extent in @([int]65536, [long]65536)) {
        $changed = $receipt.Clone()
        $changed.Executable = @{ Name = '../fixture.exe'; Bytes = $extent; Sha256 = '5' * 64 }
        Assert-Failure { Assert-BonsaiHexagonDispatchBuild -Receipt $changed -ExpectedRunnerRevision $revision -SourceSha256 $source -ManifestSha256 $manifest -LockSha256 $lock } 'Unsafe fixture filename admitted with a valid integer extent.'
    }
    foreach ($schema in @($true, 1.0)) {
        $changed = $receipt.Clone(); $changed.SchemaVersion = $schema
        Assert-Failure { Assert-BonsaiHexagonDispatchBuild -Receipt $changed -ExpectedRunnerRevision $revision -SourceSha256 $source -ManifestSha256 $manifest -LockSha256 $lock } 'A boolean or floating schema identity was accepted.'
    }
    foreach ($field in @('RustToolchain', 'SourceSha256', 'ManifestSha256', 'LockSha256', 'Target')) {
        $changed = $receipt.Clone(); $changed[$field] = @($receipt[$field])
        Assert-Failure { Assert-BonsaiHexagonDispatchBuild -Receipt $changed -ExpectedRunnerRevision $revision -SourceSha256 $source -ManifestSha256 $manifest -LockSha256 $lock } 'An array was accepted as scalar build identity.'
    }
} elseif ($Case -ceq 'Result') {
    $record = @{
        SchemaVersion = 1; Kind = 'runner-hexagon-dispatch-fixture'; Outcome = 'passed'; Stage = 'numeric-dispatch-complete'; Failure = $null; NativeCode = $null
        SourceRevision = '94256114c229674ef96e76eb2dea596e65b43818'; SelfTest = $false; BackendName = 'HTP0'
        DispatchEvidence = 'explicit-hexagon-compute-sync-and-full-output-oracle'; SessionCleanup = 'supervised-process-exit-required'
        DeviceDispatchVerified = $true; CpuFallbackAllowed = $false; ModelLoaded = $false; SwiftQualified = $false; ProfileEligible = $false
        Cases = @(@{ Columns = 128; Rows = 32; Batch = 1; OutputValuesChecked = 32; MatmulParity = $true; ExecutedOnHexagon = $true },
            @{ Columns = 128; Rows = 32; Batch = 32; OutputValuesChecked = 1024; MatmulParity = $true; ExecutedOnHexagon = $true })
    }
    $checked = Get-BonsaiHexagonDispatchFixtureResult -Json ($record | ConvertTo-Json -Depth 5) -ExitCode 0
    Assert ($checked.DeviceDispatchVerified -and -not $checked.ProfileEligible) 'Numeric access confused with Swift promotion.'
    Assert-Failure { Get-BonsaiHexagonDispatchFixtureResult -Json ($record | ConvertTo-Json -Depth 5) -ExitCode 2 } 'Contradictory exit status admitted.'
    foreach ($field in @('CpuFallbackAllowed', 'ModelLoaded', 'SwiftQualified', 'ProfileEligible')) {
        $changed = $record.Clone(); $changed[$field] = $true
        Assert-Failure { Get-BonsaiHexagonDispatchFixtureResult -Json ($changed | ConvertTo-Json -Depth 5) -ExitCode 0 } 'Fixture authorized a forbidden independent gate.'
    }
    $changed = $record.Clone(); $changed.Cases = @($record.Cases[0])
    Assert-Failure { Get-BonsaiHexagonDispatchFixtureResult -Json ($changed | ConvertTo-Json -Depth 5) -ExitCode 0 } 'Partial fixture passed.'
    $changed = $record.Clone(); $changed.Cases = @($record.Cases[0], $record.Cases[0])
    Assert-Failure { Get-BonsaiHexagonDispatchFixtureResult -Json ($changed | ConvertTo-Json -Depth 5) -ExitCode 0 } 'Duplicate fixture passed.'
    $changed = $record.Clone(); $changed.SelfTest = $true; $changed.DeviceDispatchVerified = $false; $changed.BackendName = $null
    Assert-Failure { Get-BonsaiHexagonDispatchFixtureResult -Json ($changed | ConvertTo-Json -Depth 5) -ExitCode 0 } 'Host-only self-test was accepted as device proof.'
    $pending = $record.Clone(); $pending.Outcome = 'pending'; $pending.Stage = 'allocation'; $pending.Failure = 'hexagon-buffer-allocation-failed'
    $pending.DeviceDispatchVerified = $false; $pending.Cases = @($record.Cases[0])
    $partial = Get-BonsaiHexagonDispatchFixtureResult -Json ($pending | ConvertTo-Json -Depth 5) -ExitCode 2
    Assert ($partial.Cases.Count -eq 1 -and -not $partial.DeviceDispatchVerified) 'A completed first case was lost when the second was unavailable.'
    $changed = $pending.Clone(); $changed.Cases = @($record.Cases[0].Clone()); $changed.Cases[0].OutputValuesChecked = 1
    Assert-Failure { Get-BonsaiHexagonDispatchFixtureResult -Json ($changed | ConvertTo-Json -Depth 5) -ExitCode 2 } 'Malformed partial numeric evidence admitted.'
    foreach ($code in @($true, 1.5, -1, 4294967296)) {
        $changed = $pending.Clone(); $changed.NativeCode = $code
        Assert-Failure { Get-BonsaiHexagonDispatchFixtureResult -Json ($changed | ConvertTo-Json -Depth 5) -ExitCode 2 } 'Invalid native error identity admitted.'
    }
    $json = $record | ConvertTo-Json -Depth 5 -Compress
    foreach ($ambiguous in @($json.Replace('"Outcome":"passed"', '"Outcome":"pending","Outcome":"passed"'),
        $json.Replace('"Outcome":"passed"', '"outcome":"pending","Outcome":"passed"'),
        $json.Replace('"Rows":32', '"Rows":1,"Rows":32'))) {
        Assert-Failure { Get-BonsaiHexagonDispatchFixtureResult -Json $ambiguous -ExitCode 0 } 'Duplicate JSON evidence names were accepted.'
    }
    $revision = '1' * 40
    $worker = @{
        SchemaVersion = 1; Kind = 'runner-hexagon-dispatch-worker'; SourceRevision = $revision
        Outcome = 'passed'; Stage = 'numeric-dispatch-complete'; Failure = $null; LastFixtureStage = 'readback'
        CatalogMemberVerified = $true; CatalogMembersVerified = 4
        CatalogMemberNames = @('libggml-htp-v73.so','libggml-htp-v75.so','libggml-htp-v79.so','libggml-htp-v81.so')
        SelectedDspArchitecture = 'unknown'; DeviceDispatchVerified = $true; FixtureResult = $record
        LoadedDriverObservation = 'not-observed'; LoadedDriverIdentity = $null
        RuntimeFilesVerified = 16; FixtureSha256 = '2' * 64; FixtureSourceSha256 = '3' * 64
        ManifestSha256 = '4' * 64; LockSha256 = '5' * 64; SigntoolSha256 = '6' * 64
        ModelLoaded = $false; SwiftQualified = $false; ProfileEligible = $false; DriverInstalled = $false; TrustChanged = $false
    }
    $checkedWorker = Get-BonsaiHexagonDispatchWorkerResult -Json ($worker | ConvertTo-Json -Depth 8) -ExpectedRunnerRevision $revision
    Assert ($checkedWorker.DeviceDispatchVerified) 'Complete worker evidence rejected.'
    $observed = $worker.Clone()
    $observed.LoadedDriverObservation = 'observed-verified'
    $observed.LoadedDriverIdentity = @{
        Path = 'C:\Windows\System32\DriverStore\FileRepository\fixture\libcdsprpc.dll'
        Bytes = 65536; Sha256 = '7' * 64; SignatureStatus = 'Valid'
        Publisher = 'Microsoft Windows Hardware Compatibility Publisher'; SignerThumbprint = '8' * 40
        FileHeldAgainstMutation = $true
    }
    $checkedWorker = Get-BonsaiHexagonDispatchWorkerResult -Json ($observed | ConvertTo-Json -Depth 8) -ExpectedRunnerRevision $revision
    Assert ($checkedWorker.LoadedDriverIdentity.Sha256 -ceq ('7' * 64) -and $checkedWorker.SelectedDspArchitecture -ceq 'unknown') 'Loaded host driver was confused with the selected DSP.'
    foreach ($field in @('Sha256', 'FileHeldAgainstMutation', 'Publisher')) {
        $changed = $observed.Clone(); $changed.LoadedDriverIdentity = $observed.LoadedDriverIdentity.Clone(); $changed.LoadedDriverIdentity[$field] = $null
        Assert-Failure { Get-BonsaiHexagonDispatchWorkerResult -Json ($changed | ConvertTo-Json -Depth 8) -ExpectedRunnerRevision $revision } 'Incomplete loaded driver evidence admitted.'
    }
    $changed = $observed.Clone(); $changed.LoadedDriverObservation = 'not-observed'
    Assert-Failure { Get-BonsaiHexagonDispatchWorkerResult -Json ($changed | ConvertTo-Json -Depth 8) -ExpectedRunnerRevision $revision } 'An unobserved driver acquired an identity.'
    foreach ($field in @('ModelLoaded','SwiftQualified','ProfileEligible','DriverInstalled','TrustChanged')) {
        $changed = $worker.Clone(); $changed[$field] = 0
        Assert-Failure { Get-BonsaiHexagonDispatchWorkerResult -Json ($changed | ConvertTo-Json -Depth 8) -ExpectedRunnerRevision $revision } 'A worker boolean was accepted by coercion.'
    }
    foreach ($field in @('SourceRevision','CatalogMembersVerified','RuntimeFilesVerified','FixtureSha256','DeviceDispatchVerified','FixtureResult')) {
        $changed = $worker.Clone(); $changed[$field] = $null
        Assert-Failure { Get-BonsaiHexagonDispatchWorkerResult -Json ($changed | ConvertTo-Json -Depth 8) -ExpectedRunnerRevision $revision } 'Incomplete worker evidence established device dispatch.'
    }
    $changed = $worker.Clone(); $changed.FixtureResult = $pending
    Assert-Failure { Get-BonsaiHexagonDispatchWorkerResult -Json ($changed | ConvertTo-Json -Depth 8) -ExpectedRunnerRevision $revision } 'Worker result contradicted the completed fixture.'
    $changed = $worker.Clone(); $changed.Outcome = 'pending'; $changed.DeviceDispatchVerified = $false
    $changed.Stage = 'device-dispatch'; $changed.FixtureResult = $null
    $checkedWorker = Get-BonsaiHexagonDispatchWorkerResult -Json ($changed | ConvertTo-Json -Depth 8) -ExpectedRunnerRevision $revision
    Assert ($checkedWorker.LastFixtureStage -ceq 'readback' -and -not $checkedWorker.DeviceDispatchVerified) 'Interrupted worker lost its distinct progress observation.'
    $changed.LastFixtureStage = 'context'
    $checkedWorker = Get-BonsaiHexagonDispatchWorkerResult -Json ($changed | ConvertTo-Json -Depth 8) -ExpectedRunnerRevision $revision
    Assert ($checkedWorker.LastFixtureStage -ceq 'context' -and -not $checkedWorker.DeviceDispatchVerified) 'Context-stage progress was rejected as partial evidence.'
} elseif ($Case -ceq 'Environment') {
    $environment = Get-BonsaiHexagonDispatchEnvironment -RuntimeDirectory 'C:\runtime' -SystemDirectory 'C:\Windows\System32'
    Assert ($environment.Set.PATH -ceq 'C:\runtime;C:\Windows\System32' -and $environment.Set.ADSP_LIBRARY_PATH -ceq 'C:\runtime') 'Child DLL/HTP lookup escaped fixed directories.'
    Assert ('GGML_' -cin $environment.RemovePrefixes -and 'GENIEX_' -cin $environment.RemovePrefixes -and
        'FASTRPC_' -cin $environment.RemovePrefixes -and 'RPCMEM_' -cin $environment.RemovePrefixes -and
        'ACTIONS_RUNTIME_TOKEN' -cin $environment.RemoveNames -and 'GITHUB_TOKEN' -cin $environment.RemoveNames) 'Inherited backend overrides or CI credentials were retained.'
    Assert ($environment.Set.Count -eq 2) 'Unreviewed child backend override added.'
} else {
    # The test process itself enters the existing Job gate before its synthetic
    # stdout/stderr children run. No fixture, package, verifier or driver loads.
    Import-Module (Join-Path $PSScriptRoot '../scripts/Bonsai.Native.psm1')
    $modulePath = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../scripts/Bonsai.HexagonDispatch.psm1'))
    $command = @'
$ErrorActionPreference = 'Stop'
Import-Module '__MODULE__'
& (Get-Module Bonsai.HexagonDispatch) {
    $normal = '[Console]::Out.Write("ok"); [Console]::Error.Write("error")'
    $arguments = @('-NoLogo','-NoProfile','-NonInteractive','-EncodedCommand',[Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($normal)))
    $result = Invoke-BonsaiHexagonBoundedCapture -Executable (Join-Path $PSHOME 'pwsh.exe') -ArgumentList $arguments -WorkingDirectory ([IO.Path]::GetTempPath())
    if ($result.ExitCode -ne 0 -or $result.StandardOutput -cne 'ok' -or $result.StandardError -cne 'error') { throw 'Concurrent stream capture failed.' }
    $progressPath = Join-Path ([IO.Path]::GetTempPath()) ('Bonsai-Hexagon-Progress-' + [guid]::NewGuid().ToString('N') + '.json')
    try {
        $progress = [ordered]@{ LastFixtureStage = 'not-started' }
        # An invalid vendor label must never enter retained evidence; a fixed
        # marker split across pipe reads must be recovered once its LF arrives.
        $markers = '[Console]::Error.Write("hexagon-fixture:stage=private-path`nhexagon-fixture:stage=con"); Start-Sleep -Milliseconds 30; [Console]::Error.Write("text`n"); exit 2'
        $arguments = @('-NoLogo','-NoProfile','-NonInteractive','-EncodedCommand',[Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($markers)))
        $result = Invoke-BonsaiHexagonBoundedCapture -Executable (Join-Path $PSHOME 'pwsh.exe') -ArgumentList $arguments -WorkingDirectory ([IO.Path]::GetTempPath()) -ProgressReceipt $progress -ProgressPath $progressPath
        $saved = [IO.File]::ReadAllText($progressPath) | ConvertFrom-Json -AsHashtable
        if ($result.ExitCode -ne 2 -or $progress.LastFixtureStage -cne 'context' -or $saved.LastFixtureStage -cne 'context') { throw 'Bounded native progress was not retained.' }
        $progress = [ordered]@{ LastNativeStage = 'not-started'; UnsignedProcessRequested = $null }
        $markers = '[Console]::Error.Write("hexagon-session:stage=private-label`nhexagon-session:stage=reference-process-init-"); Start-Sleep -Milliseconds 30; [Console]::Error.Write("entered`n"); exit 2'
        $arguments = @('-NoLogo','-NoProfile','-NonInteractive','-EncodedCommand',[Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($markers)))
        $result = Invoke-BonsaiHexagonBoundedCapture -Executable (Join-Path $PSHOME 'pwsh.exe') -ArgumentList $arguments -WorkingDirectory ([IO.Path]::GetTempPath()) -ProgressReceipt $progress -ProgressPath $progressPath -ProgressKind Session
        $saved = [IO.File]::ReadAllText($progressPath) | ConvertFrom-Json -AsHashtable
        if ($result.ExitCode -ne 2 -or $saved.LastNativeStage -cne 'reference-process-init-entered' -or $null -ne $saved.UnsignedProcessRequested) { throw 'An entered reference-process call lost its distinct unknown request outcome.' }
    } finally { if ([IO.File]::Exists($progressPath)) { [IO.File]::Delete($progressPath) } }
    $overflow = '[Console]::Out.Write("x" * 131073)'
    $arguments = @('-NoLogo','-NoProfile','-NonInteractive','-EncodedCommand',[Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($overflow)))
    $rejected = $false
    try { $null = Invoke-BonsaiHexagonBoundedCapture -Executable (Join-Path $PSHOME 'pwsh.exe') -ArgumentList $arguments -WorkingDirectory ([IO.Path]::GetTempPath()) }
    catch { $rejected = $_.Exception.Message -like '*fixed byte cap*' }
    if (-not $rejected) { throw 'Fixture output overflow was accepted.' }
}
'@
    $command = $command.Replace('__MODULE__', $modulePath.Replace("'", "''"))
    $result = Invoke-BonsaiNativeProcess -Executable (Join-Path $PSHOME 'pwsh.exe') -ArgumentList @('-NoLogo', '-NoProfile', '-NonInteractive', '-EncodedCommand',
        [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($command))) -WorkingDirectory ([IO.Path]::GetTempPath()) -TimeoutSeconds 5 -OwnDescendants
    Assert ($result.ExitCode -eq 0) 'Bounded synthetic capture worker failed.'
}
"Focused Hexagon dispatch $Case checks passed without native signatures or device access."
