#requires -Version 7.6
<#
.SYNOPSIS
Builds and opens the native Rust Swift Bonsai 2 chat demo.
.DESCRIPTION
Uses the existing Swift model and pinned CPU runtime; downloads no artifacts.
Enables the pinned abliteration LoRA at scale 1 unless NoAbliterateLora is set.
Builds missing app executables with the pinned Rust compiler in external temp.
The native window owns its broker and inference-process lifetime. Build outputs,
credentials and logs remain in the unique external demo directory.
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
#>
[CmdletBinding()]
[OutputType([string])]
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
    [ValidateRange(1024, 65535)][int]$TtsPort = 8767
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1')

if (-not $IsWindows -or [Runtime.InteropServices.RuntimeInformation]::OSArchitecture -ne 'Arm64' -or
    [Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture -ne 'Arm64') {
    throw 'Run the demo in native ARM64 PowerShell on Windows ARM64.'
}
if ($FrontendPort -eq $BackendPort) { throw 'FrontendPort and BackendPort must differ.' }
if ($PSBoundParameters.ContainsKey('CanaryPort') -and -not $CanaryKeyFile) {
    throw 'CanaryPort requires CanaryKeyFile.'
}
if ($CanaryKeyFile) {
    $CanaryKeyFile = Resolve-ExternalPath -Path $CanaryKeyFile
    if (-not [IO.File]::Exists($CanaryKeyFile) -or $CanaryPort -in @($FrontendPort, $BackendPort)) {
        throw 'Canary needs an existing external key file and a distinct local port.'
    }
}
if ($PSBoundParameters.ContainsKey('TtsPort') -and -not $TtsKeyFile) {
    throw 'TtsPort requires TtsKeyFile.'
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
    throw 'DataDirectory must already contain the provisioned Swift model and CPU runtime. See docs/development-guide.md.'
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
$scratch = Resolve-ExternalPath -Path (Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-Demo-' + [guid]::NewGuid().ToString('N')))
[void][IO.Directory]::CreateDirectory($scratch)
$previousDirectory = [Environment]::CurrentDirectory
Push-Location $scratch
try {
    [Environment]::CurrentDirectory = $scratch
    if (-not $ChatPath -or -not $BrokerPath) {
        $cargo = (Get-Command -Name cargo -CommandType Application -ErrorAction Stop).Source
        $manifest = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../Cargo.toml'))
        $target = Join-Path $scratch 'build'
        $arguments = @('+nightly-2026-10-22-aarch64-pc-windows-msvc', 'build', '--locked', '--release',
            '--manifest-path', $manifest, '--target-dir', $target)
        if (-not $ChatPath) { $arguments += @('--package', 'bonsai-chat') }
        if (-not $BrokerPath) { $arguments += @('--package', 'bonsai-broker') }
        Write-Information -MessageData 'Building the native chat demo with the pinned Rust compiler.' -InformationAction Continue
        & $cargo @arguments
        if ($LASTEXITCODE -ne 0) { throw 'Demo build failed.' }
        if (-not $ChatPath) { $ChatPath = Join-Path $target 'release/bonsai-chat.exe' }
        if (-not $BrokerPath) { $BrokerPath = Join-Path $target 'release/bonsai-broker.exe' }
    }
    Write-Information -MessageData "Opening Swift Bonsai 2. Closing its window stops the demo. Evidence: $scratch" -InformationAction Continue
    $start = [Diagnostics.ProcessStartInfo]::new($ChatPath)
    $start.UseShellExecute = $false
    $start.WorkingDirectory = $scratch
    foreach ($argument in @('--broker-path', $BrokerPath, '--data-directory', $DataDirectory,
            '--evidence-directory', $scratch, '--frontend-port', [string]$FrontendPort,
            '--backend-port', [string]$BackendPort)) {
        [void]$start.ArgumentList.Add($argument)
    }
    if ($NoAbliterateLora) { [void]$start.ArgumentList.Add('--no-abliterate-lora') }
    if ($CanaryKeyFile) {
        foreach ($argument in @('--canary-key-file', $CanaryKeyFile, '--canary-port', [string]$CanaryPort)) {
            [void]$start.ArgumentList.Add($argument)
        }
    }
    if ($TtsKeyFile) {
        foreach ($argument in @('--tts-key-file', $TtsKeyFile, '--tts-port', [string]$TtsPort)) {
            [void]$start.ArgumentList.Add($argument)
        }
    }
    $process = [Diagnostics.Process]::Start($start)
    if ($null -eq $process) { throw 'The native demo process could not start.' }
    try {
        $process.WaitForExit()
        if ($process.ExitCode -ne 0) { throw "Demo stopped with exit code $($process.ExitCode). Evidence is retained under $scratch." }
    } finally {
        $process.Dispose()
    }
} finally {
    [Environment]::CurrentDirectory = $previousDirectory
    Pop-Location
}
