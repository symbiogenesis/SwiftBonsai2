#requires -Version 7.6
<#
.SYNOPSIS
Verifies and launches the active installed native chat package without a compiler.
.DESCRIPTION
Run this script from an installed version or the source checkout. Swift model,
runtime and optional adapter must already be provisioned in DataDirectory. The
packaged launcher can provision pinned speech models through its maintenance
executable. No source checkout, Rust/C++ compiler or Cargo is used at launch.
Native speech requires an installed compatible ARM64 Microsoft Visual C++ runtime;
this package does not install those runtime DLLs.
#>
[CmdletBinding()]
[OutputType([string], [pscustomobject])]
param([Parameter(Mandatory)][string]$InstallationDirectory,
      [string]$DataDirectory = (Join-Path $env:LOCALAPPDATA 'SwiftBonsai2'),
      [switch]$NoSpeechModels, [switch]$NoTts, [switch]$NoAbliterateLora, [switch]$UseRepackedPtq1,
      [ValidateSet('Q8_0', 'Q4_K_M')][string]$TtsQuantization = 'Q8_0',
      [ValidateRange(0, 256)][int]$TtsThreads = 0,
      [ValidateRange(1024, 65535)][int]$FrontendPort = 5173,
      [ValidateRange(1024, 65535)][int]$BackendPort = 8081,
      [ValidateRange(1024, 65535)][int]$CanaryPort = 8766,
      [ValidateRange(1024, 65535)][int]$TtsPort = 8767)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.ChatPackage.psm1')
$package = Get-InstalledChatPackage -InstallationDirectory $InstallationDirectory -Native
$parameters = @{}
foreach ($name in $PSBoundParameters.Keys) { if ($name -cne 'InstallationDirectory') { $parameters[$name] = $PSBoundParameters[$name] } }
$parameters.ChatPath = Join-Path $package.Directory 'bin/bonsai-chat.exe'
$parameters.BrokerPath = Join-Path $package.Directory 'bin/bonsai-broker.exe'
$parameters.ToolPath = Join-Path $package.Directory 'bin/bonsai-tools.exe'
if (-not $NoSpeechModels) {
    $parameters.AsrPath = Join-Path $package.Directory 'bin/bonsai-asr.exe'
    if (-not $NoTts) { $parameters.TtsPath = Join-Path $package.Directory 'bin/bonsai-tts.exe' }
}
& (Join-Path $package.Directory 'scripts/Start-Chat.ps1') @parameters
