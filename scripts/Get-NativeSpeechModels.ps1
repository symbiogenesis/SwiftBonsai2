#requires -Version 7.6
<#
.SYNOPSIS
Downloads or verifies native ARM64 Canary STT and Qwen VoiceDesign TTS models.
.DESCRIPTION
Uses each Rust crate's immutable GGUF lock, exact length and SHA-256 verification,
and resumable external caches. Does not open audio devices.
Q8_0 is the default TTS talker. Q4_K_M explicitly selects the separately pinned
VoiceDesign talker while retaining the same Q8_0 audio codec.
DeferCachedVerification size-checks existing files for a launcher whose native
speech services verify SHA-256 before loading. New downloads are fully verified.
#>
[CmdletBinding()]
[OutputType([pscustomobject])]
param([Parameter(Mandatory)][string]$DataDirectory,
      [ValidateSet('All', 'Canary', 'QwenTts')][string]$Model = 'All',
      [ValidateSet('Q8_0', 'Q4_K_M')][string]$TtsQuantization = 'Q8_0',
      [switch]$VerifyOnly, [switch]$DeferCachedVerification, [string]$ToolPath)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
# A long-lived launch shell can retain an older exported parameter contract.
Import-Module (Join-Path $PSScriptRoot 'Bonsai.NativeSpeech.psm1') -Force
Install-NativeSpeechModels -DataDirectory $DataDirectory -Model $Model -TtsQuantization $TtsQuantization -VerifyOnly:$VerifyOnly -DeferCachedVerification:$DeferCachedVerification -ToolPath $ToolPath
