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
NativeEngine selects the separately provisioned Rust runtime and requires
NoAbliterateLora with original PQ2 weights.
#>
[CmdletBinding()]
[OutputType([string], [pscustomobject])]
param([Parameter(Mandatory)][string]$InstallationDirectory,
      [string]$DataDirectory = (Join-Path $env:LOCALAPPDATA 'SwiftBonsai2'),
      [switch]$NoSpeechModels, [switch]$NoTts, [switch]$NoAbliterateLora, [switch]$UseRepackedPtq1,
      [switch]$NativeEngine,
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
$options = @{ DataDirectory = $DataDirectory }
foreach ($name in @('NoSpeechModels', 'NoTts', 'NoAbliterateLora', 'UseRepackedPtq1',
    'NativeEngine',
    'TtsQuantization', 'TtsThreads', 'FrontendPort', 'BackendPort', 'CanaryPort', 'TtsPort')) {
    if ($PSBoundParameters.ContainsKey($name)) {
        $value = $PSBoundParameters[$name]
        $options[$name] = if ($value -is [switch]) { [bool]$value } else { $value }
    }
}
$result = Start-InstalledChatPackage -InstallationDirectory $InstallationDirectory -Options $options
if ($result.Contains('Status') -and $result.Status -ceq 'already-running') {
    Write-Information -MessageData $result.Message -InformationAction Continue
}
