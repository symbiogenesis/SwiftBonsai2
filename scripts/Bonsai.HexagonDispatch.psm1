#requires -Version 7.6
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1')
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Native.psm1')

function Get-BonsaiHexagonDispatchPlan {
    <# .SYNOPSIS
    Describes the fixed supported-type access fixture without opening files or devices.
    #>
    [CmdletBinding()]
    [OutputType([Collections.Specialized.OrderedDictionary])]
    param([ValidateRange(1,240)][int]$MaximumSeconds = 120)
    return [ordered]@{
        SchemaVersion = 1; Kind = 'runner-hexagon-dispatch-plan'; MaximumSeconds = $MaximumSeconds
        CleanupReserveSeconds = 75; ModelLoaded = $false; CpuFallbackAllowed = $false
        OwnedJobRequired = $true; CatalogMemberVerificationRequired = $true
        SourceBoundHostedFixtureRequired = $true; AsInvokerManifestRequired = $true; DriverInstallationAllowed = $false
        Cases = @(@{ Columns = 128; Rows = 32; Batch = 1 }, @{ Columns = 128; Rows = 32; Batch = 32 })
        Qualification = 'device-access-only'; ProfileEligible = $false
    }
}

function Assert-BonsaiHexagonDispatchBuild {
    <# .SYNOPSIS
    Checks exact build/source identity without treating an asserted receipt as provenance.
    #>
    [CmdletBinding()]
    [OutputType([void])]
    param([Parameter(Mandatory)][Collections.IDictionary]$Receipt,
        [Parameter(Mandatory)][string]$ExpectedRunnerRevision,
        [Parameter(Mandatory)][string]$SourceSha256,
        [Parameter(Mandatory)][string]$ManifestSha256,
        [Parameter(Mandatory)][string]$LockSha256)
    foreach ($digest in @($SourceSha256, $ManifestSha256, $LockSha256)) {
        if ($digest -cnotmatch '^[a-f0-9]{64}$') { throw 'Invalid dispatch source identity.' }
    }
    if ($ExpectedRunnerRevision -cnotmatch '^[a-f0-9]{40}$' -or
        ($Receipt.SchemaVersion -isnot [long] -and $Receipt.SchemaVersion -isnot [int]) -or $Receipt.SchemaVersion -ne 1 -or
        $Receipt.RunnerRevision -isnot [string] -or $Receipt.RunnerRevision -cne $ExpectedRunnerRevision -or
        $Receipt.RustToolchain -isnot [string] -or $Receipt.RustToolchain -cne 'nightly-2026-10-22-aarch64-pc-windows-msvc' -or
        $Receipt.SourceSha256 -isnot [string] -or $Receipt.SourceSha256 -cne $SourceSha256 -or
        $Receipt.ManifestSha256 -isnot [string] -or $Receipt.ManifestSha256 -cne $ManifestSha256 -or
        $Receipt.LockSha256 -isnot [string] -or $Receipt.LockSha256 -cne $LockSha256 -or
        $Receipt.HostedBuild -isnot [bool] -or -not $Receipt.HostedBuild -or
        $Receipt.Target -isnot [string] -or $Receipt.Target -cne 'aarch64-pc-windows-msvc' -or
        $Receipt.CompilerIdentitySha256 -isnot [string] -or $Receipt.CompilerIdentitySha256 -cnotmatch '^[a-f0-9]{64}$' -or
        $Receipt.Executable -isnot [Collections.IDictionary]) { throw 'Dispatch fixture build identity differs from the trusted source selection.' }
    $entry = $Receipt.Executable
    if ($entry.Name -isnot [string] -or $entry.Name -cne 'hexagon-dispatch-fixture.exe' -or
        ($entry.Bytes -isnot [long] -and $entry.Bytes -isnot [int]) -or
        $entry.Bytes -le 0 -or $entry.Bytes -gt 16MB -or
        $entry.Sha256 -isnot [string] -or $entry.Sha256 -cnotmatch '^[a-f0-9]{64}$') {
        throw 'Invalid dispatch fixture executable identity.'
    }
}

