#requires -Version 7.6
[CmdletBinding()]
[OutputType([void])]
param([ValidateSet('All','Download','Configuration','Paths','PE','Doctor','Capabilities','ServerPolicy')][string]$Case = 'All')
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
if (-not $IsWindows) { throw 'These checks target Windows.' }
if ($Case -eq 'All' -and $env:GITHUB_ACTIONS -ne 'true') { throw 'The full suite runs only in GitHub Actions. Select one focused -Case locally.' }
$repo = Split-Path $PSScriptRoot -Parent
Import-Module (Join-Path $repo 'scripts/Bonsai.Common.psm1') -Force
$scratch = Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-Tests-' + [Guid]::NewGuid().ToString('N'))
$scratch = Resolve-ExternalPath $scratch
[void][IO.Directory]::CreateDirectory($scratch)
Import-Module (Join-Path $PSScriptRoot 'Bonsai.TestSupport.psm1')



if ($Case -in @('All','Configuration')) {
    $lock = Read-BonsaiLock
    $invalidLock = Join-Path $scratch 'invalid-root-lock.json'
    $lockJson = $lock | ConvertTo-Json -Depth 10
    foreach ($root in @("[$lockJson]", "[[$lockJson]]", '[]', 'null', 'true', '"text"')) {
        [IO.File]::WriteAllText($invalidLock, $root)
        Assert-Failure { Read-BonsaiLock $invalidLock } 'A non-object lock root was accepted.'
    }
    $serverProfile = Get-Content -LiteralPath (Join-Path $repo 'config/profiles/reference-cpu.json') -Raw | ConvertFrom-Json -DateKind String
    $metadata = Get-Content -LiteralPath (Join-Path $repo 'config/model.metadata.json') -Raw | ConvertFrom-Json -DateKind String
    Assert ($metadata.ModelRevision -eq $lock.model.revision -and $metadata.ArtifactSha256 -eq $lock.model.sha256 -and
        $metadata.FileBytes -eq $lock.model.bytes) 'Inspected metadata and model lock disagree.'
    Assert ((($metadata.TensorTypeHistogram | Measure-Object -Property Count -Sum).Sum) -eq $metadata.TensorCount) 'Tensor histogram does not reconcile.'
    Assert (-not $metadata.TensorPayloadRead) 'Metadata inspection must not load tensor payloads.'
    Assert (-not $serverProfile.launchEnabled) 'An unqualified profile must not enable launch.'
    Assert ($serverProfile.estimatedWeightBytes -eq $lock.model.bytes) 'Weight estimate differs from artifact pin.'
    Assert (($serverProfile.estimatedWeightBytes + $serverProfile.estimatedKvBytes + $serverProfile.estimatedRecurrentBytes +
        $serverProfile.estimatedRuntimeHeadroomBytes) -eq $serverProfile.serviceCeilingBytes) 'Memory budget does not reconcile.'
    $lock.model.revision = 'main'
    $invalidLock = Join-Path $scratch 'mutable-lock.json'
    $lock | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $invalidLock
    Assert-Failure { Read-BonsaiLock $invalidLock } 'Mutable model revision was accepted.'
    $lock = Read-BonsaiLock
    $lock.runtime.asset.filename = '../wrong.zip'
    $lock | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $invalidLock
    Assert-Failure { Read-BonsaiLock $invalidLock } 'An unsafe small-artifact filename was accepted.'
    $lock = Read-BonsaiLock
    $lock.runtime.release = 'unrelated-release'
    $lock | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $invalidLock
    Assert-Failure { Read-BonsaiLock $invalidLock } 'An inconsistent runtime release was accepted.'
}
if ($Case -in @('All','Paths')) {
    foreach ($invalidPath in @('relative', 'C:relative', '%SystemDrive%\Bonsai', '\\?\C:\Bonsai',
        '//?/C:/Bonsai', '//./C:/Bonsai', '\\?/C:/Bonsai',
        (Join-Path $repo 'models'), (Join-Path $scratch 'file:stream'))) {
        Assert-Failure { Resolve-ExternalPath $invalidPath } "Unsafe path accepted: $invalidPath"
    }
    Assert ((Resolve-ExternalPath (Join-Path $scratch 'new/artifact.bin')) -eq (Join-Path $scratch 'new/artifact.bin')) 'External path resolution failed.'
    foreach ($invalidPath in @('//?/C:/Bonsai', '//./C:/Bonsai')) {
        Assert-Failure { & (Join-Path $repo 'scripts/Doctor.ps1') -DataPath $invalidPath } 'Doctor accepted an alternate device namespace spelling.'
    }
}
if ($Case -in @('All','PE')) {
    $image = [byte[]]::new(128)
    $image[0] = 0x4D; $image[1] = 0x5A; $image[60] = 64
    $image[64] = 0x50; $image[65] = 0x45; $image[68] = 0x64; $image[69] = 0xAA
    $path = Join-Path $scratch 'architecture.exe'
    [IO.File]::WriteAllBytes($path, $image)
    Assert-Arm64PE $path
    $image[60] = 4
    $image[4] = 0x50; $image[5] = 0x45; $image[8] = 0x64; $image[9] = 0xAA
    [IO.File]::WriteAllBytes($path, $image)
    Assert-Failure { Assert-Arm64PE $path } 'A PE header inside the DOS header was accepted.'
    $image[60] = 120
    $image[120] = 0x50; $image[121] = 0x45; $image[124] = 0x64; $image[125] = 0xAA
    [IO.File]::WriteAllBytes($path, $image)
    Assert-Failure { Assert-Arm64PE $path } 'A truncated COFF header was accepted.'
    $image[60] = 64
    $image[68] = 0x64; $image[69] = 0x86
    [IO.File]::WriteAllBytes($path, $image)
    Assert-Failure { Assert-Arm64PE $path } 'An x64 executable was accepted.'
    [IO.File]::WriteAllBytes($path, [byte[]]@(1,2,3))
    Assert-Failure { Assert-Arm64PE $path } 'A truncated executable was accepted.'
}
if ($Case -in @('All','Download')) {
    # Synthetic HTTP responses exercise resumption and hostile transport behavior without network access.
    if (-not ('SwiftBonsai.Tests.BonsaiTestHandler' -as [type])) {
        Add-Type -CompilerOptions @('/nullable:enable', '/checked+', '/unsafe-', '/warn:9999', '/warnaserror+', '/langversion:14.0') -TypeDefinition @'
#nullable enable
using System;
using System.Net;
using System.Net.Http;
using System.Net.Http.Headers;
using System.Threading;
using System.Threading.Tasks;
namespace SwiftBonsai.Tests {
/// <summary>Provides synthetic HTTP responses for locked-artifact download tests.</summary>
public sealed class BonsaiTestHandler : HttpMessageHandler {
    /// <summary>Gets or sets the synthetic response mode.</summary>
    public string Mode { get; set; } = string.Empty;
    /// <summary>Gets the most recently requested range start.</summary>
    public long? LastRange { get; private set; }
    /// <summary>Gets the number of requests handled.</summary>
    public int Requests { get; private set; }
    private readonly byte[] data = new byte[] {10,20,30,40,50,60};
    /// <inheritdoc/>
    protected override Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken cancellationToken) {
        ArgumentNullException.ThrowIfNull(request);
        Requests++;
        foreach (var r in request.Headers.Range?.Ranges ?? Array.Empty<RangeItemHeaderValue>()) LastRange = r.From;
        if (Mode == "cancel") throw new OperationCanceledException();
        if (Mode == "redirect") {
            var redirect = new HttpResponseMessage(HttpStatusCode.Found);
            redirect.Headers.Location = new Uri("http://example.invalid/insecure");
            return Task.FromResult(redirect);
        }
        long start = Mode == "resume" || Mode == "bad-range" ? LastRange ?? 0 : 0;
        var response = new HttpResponseMessage(start > 0 ? HttpStatusCode.PartialContent : HttpStatusCode.OK);
        var data = new byte[this.data.Length - start];
        Array.Copy(this.data, start, data, 0, data.Length);
        response.Content = new ByteArrayContent(data);
        if (start > 0) response.Content.Headers.ContentRange = new ContentRangeHeaderValue(
            Mode == "bad-range" ? 0 : start, this.data.Length - 1, this.data.Length);
        return Task.FromResult(response);
    }
}
}
'@
    }
    $module = Get-Module Bonsai.Common
    $payload = [byte[]]@(10,20,30,40,50,60)
    $hash = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($payload))
    & $module {
        $script:originalPublish = ${function:Publish-LockedArtifact}
        function script:Publish-LockedArtifact {
            [Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseShouldProcessForStateChangingFunctions', '', Justification = 'Test seam validates the private publication step of explicitly requested synthetic downloads.')]
            [CmdletBinding()]
            [OutputType([void])]
            param([Parameter(Mandatory)][string]$PartialPath, [Parameter(Mandatory)][string]$Path)
            $writerDenied = $false
            try {
                $writer = [IO.File]::Open($PartialPath, [IO.FileMode]::Open, [IO.FileAccess]::Write, [IO.FileShare]::ReadWrite)
                $writer.Dispose()
            } catch [IO.IOException] { $writerDenied = $true }
            if (-not $writerDenied) { throw 'Artifact ownership was released before publication.' }
            $renameDenied = $false
            try { [IO.File]::Move($PartialPath, $PartialPath + '.other-owner', $false) }
            catch [IO.IOException] { $renameDenied = $true }
            if (-not $renameDenied) { throw 'A verified partial could be renamed before publication.' }
            if ($script:testPublicationMode -eq 'publication-failure') { throw [IO.IOException]::new('Synthetic filesystem publication failure.') }
            if ($script:testPublicationMode -eq 'existing-output') { [IO.File]::WriteAllText($Path, 'competing artifact') }
            & $script:originalPublish -PartialPath $PartialPath -Path $Path
            $writerDenied = $false
            try {
                $writer = [IO.File]::Open($Path, [IO.FileMode]::Open, [IO.FileAccess]::Write, [IO.FileShare]::ReadWrite)
                $writer.Dispose()
            } catch [IO.IOException] { $writerDenied = $true }
            if (-not $writerDenied) { throw 'A published alias admitted a writer before ownership was released.' }
        }
    }
    foreach ($mode in @('fresh','resume','ignore-range','bad-range','redirect','cancel','oversize','bad-hash','completed',
        'existing-output','publication-failure','literal-[name]')) {
        $handler = [SwiftBonsai.Tests.BonsaiTestHandler]::new()
        $handler.Mode = $mode
        & $module {
            param($fake, $mode)
            $script:testDownloadHandler = $fake
            $script:testPublicationMode = $mode
            function script:New-ArtifactHttpClient {
                [CmdletBinding()]
                [OutputType([Net.Http.HttpClient])]
                param([Net.Http.HttpClientHandler]$Handler)
                if ($null -eq $Handler) { throw 'The production factory contract requires a handler.' }
                [Net.Http.HttpClient]::new($script:testDownloadHandler)
            }
        } $handler $mode
        $path = Join-Path $scratch ($mode + '.bin')
        if ($mode -in @('resume','ignore-range','bad-range')) { [IO.File]::WriteAllBytes($path + '.partial', $payload[0..1]) }
        if ($mode -eq 'completed') { [IO.File]::WriteAllBytes($path + '.partial', $payload) }
        $expectedBytes = if ($mode -eq 'oversize') { 5 } else { 6 }
        $expectedHash = if ($mode -eq 'bad-hash') { '0' * 64 } else { $hash }
        $action = { Receive-LockedArtifact -Uri 'https://example.invalid/pinned.bin' -Path $path -Bytes $expectedBytes -Sha256 $expectedHash }
        if ($mode -in @('existing-output','publication-failure')) {
            Assert-Failure $action "Unsafe publication accepted: $mode"
            Test-LockedArtifact ($path + '.partial') 6 $hash
            if ($mode -eq 'existing-output') {
                Assert ([IO.File]::ReadAllText($path) -ceq 'competing artifact') 'Publication replaced an existing final artifact.'
            } else { Assert (-not (Test-Path -LiteralPath $path)) 'Failed publication left a final artifact.' }
        } elseif ($mode -in @('bad-range','redirect','cancel','oversize','bad-hash')) {
            Assert-Failure $action "Unsafe download accepted: $mode"
            Assert (-not (Test-Path -LiteralPath $path)) "Invalid final artifact published: $mode"
        } else {
            & $action | Out-Null
            Test-LockedArtifact $path 6 $hash
            Assert (-not (Test-Path -LiteralPath ($path + '.partial'))) 'Verified partial was not atomically promoted.'
            if ($mode -in @('resume','ignore-range')) { Assert ($handler.LastRange -eq 2) 'Resume request omitted the byte offset.' }
            if ($mode -eq 'completed') { Assert ($handler.Requests -eq 0) 'A complete partial caused a network request.' }
        }
    }
    # Whole-file verification must reject an already open writer, even one that
    # opts into sharing reads. The check must cover length and digest together.
    $verificationPath = Join-Path $scratch 'verification-sharing.bin'
    [IO.File]::WriteAllBytes($verificationPath, $payload)
    $writer = [IO.File]::Open($verificationPath, [IO.FileMode]::Open, [IO.FileAccess]::Write, [IO.FileShare]::ReadWrite)
    try { Assert-Failure { Test-LockedArtifact $verificationPath 6 $hash } 'Artifact verification admitted an existing writer.' }
    finally { $writer.Dispose() }
    Test-LockedArtifact $verificationPath 6 $hash
    # Restore the production factory in the current session.
    Import-Module (Join-Path $repo 'scripts/Bonsai.Common.psm1') -Force
}
if ($Case -in @('All','ServerPolicy')) {
    Import-Module (Join-Path $repo 'scripts/Bonsai.ServerPolicy.psm1') -Force
    $before = [Environment]::GetEnvironmentVariables()
    $data = Join-Path $scratch 'unprovisioned data'
    $plan = & (Join-Path $repo 'scripts/Get-ServerPlan.ps1') -DataDirectory $data -Port 18080
    Assert (-not $plan.LaunchEnabled -and -not $plan.ModelProcessStarted -and
        $plan.SecurityQualification -eq 'not-qualified' -and $plan.ArtifactVerification -eq 'not-performed') 'Planning promoted an unqualified server.'
    Assert (-not (Test-Path -LiteralPath $data)) 'Read-only planning created artifact/key storage.'
    Assert ($plan.Origin -ceq 'http://127.0.0.1:18080' -and $plan.Arguments -contains $plan.PlannedKeyFile -and
        $plan.PlannedKeyFile.StartsWith($data + '\') -and $plan.Arguments -contains $plan.ModelPath) 'Plan paths or exact origin are inconsistent.'
    Assert ($plan.RemainingGates.Count -ge 6 -and $plan.RequiredAvailableBytes -eq 22GB) 'Plan omitted required qualification gaps.'
    $frozenServer = Get-Content -Raw -LiteralPath (Join-Path $repo 'src/cpu-policy/tests/fixtures/server-policy-v1.json') | ConvertFrom-Json -DateKind String -AsHashtable
    $expectedArguments = [string[]]$frozenServer.plan.Arguments
    $expectedArguments[1] = $plan.ModelPath; $expectedArguments[5] = '18080'
    $expectedArguments[7] = $plan.PlannedKeyFile; $expectedArguments[9] = $plan.Origin
    Assert ($plan.Arguments -is [string[]] -and
        (ConvertTo-Json -InputObject $plan.Arguments -Compress) -ceq (ConvertTo-Json -InputObject $expectedArguments -Compress)) 'Plan changed exact argument tokens or their order.'
    foreach ($unsafeFlag in @('--api-key','--agent','--tools','--mcp-servers-json','--mcp-servers-config',
        '--ui-mcp-proxy','--path','--models-dir','--model-url','--rpc','--metrics','--props')) {
        Assert ($plan.Arguments -cnotcontains $unsafeFlag) "Plan contains unsafe server option: $unsafeFlag"
    }
    Assert ($plan.Arguments -ccontains '--no-agent' -and $plan.Arguments -ccontains '--no-ui-mcp-proxy' -and
        $plan.Arguments -ccontains '--no-ui' -and $plan.Arguments -ccontains '--no-slots' -and
        $plan.Arguments -ccontains '--offline') 'Plan omitted explicit restrictions.'
    foreach ($setting in @(
        @{ Flag = '--reasoning'; Value = 'on' }, @{ Flag = '--reasoning-effort'; Value = 'xhigh' },
        @{ Flag = '--reasoning-budget'; Value = '-1' }, @{ Flag = '--frequency-penalty'; Value = '0.0' }
    )) {
        $index = [Array]::IndexOf($plan.Arguments, $setting.Flag)
        Assert ($index -ge 0 -and $plan.Arguments[$index + 1] -ceq $setting.Value) "Plan omitted explicit inference default: $($setting.Flag)."
    }
    foreach ($name in @('llama_arg_agent','LLAMACPP_FOO','GGML_BACKEND_PATH','AIP_MODE','aip_http_port',
        'MTMD_BACKEND_DEVICE','HF_TOKEN','HF_ENDPOINT','MODEL_ENDPOINT')) {
        $failureText = $null
        try { Assert-BonsaiServerEnvironment -Variables @{ $name = 'PRIVATE-VALUE-NEVER-PRINT' } }
        catch { $failureText = $_.Exception.Message }
        Assert ($failureText -and $failureText.Contains($name) -and
            -not $failureText.Contains('PRIVATE-VALUE-NEVER-PRINT')) 'Environment override rejection exposed a value or missed a name.'
    }
    Assert-BonsaiServerEnvironment -Variables @{
        SystemDrive = 'C:'; SystemRoot = 'C:\Windows'; windir = 'C:\Windows'
        ProgramData = 'C:\ProgramData'; LOCALAPPDATA = 'C:\Users\Owner\AppData\Local'
        APPDATA = 'C:\Users\Owner\AppData\Roaming'; USERPROFILE = 'C:\Users\Owner'
    }
    $profilePath = Join-Path $repo 'config/profiles/reference-cpu.json'
    foreach ($mutation in @(
        @{ Name = 'launchEnabled'; Value = $true }, @{ Name = 'launchEnabled'; Value = 'false' },
        @{ Name = 'contextTokens'; Value = '8192' }, @{ Name = 'contextTokens'; Value = 16384 },
        @{ Name = 'contextTokens'; Value = [short]8192 }, @{ Name = 'contextTokens'; Value = [decimal]8192 },
        @{ Name = 'estimateScope'; Value = [char]'x' }, @{ Name = 'remainingGates'; Value = [char[]]@('x') },
        @{ Name = 'cachePrompt'; Value = $true }, @{ Name = 'estimatedRuntimeHeadroomBytes'; Value = 1 },
        @{ Name = 'remainingGates'; Value = @() }, @{ Name = 'extraArguments'; Value = @('--tools','all') }
    )) {
        $serverProfile = Get-Content -Raw -LiteralPath $profilePath | ConvertFrom-Json -DateKind String -AsHashtable
        $serverProfile[$mutation.Name] = $mutation.Value
        Assert-Failure { New-BonsaiServerPlan -DataDirectory $data -Profile $serverProfile } "Unsafe or mistyped profile accepted: $($mutation.Name)"
    }
    foreach ($path in @('relative','C:relative','C:\','\\server.invalid\share','//?/C:/Bonsai','//./C:/Bonsai', $repo)) {
        Assert-Failure { New-BonsaiServerPlan -DataDirectory $path } 'Server plan accepted a nonlocal or unsafe data path.'
    }
    Assert-Failure { New-BonsaiServerPlan -DataDirectory $data -Port 0 } 'Server plan accepted a dynamic port.'
    # Exercise exact switch matching independently of native execution; a prefix
    # such as --no-agent-unsafe must not satisfy --no-agent.
    Assert-BonsaiServerHelp -HelpText '--host HOST --no-agent --api-key-file FILE' -Arguments @('--host','127.0.0.1','--no-agent','--api-key-file','C:\key path')
    Assert-Failure { Assert-BonsaiServerHelp -HelpText '--no-agent-unsafe' -Arguments @('--no-agent') } 'Help verification matched a switch prefix.'
    Assert-Failure { Assert-BonsaiServerHelp -HelpText '--host HOST' -Arguments @('--no-agent') } 'Help verification accepted a missing switch.'
    $after = [Environment]::GetEnvironmentVariables()
    Assert ($before.Count -eq $after.Count) 'Planning changed the parent environment.'
    foreach ($name in $before.Keys) { Assert ($after[$name] -ceq $before[$name]) 'Planning changed an inherited environment value.' }
}
if ($Case -in @('All','Capabilities')) {
    Import-Module (Join-Path $repo 'scripts/Bonsai.Capabilities.psm1') -Force
    $unknown = Get-BonsaiHostCapability -Facts @{}
    Assert ($unknown.schema_version -eq 1 -and $unknown.kind -ceq 'runner-host-capabilities') 'Capability identity changed.'
    Assert ($unknown.cpu.status -ceq 'unknown' -and $null -eq $unknown.cpu.isa.advsimd -and
        $null -eq $unknown.memory.available_physical_bytes -and $unknown.npu.status -ceq 'unknown') 'Missing probes fabricated support or memory.'
    $facts = @{
        OsArchitecture = 'Arm64'; ProcessArchitecture = 'Arm64'
        OperatingSystem = @{ Version = '10.0.26100'; BuildNumber = '26100'; TotalVisibleMemorySize = [uint64]33554432 }
        Computer = @{ TotalPhysicalMemory = [uint64]34359738368 }
        Processors = @(@{ Name = 'Snapdragon X2E-96-100'; Manufacturer = 'Qualcomm'; NumberOfCores = 18; NumberOfLogicalProcessors = 18 })
        Graphics = @(@{ Name = 'Adreno'; AdapterCompatibility = 'Qualcomm'; DriverVersion = '1.2.3'; DriverDate = [datetime]'2026-01-01T00:00:00Z' })
        NpuDrivers = @(@{ DeviceName = 'Compute accelerator'; Manufacturer = 'Qualcomm'; DriverVersion = '4.5.6'; DriverDate = [datetime]'2026-01-01T00:00:00Z'; IsSigned = $true })
        Memory = @{ AvailableBytes = [uint64]17179869184; CommitLimit = [uint64]68719476736; CommittedBytes = [uint64]8589934592 }
        Isa = @{ AdvSimd = $false; DotProd = $false }
        Probes = @{ OperatingSystem = $true; Computer = $true; Processors = $true; Graphics = $true; NpuDrivers = $true; Memory = $true; Isa = $true }
    }
    $known = Get-BonsaiHostCapability -Facts $facts
    Assert ($known.cpu.identities[0].name -ceq 'Snapdragon X2E-96-100' -and $known.cpu.identities[0].physical_cores -eq 18) 'Observed processor identity or counts were lost.'
    Assert ($known.memory.reported_total_physical_bytes -eq [uint64]34359738368 -and
        $known.memory.usable_physical_bytes -eq [uint64]34359738368 -and
        $known.memory.commit_limit_bytes -eq [uint64]68719476736 -and $known.memory.status -ceq 'observed') 'Memory units or observed commit facts changed.'
    Assert ($known.cpu.isa.advsimd -ceq $false -and $known.cpu.isa.dotprod -ceq $false -and
        $null -eq $known.cpu.topology.processor_groups -and $null -eq $known.gpu.api_features -and
        $null -eq $known.npu.architecture -and $null -eq $known.npu.backend_readiness) 'Marketing identity or signed-driver inventory enabled unobserved capabilities.'
    $cpuIdentity = Get-BonsaiCpuCompatibilityIdentity -Capabilities $known
    Assert ($cpuIdentity.status -ceq 'observed' -and $cpuIdentity.qualification -ceq 'unqualified' -and
        $cpuIdentity.scope -ceq 'native-windows-arm64-cpu-observations-v1' -and
        $cpuIdentity.capability_class_sha256 -cmatch '^[a-f0-9]{64}$' -and
        $cpuIdentity.backend_driver_sha256 -cmatch '^[a-f0-9]{64}$') 'Static CPU identity was missing, unscoped or asserted qualification.'
    $expectedCapabilityJson = '{"schema_version":1,"kind":"runner-cpu-capability-class","scope":"native-windows-arm64-cpu-observations-v1","architecture":"arm64","identities":[{"name":"Snapdragon X2E-96-100","manufacturer":"Qualcomm","physical_cores":18,"logical_processors":18}],"isa":{"advsimd":false,"dotprod":false}}'
    $expectedBackendJson = '{"schema_version":1,"kind":"runner-cpu-backend-driver","scope":"native-windows-arm64-cpu-observations-v1","backend":"cpu","operating_system":{"architecture":"arm64","version":"10.0.26100","build_number":"26100"}}'
    Assert ($cpuIdentity.capability_class_json -ceq $expectedCapabilityJson -and
        $cpuIdentity.backend_driver_json -ceq $expectedBackendJson) 'Version-one canonical CPU record bytes changed.'
    foreach ($kind in @('capability_class', 'backend_driver')) {
        $expectedHash = [Convert]::ToHexStringLower([Security.Cryptography.SHA256]::HashData(
            [Text.Encoding]::UTF8.GetBytes($cpuIdentity[$kind + '_json'])))
        Assert ($cpuIdentity[$kind + '_sha256'] -ceq $expectedHash) 'Retained CPU record did not reproduce its digest.'
    }
    $knownJson = $known | ConvertTo-Json -Depth 8
    $changed = $knownJson | ConvertFrom-Json -AsHashtable -DateKind String
    $changed.memory.available_physical_bytes = [uint64]1
    $changed.memory.commit_limit_bytes = [uint64]1
    $changed.memory.committed_bytes = [uint64]1
    $changed.memory.usable_physical_bytes = [uint64]1
    $changed.gpu.devices[0].driver_version = 'other-gpu-driver'
    $changed.npu.devices[0].driver_version = 'other-npu-driver'
    $changed['CollectedAtUtc'] = '2099-01-01T00:00:00Z'
    $changed['Tools'] = @('C:\private\tool.exe')
    $stable = Get-BonsaiCpuCompatibilityIdentity -Capabilities $changed
    Assert ($stable.capability_class_json -ceq $cpuIdentity.capability_class_json -and
        $stable.backend_driver_sha256 -ceq $cpuIdentity.backend_driver_sha256) 'CPU identities included live budgets, timestamps, paths or accelerator drivers.'
    $changed = $knownJson | ConvertFrom-Json -AsHashtable -DateKind String
    $changed.operating_system.build_number = '26101'
    $updated = Get-BonsaiCpuCompatibilityIdentity -Capabilities $changed
    Assert ($updated.capability_class_sha256 -ceq $cpuIdentity.capability_class_sha256 -and
        $updated.backend_driver_sha256 -cne $cpuIdentity.backend_driver_sha256) 'An OS build update did not invalidate only the CPU backend record.'
    $changed = $knownJson | ConvertFrom-Json -AsHashtable -DateKind String
    $changed.cpu.isa.advsimd = $true
    $updated = Get-BonsaiCpuCompatibilityIdentity -Capabilities $changed
    Assert ($updated.capability_class_sha256 -cne $cpuIdentity.capability_class_sha256 -and
        $updated.backend_driver_sha256 -ceq $cpuIdentity.backend_driver_sha256) 'An observed ISA change did not invalidate only the CPU capability record.'
    $changed = $knownJson | ConvertFrom-Json -AsHashtable -DateKind String
    $changed.cpu.identities += @{ name = 'Other CPU'; manufacturer = 'Other vendor'; physical_cores = 2; logical_processors = 4 }
    $ordered = Get-BonsaiCpuCompatibilityIdentity -Capabilities $changed
    [Array]::Reverse($changed.cpu.identities)
    $reordered = Get-BonsaiCpuCompatibilityIdentity -Capabilities $changed
    Assert ($ordered.capability_class_sha256 -ceq $reordered.capability_class_sha256 -and
        $ordered.capability_class_sha256 -cne $cpuIdentity.capability_class_sha256) 'CPU inventory order affected identity or added processors were ignored.'
    $changed.cpu.identities += $changed.cpu.identities[0]
    Assert ((Get-BonsaiCpuCompatibilityIdentity -Capabilities $changed).capability_class_sha256 -cne
        $ordered.capability_class_sha256) 'Repeated CPU entries lost their multiplicity.'
    foreach ($missing in @('os', 'process', 'cpu', 'isa', 'native', 'scope', 'architecture', 'version', 'name', 'manufacturer', 'cores', 'logical', 'flag', 'contradictory', 'empty')) {
        $changed = $knownJson | ConvertFrom-Json -AsHashtable -DateKind String
        switch ($missing) {
            'os' { $changed.operating_system.status = 'partial' }
            'process' { $changed.process.status = 'unknown' }
            'cpu' { $changed.cpu.status = 'unknown' }
            'isa' { $changed.cpu.isa.status = 'unknown' }
            'native' { $changed.process.is_native = $false }
            'scope' { $changed.cpu.isa.scope = 'other-process' }
            'architecture' { $changed.cpu.isa.architecture = 'x64' }
            'version' { $changed.operating_system.version = 'UNKNOWN' }
            'name' { $changed.cpu.identities[0].name = $null }
            'manufacturer' { $changed.cpu.identities[0].manufacturer = 'unknown' }
            'cores' { $changed.cpu.identities[0].physical_cores = 0 }
            'logical' { $changed.cpu.identities[0].logical_processors = 1 }
            'flag' { $changed.cpu.isa.advsimd = $null }
            'contradictory' { $changed.cpu.isa.dotprod = $true }
            'empty' { $changed.cpu.identities = @() }
        }
        $missingIdentity = Get-BonsaiCpuCompatibilityIdentity -Capabilities $changed
        Assert ($missingIdentity.status -ceq 'unknown' -and $missingIdentity.fallback_reason -ceq 'unknown-capability' -and
            $null -eq $missingIdentity.capability_class_json -and $null -eq $missingIdentity.backend_driver_json -and
            $null -eq $missingIdentity.capability_class_sha256 -and $null -eq $missingIdentity.backend_driver_sha256) "Incomplete $missing facts produced concrete CPU identity."
    }
    foreach ($schema in @(2, '1', $null)) {
        $changed = $knownJson | ConvertFrom-Json -AsHashtable -DateKind String
        $changed.schema_version = $schema
        Assert-Failure { Get-BonsaiCpuCompatibilityIdentity -Capabilities $changed } 'An unsupported CPU identity input schema was accepted.'
    }
    foreach ($kind in @(@(), @('runner-host-capabilities'), 1, $null)) {
        $changed = $knownJson | ConvertFrom-Json -AsHashtable -DateKind String
        $changed.kind = $kind
        Assert-Failure { Get-BonsaiCpuCompatibilityIdentity -Capabilities $changed } 'A non-string host capability kind produced an identity.'
    }
    $missingIdentity = Get-BonsaiCpuCompatibilityIdentity -Capabilities $unknown
    Assert ($missingIdentity.status -ceq 'unknown' -and $null -eq $missingIdentity.capability_class_sha256) 'Missing observations became a CPU identity.'
    $facts.ProcessArchitecture = 'X64'
    $emulated = Get-BonsaiHostCapability -Facts $facts
    Assert ($emulated.process.is_native -ceq $false -and $emulated.cpu.isa.architecture -ceq 'x64' -and
        $emulated.cpu.isa.scope -ceq 'current-process') 'Process ISA claims lost their emulation scope.'
    Assert ((Get-BonsaiCpuCompatibilityIdentity -Capabilities $emulated).status -ceq 'unknown') 'Emulated ISA observations produced a native CPU identity.'
    $facts.Probes.Isa = $false
    $failedIsa = Get-BonsaiHostCapability -Facts $facts
    Assert ($failedIsa.cpu.isa.status -ceq 'unknown' -and $null -eq $failedIsa.cpu.isa.advsimd) 'A failed ISA probe reused stale facts.'
    $facts.NpuDrivers = @()
    $empty = Get-BonsaiHostCapability -Facts $facts
    Assert ($empty.npu.status -ceq 'observed' -and $empty.npu.devices.Count -eq 0) 'An empty successful inventory became a failed probe.'
    $facts.Probes.NpuDrivers = $false
    $failed = Get-BonsaiHostCapability -Facts $facts
    Assert ($failed.npu.status -ceq 'unknown' -and $failed.npu.devices.Count -eq 0) 'Failed and empty inventories were conflated.'
    foreach ($invalid in @(-1, '17179869184', 1.5, $true)) {
        $facts.Memory.AvailableBytes = $invalid
        Assert-Failure { Get-BonsaiHostCapability -Facts $facts } 'A non-integer or negative memory fact was accepted.'
    }
    $facts.Memory.AvailableBytes = [uint64]0
    Assert ((Get-BonsaiHostCapability -Facts $facts).memory.available_physical_bytes -eq 0) 'Zero available memory became unknown.'
    $facts.OperatingSystem.TotalVisibleMemorySize = [uint64]::MaxValue
    Assert-Failure { Get-BonsaiHostCapability -Facts $facts } 'KiB conversion overflow was accepted.'
    $facts.OperatingSystem.TotalVisibleMemorySize = [uint64]33554432
    $facts.Probes.Isa = $true
    $facts.Isa.AdvSimd = 'false'
    Assert-Failure { Get-BonsaiHostCapability -Facts $facts } 'A string ISA flag was accepted.'
    $facts.Isa.AdvSimd = $false
    $facts.Processors[0].Name = "invalid`nidentity"
    Assert-Failure { Get-BonsaiHostCapability -Facts $facts } 'Control characters in hardware identity were accepted.'
    $facts.Processors = @(1..65 | ForEach-Object { @{ Name = 'CPU' } })
    Assert-Failure { Get-BonsaiHostCapability -Facts $facts } 'Unbounded processor inventory was accepted.'
}
if ($Case -in @('All','Doctor')) {
    $reportPath = Join-Path $scratch 'doctor.json'
    $report = & (Join-Path $repo 'scripts/Doctor.ps1') -DataPath $scratch -OutputPath $reportPath -PassThru
    $saved = Get-Content -LiteralPath $reportPath -Raw | ConvertFrom-Json -DateKind String
    Assert ($saved.SchemaVersion -eq 1 -and $report.Runtime.ProcessArchitecture) 'Doctor did not produce a versioned architecture report.'
    Assert ($saved.Capabilities.schema_version -eq 1 -and $saved.Capabilities.kind -ceq 'runner-host-capabilities' -and
        $saved.Capabilities.cpu.isa.scope -ceq 'current-process' -and $null -eq $saved.Capabilities.npu.backend_readiness) 'Doctor lost its bounded host capability contract.'
    Assert ($saved.CpuCompatibilityIdentity.schema_version -eq 1 -and
        $saved.CpuCompatibilityIdentity.kind -ceq 'runner-cpu-compatibility-identity' -and
        $saved.CpuCompatibilityIdentity.qualification -ceq 'unqualified') 'Doctor lost its scoped unqualified CPU identity.'
    if ($null -ne $saved.Memory.UsablePhysicalBytes) { Assert ($saved.Memory.UsablePhysicalBytes -gt 0) 'Usable memory must be positive bytes.' }
    Assert-Failure { & (Join-Path $repo 'scripts/Doctor.ps1') -OutputPath $reportPath } 'Doctor overwrote an existing report.'
}
Write-Information -InformationAction Continue "PASS: $Case. Synthetic evidence retained outside Git at $scratch"
