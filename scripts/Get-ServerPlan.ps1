#requires -Version 7.6
<#
.SYNOPSIS
Prepares the fixed CPU server configuration without enabling chat or loading a model.
.DESCRIPTION
By default this validates policy, paths and inherited overrides without starting
processes or writing files. -VerifyRuntime checks the installed runtime;
-VerifyArtifacts also checks the existing offline model. Verification runs only
bounded version/help probes in unique system temp. It never
downloads, creates keys, opens a browser, binds a socket, or invokes inference.
The returned arguments are unqualified development intent, not a safe launcher.
.LINK
https://github.com/ggml-org/llama.cpp/blob/620d2225cc436ab0b001e3200e590e3b286ef9ab/tools/server/server-http.cpp
#>
[CmdletBinding(DefaultParameterSetName = 'PolicyOnly')]
[OutputType([pscustomobject])]
param(
    [Parameter(Mandatory)][string]$DataDirectory,
    [ValidateRange(1024,65535)][int]$Port = 8080,
    [Parameter(ParameterSetName = 'Runtime')][switch]$VerifyRuntime,
    [Parameter(ParameterSetName = 'Artifacts')][switch]$VerifyArtifacts
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
if (-not $IsWindows) { throw 'Server planning targets Windows.' }
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1')
Import-Module (Join-Path $PSScriptRoot 'Bonsai.ServerPolicy.psm1') -Force
$plan = New-BonsaiServerPlan -DataDirectory $DataDirectory -Port $Port
if (-not $VerifyRuntime -and -not $VerifyArtifacts) { return $plan }
if ([Runtime.InteropServices.RuntimeInformation]::OSArchitecture -ne 'Arm64' -or
    [Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture -ne 'Arm64') {
    throw 'Artifact probes require native ARM64 PowerShell on Windows ARM64.'
}
$runtime = & (Join-Path $PSScriptRoot 'Get-Engine.ps1') -DataDirectory $DataDirectory -VerifyOnly
if ($VerifyArtifacts) { $null = & (Join-Path $PSScriptRoot 'Get-Model.ps1') -DataDirectory $DataDirectory -VerifyOnly }
$scratch = Resolve-ExternalPath (Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-ServerPlan-' + [Guid]::NewGuid().ToString('N')))
[IO.Directory]::CreateDirectory($scratch) | Out-Null
$process = [Diagnostics.Process]::new()
$started = $false
try {
    $start = [Diagnostics.ProcessStartInfo]::new($runtime.ServerPath)
    $start.WorkingDirectory = $scratch; $start.UseShellExecute = $false; $start.CreateNoWindow = $true
    $start.RedirectStandardOutput = $true; $start.RedirectStandardError = $true
    $start.ArgumentList.Add('--help')
    $process.StartInfo = $start
    if (-not $process.Start()) { throw 'Server help probe failed to start.' }
    $started = $true
    $stdout = $process.StandardOutput.ReadToEndAsync(); $stderr = $process.StandardError.ReadToEndAsync()
    if (-not $process.WaitForExit(10000)) {
        $process.Kill($true); $process.WaitForExit(10000) | Out-Null
        throw 'Server help probe exceeded ten seconds.'
    }
    if (-not $stdout.Wait(10000) -or -not $stderr.Wait(10000)) { throw 'Server help output drain timed out.' }
    $help = $stdout.GetAwaiter().GetResult() + $stderr.GetAwaiter().GetResult()
    if ($process.ExitCode -ne 0) { throw 'Server help probe failed.' }
    Assert-BonsaiServerHelp -HelpText $help -Arguments $plan.Arguments
    $plan.ArtifactVerification = if ($VerifyArtifacts) {
        'existing-model-and-runtime-size-sha256-and-arm64-version-verified'
    } else { 'runtime-size-sha256-and-arm64-version-verified; model-not-checked' }
    $plan.RuntimeHelpVerification = 'all-planned-switches-advertised; values/enforcement-not-qualified'
    $plan | Add-Member NoteProperty ServerPath $runtime.ServerPath
    $admission = & (Join-Path $PSScriptRoot 'Invoke-CpuCheck.ps1') -DataDirectory $DataDirectory -CheckOnly
    $plan | Add-Member NoteProperty MemoryAdmission $admission
    return $plan
} finally {
    try {
        if ($started -and -not $process.HasExited) {
            $process.Kill($true)
            if (-not $process.WaitForExit(10000)) { throw 'Server help probe did not terminate.' }
        }
    } finally {
        $process.Dispose()
        # This invocation creates only an empty cwd; never recursively delete it.
        if (-not [IO.Directory]::EnumerateFileSystemEntries($scratch).GetEnumerator().MoveNext()) {
            [IO.Directory]::Delete($scratch, $false)
        }
    }
}
