#requires -Version 7.6
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1')
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Native.psm1')

function Assert-NativeSpeechArtifact {
    [CmdletBinding()]
    [OutputType([void])]
    param([Parameter(Mandatory)][Collections.IDictionary]$Artifact, [Parameter(Mandatory)][string]$ExpectedUrl)
    if ($Artifact.filename -cnotmatch '^[\w.+-]+$' -or
        ($Artifact.bytes -isnot [long] -and $Artifact.bytes -isnot [int]) -or $Artifact.bytes -le 0 -or
        $Artifact.sha256 -cnotmatch '^[a-f0-9]{64}$' -or $Artifact.url -cne $ExpectedUrl) {
        throw 'Native speech artifacts require a pinned URL, simple filename, exact length and SHA-256.'
    }
}

function Get-NativeSpeechModelPlan {
    [CmdletBinding()]
    [OutputType([pscustomobject])]
    param([Parameter(Mandatory)][string]$DataDirectory,
          [ValidateSet('All', 'Canary', 'QwenTts')][string]$Model = 'All')
    $data = Resolve-ExternalPath -Path $DataDirectory
    $identities = @(
        @{ Kind = 'Canary'; Name = 'nvidia/canary-qwen-2.5b'; Directory = 'canary'; Lock = '../src/speech-asr/model.lock.json' },
        @{ Kind = 'QwenTts'; Name = 'Qwen/Qwen3-TTS-12Hz-1.7B-VoiceDesign'; Directory = 'qwen-tts'; Lock = '../src/speech-tts/model.lock.json' }
    )
    foreach ($identity in $identities) {
        if ($Model -ne 'All' -and $identity.Kind -ne $Model) { continue }
        $pin = Get-Content -LiteralPath (Join-Path $PSScriptRoot $identity.Lock) -Raw |
            ConvertFrom-Json -AsHashtable -DateKind String -NoEnumerate
        if ($pin.schemaVersion -ne 1 -or $pin.model -cne $identity.Name -or
            $pin.repository -cnotmatch '^[\w.-]+/[\w.-]+$' -or $pin.revision -cnotmatch '^[a-f0-9]{40}$') {
            throw 'Invalid native speech model identity.'
        }
        $directory = Resolve-ExternalPath -Path (Join-Path -Path $data -ChildPath 'speech/native' -AdditionalChildPath @($identity.Directory, $pin.revision))
        $files = if ($identity.Kind -eq 'Canary') { @{ model = $pin } } else { $pin.files }
        $expectedKeys = @(if ($identity.Kind -eq 'Canary') { 'model' } else { 'talker'; 'codec' })
        if ($files -isnot [Collections.IDictionary] -or $files.Count -ne $expectedKeys.Count) {
            throw 'Native speech model lock omitted a required artifact.'
        }
        foreach ($key in $expectedKeys) {
            if (-not $files.Contains($key)) { throw 'Native speech model lock omitted a required artifact.' }
            $artifact = $files[$key]
            $url = "https://huggingface.co/$($pin.repository)/resolve/$($pin.revision)/$($artifact.filename)"
            Assert-NativeSpeechArtifact -Artifact $artifact -ExpectedUrl $url
            [pscustomobject]@{
                Kind = $identity.Kind; Key = $key; Path = Join-Path $directory $artifact.filename
                Uri = [uri]$url; Bytes = [long]$artifact.bytes; Sha256 = $artifact.sha256
            }
        }
    }
}

function Install-NativeSpeechModels {
    <#
    .SYNOPSIS
    Downloads or verifies pinned native Canary and Qwen VoiceDesign GGUF artifacts.
    .DESCRIPTION
    Reads artifact identities from the two Rust crates' model.lock.json files.
    Reuses exact verified caches under DataDirectory/speech/native. Never loads
    a model, invokes Python, starts a service, or accesses an audio device.
    #>
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseShouldProcessForStateChangingFunctions', '', Justification = 'Explicit artifact provisioning command; verified cache writes are its requested purpose.')]
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseSingularNouns', '', Justification = 'Public launcher API provisions a selected set of speech models.')]
    [CmdletBinding()]
    [OutputType([pscustomobject])]
    param([Parameter(Mandatory)][string]$DataDirectory,
          [ValidateSet('All', 'Canary', 'QwenTts')][string]$Model = 'All', [switch]$VerifyOnly)
    $plan = @(Get-NativeSpeechModelPlan -DataDirectory $DataDirectory -Model $Model)
    $result = [ordered]@{ CanaryModelPath = $null; TtsModelPath = $null; TtsCodecPath = $null; TotalBytes = [long]0 }
    foreach ($file in $plan) {
        if ($VerifyOnly) { Test-LockedArtifact -Path $file.Path -Bytes $file.Bytes -Sha256 $file.Sha256 }
        else { [void](Receive-LockedArtifact -Uri $file.Uri -Path $file.Path -Bytes $file.Bytes -Sha256 $file.Sha256) }
        if ($file.Kind -eq 'Canary') { $result.CanaryModelPath = $file.Path }
        elseif ($file.Key -eq 'talker') { $result.TtsModelPath = $file.Path }
        else { $result.TtsCodecPath = $file.Path }
        $result.TotalBytes += $file.Bytes
    }
    return [pscustomobject]$result
}

Export-ModuleMember -Function Install-NativeSpeechModels
