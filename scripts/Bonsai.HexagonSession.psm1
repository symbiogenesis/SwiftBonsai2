#requires -Version 7.6
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.HexagonDispatch.psm1')
Import-Module (Join-Path $PSScriptRoot 'Bonsai.HexagonIdentity.psm1')
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1')
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Native.psm1')

function Get-BonsaiHexagonSessionPlan {
    <# .SYNOPSIS
    Describes one signed default-session FD mapping probe without opening devices.
    #>
    [CmdletBinding()]
    [OutputType([Collections.Specialized.OrderedDictionary])]
    param([ValidateRange(1,240)][int]$MaximumSeconds = 120, [switch]$ReferenceInitialization)
    return [ordered]@{
        SchemaVersion = 1; Kind = 'runner-hexagon-session-plan'; MaximumSeconds = $MaximumSeconds
        CleanupReserveSeconds = 75; Bytes = 4096; MapFlags = 2; SessionId = 0
        OwnedJobRequired = $true; SourceBoundHostedFixtureRequired = $true
        CatalogMemberVerificationRequired = $true; IndependentDriverIdentityRequired = $true
        DriverInstallationAllowed = $false; TrustChangesAllowed = $false
        ComputeExecuted = $false; ModelLoaded = $false; ProfileEligible = $false
        Qualification = 'native-signed-session-FD-mapping-only'
        ReferenceProcessInitializationSelected = [bool]$ReferenceInitialization
        ReferenceProcessInitializationControl = if ($ReferenceInitialization) { 'DSPRPC_CONTROL_UNSIGNED_MODULE-enable1-default-session-effective-domain-process-only' } else { 'not-selected' }
        AutomaticFallbackAllowed = $false; AutomaticRetryAllowed = $false
    }
}

function Assert-BonsaiHexagonSessionBuild {
    <# .SYNOPSIS
    Checks the exact hosted native query executable and every source input.
    #>
    [CmdletBinding()]
    [OutputType([void])]
    param([Parameter(Mandatory)][Collections.IDictionary]$Receipt,
        [Parameter(Mandatory)][string]$ExpectedRunnerRevision,
        [Parameter(Mandatory)][Collections.IDictionary]$SourceDigests,
        [Parameter(Mandatory)][string]$ManifestSha256,
        [Parameter(Mandatory)][string]$LockSha256)
    $names = @('lib.rs','windows.rs','mapping.rs','session.rs','session_native.rs','session_mapping.rs','session_mapping_native.rs','query.rs','sources.json')
    foreach ($digest in @($ManifestSha256, $LockSha256) + @($SourceDigests.Values)) {
        if ($digest -isnot [string] -or $digest -cnotmatch '^[a-f0-9]{64}$') { throw 'Invalid native source digest.' }
    }
    if ($ExpectedRunnerRevision -cnotmatch '^[a-f0-9]{40}$' -or $SourceDigests.Count -ne $names.Count -or
        ($Receipt.SchemaVersion -isnot [int] -and $Receipt.SchemaVersion -isnot [long]) -or $Receipt.SchemaVersion -ne 1 -or
        $Receipt.RunnerRevision -isnot [string] -or $Receipt.RunnerRevision -cne $ExpectedRunnerRevision -or
        $Receipt.RunId -isnot [string] -or $Receipt.RunId -cnotmatch '^[0-9]{1,32}$' -or
        $Receipt.RunAttempt -isnot [string] -or $Receipt.RunAttempt -cnotmatch '^[0-9]{1,8}$' -or
        $Receipt.HostedBuild -isnot [bool] -or -not $Receipt.HostedBuild -or
        $Receipt.RustToolchain -isnot [string] -or $Receipt.RustToolchain -cne 'nightly-2026-10-22-aarch64-pc-windows-msvc' -or
        $Receipt.Target -isnot [string] -or $Receipt.Target -cne 'aarch64-pc-windows-msvc' -or
        $Receipt.CompilerIdentitySha256 -isnot [string] -or $Receipt.CompilerIdentitySha256 -cnotmatch '^[a-f0-9]{64}$' -or
        $Receipt.ManifestSha256 -isnot [string] -or $Receipt.ManifestSha256 -cne $ManifestSha256 -or
        $Receipt.LockSha256 -isnot [string] -or $Receipt.LockSha256 -cne $LockSha256 -or
        $Receipt.SourceFiles -isnot [array] -or $Receipt.SourceFiles.Count -ne $names.Count -or
        $Receipt.Executable -isnot [Collections.IDictionary]) { throw 'Native session fixture differs from trusted hosted source.' }
    $seen = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    foreach ($entry in $Receipt.SourceFiles) {
        if ($entry -isnot [Collections.IDictionary] -or $entry.Name -isnot [string] -or
            $entry.Name -cnotin $names -or -not $SourceDigests.Contains($entry.Name) -or -not $seen.Add($entry.Name) -or
            $entry.Sha256 -isnot [string] -or $entry.Sha256 -cne $SourceDigests[$entry.Name]) {
            throw 'Native session fixture source set changed.'
        }
    }
    $exe = $Receipt.Executable
    if ($exe.Name -isnot [string] -or $exe.Name -cne 'native-hexagon-query.exe' -or
        ($exe.Bytes -isnot [int] -and $exe.Bytes -isnot [long]) -or $exe.Bytes -le 0 -or $exe.Bytes -gt 16MB -or
        $exe.Sha256 -isnot [string] -or $exe.Sha256 -cnotmatch '^[a-f0-9]{64}$') { throw 'Invalid native session executable pin.' }
    foreach ($field in @('DeviceCodeLoaded','DspCodeLoaded','ProfileEligible')) {
        if ($Receipt[$field] -isnot [bool] -or $Receipt[$field]) { throw 'Hosted preparation claimed device execution.' }
    }
}

