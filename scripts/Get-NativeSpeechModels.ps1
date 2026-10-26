#requires -Version 7.6
<#
.SYNOPSIS
Downloads or verifies native ARM64 Canary STT and Qwen VoiceDesign TTS models.
.DESCRIPTION
Uses each Rust crate's immutable GGUF lock, exact length and SHA-256 verification,
and resumable external caches. Does not open audio devices.
Q8_0 is the default TTS talker. Q4_K_M explicitly selects the separately pinned
VoiceDesign talker while retaining the same Q8_0 audio codec.
#>
[CmdletBinding()]
[OutputType([pscustomobject])]
param([Parameter(Mandatory)][string]$DataDirectory,
      [ValidateSet('All', 'Canary', 'QwenTts')][string]$Model = 'All',
      [ValidateSet('Q8_0', 'Q4_K_M')][string]$TtsQuantization = 'Q8_0',
      [switch]$VerifyOnly, [string]$ToolPath)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.NativeSpeech.psm1')
Install-NativeSpeechModels -DataDirectory $DataDirectory -Model $Model -TtsQuantization $TtsQuantization -VerifyOnly:$VerifyOnly -ToolPath $ToolPath
