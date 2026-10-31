#requires -Version 7.6
[CmdletBinding()]
[OutputType([string])]
param([ValidateSet('All','Plan','Selection','Provision','EvaluationPolicy')][string]$Case = 'All')
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
if (-not $IsWindows) { throw 'These focused contracts target Windows artifact storage.' }
if ($Case -eq 'All' -and $env:GITHUB_ACTIONS -ne 'true') {
    throw 'The full speech-model suite runs only in GitHub Actions. Select one focused -Case locally.'
}

$repo = Split-Path $PSScriptRoot -Parent
Import-Module (Join-Path $PSScriptRoot 'Bonsai.TestSupport.psm1')
$modulePath = Join-Path $repo 'scripts/Bonsai.SpeechModels.psm1'
$module = Import-Module $modulePath -Force -PassThru
$scratch = Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-SpeechModelTests-' + [guid]::NewGuid().ToString('N'))
if (-not [IO.Path]::IsPathFullyQualified($scratch) -or $scratch -match '%[^%]+%') {
    throw 'Expected expanded absolute scratch storage.'
}
[void][IO.Directory]::CreateDirectory($scratch)

if ($Case -in @('All','EvaluationPolicy')) {
    $identity = @{
        GITHUB_ACTIONS = 'true'; GITHUB_REF = 'refs/heads/main'
        GITHUB_REPOSITORY = 'symbiogenesis/SwiftBonsai2Runner'; RUNNER_ENVIRONMENT = 'self-hosted'
    }
    $previous = @{}
    foreach ($name in $identity.Keys) {
        $previous[$name] = [Environment]::GetEnvironmentVariable($name)
    }
    $evaluation = Join-Path $repo 'scripts/Invoke-TtsEvaluation.ps1'
    $uncreated = Join-Path $scratch 'uncreated evaluation'
    try {
        foreach ($name in $identity.Keys) {
            [Environment]::SetEnvironmentVariable($name, $identity[$name])
        }
        foreach ($environment in @('self-hosted', '', 'unknown')) {
            [Environment]::SetEnvironmentVariable('RUNNER_ENVIRONMENT', $environment)
            $failure = $null
            try {
                # An absent tool proves rejection precedes tool resolution,
                # native builds, model provisioning and evidence creation.
                & $evaluation -DataDirectory $uncreated -EvidenceDirectory $uncreated -ToolPath (Join-Path $uncreated 'absent.exe')
            } catch { $failure = $_.Exception.Message }
            Assert ($null -ne $failure -and $failure.Contains('complete five-minute budget')) 'The owner-device matrix reached preparation or failed without retaining its pending budget gate.'
            Assert (-not [IO.Directory]::Exists($uncreated)) 'Rejected owner-device evaluation created preparation or model storage.'
        }
    } finally {
        foreach ($name in $previous.Keys) {
            [Environment]::SetEnvironmentVariable($name, $previous[$name])
        }
    }
}

if ($Case -in @('All','Plan')) {
    $data = Join-Path $scratch 'uncreated data [literal]'
    $plan = @(Get-SpeechModelPlan -DataDirectory $data -Model All)
    Assert ($plan.Count -eq 3) 'The all-model plan omitted a speech snapshot.'
    Assert (-not (Test-Path -LiteralPath $data)) 'Read-only planning created artifact storage.'
    $expected = @{
        Canary = @{ Model = 'nvidia/canary-qwen-2.5b'; Revision = 'b1469e1bba1cfe140205529c79c434ca47180960'; Directory = 'canary'; Lock = 'tools/canary-asr/model.lock.json' }
        CanaryTokenizer = @{ Model = 'Qwen/Qwen3-1.7B'; Revision = '70d244cc86ccca08cf5af4e1e306ecf908b1ad5e'; Directory = 'canary-tokenizer'; Lock = 'tools/canary-asr/tokenizer.lock.json' }
        QwenTts = @{ Model = 'Qwen/Qwen3-TTS-12Hz-1.7B-VoiceDesign'; Revision = '5ecdb67327fd37bb2e042aab12ff7391903235d3'; Directory = 'qwen-tts'; Lock = 'tools/qwen-tts/model.lock.json' }
    }
    $seen = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    $files = 0
    [long]$totalBytes = 0
    foreach ($snapshot in $plan) {
        Assert ($seen.Add($snapshot.Kind) -and $expected.ContainsKey($snapshot.Kind)) 'The speech plan repeated or invented a snapshot kind.'
        $identity = $expected[$snapshot.Kind]
        Assert ($snapshot.Model -ceq $identity.Model -and $snapshot.Revision -ceq $identity.Revision) 'The speech plan selected an unpinned model identity.'
        $directory = Join-Path $data -ChildPath 'speech', $identity.Directory, $identity.Revision
        Assert ($snapshot.SnapshotPath -ceq $directory -and $snapshot.DataDirectory -ceq $data) 'The snapshot escaped its immutable external lane.'
        $pin = Get-Content -LiteralPath (Join-Path $repo $identity.Lock) -Raw | ConvertFrom-Json -AsHashtable -DateKind String
        Assert ($snapshot.Files.Count -eq $pin.files.Count) 'The plan omitted locked model files.'
        foreach ($file in $snapshot.Files) {
            Assert ($pin.files.Contains($file.Name)) 'The plan invented an unpinned runtime file.'
            $artifact = $pin.files[$file.Name]
            Assert ($file.Path -ceq (Join-Path $directory $file.Name)) 'A model artifact escaped its snapshot.'
            Assert ($file.Uri.AbsoluteUri -ceq $artifact.url -and $file.Uri.Scheme -ceq 'https' -and
                $file.Uri.Host -ceq 'huggingface.co' -and -not $file.Uri.Query -and -not $file.Uri.UserInfo) 'A model artifact used an unsafe or mutable URL.'
            Assert ($file.Bytes -eq $artifact.bytes -and $file.Sha256 -ceq $artifact.sha256 -and
                $file.Sha256 -cmatch '^[a-f0-9]{64}$') 'A model artifact lost its exact length or hash.'
            $files++
            $totalBytes += $file.Bytes
        }
    }
    Assert ($files -eq 19 -and $totalBytes -gt 8GB) 'Speech weights or tokenizer dependencies were omitted.'
    Assert (-not (Test-Path -LiteralPath $data)) 'Plan validation wrote model bytes.'
}