function Get-BonsaiHexagonSessionProbeResult {
    <# .SYNOPSIS
    Checks every completed native step without converting metadata or partial mapping into success.
    #>
    [CmdletBinding()]
    [OutputType([Collections.IDictionary])]
    param([Parameter(Mandatory)][AllowEmptyString()][string]$Json, [Parameter(Mandatory)][int]$ExitCode, [switch]$ReferenceInitialization)
    $record = ConvertFrom-BonsaiHexagonDispatchJson -Json $Json
    if ($ExitCode -ne 0 -or ($record.schemaVersion -isnot [int] -and $record.schemaVersion -isnot [long]) -or $record.schemaVersion -ne 1 -or
        $record.kind -isnot [string] -or $record.kind -cne 'native-fastrpc-session-mapping-probe' -or
        $record.driverDll -isnot [string] -or -not [IO.Path]::IsPathFullyQualified($record.driverDll) -or [IO.Path]::GetFileName($record.driverDll) -ine 'libcdsprpc.dll' -or
        $record.stage -isnot [string] -or $record.stage -cnotin @('session-metadata-incomplete','reference-process-init-refused-pinned','module-open-refused-pinned','module-handle-not-written-pinned','allocation-refused','descriptor-refused','allocation-not-page-aligned','cpu-roundtrip-failed','fd-mapping-failed-pinned','fd-unmapping-failed-pinned','session-fd-cycle-completed-owner-pinned','module-close-refused-pinned') -or
        $record.metadataStage -isnot [string] -or $record.metadataStage -cnotmatch '^[a-z][a-z0-9-]{0,63}$') { throw 'Invalid native session probe identity or stage.' }
    foreach ($field in @('passed','metadataPrepared','uriVerified','handleObserved','remoteModuleOpened','allocationSucceeded','descriptorObserved','cpuRoundtripVerified','mappingAttempted','freeCompleted','poisoned','moduleCloseCompleted','ownerPinnedUntilExit','driverLoaded','signedDeploymentVerified','computeExecuted','dmaExecuted','unsignedProcessRequested','referenceProcessInitializationSelected','sessionTeardownVerified','profileEligible')) {
        if ($record[$field] -isnot [bool]) { throw 'Native session probe requires exact boolean values.' }
    }
    foreach ($field in @('signedDeploymentVerified','computeExecuted','dmaExecuted','sessionTeardownVerified','profileEligible')) {
        if ($record[$field]) { throw 'Native session probe claimed an independent execution or deployment gate.' }
    }
    if (-not $record.driverLoaded) { throw 'Native session probe omitted its loaded driver.' }
    foreach ($pair in @(@('sessionId',0),@('domainRequestId',14),@('uriRequestId',15),@('bytes',4096),@('heapId',25),@('allocationFlags',1),@('mapFlags',2),@('processInitializationRequestId',2),@('processInitializationEnable',1),@('processInitializationStructureBytes',8))) {
        $value = $record[$pair[0]]
        if (($value -isnot [int] -and $value -isnot [long]) -or $value -ne $pair[1]) { throw 'Native session probe changed a fixed argument.' }
    }
    if (($record.architectureRaw -isnot [int] -and $record.architectureRaw -isnot [long]) -or $record.architectureRaw -lt 0 -or $record.architectureRaw -gt [uint]::MaxValue) { throw 'Invalid raw native architecture.' }
    foreach ($field in @('domainResult','effectiveDomain','uriResult','moduleOpenResult','descriptorError','mappingResult','unmappingResult','moduleCloseResult','processInitializationResult')) {
        $value = $record[$field]
        if ($null -ne $value -and (($value -isnot [int] -and $value -isnot [long]) -or $value -lt [int]::MinValue -or $value -gt [int]::MaxValue)) { throw 'Invalid raw native session result.' }
    }
    $low = $record.architectureRaw -band 255
    $decoded = (($low -shr 4) * 10) + ($low -band 15)
    $supported = ($low -shr 4) -ge 1 -and ($low -shr 4) -le 9 -and ($low -band 15) -le 9 -and $decoded -in @(73,81)
    if ($supported) {
        if (($record.architecture -isnot [int] -and $record.architecture -isnot [long]) -or $record.architecture -ne $decoded) { throw 'Native session architecture contradicts the raw capability.' }
    } elseif ($null -ne $record.architecture) { throw 'Unsupported native architecture became a selected family.' }
    $metadata = $supported -and $null -ne $record.domainResult -and $record.domainResult -eq 0 -and
        $null -ne $record.effectiveDomain -and $record.effectiveDomain -ge 0 -and
        $null -ne $record.uriResult -and $record.uriResult -eq 0 -and $record.uriVerified
    if ($record.metadataPrepared -ne $metadata -or ($metadata -and $record.metadataStage -cne 'session-metadata-prepared')) { throw 'Native session metadata is contradictory.' }
    $openAttempted = $null -ne $record.moduleOpenResult
    $initializationAttempted = $null -ne $record.processInitializationResult
    if ($record.referenceProcessInitializationSelected -ne [bool]$ReferenceInitialization -or
        $record.unsignedProcessRequested -ne $initializationAttempted -or
        ($initializationAttempted -and -not $ReferenceInitialization) -or
        ($ReferenceInitialization -and $openAttempted -and (-not $initializationAttempted -or $record.processInitializationResult -ne 0))) {
        throw 'Native reference-process initialization selection or outcome is contradictory.'
    }
    $opened = $openAttempted -and $record.moduleOpenResult -eq 0 -and $record.handleObserved -and $metadata
    if ($record.remoteModuleOpened -ne $opened -or $record.ownerPinnedUntilExit -ne ($openAttempted -or $initializationAttempted) -or
        ($openAttempted -and -not $metadata) -or ($record.handleObserved -and -not $openAttempted) -or
        ($record.allocationSucceeded -and -not $opened)) { throw 'Native session open evidence is contradictory.' }
    if (($record.descriptorObserved -and -not $record.allocationSucceeded) -or
        ($null -ne $record.descriptorError -and ($record.descriptorError -ge 0 -or -not $record.allocationSucceeded -or $record.descriptorObserved)) -or
        (($record.allocationSucceeded -and -not $record.descriptorObserved) -ne ($null -ne $record.descriptorError)) -or
        ($record.cpuRoundtripVerified -and (-not $record.descriptorObserved -or -not $record.allocationSucceeded))) { throw 'Native allocation evidence is contradictory.' }
    if ($record.mappingAttempted -ne ($null -ne $record.mappingResult) -or
        ($record.mappingAttempted -and (-not $opened -or -not $record.cpuRoundtripVerified -or -not $record.descriptorObserved)) -or
        ($null -ne $record.unmappingResult -and ($null -eq $record.mappingResult -or $record.mappingResult -ne 0)) -or
        ($record.freeCompleted -and (-not $record.allocationSucceeded -or ($record.mappingAttempted -and ($null -eq $record.unmappingResult -or $record.unmappingResult -ne 0))))) {
        throw 'Native session mapping evidence is contradictory.'
    }
    $uncertain = $record.mappingAttempted -and ($record.mappingResult -ne 0 -or ($null -ne $record.unmappingResult -and $record.unmappingResult -ne 0))
    if ($record.poisoned -ne $uncertain -or ($uncertain -and ($record.freeCompleted -or $null -ne $record.moduleCloseResult)) -or
        ($null -ne $record.moduleCloseResult -and (-not $opened -or ($record.allocationSucceeded -and -not $record.freeCompleted))) -or
        $record.moduleCloseCompleted -ne ($null -ne $record.moduleCloseResult -and $record.moduleCloseResult -eq 0)) { throw 'Native session cleanup evidence is contradictory.' }
    $passed = (-not $ReferenceInitialization -or ($initializationAttempted -and $record.processInitializationResult -eq 0)) -and
        $metadata -and $opened -and $record.allocationSucceeded -and $record.descriptorObserved -and
        $record.cpuRoundtripVerified -and $record.mappingAttempted -and $record.mappingResult -eq 0 -and
        $null -ne $record.unmappingResult -and $record.unmappingResult -eq 0 -and $record.freeCompleted -and
        -not $record.poisoned -and $record.moduleCloseCompleted -and $record.ownerPinnedUntilExit
    if ($record.passed -ne $passed -or ($passed -ne ($record.stage -ceq 'session-fd-cycle-completed-owner-pinned'))) { throw 'Native session completion is contradictory.' }
    $stageMatches = switch -CaseSensitive ($record.stage) {
        'session-metadata-incomplete' { -not $metadata -and -not $openAttempted -and (-not $initializationAttempted -or $record.processInitializationResult -eq 0) }
        'reference-process-init-refused-pinned' { $ReferenceInitialization -and $initializationAttempted -and $record.processInitializationResult -ne 0 -and -not $openAttempted }
        'module-open-refused-pinned' { $openAttempted -and $record.moduleOpenResult -ne 0 -and -not $opened }
        'module-handle-not-written-pinned' { $openAttempted -and $record.moduleOpenResult -eq 0 -and -not $record.handleObserved -and -not $opened }
        'allocation-refused' { $opened -and -not $record.allocationSucceeded -and $null -ne $record.moduleCloseResult }
        'descriptor-refused' { $record.allocationSucceeded -and -not $record.descriptorObserved -and $null -ne $record.descriptorError -and $record.descriptorError -lt 0 -and $record.freeCompleted -and $null -ne $record.moduleCloseResult }
        'allocation-not-page-aligned' { $record.allocationSucceeded -and $record.descriptorObserved -and -not $record.cpuRoundtripVerified -and -not $record.mappingAttempted -and $record.freeCompleted -and $null -ne $record.moduleCloseResult }
        'cpu-roundtrip-failed' { $record.allocationSucceeded -and $record.descriptorObserved -and -not $record.cpuRoundtripVerified -and -not $record.mappingAttempted -and $record.freeCompleted -and $null -ne $record.moduleCloseResult }
        'fd-mapping-failed-pinned' { $record.mappingAttempted -and $record.mappingResult -ne 0 -and $record.poisoned }
        'fd-unmapping-failed-pinned' { $record.mappingAttempted -and $record.mappingResult -eq 0 -and $null -ne $record.unmappingResult -and $record.unmappingResult -ne 0 -and $record.poisoned }
        'session-fd-cycle-completed-owner-pinned' { $passed }
        'module-close-refused-pinned' { $record.freeCompleted -and $record.mappingAttempted -and $null -ne $record.unmappingResult -and $record.unmappingResult -eq 0 -and $null -ne $record.moduleCloseResult -and $record.moduleCloseResult -ne 0 }
    }
    if (-not $stageMatches) { throw 'Native session terminal stage contradicts its raw outcomes.' }
    return $record
}

