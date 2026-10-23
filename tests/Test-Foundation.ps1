#requires -Version 7.4
[CmdletBinding()]
param([ValidateSet('All','Download','Configuration','Paths','PE','Doctor')][string]$Case = 'All')
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if (-not $IsWindows) { throw 'These checks target Windows.' }
if ($Case -eq 'All' -and $env:GITHUB_ACTIONS -ne 'true') { throw 'The full suite runs only in GitHub Actions. Select one focused -Case locally.' }
$repo = Split-Path $PSScriptRoot -Parent
Import-Module (Join-Path $repo 'scripts/Bonsai.Common.psm1') -Force
$scratch = Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-Tests-' + [Guid]::NewGuid().ToString('N'))
$scratch = Resolve-ExternalPath $scratch
[IO.Directory]::CreateDirectory($scratch) | Out-Null
function Assert([bool]$Condition, [string]$Message) { if (-not $Condition) { throw $Message } }
function Expect-Failure([scriptblock]$Action, [string]$Message) {
    $failed = $false
    try { & $Action | Out-Null } catch { $failed = $true }
    Assert $failed $Message
}

if ($Case -in @('All','Configuration')) {
    $lock = Read-BonsaiLock
    $profile = Get-Content -LiteralPath (Join-Path $repo 'config/profiles/reference-cpu.json') -Raw | ConvertFrom-Json
    Assert (-not $profile.launchEnabled) 'An unqualified profile must not enable launch.'
    Assert ($profile.estimatedWeightBytes -eq $lock.model.bytes) 'Weight estimate differs from artifact pin.'
    Assert (($profile.estimatedWeightBytes + $profile.estimatedKvBytes + $profile.estimatedRecurrentBytes +
        $profile.estimatedRuntimeHeadroomBytes) -eq $profile.serviceCeilingBytes) 'Memory budget does not reconcile.'
    $lock.model.revision = 'main'
    $invalidLock = Join-Path $scratch 'mutable-lock.json'
    $lock | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $invalidLock
    Expect-Failure { Read-BonsaiLock $invalidLock } 'Mutable model revision was accepted.'
    $lock = Read-BonsaiLock
    $lock.runtime.asset.filename = '../wrong.zip'
    $lock | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $invalidLock
    Expect-Failure { Read-BonsaiLock $invalidLock } 'An unsafe small-artifact filename was accepted.'
    $lock = Read-BonsaiLock
    $lock.runtime.release = 'unrelated-release'
    $lock | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $invalidLock
    Expect-Failure { Read-BonsaiLock $invalidLock } 'An inconsistent runtime release was accepted.'
}
if ($Case -in @('All','Paths')) {
    foreach ($invalidPath in @('relative', 'C:relative', '%SystemDrive%\Bonsai', '\\?\C:\Bonsai',
        (Join-Path $repo 'models'), (Join-Path $scratch 'file:stream'))) {
        Expect-Failure { Resolve-ExternalPath $invalidPath } "Unsafe path accepted: $invalidPath"
    }
    Assert ((Resolve-ExternalPath (Join-Path $scratch 'new/artifact.bin')) -eq (Join-Path $scratch 'new/artifact.bin')) 'External path resolution failed.'
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
    Expect-Failure { Assert-Arm64PE $path } 'An x64 executable was accepted.'
    [IO.File]::WriteAllBytes($path, [byte[]]@(1,2,3))
    Expect-Failure { Assert-Arm64PE $path } 'A truncated executable was accepted.'
}
if ($Case -in @('All','Download')) {
    # Synthetic HTTP responses exercise resumption and hostile transport behavior without network access.
    if (-not ('BonsaiTestHandler' -as [type])) {
        Add-Type -TypeDefinition @'
using System;
using System.Net;
using System.Net.Http;
using System.Net.Http.Headers;
using System.Threading;
using System.Threading.Tasks;
public sealed class BonsaiTestHandler : HttpMessageHandler {
    public string Mode;
    public long? LastRange;
    public int Requests;
    public byte[] Data = new byte[] {10,20,30,40,50,60};
    protected override Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken ct) {
        Requests++;
        foreach (var r in request.Headers.Range?.Ranges ?? new RangeItemHeaderValue[0]) LastRange = r.From;
        if (Mode == "cancel") throw new OperationCanceledException();
        if (Mode == "redirect") {
            var redirect = new HttpResponseMessage(HttpStatusCode.Found);
            redirect.Headers.Location = new Uri("http://example.invalid/insecure");
            return Task.FromResult(redirect);
        }
        long start = Mode == "resume" || Mode == "bad-range" ? LastRange ?? 0 : 0;
        var response = new HttpResponseMessage(start > 0 ? HttpStatusCode.PartialContent : HttpStatusCode.OK);
        var data = new byte[Data.Length - start];
        Array.Copy(Data, start, data, 0, data.Length);
        response.Content = new ByteArrayContent(data);
        if (start > 0) response.Content.Headers.ContentRange = new ContentRangeHeaderValue(
            Mode == "bad-range" ? 0 : start, Data.Length - 1, Data.Length);
        return Task.FromResult(response);
    }
}
'@
    }
    $module = Get-Module Bonsai.Common
    $payload = [byte[]]@(10,20,30,40,50,60)
    $hash = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($payload))
    foreach ($mode in @('fresh','resume','ignore-range','bad-range','redirect','cancel','oversize','bad-hash','completed')) {
        $handler = [BonsaiTestHandler]::new()
        $handler.Mode = $mode
        & $module {
            param($fake)
            $script:testDownloadHandler = $fake
            function script:New-ArtifactHttpClient { param($Handler); [Net.Http.HttpClient]::new($script:testDownloadHandler) }
        } $handler
        $path = Join-Path $scratch ($mode + '.bin')
        if ($mode -in @('resume','ignore-range','bad-range')) { [IO.File]::WriteAllBytes($path + '.partial', $payload[0..1]) }
        if ($mode -eq 'completed') { [IO.File]::WriteAllBytes($path + '.partial', $payload) }
        $expectedBytes = if ($mode -eq 'oversize') { 5 } else { 6 }
        $expectedHash = if ($mode -eq 'bad-hash') { '0' * 64 } else { $hash }
        $action = { Receive-LockedArtifact -Uri 'https://example.invalid/pinned.bin' -Path $path -Bytes $expectedBytes -Sha256 $expectedHash }
        if ($mode -in @('bad-range','redirect','cancel','oversize','bad-hash')) {
            Expect-Failure $action "Unsafe download accepted: $mode"
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
if ($Case -in @('All','Doctor')) {
    $reportPath = Join-Path $scratch 'doctor.json'
    $report = & (Join-Path $repo 'scripts/Doctor.ps1') -DataPath $scratch -OutputPath $reportPath -PassThru
    $saved = Get-Content -LiteralPath $reportPath -Raw | ConvertFrom-Json
    Assert ($saved.SchemaVersion -eq 1 -and $report.Runtime.ProcessArchitecture) 'Doctor did not produce a versioned architecture report.'
    if ($null -ne $saved.Memory.UsablePhysicalBytes) { Assert ($saved.Memory.UsablePhysicalBytes -gt 0) 'Usable memory must be positive bytes.' }
    Expect-Failure { & (Join-Path $repo 'scripts/Doctor.ps1') -OutputPath $reportPath } 'Doctor overwrote an existing report.'
}
Write-Host "PASS: $Case. Synthetic evidence retained outside Git at $scratch"