function Get-BonsaiHexagonDispatchEnvironment {
    <# .SYNOPSIS
    Returns the reviewed child-only environment removal and lookup policy.
    #>
    [CmdletBinding()]
    [OutputType([Collections.Specialized.OrderedDictionary])]
    param([Parameter(Mandatory)][string]$RuntimeDirectory, [Parameter(Mandatory)][string]$SystemDirectory)
    return [ordered]@{
        RemovePrefixes = @('GGML_', 'GENIEX_', 'QNN_', 'QAIRT_', 'HEXAGON_', 'ADSP_', 'CDSP_', 'FASTRPC_', 'RPCMEM_')
        RemoveNames = @('ACTIONS_RUNTIME_TOKEN', 'ACTIONS_ID_TOKEN_REQUEST_TOKEN', 'ACTIONS_ID_TOKEN_REQUEST_URL',
            'GITHUB_TOKEN', 'GH_TOKEN', 'ACTIONS_RUNNER_INPUT_TOKEN', 'LD_LIBRARY_PATH', 'LD_PRELOAD')
        Set = @{ PATH = $RuntimeDirectory + ';' + $SystemDirectory; ADSP_LIBRARY_PATH = $RuntimeDirectory }
    }
}

function ConvertFrom-BonsaiHexagonDispatchJson {
    <# .SYNOPSIS
    Rejects ambiguous object names before PowerShell can overwrite JSON evidence.
    #>
    [CmdletBinding()]
    [OutputType([Collections.IDictionary])]
    param([Parameter(Mandatory)][AllowEmptyString()][string]$Json)
    if ([Text.Encoding]::UTF8.GetByteCount($Json) -gt 65536) { throw 'Dispatch JSON exceeds its byte bound.' }
    $options = [System.Text.Json.JsonDocumentOptions]::new()
    $options.MaxDepth = 32
    $document = [System.Text.Json.JsonDocument]::Parse($Json, $options)
    try {
        $pending = [Collections.Generic.Stack[System.Text.Json.JsonElement]]::new()
        $pending.Push($document.RootElement)
        while ($pending.Count) {
            $element = $pending.Pop()
            if ($element.ValueKind -eq [System.Text.Json.JsonValueKind]::Object) {
                $names = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
                foreach ($property in $element.EnumerateObject()) {
                    if (-not $names.Add($property.Name)) { throw 'Dispatch JSON contains duplicate object names.' }
                    $pending.Push($property.Value)
                }
            } elseif ($element.ValueKind -eq [System.Text.Json.JsonValueKind]::Array) {
                foreach ($value in $element.EnumerateArray()) { $pending.Push($value) }
            }
        }
    } finally { $document.Dispose() }
    $record = ConvertFrom-Json -InputObject $Json -AsHashtable -NoEnumerate -DateKind String -Depth 32
    if ($record -isnot [Collections.IDictionary]) { throw 'Dispatch JSON must contain one object.' }
    return $record
}

