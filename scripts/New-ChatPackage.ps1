#requires -Version 7.6
<#
.SYNOPSIS
Creates a portable native ARM64 chat package from reviewed prebuilt executables.
.DESCRIPTION
Copies the fixed launcher dependency closure, project LICENSE/NOTICE and the
caller-reviewed combined third-party notice file. No Cargo build, network access,
model execution or signing occurs. SourceRevision records the caller's reviewed
build identity; this command cannot prove binary provenance or notice completeness.
Retain the returned manifest hash independently for installation review.
#>
[CmdletBinding()]
[OutputType([pscustomobject])]
param([Parameter(Mandatory)][string]$OutputDirectory,
      [Parameter(Mandatory)][ValidatePattern('^[0-9]+\.[0-9]+\.[0-9]+$')][string]$Version,
      [Parameter(Mandatory)][ValidatePattern('^[a-f0-9]{40}$')][string]$SourceRevision,
      [Parameter(Mandatory)][string]$ChatPath, [Parameter(Mandatory)][string]$BrokerPath,
      [Parameter(Mandatory)][string]$AsrPath, [Parameter(Mandatory)][string]$TtsPath,
      [Parameter(Mandatory)][string]$ToolPath, [Parameter(Mandatory)][string]$ThirdPartyNoticePath)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.ChatPackage.psm1')
New-ChatPackage -OutputDirectory $OutputDirectory -Version $Version -SourceRevision $SourceRevision -ThirdPartyNoticePath $ThirdPartyNoticePath -Binary @{
    'bonsai-chat' = $ChatPath; 'bonsai-broker' = $BrokerPath; 'bonsai-asr' = $AsrPath
    'bonsai-tts' = $TtsPath; 'bonsai-tools' = $ToolPath
}