if ($Case -in @('All','Selection')) {
    $data = Join-Path $scratch 'selection'
    $canary = @(Get-SpeechModelPlan -DataDirectory $data -Model Canary)
    $tts = @(Get-SpeechModelPlan -DataDirectory $data -Model QwenTts)
    Assert ($canary.Count -eq 2 -and $canary.Kind -ccontains 'Canary' -and
        $canary.Kind -ccontains 'CanaryTokenizer') 'Canary selection omitted its pinned tokenizer.'
    Assert ($tts.Count -eq 1 -and $tts[0].Kind -ceq 'QwenTts') 'TTS-only selection downloaded another model.'
    foreach ($invalid in @('relative', 'C:relative', '%LOCALAPPDATA%\speech', $repo, (Join-Path $scratch 'speech:stream'))) {
        Assert-Failure { Get-SpeechModelPlan -DataDirectory $invalid -Model All } 'Unsafe model storage was accepted.'
    }
    Assert-Failure { Get-SpeechModelPlan -DataDirectory $data -Model 'arbitrary' } 'An arbitrary requested model was accepted.'
    Assert (-not (Test-Path -LiteralPath $data)) 'Selection checks created model storage.'
}

if ($Case -in @('All','Provision')) {
    $data = Join-Path $scratch 'provision'
    $plan = @(Get-SpeechModelPlan -DataDirectory $data -Model All)
    & $module {
        $script:received = [Collections.Generic.List[object]]::new()
        $script:verified = [Collections.Generic.List[object]]::new()
        function script:Receive-LockedArtifact {
            [CmdletBinding()]
            [OutputType([string])]
            param([uri]$Uri, [string]$Path, [long]$Bytes, [string]$Sha256, [int]$TimeoutSeconds)
            $script:received.Add([pscustomobject]@{ Uri = $Uri; Path = $Path; Bytes = $Bytes; Sha256 = $Sha256; TimeoutSeconds = $TimeoutSeconds })
            return $Path
        }
        function script:Test-LockedArtifact {
            [CmdletBinding()]
            [OutputType([void])]
            param([string]$Path, [long]$Bytes, [string]$Sha256)
            $script:verified.Add([pscustomobject]@{ Path = $Path; Bytes = $Bytes; Sha256 = $Sha256 })
        }
    }
    Invoke-SpeechModelProvision -Plan $plan -VerifyOnly
    $counts = & $module { @($script:received.Count, $script:verified.Count) }
    Assert ($counts[0] -eq 0 -and $counts[1] -eq 19) 'VerifyOnly performed a download or omitted cached files.'
    Invoke-SpeechModelProvision -Plan $plan
    $received = @(& $module { $script:received.ToArray() })
    Assert ($received.Count -eq 19) 'Provisioning omitted a locked artifact.'
    $expectedFiles = @($plan | ForEach-Object { $_.Files })
    for ($index = 0; $index -lt $expectedFiles.Count; $index++) {
        $wanted = $expectedFiles[$index]
        $actual = $received[$index]
        Assert ($actual.Path -ceq $wanted.Path -and $actual.Uri.AbsoluteUri -ceq $wanted.Uri.AbsoluteUri -and
            $actual.Bytes -eq $wanted.Bytes -and $actual.Sha256 -ceq $wanted.Sha256) 'Provisioning changed the verified downloader contract.'
    }
    foreach ($field in @('Path','Uri','Bytes','Sha256')) {
        $invalidPlan = @(Get-SpeechModelPlan -DataDirectory $data -Model All)
        switch ($field) {
            Path { $invalidPlan[-1].Files[0].Path = Join-Path $scratch 'escaped.bin' }
            Uri { $invalidPlan[-1].Files[0].Uri = [uri]'https://example.invalid/unreviewed.bin' }
            Bytes { $invalidPlan[-1].Files[0].Bytes++ }
            Sha256 { $invalidPlan[-1].Files[0].Sha256 = '0' * 64 }
        }
        Assert-Failure { Invoke-SpeechModelProvision -Plan $invalidPlan } 'A mutated public plan was accepted.'
        $counts = & $module { @($script:received.Count, $script:verified.Count) }
        Assert ($counts[0] -eq 19 -and $counts[1] -eq 19) 'Invalid plan caused I/O before full validation.'
    }
    Assert (-not (Test-Path -LiteralPath $data)) 'Mocked provisioning unexpectedly wrote model storage.'
    Import-Module $modulePath -Force | Out-Null
}

Write-Output "Speech model $Case checks passed. Evidence: $scratch"
