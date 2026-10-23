#requires -Version 7.6
[CmdletBinding()]
[OutputType([void])]
param([ValidateSet('All','Download','Configuration','Paths','PE','Doctor','ServerPolicy')][string]$Case = 'All')
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
function Assert {
    [CmdletBinding()]
    [OutputType([void])]
    param([bool]$Condition, [string]$Message)
    if (-not $Condition) { throw $Message }
}
function Assert-Failure {
    [CmdletBinding()]
    [OutputType([void])]
    param([scriptblock]$Action, [string]$Message)
    $failed = $false
    try { & $Action | Out-Null } catch { $failed = $true }
    Assert $failed $Message
}

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
    foreach ($mode in @('fresh','resume','ignore-range','bad-range','redirect','cancel','oversize','bad-hash','completed')) {
        $handler = [SwiftBonsai.Tests.BonsaiTestHandler]::new()
        $handler.Mode = $mode
        & $module {
            param($fake)
            $script:testDownloadHandler = $fake
            function script:New-ArtifactHttpClient {
                [CmdletBinding()]
                [OutputType([Net.Http.HttpClient])]
                param([Net.Http.HttpClientHandler]$Handler)
                if ($null -eq $Handler) { throw 'The production factory contract requires a handler.' }
                [Net.Http.HttpClient]::new($script:testDownloadHandler)
            }
        } $handler
        $path = Join-Path $scratch ($mode + '.bin')
        if ($mode -in @('resume','ignore-range','bad-range')) { [IO.File]::WriteAllBytes($path + '.partial', $payload[0..1]) }
        if ($mode -eq 'completed') { [IO.File]::WriteAllBytes($path + '.partial', $payload) }
        $expectedBytes = if ($mode -eq 'oversize') { 5 } else { 6 }
        $expectedHash = if ($mode -eq 'bad-hash') { '0' * 64 } else { $hash }
        $action = { Receive-LockedArtifact -Uri 'https://example.invalid/pinned.bin' -Path $path -Bytes $expectedBytes -Sha256 $expectedHash }
        if ($mode -in @('bad-range','redirect','cancel','oversize','bad-hash')) {
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
    foreach ($unsafeFlag in @('--api-key','--agent','--tools','--mcp-servers-json','--mcp-servers-config',
        '--ui-mcp-proxy','--path','--models-dir','--model-url','--rpc','--metrics','--props')) {
        Assert ($plan.Arguments -cnotcontains $unsafeFlag) "Plan contains unsafe server option: $unsafeFlag"
    }
    Assert ($plan.Arguments -ccontains '--no-agent' -and $plan.Arguments -ccontains '--no-ui-mcp-proxy' -and
        $plan.Arguments -ccontains '--no-ui' -and $plan.Arguments -ccontains '--no-slots' -and
        $plan.Arguments -ccontains '--offline') 'Plan omitted explicit restrictions.'
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
if ($Case -in @('All','Doctor')) {
    $reportPath = Join-Path $scratch 'doctor.json'
    $report = & (Join-Path $repo 'scripts/Doctor.ps1') -DataPath $scratch -OutputPath $reportPath -PassThru
    $saved = Get-Content -LiteralPath $reportPath -Raw | ConvertFrom-Json -DateKind String
    Assert ($saved.SchemaVersion -eq 1 -and $report.Runtime.ProcessArchitecture) 'Doctor did not produce a versioned architecture report.'
    if ($null -ne $saved.Memory.UsablePhysicalBytes) { Assert ($saved.Memory.UsablePhysicalBytes -gt 0) 'Usable memory must be positive bytes.' }
    Assert-Failure { & (Join-Path $repo 'scripts/Doctor.ps1') -OutputPath $reportPath } 'Doctor overwrote an existing report.'
}
Write-Information -InformationAction Continue "PASS: $Case. Synthetic evidence retained outside Git at $scratch"
