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
NativeEngine selects the separately provisioned Rust runtime, preserving the
selected model and adapter options. NativeRowWorkers accepts one or four;
four requires NativeEngine and the reviewed 0.1.9 or 0.1.10 runtime. One is the default.
#>
[CmdletBinding()]
[OutputType([string], [pscustomobject])]
param([Parameter(Mandatory)][string]$InstallationDirectory,
      [string]$DataDirectory = (Join-Path $env:LOCALAPPDATA 'SwiftBonsai2'),
      [switch]$NoSpeechModels, [switch]$NoTts, [switch]$NoAbliterateLora, [switch]$UseRepackedPtq1,
      [switch]$NativeEngine,
      [ValidateSet(1, 4)][int]$NativeRowWorkers = 1,
      [ValidateSet('Q8_0', 'Q4_K_M')][string]$TtsQuantization = 'Q8_0',
      [ValidateRange(0, 256)][int]$TtsThreads = 0,
      [ValidateRange(1024, 65535)][int]$FrontendPort = 5173,
      [ValidateRange(1024, 65535)][int]$BackendPort = 8081,
      [ValidateRange(1024, 65535)][int]$CanaryPort = 8766,
      [ValidateRange(1024, 65535)][int]$TtsPort = 8767)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
if ($NativeRowWorkers -eq 4 -and -not $NativeEngine) { throw 'NativeRowWorkers 4 requires NativeEngine.' }
Import-Module (Join-Path $PSScriptRoot 'Bonsai.ChatPackage.psm1')
$options = @{ DataDirectory = $DataDirectory }
foreach ($name in @('NoSpeechModels', 'NoTts', 'NoAbliterateLora', 'UseRepackedPtq1',
    'NativeEngine', 'NativeRowWorkers',
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
