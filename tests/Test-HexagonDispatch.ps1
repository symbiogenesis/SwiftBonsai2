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
    $changed = $receipt.Clone()
    $changed.Executable = @{ Name = '../fixture.exe'; Bytes = 65536; Sha256 = '5' * 64 }
    Assert-Failure { Assert-BonsaiHexagonDispatchBuild -Receipt $changed -ExpectedRunnerRevision $revision -SourceSha256 $source -ManifestSha256 $manifest -LockSha256 $lock } 'Unsafe fixture filename admitted.'
    foreach ($field in @('RustToolchain', 'SourceSha256', 'ManifestSha256', 'LockSha256', 'Target')) {
        $changed = $receipt.Clone(); $changed[$field] = @($receipt[$field])
        Assert-Failure { Assert-BonsaiHexagonDispatchBuild -Receipt $changed -ExpectedRunnerRevision $revision -SourceSha256 $source -ManifestSha256 $manifest -LockSha256 $lock } 'An array was accepted as scalar build identity.'
    }
} elseif ($Case -ceq 'Result') {
    $record = @{
        SchemaVersion = 1; Kind = 'runner-hexagon-dispatch-fixture'; Outcome = 'passed'; Stage = 'completed'; Failure = $null
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
