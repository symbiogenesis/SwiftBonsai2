#requires -Version 7.6
<#
.SYNOPSIS
Builds and opens the native Rust Swift Bonsai 2 chat demo.
.DESCRIPTION
Uses the existing Swift model and pinned CPU runtime. Downloads and verifies
the pinned native Canary STT and Qwen VoiceDesign TTS GGUF files by
default, reusing verified files and resuming interrupted downloads.
Enables the pinned abliteration LoRA at scale 1 unless NoAbliterateLora is set.
Builds missing app executables with the pinned Rust compiler in external temp.
The native window owns its broker and inference-process lifetime. Build outputs,
credentials and logs remain in the unique external demo directory.
NoSpeechModels skips speech model provisioning. SpeechModelsOnly downloads or
verifies speech files and returns their paths without building or launching the app.
The demo builds native ARM64 Rust speech companions using pinned native engines.
The app loads Canary only after an enabled, permitted microphone captures a clip;
it loads Qwen only for Read Aloud. No Python runtime is used.
NoTts skips automatic TTS setup. Explicit keys select externally managed services.
Downloading files and preparing TTS never opens the microphone.
.EXAMPLE
./scripts/Start-Demo.ps1
.EXAMPLE
./scripts/Start-Demo.ps1 -ChatPath C:\Demo\bonsai-chat.exe -BrokerPath C:\Demo\bonsai-broker.exe
.EXAMPLE
./scripts/Start-Demo.ps1 -NoAbliterateLora
.EXAMPLE
./scripts/Start-Demo.ps1 -CanaryKeyFile C:\Speech\canary.key -CanaryPort 8766
.EXAMPLE
./scripts/Start-Demo.ps1 -CanaryKeyFile C:\Speech\canary.key -TtsKeyFile C:\Speech\qwen-tts.key
.EXAMPLE
./scripts/Start-Demo.ps1 -SpeechModelsOnly
.EXAMPLE
./scripts/Start-Demo.ps1 -NoSpeechModels
#>
[CmdletBinding()]
[OutputType([string], [pscustomobject])]
param(
    [string]$DataDirectory = (Join-Path $env:LOCALAPPDATA 'SwiftBonsai2'),
    [string]$ChatPath,
    [string]$BrokerPath,
    [string]$AsrPath,
    [string]$TtsPath,
    [switch]$NoAbliterateLora,
    [ValidateRange(1024, 65535)][int]$FrontendPort = 5173,
    [ValidateRange(1024, 65535)][int]$BackendPort = 8081,
    [string]$CanaryKeyFile,
    [ValidateRange(1024, 65535)][int]$CanaryPort = 8766,
    [string]$TtsKeyFile,
    [ValidateRange(1024, 65535)][int]$TtsPort = 8767,
    [switch]$NoTts,
    [switch]$NoSpeechModels,
    [switch]$SpeechModelsOnly
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1')
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Native.psm1')

if (-not $SpeechModelsOnly -and (-not $IsWindows -or
    [Runtime.InteropServices.RuntimeInformation]::OSArchitecture -ne 'Arm64' -or
    [Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture -ne 'Arm64')) {
    throw 'Run the demo in native ARM64 PowerShell on Windows ARM64.'
}
if ($FrontendPort -eq $BackendPort) { throw 'FrontendPort and BackendPort must differ.' }
if ($NoSpeechModels -and $SpeechModelsOnly) {
    throw 'SpeechModelsOnly cannot be combined with NoSpeechModels.'
}
if ($CanaryKeyFile) {
    $CanaryKeyFile = Resolve-ExternalPath -Path $CanaryKeyFile
    if (-not [IO.File]::Exists($CanaryKeyFile) -or $CanaryPort -in @($FrontendPort, $BackendPort)) {
        throw 'Canary needs an existing external key file and a distinct local port.'
    }
}
if ($NoTts -and $TtsKeyFile) {
    throw 'NoTts cannot be combined with TtsKeyFile.'
}
if ($TtsKeyFile) {
    $TtsKeyFile = Resolve-ExternalPath -Path $TtsKeyFile
    if (-not [IO.File]::Exists($TtsKeyFile) -or $TtsPort -in @($FrontendPort, $BackendPort) -or
        ($CanaryKeyFile -and $TtsPort -eq $CanaryPort)) {
        throw 'Qwen TTS needs an existing external key file and a distinct local port.'
    }
}
$DataDirectory = Resolve-ExternalPath -Path $DataDirectory
if (-not [IO.Directory]::Exists($DataDirectory)) {
    if ($SpeechModelsOnly) { [void][IO.Directory]::CreateDirectory($DataDirectory) }
    else { throw 'DataDirectory must already contain the provisioned Swift model and CPU runtime. See docs/development-guide.md.' }
}
foreach ($name in @('ChatPath', 'BrokerPath', 'AsrPath', 'TtsPath')) {
    $path = Get-Variable -Name $name -ValueOnly
    if ($path) {
        $path = Resolve-ExternalPath -Path $path
        if (-not [IO.File]::Exists($path) -or [IO.Path]::GetExtension($path) -ine '.exe') {
            throw "$name must name an existing executable outside Git."
        }
        Set-Variable -Name $name -Value $path
    }
}
$speechModels = $null
if (-not $NoSpeechModels) {
    Write-Information -MessageData 'Preparing pinned native Canary and Qwen VoiceDesign files (approximately 4.8 GiB total). Verified cached files are reused.' -InformationAction Continue
    $speechModels = & (Join-Path $PSScriptRoot 'Get-NativeSpeechModels.ps1') -DataDirectory $DataDirectory
    Write-Information -MessageData ("Canary model: {0}`nQwen VoiceDesign model: {1}`nQwen audio codec: {2}" -f
        $speechModels.CanaryModelPath, $speechModels.TtsModelPath,
        $speechModels.TtsCodecPath) -InformationAction Continue
}
if ($SpeechModelsOnly) {
    $speechModels
    return
}
$scratch = Resolve-ExternalPath -Path (Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-Demo-' + [guid]::NewGuid().ToString('N')))
[void][IO.Directory]::CreateDirectory($scratch)
if (-not $ChatPath -or -not $BrokerPath) {
    $cargo = (Get-Command -Name cargo -CommandType Application -ErrorAction Stop).Source
    $manifest = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../Cargo.toml'))
    $target = Join-Path $scratch 'build'
    $arguments = @('+nightly-2026-10-22-aarch64-pc-windows-msvc', 'build', '--locked', '--release',
        '--manifest-path', $manifest, '--target-dir', $target)
    if (-not $ChatPath) { $arguments += @('--package', 'bonsai-chat') }
    if (-not $BrokerPath) { $arguments += @('--package', 'bonsai-broker') }
    Write-Information -MessageData 'Building the native chat demo with the pinned Rust compiler.' -InformationAction Continue
    $log = Join-Path $scratch 'build.log'
    $result = Invoke-BonsaiNativeProcess -Executable $cargo -ArgumentList $arguments -WorkingDirectory $scratch -LogPath $log
    if ($result.ExitCode -ne 0) { throw "Demo build failed; inspect $log" }
    if (-not $ChatPath) { $ChatPath = Join-Path $target 'release/bonsai-chat.exe' }
    if (-not $BrokerPath) { $BrokerPath = Join-Path $target 'release/bonsai-broker.exe' }
}
$ownCanary = -not $CanaryKeyFile -and -not $NoSpeechModels
$ownTts = -not $TtsKeyFile -and -not $NoTts -and -not $NoSpeechModels
if (($ownCanary -and $CanaryPort -in @($FrontendPort, $BackendPort)) -or
    ($ownTts -and $TtsPort -in @($FrontendPort, $BackendPort)) -or
    (($ownCanary -or $CanaryKeyFile) -and ($ownTts -or $TtsKeyFile) -and $CanaryPort -eq $TtsPort)) {
    throw 'Every configured speech service needs a distinct local port.'
}
if (($AsrPath -and -not $ownCanary) -or ($TtsPath -and -not $ownTts)) {
    throw 'A prebuilt speech executable requires automatic setup for that service.'
}
if (($ownCanary -and -not $AsrPath) -or ($ownTts -and -not $TtsPath)) {
    Import-Module (Join-Path $PSScriptRoot 'Bonsai.NativeSpeech.psm1')
    $selection = if ($ownCanary -and -not $AsrPath) {
        if ($ownTts -and -not $TtsPath) { 'All' } else { 'Canary' }
    } else { 'QwenTts' }
    Write-Information -MessageData 'Building the native ARM64 speech companions. Pinned compiler and engine sources are cached outside Git.' -InformationAction Continue
    $nativeSpeech = Invoke-NativeSpeechBuild -DataDirectory $DataDirectory -WorkingDirectory $scratch -RepositoryRoot (Split-Path $PSScriptRoot -Parent) -Model $selection
    if ($ownCanary -and -not $AsrPath) { $AsrPath = $nativeSpeech.AsrPath }
    if ($ownTts -and -not $TtsPath) { $TtsPath = $nativeSpeech.TtsPath }
}
foreach ($service in @('Canary', 'Tts')) {
    if (($service -ceq 'Canary' -and $ownCanary) -or ($service -ceq 'Tts' -and $ownTts)) {
        $keyDirectory = Join-Path $scratch ($service.ToLowerInvariant() + '-credentials')
        [void][IO.Directory]::CreateDirectory($keyDirectory)
        $keys = & (Join-Path $PSScriptRoot 'New-DevelopmentKeys.ps1') -ParentDirectory $keyDirectory
        Set-Variable -Name ($service + 'KeyFile') -Value $keys.FrontendKeyFile
    }
}
Write-Information -MessageData "Opening Swift Bonsai 2. Closing its window stops the demo. Evidence: $scratch" -InformationAction Continue
$arguments = @('--broker-path', $BrokerPath, '--data-directory', $DataDirectory,
    '--evidence-directory', $scratch, '--frontend-port', [string]$FrontendPort,
    '--backend-port', [string]$BackendPort)
if ($NoAbliterateLora) { $arguments += '--no-abliterate-lora' }
if ($CanaryKeyFile) {
    $arguments += @('--canary-key-file', $CanaryKeyFile, '--canary-port', [string]$CanaryPort)
}
if ($TtsKeyFile) {
    $arguments += @('--tts-key-file', $TtsKeyFile, '--tts-port', [string]$TtsPort)
}
if ($ownCanary) {
    $arguments += @('--canary-executable', $AsrPath, '--canary-model', $speechModels.CanaryModelPath)
}
if ($ownTts) {
    $arguments += @('--tts-executable', $TtsPath, '--tts-model', $speechModels.TtsModelPath, '--tts-codec', $speechModels.TtsCodecPath)
}
$result = Invoke-BonsaiNativeProcess -Executable $ChatPath -ArgumentList $arguments -WorkingDirectory $scratch -LogPath (Join-Path $scratch 'demo.log')
if ($result.ExitCode -ne 0) { throw "Demo stopped with exit code $($result.ExitCode). Evidence is retained under $scratch." }
