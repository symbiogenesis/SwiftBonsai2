#requires -Version 7.6
<#
.SYNOPSIS
Prepares the fixed CPU server configuration without enabling chat or loading a model.
.DESCRIPTION
By default this validates paths and uses the maintenance tool for policy and
inherited override checks, without starting inference. The tool resolver may
build pinned sources and retain external request/build evidence. -VerifyRuntime checks the installed runtime;
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
    [Parameter(ParameterSetName = 'Artifacts')][switch]$VerifyArtifacts,
    [string]$ToolPath
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
if (-not $IsWindows) { throw 'Server planning targets Windows.' }
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1')
Import-Module (Join-Path $PSScriptRoot 'Bonsai.ServerPolicy.psm1') -Force
$plan = New-BonsaiServerPlan -DataDirectory $DataDirectory -Port $Port -ToolPath $ToolPath
if (-not $VerifyRuntime -and -not $VerifyArtifacts) { return $plan }
if ([Runtime.InteropServices.RuntimeInformation]::OSArchitecture -ne 'Arm64' -or
    [Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture -ne 'Arm64') {
    throw 'Artifact probes require native ARM64 PowerShell on Windows ARM64.'
}
$runtime = & (Join-Path $PSScriptRoot 'Get-Engine.ps1') -DataDirectory $DataDirectory -VerifyOnly -IncludeHelp -ToolPath $ToolPath
if ($VerifyArtifacts) { $null = & (Join-Path $PSScriptRoot 'Get-Model.ps1') -DataDirectory $DataDirectory -VerifyOnly }
$help = $runtime.RuntimeHelp
Assert-BonsaiServerHelp -HelpText $help -Arguments $plan.Arguments
$plan.ArtifactVerification = if ($VerifyArtifacts) {
    'existing-model-and-runtime-size-sha256-and-arm64-version-verified'
} else { 'runtime-size-sha256-and-arm64-version-verified; model-not-checked' }
$plan.RuntimeHelpVerification = 'all-planned-switches-advertised; values/enforcement-not-qualified'
$plan | Add-Member NoteProperty ServerPath $runtime.ServerPath
$admission = & (Join-Path $PSScriptRoot 'Invoke-CpuCheck.ps1') -DataDirectory $DataDirectory -CheckOnly -ToolPath $ToolPath
$plan | Add-Member NoteProperty MemoryAdmission $admission
return $plan