function Get-BonsaiHexagonSessionWorkerResult {
    <# .SYNOPSIS
    Validates retained native session evidence without granting admission or teardown.
    #>
    [CmdletBinding()]
    [OutputType([Collections.IDictionary])]
    param([Parameter(Mandatory)][string]$Json, [Parameter(Mandatory)][string]$ExpectedRunnerRevision, [switch]$ReferenceInitialization)
    $record = ConvertFrom-BonsaiHexagonDispatchJson -Json $Json
    if (($record.SchemaVersion -isnot [int] -and $record.SchemaVersion -isnot [long]) -or $record.SchemaVersion -ne 1 -or
        $record.Kind -isnot [string] -or $record.Kind -cne 'runner-hexagon-session-worker' -or
        $record.SourceRevision -isnot [string] -or $record.SourceRevision -cne $ExpectedRunnerRevision -or
        $record.Outcome -isnot [string] -or $record.Outcome -cnotin @('passed','pending') -or
        $record.Stage -isnot [string] -or $record.Stage -cnotin @('source-identity','runtime-identity','catalog-verification','driver-inspection','session-mapping','session-mapping-complete') -or
        $record.LastNativeStage -isnot [string] -or $record.LastNativeStage -cnotin @('not-started','reference-process-init-entered','module-open-entered','allocation-entered','descriptor-entered','fd-mapping-entered','fd-unmapping-entered','free-entered','module-close-entered') -or
        ($null -ne $record.Failure -and ($record.Failure -isnot [string] -or $record.Failure -cnotmatch '^[a-z][a-z0-9-]{0,95}$'))) {
        throw 'Invalid native session worker identity or stage.'
    }
    foreach ($field in @('ModelLoaded','SwiftQualified','ProfileEligible','DriverInstalled','TrustChanged','ComputeExecuted')) {
        if ($record[$field] -isnot [bool] -or $record[$field]) { throw 'Native session worker claimed an independent gate.' }
    }
    foreach ($field in @('SignedPackageVerified','DriverIdentityVerified','MappingCycleVerified')) {
        if ($record[$field] -isnot [bool]) { throw 'Native session worker requires exact booleans.' }
    }
    if ($record.ReferenceProcessInitializationSelected -isnot [bool] -or $record.ReferenceProcessInitializationSelected -ne [bool]$ReferenceInitialization -or
        ($null -ne $record.UnsignedProcessRequested -and $record.UnsignedProcessRequested -isnot [bool]) -or
        ($null -ne $record.ProcessInitializationResult -and (($record.ProcessInitializationResult -isnot [int] -and $record.ProcessInitializationResult -isnot [long]) -or
            $record.ProcessInitializationResult -lt [int]::MinValue -or $record.ProcessInitializationResult -gt [int]::MaxValue))) {
        throw 'Invalid native worker reference-process initialization selection or outcome.'
    }
    if (-not $ReferenceInitialization -and ($null -eq $record.UnsignedProcessRequested -or $record.UnsignedProcessRequested -or
        $null -ne $record.ProcessInitializationResult -or $record.LastNativeStage -ceq 'reference-process-init-entered')) {
        throw 'Default session worker claimed an unselected reference-process request.'
    }
    if ($ReferenceInitialization -and ($null -eq $record.UnsignedProcessRequested -or $record.UnsignedProcessRequested) -and
        (-not $record.DriverIdentityVerified -or $record.Stage -cnotin @('session-mapping','session-mapping-complete'))) { throw 'Reference-process request escaped signed and guarded native execution.' }
    if ($null -ne $record.ProcessInitializationResult -and ($null -eq $record.UnsignedProcessRequested -or -not $record.UnsignedProcessRequested)) { throw 'Native process-init outcome omitted its request.' }
    if (($record.RuntimeFilesVerified -isnot [int] -and $record.RuntimeFilesVerified -isnot [long]) -or
        $record.RuntimeFilesVerified -lt 0 -or $record.RuntimeFilesVerified -gt 16 -or
        ($record.SignedPackageVerified -and $record.RuntimeFilesVerified -ne 16)) { throw 'Invalid native package verification count.' }
    if ($record.DriverIdentityVerified) {
        if (-not $record.SignedPackageVerified -or $record.DriverIdentity -isnot [Collections.IDictionary]) { throw 'Verified driver omitted package/identity evidence.' }
        Assert-BonsaiHexagonDriverIdentity -Identity $record.DriverIdentity
    } elseif ($null -ne $record.DriverIdentity) { throw 'Unverified native driver identity was retained.' }
    foreach ($field in @('ExecutableSha256','BuildReceiptSha256','ManifestSha256','LockSha256')) {
        if ($null -ne $record[$field] -and ($record[$field] -isnot [string] -or $record[$field] -cnotmatch '^[a-f0-9]{64}$')) { throw 'Invalid native source receipt digest.' }
        if ($record.SignedPackageVerified -and $null -eq $record[$field]) { throw 'Verified native package omitted source binding.' }
    }
    if ($null -ne $record.NativeResult) {
        $native = Get-BonsaiHexagonSessionProbeResult -Json ($record.NativeResult | ConvertTo-Json -Depth 12 -Compress) -ExitCode 0 -ReferenceInitialization:$ReferenceInitialization
        if (-not $record.DriverIdentityVerified -or $native.driverDll -cne $record.DriverIdentity.Path -or
            $record.MappingCycleVerified -ne $native.passed -or $null -eq $record.UnsignedProcessRequested -or
            $record.UnsignedProcessRequested -ne $native.unsignedProcessRequested -or
            $record.ProcessInitializationResult -ne $native.processInitializationResult) { throw 'Native session result contradicts independently guarded driver evidence.' }
    } elseif ($record.MappingCycleVerified -or $null -ne $record.ProcessInitializationResult -or $record.UnsignedProcessRequested -eq $true) { throw 'Native mapping or process initialization was claimed without its complete result.' }
    if ($record.MappingCycleVerified -ne ($record.Outcome -ceq 'passed') -or
        ($record.MappingCycleVerified -and ($record.Stage -cne 'session-mapping-complete' -or $null -ne $record.Failure))) {
        throw 'Native session worker completion contradicts its evidence.'
    }
    return $record
}

