#requires -Version 7.6
<#
.SYNOPSIS
Runs one focused packaged-supervisor lifecycle regression using a synthetic child.
.DESCRIPTION
Checks caller death, timeout or native failure without loading weights, running
inference or executing a benchmark. Retains receipts and logs outside the checkout.
#>
[CmdletBinding()]
[OutputType([string])]
param(
    [Parameter(Mandatory)][ValidateSet('ParentDeath', 'Timeout', 'ExitCode')][string]$Case,
    [string]$ToolPath,
    [string]$ToolsModulePath
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
if (-not $IsWindows) { throw 'Packaged process supervision checks require Windows.' }
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
if (-not $ToolsModulePath) { $ToolsModulePath = Join-Path $repo 'scripts/Bonsai.Tools.psm1' }
Import-Module $ToolsModulePath
$scratch = Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-SupervisionTest-' + [guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($scratch)
$probe = Join-Path $scratch 'probe.ps1'
$marker = Join-Path $scratch 'child.json'
@'
#requires -Version 7.6
[CmdletBinding()]
param([Parameter(Mandatory)][string]$Marker, [Parameter(Mandatory)][string]$Mode)
$ErrorActionPreference = 'Stop'
$identity = @{ ProcessId = $PID; CreationTime = [Diagnostics.Process]::GetCurrentProcess().StartTime.ToUniversalTime().ToFileTimeUtc() } | ConvertTo-Json
[IO.File]::WriteAllText($Marker, $identity)
if ($Mode -eq 'ExitCode') { [Console]::Out.Write('retained-failure-output'); exit 7 }
Start-Sleep -Seconds 120
'@ | Set-Content -LiteralPath $probe

function Read-PublishedText {
    [CmdletBinding()]
    [OutputType([string])]
    param([string]$Path)
    $deadline = [Environment]::TickCount64 + 20000
    while ([Environment]::TickCount64 -lt $deadline) {
        if ([IO.File]::Exists($Path)) {
            try {
                $text = [IO.File]::ReadAllText($Path)
                if ($text.Length -gt 0) { return $text }
            } catch [IO.IOException] {
                if (($_.Exception.HResult -band 0xffff) -notin @(32, 33)) { throw }
            }
        }
        Start-Sleep -Milliseconds 20
    }
    throw "Synthetic child did not publish evidence: $Path"
}

$owner = $child = $null
try {
    if ($Case -eq 'ParentDeath') {
        $ownerScript = Join-Path $scratch 'owner.ps1'
        @'
#requires -Version 7.6
[CmdletBinding()]
param([string]$ModulePath, [string]$ToolPath, [string]$Lane, [string]$Probe, [string]$Marker)
$ErrorActionPreference = 'Stop'
Import-Module $ModulePath
$null = Invoke-BonsaiTool -Command completion -ToolPath $ToolPath -Request @{
    schemaVersion = 1; executable = (Join-Path $PSHOME 'pwsh.exe')
    arguments = @('-NoLogo', '-NoProfile', '-NonInteractive', '-File', $Probe, '-Marker', $Marker, '-Mode', 'ParentDeath')
    workingDirectory = $Lane; evidenceDirectory = $Lane; artifacts = @()
    memoryLimitBytes = 1GB; requiredAvailableBytes = 0; physicalReserveBytes = 0; timeoutSeconds = 120
    stdoutLimitBytes = 1MB; stderrLimitBytes = 1MB; requireNonemptyStdout = $false; ciOnly = $false
}
'@ | Set-Content -LiteralPath $ownerScript
        $start = [Diagnostics.ProcessStartInfo]::new((Join-Path $PSHOME 'pwsh.exe'))
        $start.WorkingDirectory = $scratch; $start.UseShellExecute = $false; $start.CreateNoWindow = $true
        $start.RedirectStandardOutput = $true; $start.RedirectStandardError = $true
        foreach ($argument in @('-NoLogo', '-NoProfile', '-NonInteractive', '-File', $ownerScript,
            '-ModulePath', $ToolsModulePath, '-ToolPath', $ToolPath, '-Lane', $scratch, '-Probe', $probe, '-Marker', $marker)) {
            $start.ArgumentList.Add($argument)
        }
        $owner = [Diagnostics.Process]::Start($start)
        $stdout = $owner.StandardOutput.ReadToEndAsync(); $stderr = $owner.StandardError.ReadToEndAsync()
        $identity = Read-PublishedText $marker | ConvertFrom-Json -DateKind String
        $child = [Diagnostics.Process]::GetProcessById($identity.ProcessId)
        if ($child.StartTime.ToUniversalTime().ToFileTimeUtc() -ne $identity.CreationTime) { throw 'Synthetic child PID was reused.' }
        # Kill just PowerShell; do not recursively kill the Rust helper or probe.
        $owner.Kill()
        if (-not $owner.WaitForExit(10000) -or -not $child.WaitForExit(10000)) { throw 'Calling PowerShell death left the supervised native child running.' }
        if ($stdout.Wait(10000) -and $stderr.Wait(10000)) {
            [IO.File]::WriteAllText((Join-Path $scratch 'owner.log'), $stdout.GetAwaiter().GetResult() + $stderr.GetAwaiter().GetResult())
        }
    } else {
        $failed = $false
        try {
            $null = Invoke-BonsaiTool -Command completion -ToolPath $ToolPath -Request @{
                schemaVersion = 1; executable = (Join-Path $PSHOME 'pwsh.exe')
                arguments = @('-NoLogo', '-NoProfile', '-NonInteractive', '-File', $probe, '-Marker', $marker, '-Mode', $Case)
                workingDirectory = $scratch; evidenceDirectory = $scratch; artifacts = @()
                memoryLimitBytes = 1GB; requiredAvailableBytes = 0; physicalReserveBytes = 0; timeoutSeconds = 2
                stdoutLimitBytes = 1MB; stderrLimitBytes = 1MB; requireNonemptyStdout = $false; ciOnly = $false
            }
        } catch { $failed = $true }
        if (-not $failed) { throw 'A failing synthetic operation unexpectedly succeeded.' }
    }
    $receipt = Read-PublishedText (Join-Path $scratch 'supervision.json') | ConvertFrom-Json -DateKind String
    if ($receipt.Status -cne 'failed' -or -not $receipt.ModelProcessStarted) { throw 'Supervision failure accounting was not retained.' }
    if ($Case -eq 'ParentDeath' -and $receipt.Failure -notmatch 'caller process exited') { throw 'The caller-death guard did not explain its shutdown.' }
    if ($Case -eq 'Timeout' -and $receipt.Failure -notmatch 'timeout') { throw 'The native deadline was not enforced.' }
    if ($Case -eq 'ExitCode' -and $receipt.ExitCode -ne 7) { throw 'The exact native failure exit code was lost.' }
    Write-Output "Focused packaged supervisor $Case check passed. Evidence: $scratch"
} finally {
    if ($child) {
        try { if (-not $child.HasExited) { $child.Kill(); $null = $child.WaitForExit(10000) } } finally { $child.Dispose() }
    }
    if ($owner) {
        try { if (-not $owner.HasExited) { $owner.Kill($true); $null = $owner.WaitForExit(10000) } } finally { $owner.Dispose() }
    }
    foreach ($generated in @($probe, (Join-Path $scratch 'owner.ps1'))) {
        if ([IO.File]::Exists($generated)) { Remove-Item -LiteralPath $generated -Force }
    }
}
