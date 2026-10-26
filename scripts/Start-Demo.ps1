#requires -Version 7.6
<#
.SYNOPSIS
Builds and opens the native Rust Swift Bonsai 2 chat demo.
.DESCRIPTION
Uses the existing Swift model and pinned CPU runtime. Downloads and verifies
the pinned Canary STT, its tokenizer, and Qwen VoiceDesign TTS model files by
default, reusing verified files and resuming interrupted downloads.
Enables the pinned abliteration LoRA at scale 1 unless NoAbliterateLora is set.
Builds missing app executables with the pinned Rust compiler in external temp.
The native window owns its broker and inference-process lifetime. Build outputs,
credentials and logs remain in the unique external demo directory.
NoSpeechModels skips speech model provisioning. SpeechModelsOnly downloads or
verifies speech files and returns their paths without building or launching.
The default demo also installs a private pinned Qwen TTS Python runtime. The
native app starts its local TTS service only for Read Aloud, then releases it.
NoTts skips this runtime setup. Canary runtime/service setup remains separate.
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
if ($PSBoundParameters.ContainsKey('CanaryPort') -and -not $CanaryKeyFile) {
    throw 'CanaryPort requires CanaryKeyFile.'
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
foreach ($name in @('ChatPath', 'BrokerPath')) {
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
    Write-Information -MessageData 'Preparing pinned Canary STT and Qwen VoiceDesign TTS files. First download is approximately 9 GiB; verified files are reused.' -InformationAction Continue
    $speechModels = & (Join-Path $PSScriptRoot 'Get-SpeechModels.ps1') -DataDirectory $DataDirectory
    Write-Information -MessageData ("Canary model: {0}`nCanary tokenizer: {1}`nQwen VoiceDesign model: {2}" -f
        $speechModels.CanarySnapshotPath, $speechModels.CanaryTokenizerSnapshotPath,
        $speechModels.TtsSnapshotPath) -InformationAction Continue
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
$ttsPython = $ttsService = $null
if (-not $TtsKeyFile -and -not $NoTts -and -not $NoSpeechModels) {
    if ($TtsPort -in @($FrontendPort, $BackendPort) -or
        ($CanaryKeyFile -and $TtsPort -eq $CanaryPort)) {
        throw 'Qwen TTS needs a distinct local port.'
    }
    Import-Module (Join-Path $PSScriptRoot 'Bonsai.TtsRuntime.psm1')
    Write-Information -MessageData 'Preparing the private Qwen TTS runtime. The first setup downloads its pinned Python dependencies; later launches reuse them.' -InformationAction Continue
    $ttsPython = Install-QwenTtsRuntime -DataDirectory $DataDirectory -WorkingDirectory $scratch
    $keys = & (Join-Path $PSScriptRoot 'New-DevelopmentKeys.ps1') -ParentDirectory $scratch
    $TtsKeyFile = $keys.FrontendKeyFile
    $serviceDirectory = Join-Path $scratch 'qwen-tts-service'
    [void][IO.Directory]::CreateDirectory($serviceDirectory)
    foreach ($file in @('server.py','model.lock.json')) {
        Copy-Item -LiteralPath (Join-Path $PSScriptRoot "../tools/qwen-tts/$file") -Destination $serviceDirectory
    }
    $ttsService = Join-Path $serviceDirectory 'server.py'
    Write-Information -MessageData 'Read Aloud is configured for local Qwen VoiceDesign. Its model loads only when requested, and the app releases it when reading finishes.' -InformationAction Continue
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
if ($ttsPython) {
    $arguments += @('--tts-python', $ttsPython, '--tts-service', $ttsService, '--tts-snapshot', $speechModels.TtsSnapshotPath)
}
$result = Invoke-BonsaiNativeProcess -Executable $ChatPath -ArgumentList $arguments -WorkingDirectory $scratch -LogPath (Join-Path $scratch 'demo.log')
if ($result.ExitCode -ne 0) { throw "Demo stopped with exit code $($result.ExitCode). Evidence is retained under $scratch." }