function Invoke-BonsaiHexagonSessionWorker {
    <# .SYNOPSIS
    Verifies all signed package/source bytes before one owned native FD mapping probe.
    #>
    [CmdletBinding()]
    [OutputType([Collections.Specialized.OrderedDictionary])]
    param([Parameter(Mandatory)][Collections.IDictionary]$Request)
    if ($Request.ReferenceProcessInitializationSelected -isnot [bool]) { throw 'The reference-process initialization mode requires an explicit boolean selection.' }
    $holds = [Collections.Generic.List[IO.FileStream]]::new()
    $report = [ordered]@{
        SchemaVersion = 1; Kind = 'runner-hexagon-session-worker'; SourceRevision = $Request.ExpectedRunnerRevision
        Outcome = 'pending'; Stage = 'source-identity'; Failure = $null
        LastNativeStage = 'not-started'
        ReferenceProcessInitializationSelected = $Request.ReferenceProcessInitializationSelected
        UnsignedProcessRequested = $false; ProcessInitializationResult = $null
        SignedPackageVerified = $false; RuntimeFilesVerified = 0; DriverIdentityVerified = $false; DriverIdentity = $null
        MappingCycleVerified = $false; NativeResult = $null
        ExecutableSha256 = $null; BuildReceiptSha256 = $null; ManifestSha256 = $null; LockSha256 = $null
        ModelLoaded = $false; SwiftQualified = $false; ProfileEligible = $false
        DriverInstalled = $false; TrustChanged = $false; ComputeExecuted = $false
    }
    try {
        Save-BonsaiHexagonDispatchWorkerReceipt -Report $report -Path $Request.WorkerReceiptPath
        $environment = Get-BonsaiHexagonDispatchEnvironment -RuntimeDirectory $Request.PackageDirectory -SystemDirectory ([Environment]::SystemDirectory)
        foreach ($variable in @(Get-ChildItem Env:)) {
            if ($environment.RemoveNames -contains $variable.Name -or @($environment.RemovePrefixes | Where-Object { $variable.Name.StartsWith($_, [StringComparison]::OrdinalIgnoreCase) }).Count) {
                [Environment]::SetEnvironmentVariable($variable.Name, $null, 'Process')
            }
        }
        [Environment]::SetEnvironmentVariable('PATH', [Environment]::SystemDirectory, 'Process')
        $receiptPath = Join-Path $Request.FixtureDirectory 'native-build.json'
        $entries = @([IO.Directory]::EnumerateFileSystemEntries($Request.FixtureDirectory))
        if ($entries.Count -ne 2) { throw 'Native session bundle must contain exactly two files.' }
        foreach ($entry in $entries) {
            if ([IO.Path]::GetFileName($entry) -cnotin @('native-build.json','native-hexagon-query.exe') -or
                ([IO.File]::GetAttributes($entry) -band ([IO.FileAttributes]::Directory -bor [IO.FileAttributes]::ReparsePoint))) { throw 'Unreviewed native session bundle entry.' }
        }
        $receiptStream = [IO.FileStream]::new($receiptPath, 'Open', 'Read', 'Read'); $holds.Add($receiptStream)
        $receipt = Read-BonsaiHexagonDispatchJson -Stream $receiptStream
        $receiptStream.Position = 0
        $report.BuildReceiptSha256 = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($receiptStream)).ToLowerInvariant()
        $sourceDigests = @{}
        foreach ($name in @('lib.rs','windows.rs','mapping.rs','session.rs','session_native.rs','session_mapping.rs','session_mapping_native.rs','query.rs','sources.json')) {
            $stream = [IO.FileStream]::new((Join-Path $Request.NativeSourceDirectory $name), 'Open', 'Read', 'Read'); $holds.Add($stream)
            if ($stream.Length -le 0 -or $stream.Length -gt 1MB) { throw 'Native source exceeds its byte bound.' }
            $sourceDigests[$name] = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($stream)).ToLowerInvariant()
        }
        $manifestStream = [IO.FileStream]::new($Request.ManifestPath, 'Open', 'Read', 'Read'); $holds.Add($manifestStream)
        if ($manifestStream.Length -le 0 -or $manifestStream.Length -gt 65536) { throw 'Native manifest exceeds its byte bound.' }
        $report.ManifestSha256 = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($manifestStream)).ToLowerInvariant()
        $lockStream = [IO.FileStream]::new($Request.LockPath, 'Open', 'Read', 'Read'); $holds.Add($lockStream)
        $lock = Read-BonsaiHexagonDispatchJson -Stream $lockStream
        $lockStream.Position = 0
        $report.LockSha256 = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($lockStream)).ToLowerInvariant()
        Assert-BonsaiHexagonSessionBuild -Receipt $receipt -ExpectedRunnerRevision $Request.ExpectedRunnerRevision -SourceDigests $sourceDigests -ManifestSha256 $report.ManifestSha256 -LockSha256 $report.LockSha256
        $fixturePath = Join-Path $Request.FixtureDirectory 'native-hexagon-query.exe'
        $holds.Add((Get-BonsaiHexagonDispatchFile -Path $fixturePath -Pin @{ bytes = $receipt.Executable.Bytes; sha256 = $receipt.Executable.Sha256 }))
        Assert-Arm64PE -Path $fixturePath
        $report.ExecutableSha256 = $receipt.Executable.Sha256
        $report.Stage = 'runtime-identity'
        Save-BonsaiHexagonDispatchWorkerReceipt -Report $report -Path $Request.WorkerReceiptPath
        $members = @('libggml-htp-v73.so','libggml-htp-v75.so','libggml-htp-v79.so','libggml-htp-v81.so')
        if (($lock.schemaVersion -isnot [int] -and $lock.schemaVersion -isnot [long]) -or $lock.schemaVersion -ne 1 -or
            $lock.ggmlCommit -cne '94256114c229674ef96e76eb2dea596e65b43818' -or
            $lock.files -isnot [array] -or $lock.files.Count -ne 16 -or $lock.catalog.name -cne 'libggml-htp.cat' -or
            $lock.catalog.members -isnot [array] -or $lock.catalog.members.Count -ne 4) { throw 'Invalid native signed-package lock.' }
        for ($index = 0; $index -lt 4; $index++) {
            if ($lock.catalog.members[$index] -isnot [string] -or $lock.catalog.members[$index] -cne $members[$index]) { throw 'Native catalog member set changed.' }
        }
        $names = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
        [long]$total = 0
        foreach ($pin in $lock.files) {
            if ($pin -isnot [Collections.IDictionary] -or $pin.name -isnot [string] -or
                $pin.name -cnotmatch '^[A-Za-z0-9][A-Za-z0-9_.-]{0,127}$' -or -not $names.Add($pin.name)) { throw 'Invalid native runtime file pin.' }
            $holds.Add((Get-BonsaiHexagonDispatchFile -Path (Join-Path $Request.PackageDirectory $pin.name) -Pin $pin))
            $total += $pin.bytes
            if ($total -gt 256MB) { throw 'Native runtime package exceeds its byte bound.' }
            $report.RuntimeFilesVerified++
        }
        $entries = @([IO.Directory]::EnumerateFileSystemEntries($Request.PackageDirectory))
        if ($entries.Count -ne 16) { throw 'Native package directory contains unpinned files.' }
        foreach ($entry in $entries) {
            if (-not $names.Contains([IO.Path]::GetFileName($entry)) -or
                ([IO.File]::GetAttributes($entry) -band ([IO.FileAttributes]::Directory -bor [IO.FileAttributes]::ReparsePoint))) { throw 'Native runtime contains an unpinned entry or link.' }
        }
        foreach ($member in @('libggml-htp.cat','libggml-htp.inf') + $members) {
            if (-not $names.Contains($member)) { throw 'Native runtime omits a signed module deployment input.' }
        }
        $report.Stage = 'catalog-verification'
        Save-BonsaiHexagonDispatchWorkerReceipt -Report $report -Path $Request.WorkerReceiptPath
        $holds.Add((Get-BonsaiHexagonDispatchFile -Path $Request.SigntoolPath -Pin $lock.signtool))
        $arguments = @('verify','/pa','/v','/c',(Join-Path $Request.PackageDirectory 'libggml-htp.cat')) + @(foreach ($member in $members) { Join-Path $Request.PackageDirectory $member })
        $signature = Invoke-BonsaiNativeProcess -Executable $Request.SigntoolPath -ArgumentList $arguments -WorkingDirectory $Request.PackageDirectory -TimeoutSeconds 10
        if ($signature.ExitCode -ne 0) { throw 'Native module catalog membership verification failed.' }
        $report.SignedPackageVerified = $true
        $report.Stage = 'driver-inspection'
        Save-BonsaiHexagonDispatchWorkerReceipt -Report $report -Path $Request.WorkerReceiptPath
        $inspection = Invoke-BonsaiHexagonBoundedCapture -Executable $fixturePath -ArgumentList @('--inspect') -WorkingDirectory $Request.PackageDirectory
        $driver = ConvertFrom-BonsaiHexagonDispatchJson -Json $inspection.StandardOutput
        if ($inspection.ExitCode -ne 0 -or ($driver.schemaVersion -isnot [int] -and $driver.schemaVersion -isnot [long]) -or
            $driver.schemaVersion -ne 1 -or $driver.kind -isnot [string] -or $driver.kind -cne 'native-fastrpc-inspection' -or $driver.driverDll -isnot [string]) { throw 'Native driver inspection failed.' }
        foreach ($field in @('driverLoaded','dspCodeLoaded','profileEligible')) {
            if ($driver[$field] -isnot [bool] -or $driver[$field]) { throw 'Native inspection claimed execution.' }
        }
        $driverGuard = Get-BonsaiHexagonDriverGuard -Path $driver.driverDll
        $holds.Add($driverGuard.Stream)
        $report.DriverIdentity = $driverGuard.Identity
        $report.DriverIdentityVerified = $true
        $report.Stage = 'session-mapping'
        Save-BonsaiHexagonDispatchWorkerReceipt -Report $report -Path $Request.WorkerReceiptPath
        # This is the owned child worker's environment only. The query admits
        # exactly the guarded route and rechecks its own module file handles.
        [Environment]::SetEnvironmentVariable('ADSP_LIBRARY_PATH', $Request.PackageDirectory, 'Process')
        $mode = if ($Request.ReferenceProcessInitializationSelected) { '--reference-session-mapping-probe' } else { '--session-mapping-probe' }
        if ($Request.ReferenceProcessInitializationSelected) {
            # An entered marker alone cannot prove the control call executed.
            $report.UnsignedProcessRequested = $null
            Save-BonsaiHexagonDispatchWorkerReceipt -Report $report -Path $Request.WorkerReceiptPath
        }
        $probe = Invoke-BonsaiHexagonBoundedCapture -Executable $fixturePath -ArgumentList @($mode, $Request.PackageDirectory) -WorkingDirectory $Request.PackageDirectory -ProgressReceipt $report -ProgressPath $Request.WorkerReceiptPath -ProgressKind Session
        $native = Get-BonsaiHexagonSessionProbeResult -Json $probe.StandardOutput -ExitCode $probe.ExitCode -ReferenceInitialization:$Request.ReferenceProcessInitializationSelected
        if ($native.driverDll -cne $driverGuard.Identity.Path) { throw 'Native session loaded a different inspected driver.' }
        $report.NativeResult = $native
        $report.UnsignedProcessRequested = $native.unsignedProcessRequested
        $report.ProcessInitializationResult = $native.processInitializationResult
        if ($native.passed) {
            $report.MappingCycleVerified = $true
            $report.Outcome = 'passed'
            $report.Stage = 'session-mapping-complete'
        } else { $report.Failure = 'native-session-mapping-incomplete' }
    } catch {
        $report.Failure = $report.Stage + '-rejected'
        $report.Outcome = 'pending'
    } finally {
        # BoundedCapture cannot return until the child has exited. If it cannot
        # confirm exit, the outer kill-on-close Job owns these retained handles.
        foreach ($stream in $holds) { $stream.Dispose() }
        Save-BonsaiHexagonDispatchWorkerReceipt -Report $report -Path $Request.WorkerReceiptPath
    }
    return $report
}

Export-ModuleMember -Function Get-BonsaiHexagonSessionPlan, Assert-BonsaiHexagonSessionBuild, Get-BonsaiHexagonSessionProbeResult, Get-BonsaiHexagonSessionWorkerResult, Invoke-BonsaiHexagonSessionWorker