function Get-BonsaiHexagonDispatchFixtureResult {
    <# .SYNOPSIS
    Requires explicit device dispatch and every numeric output rather than library presence.
    #>
    [CmdletBinding()]
    [OutputType([Collections.IDictionary])]
    param([Parameter(Mandatory)][AllowEmptyString()][string]$Json, [Parameter(Mandatory)][int]$ExitCode)
    $record = ConvertFrom-BonsaiHexagonDispatchJson -Json $Json
    if ($record -isnot [Collections.IDictionary] -or
        ($record.SchemaVersion -isnot [long] -and $record.SchemaVersion -isnot [int]) -or $record.SchemaVersion -ne 1 -or
        $record.Kind -isnot [string] -or $record.Kind -cne 'runner-hexagon-dispatch-fixture' -or
        $record.SourceRevision -isnot [string] -or $record.SourceRevision -cne '94256114c229674ef96e76eb2dea596e65b43818' -or
        $record.Outcome -isnot [string] -or $record.Outcome -cnotin @('passed', 'pending', 'failed') -or
        $record.SelfTest -isnot [bool] -or $record.SelfTest -or
        $record.DispatchEvidence -isnot [string] -or $record.DispatchEvidence -cne 'explicit-hexagon-compute-sync-and-full-output-oracle' -or
        $record.SessionCleanup -isnot [string] -or $record.SessionCleanup -cne 'supervised-process-exit-required' -or
        $record.Stage -isnot [string] -or $record.Stage -cnotmatch '^[a-z][a-z0-9-]{0,63}$' -or
        ($null -ne $record.Failure -and ($record.Failure -isnot [string] -or $record.Failure -cnotmatch '^[a-z][a-z0-9-]{0,95}$')) -or
        $record.DeviceDispatchVerified -isnot [bool] -or
        $record.CpuFallbackAllowed -isnot [bool] -or $record.CpuFallbackAllowed -or
        $record.ModelLoaded -isnot [bool] -or $record.ModelLoaded -or
        $record.SwiftQualified -isnot [bool] -or $record.SwiftQualified -or
        $record.ProfileEligible -isnot [bool] -or $record.ProfileEligible -or
        $record.Cases -isnot [array] -or $record.Cases.Count -gt 2) {
        throw 'Invalid dispatch fixture result contract.'
    }
    if ($null -ne $record.NativeCode -and
        (($record.NativeCode -isnot [long] -and $record.NativeCode -isnot [int]) -or
            $record.NativeCode -lt 0 -or $record.NativeCode -gt [uint]::MaxValue)) { throw 'Invalid native dispatch error code.' }
    if ($null -ne $record.BackendName -and ($record.BackendName -isnot [string] -or $record.BackendName -cne 'HTP0')) {
        throw 'Dispatch reported an unreviewed backend identity.'
    }
    $expectedExit = switch -CaseSensitive ($record.Outcome) { 'passed' { 0 }; 'pending' { 2 }; 'failed' { 1 } }
    if ($ExitCode -ne $expectedExit -or $record.DeviceDispatchVerified -ne ($record.Outcome -ceq 'passed')) {
        throw 'Dispatch exit status and device result disagree.'
    }
    if ($record.Outcome -ceq 'passed') {
        if ($record.Cases.Count -ne 2 -or $null -ne $record.Failure -or $null -ne $record.NativeCode -or
            $record.BackendName -cne 'HTP0' -or $record.Stage -cne 'numeric-dispatch-complete') {
            throw 'Dispatch did not complete both fixed numeric cases on HTP0.'
        }
    } elseif ($null -eq $record.Failure) { throw 'Incomplete dispatch omitted its fixed failure reason.' }
    $batches = [Collections.Generic.HashSet[int]]::new()
    foreach ($case in $record.Cases) {
        if ($case -isnot [Collections.IDictionary]) { throw 'Dispatch case must be an object.' }
        foreach ($field in @('Columns', 'Rows', 'Batch', 'OutputValuesChecked')) {
            if ($case[$field] -isnot [long] -and $case[$field] -isnot [int]) { throw 'Dispatch case requires exact integer extents.' }
        }
        if ($record.BackendName -cne 'HTP0' -or $case.Columns -ne 128 -or $case.Rows -ne 32 -or
            $case.Batch -notin @(1, 32) -or -not $batches.Add([int]$case.Batch) -or
            $case.OutputValuesChecked -ne (32 * $case.Batch) -or
            $case.MatmulParity -isnot [bool] -or -not $case.MatmulParity -or
            $case.ExecutedOnHexagon -isnot [bool] -or -not $case.ExecutedOnHexagon) {
            throw 'Dispatch fixture output lacks complete fixed-shape parity.'
        }
    }
    return $record
}

