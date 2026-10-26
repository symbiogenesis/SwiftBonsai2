#requires -Version 7.6
<#
.SYNOPSIS
Downloads or verifies native ARM64 Canary STT and Qwen VoiceDesign TTS models.
.DESCRIPTION
Uses each Rust crate's immutable GGUF lock, exact length and SHA-256 verification,
and resumable external caches. Does not install Python or open audio devices.
#>
[CmdletBinding()]
[OutputType([pscustomobject])]
param([Parameter(Mandatory)][string]$DataDirectory,
      [ValidateSet('All', 'Canary', 'QwenTts')][string]$Model = 'All', [switch]$VerifyOnly)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.NativeSpeech.psm1')
Install-NativeSpeechModels -DataDirectory $DataDirectory -Model $Model -VerifyOnly:$VerifyOnly
