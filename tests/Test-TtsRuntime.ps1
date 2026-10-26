#requires -Version 7.6
[CmdletBinding()]
[OutputType([string])]
param([ValidateSet('All', 'Plan', 'Launcher')][string]$Case = 'All')
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
if (-not $IsWindows) { throw 'The TTS demo contracts target Windows runtime storage.' }
if ($Case -eq 'All' -and $env:GITHUB_ACTIONS -ne 'true') {
    throw 'The full TTS runtime suite runs only in GitHub Actions. Select one focused -Case locally.'
}

$repo = Split-Path $PSScriptRoot -Parent
Import-Module (Join-Path $PSScriptRoot 'Bonsai.TestSupport.psm1')
$scratch = Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-TtsRuntimeTests-' + [guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($scratch)

if ($Case -in @('All', 'Plan')) {
    Import-Module (Join-Path $repo 'scripts/Bonsai.TtsRuntime.psm1') -Force
    $data = Join-Path $scratch 'uncreated data [literal]'
    $plan = Get-QwenTtsRuntimePlan -DataDirectory $data
    Assert (-not (Test-Path -LiteralPath $data)) 'Read-only TTS runtime planning created storage.'
    Assert ($plan.Identity -cmatch '^[a-f0-9]{64}$') 'The runtime environment is not keyed by its immutable manifest.'
    $lockPath = Join-Path $repo 'tools/qwen-tts/runtime.lock.json'
    $lockBytes = [IO.File]::ReadAllBytes($lockPath)
    $identity = [Convert]::ToHexStringLower([Security.Cryptography.SHA256]::HashData($lockBytes))
    $pin = [Text.Encoding]::UTF8.GetString($lockBytes) | ConvertFrom-Json -AsHashtable -DateKind String
    Assert ($plan.Identity -ceq $identity -and $plan.Artifacts.Count -eq $pin.artifacts.Count) 'Runtime planning omitted pinned dependencies or used another environment identity.'
    Assert ($plan.RuntimeDirectory.StartsWith($data + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) 'The runtime escaped its external data directory.'
    Assert ($plan.PythonPath -ceq (Join-Path $plan.RuntimeDirectory 'python.exe')) 'The runtime does not use its isolated Python interpreter.'
    Assert ($plan.Artifacts.Count -ge 3) 'The TTS runtime plan omitted Python, pip, or inference packages.'
    $names = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
    foreach ($artifact in $plan.Artifacts) {
        $locked = @($pin.artifacts | Where-Object name -CEQ $artifact.Name)
        Assert ($locked.Count -eq 1 -and $locked[0].url -ceq $artifact.Uri.AbsoluteUri -and
            $locked[0].sha256 -ceq $artifact.Sha256 -and $locked[0].bytes -eq $artifact.Bytes -and
            $locked[0].kind -ceq $artifact.Kind) 'Runtime planning changed a pinned package identity.'
        Assert ($names.Add($artifact.Name) -and $artifact.Name -cmatch '^[A-Za-z0-9_.+-]+$') 'Runtime artifact names are repeated or unsafe.'
        Assert ($artifact.Path.StartsWith($data + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) 'An artifact escaped external storage.'
        Assert ($artifact.Bytes -gt 0 -and $artifact.Sha256 -cmatch '^[a-f0-9]{64}$') 'A runtime artifact is missing its exact length or SHA-256.'
        Assert ($artifact.Uri.IsAbsoluteUri -and $artifact.Uri.Scheme -ceq 'https' -and -not $artifact.Uri.UserInfo -and
            -not $artifact.Uri.Query -and -not $artifact.Uri.Fragment) 'A runtime artifact has a mutable or unsafe URL.'
        Assert ($artifact.Uri.Host -cin @('www.python.org', 'python.org', 'files.pythonhosted.org', 'download.pytorch.org', 'download-r2.pytorch.org')) 'A runtime artifact uses an unreviewed publisher.'
    }
    foreach ($invalid in @('relative', 'C:relative', '%LOCALAPPDATA%\runtime', $repo, (Join-Path $scratch 'runtime:stream'))) {
        Assert-Failure { Get-QwenTtsRuntimePlan -DataDirectory $invalid } 'Unsafe TTS runtime storage was accepted.'
    }
    Assert (-not (Test-Path -LiteralPath $data)) 'Runtime plan validation wrote package bytes.'
    $longData = Join-Path $scratch ('long-' + ('x' * 100))
    Assert-Failure { Install-QwenTtsRuntime -DataDirectory $longData -WorkingDirectory $scratch } 'An unsupported Windows DLL path length reached package installation.'
    Assert (-not (Test-Path -LiteralPath $longData)) 'Runtime path rejection wrote or downloaded dependencies.'
}

if ($Case -in @('All', 'Launcher')) {
    $launcher = Join-Path $repo 'scripts/Start-Demo.ps1'
    $tokens = $null
    $errors = $null
    $ast = [Management.Automation.Language.Parser]::ParseFile($launcher, [ref]$tokens, [ref]$errors)
    Assert ($errors.Count -eq 0) 'The demo launcher has parse errors.'
    $parameters = @($ast.ParamBlock.Parameters | ForEach-Object { $_.Name.VariablePath.UserPath })
    Assert ($parameters -ccontains 'NoTts') 'The demo has no explicit automatic TTS opt-out.'
    # Run the actual launcher decisions and argument construction, substituting
    # only external acquisition and the final process boundary. No compiler,
    # package resolver, model loader, microphone or audio device is involved.
    $fixture = Join-Path $scratch 'launcher'
    $scripts = Join-Path $fixture 'scripts'
    $service = Join-Path $fixture 'tools/qwen-tts'
    [void][IO.Directory]::CreateDirectory($scripts)
    [void][IO.Directory]::CreateDirectory($service)
    $data = Join-Path $fixture 'data'
    [void][IO.Directory]::CreateDirectory($data)
    $receipt = Join-Path $fixture 'launch.json'
    $runtimeReceipt = Join-Path $data 'runtime-called'
    $capture = @'
function Invoke-BonsaiNativeProcess {
    [CmdletBinding()]
    [OutputType([pscustomobject])]
    param([string]$Executable, [string[]]$ArgumentList, [string]$WorkingDirectory, [string]$LogPath)
    $record = @{ Executable = $Executable; Arguments = $ArgumentList; WorkingDirectory = $WorkingDirectory; LogPath = $LogPath }
    $receipt = '__RECEIPT__'
    if ([IO.Path]::GetFileName($Executable) -ceq 'cargo.exe') { $receipt += '.build' }
    [IO.File]::WriteAllText($receipt, (ConvertTo-Json -InputObject $record))
    return [pscustomobject]@{ ExitCode = [int][IO.File]::Exists($receipt + '.fail') }
}
Export-ModuleMember -Function Invoke-BonsaiNativeProcess
'@
    $capture = $capture.Replace('__RECEIPT__', $receipt.Replace("'", "''"))
    $copy = Join-Path $scripts 'Start-Demo.ps1'
    $source = [IO.File]::ReadAllText($launcher)
    $cargoPath = (Join-Path $fixture 'cargo.exe').Replace("'", "''")
    $source = $source.Insert($ast.ParamBlock.Extent.EndOffset, "`nfunction Get-Command { [pscustomobject]@{ Source = '$cargoPath' } }`n")
    [IO.File]::WriteAllText($copy, $source)
    [IO.File]::WriteAllText((Join-Path $scripts 'Bonsai.Native.psm1'), $capture)
    Copy-Item -LiteralPath (Join-Path $repo 'scripts/Bonsai.Common.psm1') -Destination $scripts
    [IO.File]::WriteAllText((Join-Path $service 'server.py'), '# Isolated launcher fixture; never executed.')
    [IO.File]::WriteAllText((Join-Path $service 'model.lock.json'), '{}')
    $fakeDownloader = @'
[CmdletBinding()]
param([string]$DataDirectory, [string]$Model = 'All', [switch]$VerifyOnly)
$snapshot = Join-Path $DataDirectory 'tts-snapshot'
[void][IO.Directory]::CreateDirectory($snapshot)
[pscustomobject]@{ Status = 'verified'; CanarySnapshotPath = 'canary'; CanaryTokenizerSnapshotPath = 'tokenizer'; TtsSnapshotPath = $snapshot; TotalBytes = 1 }
'@
    [IO.File]::WriteAllText((Join-Path $scripts 'Get-SpeechModels.ps1'), $fakeDownloader)
    $fakeRuntime = @'
function Install-QwenTtsRuntime {
    [CmdletBinding()]
    [OutputType([string])]
    param([string]$DataDirectory, [string]$WorkingDirectory)
    if ([IO.File]::Exists((Join-Path $DataDirectory 'runtime-failure'))) { throw 'Deliberate runtime setup failure.' }
    [IO.File]::WriteAllText((Join-Path $DataDirectory 'runtime-called'), $WorkingDirectory)
    $python = Join-Path $DataDirectory 'python.exe'
    [IO.File]::WriteAllText($python, '')
    return $python
}
Export-ModuleMember -Function Install-QwenTtsRuntime
'@
    [IO.File]::WriteAllText((Join-Path $scripts 'Bonsai.TtsRuntime.psm1'), $fakeRuntime)
    $fakeKeys = @'
[CmdletBinding()]
param([string]$ParentDirectory, [string]$ToolPath)
$key = Join-Path $ParentDirectory 'fixture-tts.key'
[IO.File]::WriteAllText($key, ('a' * 64))
[pscustomobject]@{ FrontendKeyFile = $key; BackendKeyFile = $key }
'@
    [IO.File]::WriteAllText((Join-Path $scripts 'New-DevelopmentKeys.ps1'), $fakeKeys)
    $chat = Join-Path $fixture 'chat.exe'
    $broker = Join-Path $fixture 'broker.exe'
    [IO.File]::WriteAllText($chat, '')
    [IO.File]::WriteAllText($broker, '')
    $launch = @{ DataDirectory = $data; ChatPath = $chat; BrokerPath = $broker }
    $previousDirectory = [Environment]::CurrentDirectory
    $previousLocation = (Get-Location).Path
    & $copy @launch | Out-Null
    $process = Get-Content -LiteralPath $receipt -Raw | ConvertFrom-Json
    $arguments = @($process.Arguments)
    Assert ($process.Executable -ceq $chat -and [IO.Directory]::Exists($process.WorkingDirectory) -and
        $process.LogPath -ceq (Join-Path $process.WorkingDirectory 'demo.log')) 'The native app lost its executable, external working directory or retained log.'
    Assert ([Environment]::CurrentDirectory -ceq $previousDirectory -and (Get-Location).Path -ceq $previousLocation) 'The launcher changed the caller working directory.'
    Assert ([IO.File]::Exists($runtimeReceipt)) 'Ordinary demo launch omitted automatic TTS runtime setup.'
    foreach ($option in @('--tts-python', '--tts-service', '--tts-snapshot', '--tts-key-file', '--tts-port')) {
        Assert ($arguments -ccontains $option) 'Ordinary demo launch omitted owned TTS configuration.'
    }
    $scriptIndex = [Array]::IndexOf($arguments, '--tts-service')
    $copiedService = [string]$arguments[$scriptIndex + 1]
    Assert ([IO.File]::Exists($copiedService) -and -not $copiedService.StartsWith($repo, [StringComparison]::OrdinalIgnoreCase)) 'The TTS service was not staged to external owned storage.'

    [IO.File]::Delete($runtimeReceipt)
    [IO.File]::Delete($receipt)
    $failure = Join-Path $data 'runtime-failure'
    [IO.File]::WriteAllText($failure, '')
    Assert-Failure { & $copy @launch } 'Runtime setup failure did not fail demo startup.'
    Assert (-not [IO.File]::Exists($receipt)) 'The demo launched with an unavailable automatic TTS runtime.'
    [IO.File]::Delete($failure)
    $externalKey = Join-Path $data 'external-tts.key'
    [IO.File]::WriteAllText($externalKey, ('b' * 64))
    & $copy @launch -TtsKeyFile $externalKey | Out-Null
    $arguments = @((Get-Content -LiteralPath $receipt -Raw | ConvertFrom-Json).Arguments)
    Assert (-not [IO.File]::Exists($runtimeReceipt)) 'An explicit external TTS service triggered owned runtime installation.'
    Assert ($arguments -ccontains '--tts-key-file' -and $arguments -cnotcontains '--tts-python') 'External TTS configuration was replaced by an owned service.'

    foreach ($switch in @('NoTts', 'NoSpeechModels')) {
        $optOut = @{ $switch = $true }
        & $copy @launch @optOut | Out-Null
        $arguments = @((Get-Content -LiteralPath $receipt -Raw | ConvertFrom-Json).Arguments)
        Assert (-not [IO.File]::Exists($runtimeReceipt)) 'The automatic TTS opt-out installed a runtime.'
        Assert ($arguments -cnotcontains '--tts-python') 'The automatic TTS opt-out passed an owned service.'
    }
    foreach ($selected in @(
        @{ Paths = @{ BrokerPath = $broker }; Packages = @('bonsai-chat') },
        @{ Paths = @{ ChatPath = $chat }; Packages = @('bonsai-broker') },
        @{ Paths = @{}; Packages = @('bonsai-chat', 'bonsai-broker') }
    )) {
        $paths = $selected.Paths
        & $copy -DataDirectory $data -NoSpeechModels @paths | Out-Null
        $build = Get-Content -LiteralPath ($receipt + '.build') -Raw | ConvertFrom-Json
        $packages = for ($index = 0; $index -lt $build.Arguments.Count; $index++) {
            if ($build.Arguments[$index] -ceq '--package') { $build.Arguments[$index + 1] }
        }
        Assert (($packages -join ',') -ceq ($selected.Packages -join ',')) 'Demo compilation changed the selected missing app packages.'
        Assert ($build.LogPath -ceq (Join-Path $build.WorkingDirectory 'build.log') -and
            $build.Arguments -ccontains '--locked' -and $build.Arguments -ccontains '--release') 'Demo compilation lost its retained log or locked Release build.'
    }
    [IO.File]::Delete($receipt)
    [IO.File]::WriteAllText(($receipt + '.build.fail'), '')
    Assert-Failure { & $copy -DataDirectory $data -NoSpeechModels } 'A failed compiler did not stop demo startup.'
    Assert (-not [IO.File]::Exists($receipt)) 'A failed compiler still launched the demo.'
    [IO.File]::WriteAllText(($receipt + '.fail'), '')
    Assert-Failure { & $copy @launch -NoSpeechModels } 'A nonzero native app exit was accepted.'
    [IO.File]::Delete($receipt)
    & $copy -DataDirectory $data -SpeechModelsOnly | Out-Null
    Assert (-not [IO.File]::Exists($runtimeReceipt) -and -not [IO.File]::Exists($receipt)) 'Download-only mode installed a runtime or opened the native app.'
}

Write-Output "TTS runtime $Case checks passed. Evidence: $scratch"