function Get-BonsaiHexagonDispatchWorkerResult {
    <# .SYNOPSIS
    Validates complete or partial worker evidence without granting teardown authority.
    #>
    [CmdletBinding()]
    [OutputType([Collections.IDictionary])]
    param([Parameter(Mandatory)][string]$Json, [Parameter(Mandatory)][string]$ExpectedRunnerRevision)
    $record = ConvertFrom-BonsaiHexagonDispatchJson -Json $Json
    if (($record.SchemaVersion -isnot [long] -and $record.SchemaVersion -isnot [int]) -or $record.SchemaVersion -ne 1 -or
        $record.Kind -isnot [string] -or $record.Kind -cne 'runner-hexagon-dispatch-worker' -or
        $record.SourceRevision -isnot [string] -or $record.SourceRevision -cne $ExpectedRunnerRevision -or
        $record.Outcome -isnot [string] -or $record.Outcome -cnotin @('passed', 'pending', 'failed') -or
        $record.Stage -isnot [string] -or $record.Stage -cnotmatch '^[a-z][a-z0-9-]{0,63}$' -or
        $record.LastFixtureStage -isnot [string] -or $record.LastFixtureStage -cnotin @('not-started', 'load', 'registry', 'backend-init', 'context', 'allocation', 'transfer', 'dispatch', 'readback') -or
        ($null -ne $record.Failure -and ($record.Failure -isnot [string] -or $record.Failure -cnotmatch '^[a-z][a-z0-9-]{0,95}$')) -or
        $record.SelectedDspArchitecture -isnot [string] -or $record.SelectedDspArchitecture -cne 'unknown') {
        throw 'Invalid dispatch worker identity or stage.'
    }
    foreach ($field in @('ModelLoaded', 'SwiftQualified', 'ProfileEligible', 'DriverInstalled', 'TrustChanged')) {
        if ($record[$field] -isnot [bool] -or $record[$field]) { throw 'Dispatch worker claimed an independent qualification or deployment gate.' }
    }
    if ($record.DeviceDispatchVerified -isnot [bool] -or $record.DeviceDispatchVerified -ne ($record.Outcome -ceq 'passed') -or
        $record.CatalogMemberVerified -isnot [bool] -or
        ($record.CatalogMembersVerified -isnot [long] -and $record.CatalogMembersVerified -isnot [int]) -or
        $record.CatalogMemberNames -isnot [array] -or
        ($record.RuntimeFilesVerified -isnot [long] -and $record.RuntimeFilesVerified -isnot [int]) -or
        $record.RuntimeFilesVerified -lt 0 -or $record.RuntimeFilesVerified -gt 16) {
        throw 'Invalid dispatch worker completion counters.'
    }
    $members = @('libggml-htp-v73.so', 'libggml-htp-v75.so', 'libggml-htp-v79.so', 'libggml-htp-v81.so')
    if ($record.CatalogMemberVerified) {
        if ($record.CatalogMembersVerified -ne 4 -or $record.CatalogMemberNames.Count -ne 4) { throw 'Dispatch omitted reviewed catalog members.' }
        for ($index = 0; $index -lt 4; $index++) {
            if ($record.CatalogMemberNames[$index] -isnot [string] -or $record.CatalogMemberNames[$index] -cne $members[$index]) { throw 'Dispatch catalog member identity changed.' }
        }
    } elseif ($record.CatalogMembersVerified -ne 0 -or $record.CatalogMemberNames.Count) { throw 'Unverified catalog membership was counted.' }
    foreach ($field in @('FixtureSha256', 'FixtureSourceSha256', 'ManifestSha256', 'LockSha256', 'SigntoolSha256')) {
        if ($null -ne $record[$field] -and ($record[$field] -isnot [string] -or $record[$field] -cnotmatch '^[a-f0-9]{64}$')) { throw 'Invalid dispatch worker source digest.' }
        if ($record.DeviceDispatchVerified -and $null -eq $record[$field]) { throw 'Completed dispatch omitted its source identity.' }
    }
    if ($null -ne $record.FixtureResult) {
        $fixtureExit = switch -CaseSensitive ($record.FixtureResult.Outcome) { 'passed' { 0 }; 'pending' { 2 }; 'failed' { 1 }; default { throw 'Unknown nested fixture outcome.' } }
        $fixture = Get-BonsaiHexagonDispatchFixtureResult -Json ($record.FixtureResult | ConvertTo-Json -Depth 16 -Compress) -ExitCode $fixtureExit
        if ($record.Outcome -cne $fixture.Outcome -or $record.Stage -cne $fixture.Stage -or $record.Failure -cne $fixture.Failure) { throw 'Dispatch worker and fixture disagree.' }
    } elseif ($record.DeviceDispatchVerified -or $record.Outcome -ceq 'failed') { throw 'Dispatch claimed execution without numeric fixture evidence.' }
    if ($record.DeviceDispatchVerified -and (-not $record.CatalogMemberVerified -or $record.RuntimeFilesVerified -ne 16)) {
        throw 'Completed dispatch omitted verified runtime inputs.'
    }
    return $record
}

function Get-BonsaiHexagonDispatchFile {
    <# .SYNOPSIS
    Holds exact checked file bytes against replacement until the diagnostic exits.
    #>
    [CmdletBinding()]
    [OutputType([IO.FileStream])]
    param([Parameter(Mandatory)][string]$Path, [Parameter(Mandatory)][Collections.IDictionary]$Pin)
    if (($Pin.bytes -isnot [long] -and $Pin.bytes -isnot [int]) -or $Pin.bytes -le 0 -or $Pin.bytes -gt 128MB -or
        $Pin.sha256 -isnot [string] -or $Pin.sha256 -cnotmatch '^[a-f0-9]{64}$') { throw 'Invalid dispatch file pin.' }
    $full = Resolve-ExternalPath $Path
    $stream = [IO.FileStream]::new($full, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
    try {
        if ($stream.Length -ne $Pin.bytes) { throw 'Dispatch file length differs from its pin.' }
        $digest = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($stream)).ToLowerInvariant()
        if ($digest -cne $Pin.sha256) { throw 'Dispatch file hash differs from its pin.' }
        $stream.Position = 0
        return $stream
    } catch { $stream.Dispose(); throw }
}

