#requires -Version 7.6
<#
.SYNOPSIS
Opens Swift Bonsai chat through the compatible Start-Chat launcher.
.DESCRIPTION
Retains the original development launcher parameters. Start-Chat owns native
build caching, speech setup and the application session. Prefer Start-Chat for
new shortcuts and instructions.
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
& (Join-Path $PSScriptRoot 'Start-Chat.ps1') @PSBoundParameters