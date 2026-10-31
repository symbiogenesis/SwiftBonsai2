#requires -Version 7.6
<# .SYNOPSIS
Checks native signed-session supervision with synthetic source and result records only.
#>
[CmdletBinding()]
[OutputType([string])]
param([Parameter(Mandatory)][ValidateSet('Plan','Build','Result','Environment')][string]$Case)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.TestSupport.psm1')
Import-Module (Join-Path $PSScriptRoot '../scripts/Bonsai.HexagonSession.psm1')
Import-Module (Join-Path $PSScriptRoot '../scripts/Bonsai.HexagonDispatch.psm1')
if ($Case -ceq 'Plan') {
    $plan = & (Join-Path $PSScriptRoot '../scripts/Invoke-HexagonSessionProbe.ps1') -PlanOnly
    Assert ($plan.MaximumSeconds -eq 120 -and $plan.CleanupReserveSeconds -eq 75 -and $plan.OwnedJobRequired -and
        $plan.Bytes -eq 4096 -and $plan.MapFlags -eq 2 -and $plan.SessionId -eq 0) 'Native session probe lost its bounded default-session FD contract.'
    Assert ($plan.CatalogMemberVerificationRequired -and $plan.SourceBoundHostedFixtureRequired -and $plan.IndependentDriverIdentityRequired -and
        -not $plan.DriverInstallationAllowed -and -not $plan.TrustChangesAllowed -and -not $plan.ComputeExecuted -and -not $plan.ModelLoaded -and -not $plan.ProfileEligible) 'Native session plan authorized an independent gate.'
} elseif ($Case -ceq 'Build') {
    $revision = '1' * 40
    $digests = @{}
    $files = @(foreach ($name in @('lib.rs','windows.rs','mapping.rs','session.rs','session_native.rs','session_mapping.rs','session_mapping_native.rs','query.rs','sources.json')) {
        $digests[$name] = '2' * 64
        @{ Name = $name; Sha256 = '2' * 64 }
    })
    $receipt = @{
        SchemaVersion = 1; RunnerRevision = $revision; RunId = '1234'; RunAttempt = '1'
        RustToolchain = 'nightly-2026-10-22-aarch64-pc-windows-msvc'; Target = 'aarch64-pc-windows-msvc'; HostedBuild = $true
        CompilerIdentitySha256 = '3' * 64; SourceFiles = $files; ManifestSha256 = '4' * 64; LockSha256 = '5' * 64
        Executable = @{ Name = 'native-hexagon-query.exe'; Bytes = 65536; Sha256 = '6' * 64 }
        DeviceCodeLoaded = $false; DspCodeLoaded = $false; ProfileEligible = $false
    }
    $arguments = @{ ExpectedRunnerRevision = $revision; SourceDigests = $digests; ManifestSha256 = '4' * 64; LockSha256 = '5' * 64 }
    Assert-BonsaiHexagonSessionBuild -Receipt $receipt @arguments
    foreach ($field in @('RunnerRevision','RunId','RunAttempt','HostedBuild','SourceFiles','ManifestSha256','LockSha256','Target','CompilerIdentitySha256')) {
        $changed = $receipt.Clone(); $changed[$field] = $null
        Assert-Failure { Assert-BonsaiHexagonSessionBuild -Receipt $changed @arguments } 'Incomplete native hosted-build identity admitted.'
    }
    foreach ($field in @('DeviceCodeLoaded','DspCodeLoaded','ProfileEligible')) {
        $changed = $receipt.Clone(); $changed[$field] = 0
        Assert-Failure { Assert-BonsaiHexagonSessionBuild -Receipt $changed @arguments } 'Coerced hosted device claim admitted.'
    }
    $changed = $receipt.Clone(); $changed.SourceFiles = @($files[0]) * 9
    Assert-Failure { Assert-BonsaiHexagonSessionBuild -Receipt $changed @arguments } 'Duplicate native source identity admitted.'
    $changed = $receipt.Clone(); $changed.Executable = $receipt.Executable.Clone(); $changed.Executable.Name = '../native-hexagon-query.exe'
    Assert-Failure { Assert-BonsaiHexagonSessionBuild -Receipt $changed @arguments } 'Native executable path traversal admitted.'
} elseif ($Case -ceq 'Environment') {
    $environment = Get-BonsaiHexagonDispatchEnvironment -RuntimeDirectory 'C:\runtime' -SystemDirectory 'C:\Windows\System32'
    Assert ('FASTRPC_' -cin $environment.RemovePrefixes -and 'RPCMEM_' -cin $environment.RemovePrefixes -and
        'HEXAGON_' -cin $environment.RemovePrefixes -and 'ADSP_' -cin $environment.RemovePrefixes -and
        'ACTIONS_RUNTIME_TOKEN' -cin $environment.RemoveNames -and 'GITHUB_TOKEN' -cin $environment.RemoveNames) 'Native session worker inherited runtime overrides or CI credentials.'
} else {
    $driverPath = 'C:\Windows\System32\DriverStore\FileRepository\fixture\libcdsprpc.dll'
    $native = @{
        schemaVersion = 1; kind = 'native-fastrpc-session-mapping-probe'; driverDll = $driverPath; passed = $true
        stage = 'session-fd-cycle-completed-owner-pinned'; metadataPrepared = $true; metadataStage = 'session-metadata-prepared'
        architectureRaw = 32897; architecture = 81; sessionId = 0; domainRequestId = 14; domainResult = 0; effectiveDomain = 3
        uriRequestId = 15; uriResult = 0; uriVerified = $true; moduleOpenResult = 0; handleObserved = $true; remoteModuleOpened = $true
        bytes = 4096; heapId = 25; allocationFlags = 1; mapFlags = 2; allocationSucceeded = $true; descriptorObserved = $true
        descriptorError = $null; cpuRoundtripVerified = $true; mappingAttempted = $true; mappingResult = 0; unmappingResult = 0
        freeCompleted = $true; poisoned = $false; moduleCloseResult = 0; moduleCloseCompleted = $true; ownerPinnedUntilExit = $true
        driverLoaded = $true; signedDeploymentVerified = $false; computeExecuted = $false; dmaExecuted = $false
        unsignedProcessRequested = $false; sessionTeardownVerified = $false; profileEligible = $false
    }
    $checked = Get-BonsaiHexagonSessionProbeResult -Json ($native | ConvertTo-Json -Depth 8) -ExitCode 0
    Assert ($checked.passed -and -not $checked.computeExecuted -and -not $checked.sessionTeardownVerified) 'FD mapping became compute or vendor teardown evidence.'
    foreach ($field in @('profileEligible','computeExecuted','dmaExecuted','unsignedProcessRequested','signedDeploymentVerified','sessionTeardownVerified')) {
        $changed = $native.Clone(); $changed[$field] = $true
        Assert-Failure { Get-BonsaiHexagonSessionProbeResult -Json ($changed | ConvertTo-Json -Depth 8) -ExitCode 0 } 'Native probe claimed an independent gate.'
    }
    foreach ($field in @('mapFlags','bytes','moduleCloseCompleted','uriVerified','mappingAttempted','ownerPinnedUntilExit')) {
        $changed = $native.Clone(); $changed[$field] = 0
        Assert-Failure { Get-BonsaiHexagonSessionProbeResult -Json ($changed | ConvertTo-Json -Depth 8) -ExitCode 0 } 'Native result accepted a changed argument or coerced boolean.'
    }
    $changed = $native.Clone(); $changed.architecture = 73
    Assert-Failure { Get-BonsaiHexagonSessionProbeResult -Json ($changed | ConvertTo-Json -Depth 8) -ExitCode 0 } 'Native probe substituted a device family.'
    $json = $native | ConvertTo-Json -Depth 8 -Compress
    Assert-Failure { Get-BonsaiHexagonSessionProbeResult -Json $json.Replace('"passed":true','"Passed":false,"passed":true') -ExitCode 0 } 'Ambiguous native JSON identity admitted.'
    $refused = $native.Clone(); $refused.passed = $false; $refused.stage = 'fd-mapping-failed-pinned'
    $refused.mappingResult = 512; $refused.unmappingResult = $null; $refused.freeCompleted = $false
    $refused.poisoned = $true; $refused.moduleCloseResult = $null; $refused.moduleCloseCompleted = $false
    $checked = Get-BonsaiHexagonSessionProbeResult -Json ($refused | ConvertTo-Json -Depth 8) -ExitCode 0
    Assert (-not $checked.passed -and $checked.remoteModuleOpened -and $checked.mappingResult -eq 512 -and $checked.poisoned) 'Scoped FD refusal lost completed open or uncertain ownership.'
    $changed = $refused.Clone(); $changed.freeCompleted = $true
    Assert-Failure { Get-BonsaiHexagonSessionProbeResult -Json ($changed | ConvertTo-Json -Depth 8) -ExitCode 0 } 'An uncertain mapping freed its scratch.'
    $changed = $refused.Clone(); $changed.stage = 'session-fd-cycle-completed-owner-pinned'
    Assert-Failure { Get-BonsaiHexagonSessionProbeResult -Json ($changed | ConvertTo-Json -Depth 8) -ExitCode 0 } 'Incomplete raw outcomes acquired a completed terminal stage.'
    $changed = $refused.Clone(); $changed.stage = 'fd-unmapping-failed-pinned'
    Assert-Failure { Get-BonsaiHexagonSessionProbeResult -Json ($changed | ConvertTo-Json -Depth 8) -ExitCode 0 } 'A mapping refusal became an attempted unmap.'
    $openRefused = $refused.Clone(); $openRefused.stage = 'module-open-refused-pinned'; $openRefused.moduleOpenResult = 512
    $openRefused.handleObserved = $false; $openRefused.remoteModuleOpened = $false; $openRefused.allocationSucceeded = $false
    $openRefused.descriptorObserved = $false; $openRefused.cpuRoundtripVerified = $false; $openRefused.mappingAttempted = $false
    $openRefused.mappingResult = $null; $openRefused.poisoned = $false
    $checked = Get-BonsaiHexagonSessionProbeResult -Json ($openRefused | ConvertTo-Json -Depth 8) -ExitCode 0
    Assert (-not $checked.passed -and -not $checked.mappingAttempted -and $checked.ownerPinnedUntilExit) 'A module-open refusal became mapping or released the owner.'
    $revision = '1' * 40
    $worker = @{
        SchemaVersion = 1; Kind = 'runner-hexagon-session-worker'; SourceRevision = $revision
        Outcome = 'passed'; Stage = 'session-mapping-complete'; Failure = $null; LastNativeStage = 'module-close-entered'
        SignedPackageVerified = $true; RuntimeFilesVerified = 16; DriverIdentityVerified = $true
        DriverIdentity = @{ Path = $driverPath; Bytes = 65536; Sha256 = '2' * 64; SignatureStatus = 'Valid'; Publisher = 'Microsoft Windows Hardware Compatibility Publisher'; SignerThumbprint = '3' * 40; FileHeldAgainstMutation = $true }
        MappingCycleVerified = $true; NativeResult = $native; ExecutableSha256 = '4' * 64; BuildReceiptSha256 = '5' * 64; ManifestSha256 = '6' * 64; LockSha256 = '7' * 64
        ModelLoaded = $false; SwiftQualified = $false; ProfileEligible = $false; DriverInstalled = $false; TrustChanged = $false; ComputeExecuted = $false
    }
    $checked = Get-BonsaiHexagonSessionWorkerResult -Json ($worker | ConvertTo-Json -Depth 12) -ExpectedRunnerRevision $revision
    Assert ($checked.MappingCycleVerified -and -not $checked.ProfileEligible) 'Native worker lost mapping evidence or granted admission.'
    foreach ($field in @('DriverIdentity','NativeResult','ExecutableSha256','SignedPackageVerified')) {
        $changed = $worker.Clone(); $changed[$field] = $null
        Assert-Failure { Get-BonsaiHexagonSessionWorkerResult -Json ($changed | ConvertTo-Json -Depth 12) -ExpectedRunnerRevision $revision } 'Incomplete native worker source/package evidence established mapping.'
    }
    $changed = $worker.Clone(); $changed.Outcome = 'pending'; $changed.Stage = 'session-mapping'; $changed.MappingCycleVerified = $false
    $changed.NativeResult = $null; $changed.LastNativeStage = 'fd-mapping-entered'
    $checked = Get-BonsaiHexagonSessionWorkerResult -Json ($changed | ConvertTo-Json -Depth 12) -ExpectedRunnerRevision $revision
    Assert ($checked.LastNativeStage -ceq 'fd-mapping-entered' -and -not $checked.MappingCycleVerified) 'Interrupted native call lost its distinct entered-stage evidence.'
}
"Focused native Hexagon session $Case checks passed without loading a device."
