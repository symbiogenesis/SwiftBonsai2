#requires -Version 7.6
<#
.SYNOPSIS
Downloads or verifies pinned Canary STT and Qwen VoiceDesign TTS snapshots.
.DESCRIPTION
Uses immutable model, tokenizer and codec locks with the shared resumable,
size/SHA-256-verified download helper. Stores regular snapshots outside Git,
reuses verified files and rejects altered artifacts without overwriting them.
Does not install Python dependencies, load a model, start a service or open audio.
#>
[CmdletBinding()]
[OutputType([pscustomobject])]
param(
    [Parameter(Mandatory)][string]$DataDirectory,
    [ValidateSet('All', 'Canary', 'QwenTts')][string]$Model = 'All',
    [switch]$VerifyOnly
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.SpeechModels.psm1')
$plan = @(Get-SpeechModelPlan -DataDirectory $DataDirectory -Model $Model)
Invoke-SpeechModelProvision -Plan $plan -VerifyOnly:$VerifyOnly
$result = [ordered]@{
    Status = 'verified'; CanarySnapshotPath = $null; CanaryTokenizerSnapshotPath = $null
    TtsSnapshotPath = $null; TotalBytes = [long]0
}
foreach ($snapshot in $plan) {
    switch -CaseSensitive ($snapshot.Kind) {
        'Canary' { $result.CanarySnapshotPath = $snapshot.SnapshotPath }
        'CanaryTokenizer' { $result.CanaryTokenizerSnapshotPath = $snapshot.SnapshotPath }
        'QwenTts' { $result.TtsSnapshotPath = $snapshot.SnapshotPath }
        default { throw 'Unsupported speech snapshot kind.' }
    }
    foreach ($file in $snapshot.Files) { $result.TotalBytes += $file.Bytes }
}
[pscustomobject]$result