function Read-BonsaiHexagonDispatchJson {
    <# .SYNOPSIS
    Reads one bounded JSON object from an already retained source or receipt handle.
    #>
    [CmdletBinding()]
    [OutputType([Collections.IDictionary])]
    param([Parameter(Mandatory)][IO.FileStream]$Stream)
    if ($Stream.Length -le 0 -or $Stream.Length -gt 65536) { throw 'Dispatch metadata exceeds its byte bound.' }
    $bytes = [byte[]]::new([int]$Stream.Length)
    $Stream.Position = 0
    $Stream.ReadExactly($bytes)
    $json = [Text.UTF8Encoding]::new($false, $true).GetString($bytes)
    return ConvertFrom-BonsaiHexagonDispatchJson -Json $json
}

function Save-BonsaiHexagonDispatchWorkerReceipt {
    <# .SYNOPSIS
    Retains completed verification stages before a later dispatch can block or fail.
    #>
    [CmdletBinding()]
    [OutputType([void])]
    param([Parameter(Mandatory)][Collections.IDictionary]$Report, [Parameter(Mandatory)][string]$Path)
    $temporary = $Path + '.' + [guid]::NewGuid().ToString('N') + '.tmp'
    $stream = [IO.FileStream]::new($temporary, 'CreateNew', 'Write', 'None')
    try {
        $bytes = [Text.UTF8Encoding]::new($false).GetBytes(($Report | ConvertTo-Json -Depth 16) + "`n")
        $stream.Write($bytes)
        $stream.Flush($true)
    } finally { $stream.Dispose() }
    [IO.File]::Move($temporary, $Path, $true)
}

function Invoke-BonsaiHexagonBoundedCapture {
    <# .SYNOPSIS
    Captures fixed fixture streams with byte caps while the surrounding Job owns lifetime.
    .DESCRIPTION
    This private helper runs only inside the parent's owned worker. The shared
    Job deadline remains authoritative while reads, process exit or teardown
    wait. Never return in-use buffers or checked input handles to the caller
    before the fixture's process exit is confirmed.
    #>
    [CmdletBinding()]
    [OutputType([pscustomobject])]
    param([Parameter(Mandatory)][string]$Executable,
        [AllowEmptyCollection()][string[]]$ArgumentList = @(),
        [Parameter(Mandatory)][string]$WorkingDirectory,
        [Collections.IDictionary]$ProgressReceipt,
        [string]$ProgressPath)
    if (($null -ne $ProgressReceipt) -ne (-not [string]::IsNullOrEmpty($ProgressPath))) { throw 'Progress receipt and path must be supplied together.' }
    $start = [Diagnostics.ProcessStartInfo]::new($Executable)
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.WorkingDirectory = $WorkingDirectory
    $start.RedirectStandardInput = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    foreach ($argument in $ArgumentList) { $start.ArgumentList.Add($argument) }
    $child = [Diagnostics.Process]::new()
    $child.StartInfo = $start
    $started = $false
    $channels = @()
    try {
        if (-not $child.Start()) { throw 'The fixed fixture failed to start.' }
        $started = $true
        $child.StandardInput.Close()
        $channels = @(
            @{ Stream = $child.StandardOutput.BaseStream; Bytes = [byte[]]::new(4096); Store = [IO.MemoryStream]::new(); Limit = 131072; Ended = $false; Pending = $null },
            @{ Stream = $child.StandardError.BaseStream; Bytes = [byte[]]::new(4096); Store = [IO.MemoryStream]::new(); Limit = 65536; Ended = $false; Pending = $null }
        )
        foreach ($channel in $channels) { $channel.Pending = $channel.Stream.ReadAsync($channel.Bytes, 0, $channel.Bytes.Length) }
        while ($true) {
            foreach ($channel in $channels) {
                if (-not $channel.Ended -and $channel.Pending.IsCompleted) {
                    $count = $channel.Pending.GetAwaiter().GetResult()
                    if ($count -eq 0) { $channel.Ended = $true }
                    else {
                        if ($channel.Store.Length + $count -gt $channel.Limit) { throw 'Fixture stream exceeded its fixed byte cap.' }
                        $channel.Store.Write($channel.Bytes, 0, $count)
                        if ($null -ne $ProgressReceipt -and [object]::ReferenceEquals($channel, $channels[1])) {
                            # Retain only fixed executable markers, never arbitrary
                            # vendor stderr, local paths or credential text. A later
                            # abort/hang must not erase the last entered fixture stage.
                            $progress = [Text.Encoding]::UTF8.GetString($channel.Store.ToArray())
                            $markers = [regex]::Matches($progress, '(?m)^hexagon-fixture:stage=(load|registry|backend-init|context|allocation|transfer|dispatch|readback)\r?\n')
                            if ($markers.Count) {
                                $stage = $markers[$markers.Count - 1].Groups[1].Value
                                if ($ProgressReceipt.LastFixtureStage -cne $stage) {
                                    $ProgressReceipt.LastFixtureStage = $stage
                                    Save-BonsaiHexagonDispatchWorkerReceipt -Report $ProgressReceipt -Path $ProgressPath
                                }
                            }
                        }
                        $channel.Pending = $channel.Stream.ReadAsync($channel.Bytes, 0, $channel.Bytes.Length)
                    }
                }
            }
            if ($channels[0].Ended -and $channels[1].Ended -and $child.HasExited) { break }
            # Both pipe reads stay outstanding concurrently; a quiet stderr
            # cannot block stdout drainage or turn a byte cap into a deadlock.
            [Threading.Thread]::Sleep(10)
        }
        $utf8 = [Text.UTF8Encoding]::new($false, $true)
        return [pscustomobject]@{
            ExitCode = $child.ExitCode
            StandardOutput = $utf8.GetString($channels[0].Store.ToArray())
            StandardError = $utf8.GetString($channels[1].Store.ToArray())
        }
    } finally {
        if ($started) {
            if (-not $child.HasExited) {
                try { $child.Kill($true) } catch { [void]$child.WaitForExit(1) }
            }
            # If termination is refused, keep checked inputs and async buffers
            # alive until the outer Job deadline tears down the entire worker.
            # Returning or disposing them while native code remains live is invalid.
            while (-not $child.WaitForExit(1000)) { }
        }
        foreach ($channel in $channels) { $channel.Stream.Dispose(); $channel.Store.Dispose() }
        $child.Dispose()
    }
}

