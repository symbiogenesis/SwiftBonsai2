#requires -Version 7.6
<#
.SYNOPSIS
Runs one synthetic, trusted-input CPU load/decode check; never enables chat serving.
.DESCRIPTION
Vetted against the pinned tools/completion source and its actual --help. The modern
llama-cli launches a server, so this check uses the in-process llama-completion.
The embedded template's default thinking behavior is preserved. Sixteen generated
tokens establish a bounded decode check, not answer quality or performance.
Memory admission reserves the whole 18 GiB service allowance plus 4 GiB available
headroom. A Windows Job enforces job-wide committed memory, one active process,
and kill-on-close ownership from creation. The additional one-second physical
memory watchdog is not a peak measurement. This provides no filesystem, token,
or egress isolation. No applications are closed or system settings changed.
.LINK
https://github.com/ggml-org/llama.cpp/blob/620d2225cc436ab0b001e3200e590e3b286ef9ab/tools/completion/completion.cpp
.LINK
https://learn.microsoft.com/en-us/windows/win32/api/sysinfoapi/nf-sysinfoapi-globalmemorystatusex
#>
[CmdletBinding()]
[OutputType([pscustomobject])]
param([Parameter(Mandatory)][string]$DataDirectory, [switch]$CheckOnly)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
if (-not $IsWindows -or [Runtime.InteropServices.RuntimeInformation]::OSArchitecture -ne 'Arm64' -or
    [Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture -ne 'Arm64') {
    throw 'Use native ARM64 PowerShell on Windows ARM64.'
}
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1') -Force
$data = Resolve-ExternalPath $DataDirectory
$overrides = @(Get-ChildItem Env: | Where-Object Name -Match '^(LLAMA_|LLAMACPP_|GGML_|AIP_|MTMD_|HF_TOKEN$|HF_ENDPOINT$|MODEL_ENDPOINT$)')
if ($overrides.Count) { throw ('Inherited model/backend overrides are unsupported: ' + ($overrides.Name -join ', ')) }
if (-not ('SwiftBonsai.PhysicalMemory' -as [type])) {
    Add-Type -CompilerOptions @('/nullable:enable', '/checked+', '/unsafe-', '/warn:9999', '/warnaserror+', '/langversion:14.0') -TypeDefinition @'
#nullable enable
using System.ComponentModel;
using System.Runtime.InteropServices;
namespace SwiftBonsai {
    /// <summary>Reads current Windows physical memory availability.</summary>
    public static class PhysicalMemory {
        [StructLayout(LayoutKind.Sequential)]
        private record struct Status {
            public uint Length, Load;
            public ulong TotalPhysical, AvailablePhysical, TotalPageFile, AvailablePageFile;
            public ulong TotalVirtual, AvailableVirtual, AvailableExtendedVirtual;
        }
        [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
        [DllImport("kernel32.dll", EntryPoint="GlobalMemoryStatusEx", ExactSpelling=true, SetLastError=true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool QueryMemoryStatus(ref Status status);
        /// <summary>Returns the physical memory currently available, in bytes.</summary>
        public static ulong AvailableBytes() {
            var status = new Status { Length = (uint)Marshal.SizeOf<Status>() };
            if (!QueryMemoryStatus(ref status)) throw new Win32Exception(Marshal.GetLastWin32Error());
            return status.AvailablePhysical;
        }
    }
}
'@
}
$lock = Read-BonsaiLock
$serverProfile = Get-Content -LiteralPath (Join-Path $PSScriptRoot '../config/profiles/reference-cpu.json') -Raw | ConvertFrom-Json -DateKind String
[long]$ceiling = $serverProfile.serviceCeilingBytes
if ($ceiling -ne 18GB) { throw 'Review this check before changing the reference service allowance.' }
[long]$reserve = 4GB
[long]$available = [SwiftBonsai.PhysicalMemory]::AvailableBytes()
$report = [ordered]@{
    SchemaVersion = 1; RecordedAtUtc = [DateTime]::UtcNow.ToString('o')
    Status = 'admission-passed'; AvailablePhysicalBytesAtAdmission = $available
    RequiredAvailableBytes = ($ceiling + $reserve); ServiceCeilingBytes = $ceiling
    AvailableMemoryFloorBytes = $reserve; TokenLimit = 16; TimeoutSeconds = 600
    ProcessOwnership = 'windows-job-at-creation-kill-on-close'; JobCommittedMemoryLimitBytes = $ceiling
    ActiveProcessLimit = 1; BreakawayAllowed = $false
    ModelRevision = $lock.model.revision; RuntimeCommit = $lock.runtime.commit
    ModelProcessStarted = $false; Qualification = 'trusted-input-development-only; no daily security or performance acceptance'
}
if ($available -lt $report.RequiredAvailableBytes) {
    $report.Status = 'not-run-insufficient-available-memory'
    $report['Action'] = 'Free memory by closing applications you choose, then rerun. This check will not close them.'
}
if ($CheckOnly -or $report.Status -ne 'admission-passed') { return [pscustomobject]$report }

$runtime = & (Join-Path $PSScriptRoot 'Get-Engine.ps1') -DataDirectory $data -VerifyOnly
$modelPath = Resolve-ExternalPath (Join-Path $data -ChildPath 'models', $lock.model.revision, $lock.model.filename)
$executable = Resolve-ExternalPath (Join-Path ([IO.Path]::GetDirectoryName($runtime.ServerPath)) 'llama-completion.exe')
Assert-Arm64PE $executable
if (-not ('SwiftBonsai.OwnedProcess' -as [type])) {
    Add-Type -Path (Join-Path $PSScriptRoot 'Bonsai.Process.cs') -CompilerOptions @('/nullable:enable', '/checked+', '/unsafe-', '/warn:9999', '/warnaserror+', '/langversion:14.0')
}
$guard = $owned = $process = $scratch = $null
function Stop-OwnedProcess {
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseShouldProcessForStateChangingFunctions', '', Justification = 'Mandatory finally/catch cleanup must terminate only this invocation''s process, including under WhatIf.')]
    [CmdletBinding()]
    [OutputType([void])]
    param()
    if ($owned) { $owned.Stop(10000) }
}
function Save-CheckEvidence {
    [CmdletBinding()]
    [OutputType([void])]
    param()
    if (-not $scratch -or -not [IO.Directory]::Exists($scratch)) { return }
    if ($owned) { $report['WindowsReportedPeakJobMemoryBytes'] = $owned.GetPeakJobMemoryBytes() }
    $report['EvidenceDirectory'] = $scratch
    $report | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $scratch 'result.json')
}
try {
    # Hold a read-only model handle through hashing and native execution.
    $guard = [IO.File]::Open($modelPath, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
    $scratch = Resolve-ExternalPath (Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-CpuCheck-' + [Guid]::NewGuid().ToString('N')))
    [IO.Directory]::CreateDirectory($scratch) | Out-Null
    Test-LockedArtifact $modelPath $lock.model.bytes $lock.model.sha256
    # Recheck after verification, since available memory is volatile.
    if ([SwiftBonsai.PhysicalMemory]::AvailableBytes() -lt $report.RequiredAvailableBytes) { throw 'Memory admission changed during artifact verification; no model was launched.' }
    $arguments = @('--model', $modelPath, '--ctx-size', '8192', '--parallel', '1',
        '--cache-type-k', 'f16', '--cache-type-v', 'f16', '--gpu-layers', '0', '--device', 'none',
        '--no-kv-offload', '--no-op-offload', '--fit', 'off', '--threads', '4', '--threads-batch', '4',
        '--poll', '0', '--poll-batch', '0', '--batch-size', '256', '--ubatch-size', '128',
        '--load-mode', 'mmap', '--no-repack', '--no-context-shift', '--offline', '--no-warmup',
        '--no-perf', '--log-verbosity', '2', '--color', 'off', '--simple-io', '--no-display-prompt',
        '--conversation', '--jinja', '--single-turn', '--n-predict', '16',
        '--system-prompt', 'You are a helpful assistant.', '--prompt', 'Reply with the word ready.')
    $owned = [SwiftBonsai.OwnedProcess]::Start($executable, [string[]]$arguments, $scratch,
        (Join-Path $scratch 'stdout.txt'), (Join-Path $scratch 'stderr.txt'), [ulong]$ceiling, 1)
    $process = $owned.Process
    $report.ModelProcessStarted = $true
    $deadline = [Environment]::TickCount64 + 1000L * $report.TimeoutSeconds
    while (-not $process.WaitForExit(1000)) {
        if ([Environment]::TickCount64 -ge $deadline) { throw 'CPU loading/decode check exceeded its safety timeout.' }
        try {
            $process.Refresh()
            $workingSet = $process.WorkingSet64; $privateBytes = $process.PrivateMemorySize64
        } catch [InvalidOperationException] {
            if ($process.HasExited) { break }
            throw
        }
        if ($workingSet -gt $ceiling -or $privateBytes -gt $ceiling -or
            [SwiftBonsai.PhysicalMemory]::AvailableBytes() -lt $reserve) { throw 'CPU check stopped by its current-memory safety watchdog.' }
    }
    $outputFile = [IO.FileStream]::new((Join-Path $scratch 'stdout.txt'), [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::ReadWrite)
    $reader = [IO.StreamReader]::new($outputFile)
    try { $text = $reader.ReadToEnd() } finally { $reader.Dispose(); $outputFile.Dispose() }
    if ($process.ExitCode -ne 0 -or [string]::IsNullOrWhiteSpace($text)) { throw "CPU load/decode failed; inspect technical logs at $scratch" }
    $report.Status = 'bounded-load-and-decode-completed'
    $report['ExitCode'] = $process.ExitCode
    $report['EffectiveArguments'] = $arguments
    Save-CheckEvidence
    return [pscustomobject]$report
} catch {
    $failure = $_
    $report.Status = 'failed'
    $report['Failure'] = $failure.Exception.Message
    try { Stop-OwnedProcess } catch { $report['CancellationFailure'] = $_.Exception.Message }
    Save-CheckEvidence
    if ($scratch) { Write-Warning "CPU check failure evidence retained at $scratch" }
    throw $failure
} finally {
    try { Stop-OwnedProcess } finally {
        if ($owned) { $owned.Dispose() }
        if ($guard) { $guard.Dispose() }
    }
}