function Invoke-BonsaiHexagonDispatchWorker {
    <# .SYNOPSIS
    Verifies pinned files and catalog membership inside the parent's owned Job before dispatch.
    #>
    [CmdletBinding()]
    [OutputType([Collections.Specialized.OrderedDictionary])]
    param([Parameter(Mandatory)][Collections.IDictionary]$Request)
    $holds = [Collections.Generic.List[IO.FileStream]]::new()
    $report = [ordered]@{
        SchemaVersion = 1; Kind = 'runner-hexagon-dispatch-worker'; Outcome = 'pending'; Stage = 'source-identity'
        Failure = $null; CatalogMemberVerified = $false; CatalogMembersVerified = 0; CatalogMemberNames = @()
        SelectedDspArchitecture = 'unknown'; LastFixtureStage = 'not-started'; DeviceDispatchVerified = $false; FixtureResult = $null
        FixtureSha256 = $null; FixtureSourceSha256 = $null; ManifestSha256 = $null; LockSha256 = $null; SigntoolSha256 = $null
        RuntimeFilesVerified = 0; SourceRevision = $Request.ExpectedRunnerRevision
        ModelLoaded = $false; SwiftQualified = $false; ProfileEligible = $false; DriverInstalled = $false; TrustChanged = $false
    }
    try {
        Save-BonsaiHexagonDispatchWorkerReceipt -Report $report -Path $Request.WorkerReceiptPath
        $environment = Get-BonsaiHexagonDispatchEnvironment -RuntimeDirectory $Request.PackageDirectory -SystemDirectory ([Environment]::SystemDirectory)
        foreach ($variable in @(Get-ChildItem Env:)) {
            if ($environment.RemoveNames -contains $variable.Name -or @($environment.RemovePrefixes | Where-Object { $variable.Name.StartsWith($_, [StringComparison]::OrdinalIgnoreCase) }).Count) {
                [Environment]::SetEnvironmentVariable($variable.Name, $null, 'Process')
            }
        }
        foreach ($name in $environment.Set.Keys) { [Environment]::SetEnvironmentVariable($name, $environment.Set[$name], 'Process') }
        $lockStream = [IO.FileStream]::new($Request.LockPath, 'Open', 'Read', 'Read')
        $holds.Add($lockStream)
        $lock = Read-BonsaiHexagonDispatchJson -Stream $lockStream
        $lockStream.Position = 0
        $lockDigest = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($lockStream)).ToLowerInvariant()
        $report.LockSha256 = $lockDigest
        if ($lock.schemaVersion -ne 1 -or $lock.ggmlCommit -cne '94256114c229674ef96e76eb2dea596e65b43818' -or
            $lock.files -isnot [array] -or $lock.files.Count -lt 3 -or $lock.files.Count -gt 32 -or
            $lock.catalog.name -cne 'libggml-htp.cat' -or $lock.catalog.member -cne 'libggml-htp-v81.so' -or
            $lock.signtool -isnot [Collections.IDictionary]) { throw 'Invalid exact signed-runtime selection.' }
        $expectedMembers = @('libggml-htp-v73.so', 'libggml-htp-v75.so', 'libggml-htp-v79.so', 'libggml-htp-v81.so')
        if ($lock.catalog.members -isnot [array] -or $lock.catalog.members.Count -ne $expectedMembers.Count) {
            throw 'Every possible bundled DSP architecture requires catalog membership verification.'
        }
        for ($memberIndex = 0; $memberIndex -lt $expectedMembers.Count; $memberIndex++) {
            if ($lock.catalog.members[$memberIndex] -isnot [string] -or $lock.catalog.members[$memberIndex] -cne $expectedMembers[$memberIndex]) {
                throw 'The catalog member set differs from the exact reviewed bundle.'
            }
        }
        $sourceStream = [IO.FileStream]::new($Request.SourcePath, 'Open', 'Read', 'Read')
        $holds.Add($sourceStream)
        if ($sourceStream.Length -le 0 -or $sourceStream.Length -gt 1MB) { throw 'Dispatch source exceeds its byte bound.' }
        $sourceDigest = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($sourceStream)).ToLowerInvariant()
        $report.FixtureSourceSha256 = $sourceDigest
        $manifestStream = [IO.FileStream]::new($Request.ManifestPath, 'Open', 'Read', 'Read')
        $holds.Add($manifestStream)
        if ($manifestStream.Length -le 0 -or $manifestStream.Length -gt 65536) { throw 'Dispatch manifest exceeds its byte bound.' }
        $manifestDigest = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($manifestStream)).ToLowerInvariant()
        $report.ManifestSha256 = $manifestDigest
        $manifestStream.Position = 0
        $xmlSettings = [Xml.XmlReaderSettings]::new()
        $xmlSettings.DtdProcessing = [Xml.DtdProcessing]::Prohibit
        $xmlSettings.XmlResolver = $null
        $xml = [Xml.XmlDocument]::new()
        $xml.XmlResolver = $null
        $reader = [Xml.XmlReader]::Create($manifestStream, $xmlSettings)
        try { $xml.Load($reader) } finally { $reader.Dispose() }
        $levels = $xml.SelectNodes("//*[local-name()='requestedExecutionLevel']")
        if ($levels.Count -ne 1 -or $levels[0].GetAttribute('level') -cne 'asInvoker' -or $levels[0].GetAttribute('uiAccess') -cne 'false') {
            throw 'The dispatch fixture requires an explicit non-elevating manifest.'
        }
        $bundleEntries = @([IO.Directory]::EnumerateFileSystemEntries($Request.FixtureDirectory))
        if ($bundleEntries.Count -ne 2) { throw 'The fixture bundle must contain exactly two pinned entries.' }
        foreach ($bundleEntry in $bundleEntries) {
            if ([IO.Path]::GetFileName($bundleEntry) -cnotin @('hexagon-dispatch-fixture.exe', 'fixture-build.json') -or
                ([IO.File]::GetAttributes($bundleEntry) -band ([IO.FileAttributes]::Directory -bor [IO.FileAttributes]::ReparsePoint))) {
                throw 'The fixture bundle contains an unreviewed dependency or link.'
            }
        }
        $receiptPath = Join-Path $Request.FixtureDirectory 'fixture-build.json'
        $receiptStream = [IO.FileStream]::new($receiptPath, 'Open', 'Read', 'Read')
        $holds.Add($receiptStream)
        $receipt = Read-BonsaiHexagonDispatchJson -Stream $receiptStream
        Assert-BonsaiHexagonDispatchBuild -Receipt $receipt -ExpectedRunnerRevision $Request.ExpectedRunnerRevision -SourceSha256 $sourceDigest -ManifestSha256 $manifestDigest -LockSha256 $lockDigest
        $fixturePath = Join-Path $Request.FixtureDirectory 'hexagon-dispatch-fixture.exe'
        $holds.Add((Get-BonsaiHexagonDispatchFile -Path $fixturePath -Pin @{ bytes = $receipt.Executable.Bytes; sha256 = $receipt.Executable.Sha256 }))
        Assert-Arm64PE -Path $fixturePath
        $report.FixtureSha256 = $receipt.Executable.Sha256
        $report.Stage = 'runtime-identity'
        Save-BonsaiHexagonDispatchWorkerReceipt -Report $report -Path $Request.WorkerReceiptPath
        $names = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
        [long]$totalBytes = 0
        foreach ($pin in $lock.files) {
            if ($pin -isnot [Collections.IDictionary] -or $pin.name -isnot [string] -or
                $pin.name -cnotmatch '^[A-Za-z0-9][A-Za-z0-9_.-]{0,127}$' -or -not $names.Add($pin.name)) {
                throw 'Invalid or duplicate runtime filename.'
            }
            $holds.Add((Get-BonsaiHexagonDispatchFile -Path (Join-Path $Request.PackageDirectory $pin.name) -Pin $pin))
            $totalBytes += $pin.bytes
            if ($totalBytes -gt 256MB) { throw 'Runtime package exceeds the aggregate byte bound.' }
            $report.RuntimeFilesVerified++
        }
        $seen = 0
        foreach ($entry in [IO.Directory]::EnumerateFileSystemEntries($Request.PackageDirectory)) {
            $seen++
            if ($seen -gt 32 -or -not $names.Contains([IO.Path]::GetFileName($entry)) -or
                ([IO.File]::GetAttributes($entry) -band ([IO.FileAttributes]::Directory -bor [IO.FileAttributes]::ReparsePoint))) {
                throw 'Runtime directory contains unpinned entries.'
            }
        }
        if ($seen -ne $names.Count -or -not $names.Contains($lock.catalog.name) -or -not $names.Contains($lock.catalog.member)) {
            throw 'Runtime directory omits a pinned catalog or DSP member.'
        }
        foreach ($member in $expectedMembers) {
            if (-not $names.Contains($member)) { throw 'The runtime omits a possible auto-selected DSP library.' }
        }
        $report.Stage = 'catalog-verification'
        Save-BonsaiHexagonDispatchWorkerReceipt -Report $report -Path $Request.WorkerReceiptPath
        $holds.Add((Get-BonsaiHexagonDispatchFile -Path $Request.SigntoolPath -Pin $lock.signtool))
        $report.SigntoolSha256 = $lock.signtool.sha256
        # The generic backend auto-detects its DSP architecture. Verify all four
        # possible pinned libraries, not just v81, before allowing that choice.
        # This neither installs the catalog nor identifies the selected DSP.
        $signatureArguments = @('verify', '/pa', '/v', '/c', (Join-Path $Request.PackageDirectory $lock.catalog.name)) +
            @(foreach ($member in $expectedMembers) { Join-Path $Request.PackageDirectory $member })
        $signature = Invoke-BonsaiNativeProcess -Executable $Request.SigntoolPath -ArgumentList $signatureArguments -WorkingDirectory $Request.PackageDirectory -TimeoutSeconds 10
        if ($signature.ExitCode -ne 0) { $report.Failure = 'catalog-membership-unverified'; return $report }
        $report.CatalogMemberVerified = $true
        $report.CatalogMembersVerified = $expectedMembers.Count
        $report.CatalogMemberNames = $expectedMembers
        $report.Stage = 'device-dispatch'
        Save-BonsaiHexagonDispatchWorkerReceipt -Report $report -Path $Request.WorkerReceiptPath
        # The surrounding worker already belongs to the owner's kill-on-close
        # Job. The fixture inherits it before any DSP/runtime code can execute.
        $fixture = Invoke-BonsaiHexagonBoundedCapture -Executable $fixturePath -ArgumentList @('--runtime-directory', $Request.PackageDirectory) -WorkingDirectory $Request.PackageDirectory -ProgressReceipt $report -ProgressPath $Request.WorkerReceiptPath
        $numeric = Get-BonsaiHexagonDispatchFixtureResult -Json $fixture.StandardOutput -ExitCode $fixture.ExitCode
        $report.FixtureResult = $numeric
        $report.Outcome = $numeric.Outcome
        $report.Stage = $numeric.Stage
        $report.Failure = $numeric.Failure
        $report.DeviceDispatchVerified = $numeric.DeviceDispatchVerified
    } catch {
        # Stable stage names preserve a useful blocker without publishing provider,
        # argument, local path or credential text from an arbitrary native error.
        $report.Failure = $report.Stage + '-rejected'
        $report.Outcome = 'pending'
    } finally {
        foreach ($stream in $holds) { $stream.Dispose() }
        Save-BonsaiHexagonDispatchWorkerReceipt -Report $report -Path $Request.WorkerReceiptPath
    }
    return $report
}

Export-ModuleMember -Function Get-BonsaiHexagonDispatchPlan, Assert-BonsaiHexagonDispatchBuild, Get-BonsaiHexagonDispatchEnvironment, Get-BonsaiHexagonDispatchFixtureResult, Get-BonsaiHexagonDispatchWorkerResult, Invoke-BonsaiHexagonDispatchWorker
