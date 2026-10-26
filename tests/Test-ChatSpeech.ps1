#requires -Version 7.6
[CmdletBinding()]
[OutputType([string])]
param([ValidateSet('All', 'Launcher', 'StartupWork', 'DownloadOnly', 'Rejections', 'Cache')][string]$Case = 'All')
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
if (-not $IsWindows) { throw 'Native speech launch contracts target Windows.' }
if ($Case -ceq 'All' -and $env:GITHUB_ACTIONS -cne 'true') {
    throw 'Complete suites run only in GitHub Actions. Select a focused case locally.'
}
$repo = Split-Path $PSScriptRoot -Parent
Import-Module (Join-Path $PSScriptRoot 'Bonsai.TestSupport.psm1')
$scratch = Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-ChatSpeech-' + [guid]::NewGuid().ToString('N'))
$scripts = Join-Path $scratch 'scripts'
$data = Join-Path $scratch 'data'
[void][IO.Directory]::CreateDirectory($scripts)
[void][IO.Directory]::CreateDirectory($data)
$receipt = Join-Path $scratch 'launch.json'
$copy = Join-Path $scripts 'Start-Chat.ps1'
$source = [IO.File]::ReadAllText((Join-Path $repo 'scripts/Start-Chat.ps1'))
$tokens = $null
$errors = $null
$ast = [Management.Automation.Language.Parser]::ParseInput($source, [ref]$tokens, [ref]$errors)
Assert ($errors.Count -eq 0) 'Launcher parse failure.'
$cargo = (Join-Path $scratch 'cargo.exe').Replace("'", "''")
$source = $source.Insert($ast.ParamBlock.Extent.EndOffset, "`nfunction Get-Command { [pscustomobject]@{ Source = '$cargo' } }`n")
[IO.File]::WriteAllText($copy, $source)
Copy-Item -LiteralPath (Join-Path $repo 'scripts/Bonsai.Common.psm1') -Destination $scripts
$toolResolver = @'
function Resolve-BonsaiRustTool {
    [CmdletBinding()]
    [OutputType([string])]
    param([string]$Name, [string]$ManifestRelativePath, [string]$ToolPath)
    [IO.File]::AppendAllText('__RESOLVED__', "resolved`n")
    if ($ToolPath) { return $ToolPath }
    return '__TOOL__'
}
Export-ModuleMember -Function Resolve-BonsaiRustTool
'@
[IO.File]::WriteAllText((Join-Path $scripts 'Bonsai.Tools.psm1'), $toolResolver.Replace('__RESOLVED__', (Join-Path $data 'tool-resolved').Replace("'", "''")).Replace('__TOOL__', (Join-Path $data 'bonsai-tools.exe').Replace("'", "''")))
$capture = @'
function Invoke-BonsaiNativeProcess {
    [CmdletBinding()]
    [OutputType([pscustomobject])]
    param([string]$Executable, [string[]]$ArgumentList, [string]$WorkingDirectory, [string]$LogPath)
    $record = @{ Executable = $Executable; Arguments = $ArgumentList; WorkingDirectory = $WorkingDirectory; LogPath = $LogPath }
    $receipt = '__RECEIPT__'
    if ([IO.Path]::GetFileName($Executable) -ceq 'cargo.exe') { $receipt += '.build' }
    [IO.File]::WriteAllText($receipt, (ConvertTo-Json -InputObject $record))
    [pscustomobject]@{ ExitCode = [int][IO.File]::Exists($receipt + '.fail') }
}
Export-ModuleMember -Function Invoke-BonsaiNativeProcess
'@
[IO.File]::WriteAllText((Join-Path $scripts 'Bonsai.Native.psm1'), $capture.Replace('__RECEIPT__', $receipt.Replace("'", "''")))
$download = @'
[CmdletBinding()]
param([string]$DataDirectory, [string]$Model, [string]$TtsQuantization = 'Q8_0', [switch]$DeferCachedVerification, [string]$ToolPath)
[IO.File]::WriteAllText((Join-Path $DataDirectory 'download-called'), '')
[IO.File]::WriteAllText((Join-Path $DataDirectory 'download-plan.json'), (ConvertTo-Json @{ Model = $Model; Deferred = [bool]$DeferCachedVerification; ToolPath = $ToolPath }))
[pscustomobject]@{ CanaryModelPath = 'canary.gguf'; TtsModelPath = 'talker.gguf'; TtsCodecPath = 'codec.gguf'; TotalBytes = 1 }
'@
[IO.File]::WriteAllText((Join-Path $scripts 'Get-NativeSpeechModels.ps1'), $download)
$build = @'
function Invoke-NativeSpeechBuild {
    [CmdletBinding()]
    [OutputType([pscustomobject])]
    param([string]$DataDirectory, [string]$WorkingDirectory, [string]$RepositoryRoot, [string]$Model, [string]$ToolPath)
    [IO.File]::WriteAllText((Join-Path $DataDirectory 'native-build-called'), $Model)
    [IO.File]::WriteAllText((Join-Path $DataDirectory 'build-tool'), $ToolPath)
    if ([IO.File]::Exists((Join-Path $DataDirectory 'native-build-failure'))) { throw 'Deliberate native build failure.' }
    [pscustomobject]@{ AsrPath = (Join-Path $DataDirectory 'asr.exe'); TtsPath = (Join-Path $DataDirectory 'tts.exe') }
}
Export-ModuleMember -Function Invoke-NativeSpeechBuild
'@
[IO.File]::WriteAllText((Join-Path $scripts 'Bonsai.NativeSpeech.psm1'), $build)
$keys = @'
[CmdletBinding()]
param([string]$ParentDirectory, [string]$ToolPath)
$key = Join-Path $ParentDirectory 'fixture.key'
[IO.File]::WriteAllText($key, ('a' * 64))
[IO.File]::WriteAllText((Join-Path $ParentDirectory 'key-tool'), $ToolPath)
[pscustomobject]@{ FrontendKeyFile = $key }
'@
[IO.File]::WriteAllText((Join-Path $scripts 'New-DevelopmentKeys.ps1'), $keys)
$chat = Join-Path $scratch 'chat.exe'
$broker = Join-Path $scratch 'broker.exe'
[IO.File]::WriteAllText($chat, '')
[IO.File]::WriteAllText($broker, '')
$launch = @{ DataDirectory = $data; ChatPath = $chat; BrokerPath = $broker }

if ($Case -cin @('All', 'StartupWork')) {
    $resolved = Join-Path $data 'tool-resolved'
    $planPath = Join-Path $data 'download-plan.json'
    & $copy @launch | Out-Null
    $plan = Get-Content -LiteralPath $planPath -Raw | ConvertFrom-Json
    Assert ($plan.Model -ceq 'All' -and $plan.Deferred) 'Warm launch retained eager speech hashing or omitted an owned engine.'
    Assert (@([IO.File]::ReadAllLines($resolved)).Count -eq 1) 'Maintenance tool was resolved more than once per launch.'
    Assert ([IO.File]::ReadAllText((Join-Path $data 'build-tool')) -ceq $plan.ToolPath) 'Speech compilation did not share the resolved maintenance tool.'
    $record = Get-Content -LiteralPath $receipt -Raw | ConvertFrom-Json
    foreach ($service in @('canary', 'tts')) {
        $keyTool = Join-Path -Path $record.WorkingDirectory -ChildPath ($service + '-credentials') -AdditionalChildPath 'key-tool'
        Assert ([IO.File]::ReadAllText($keyTool) -ceq $plan.ToolPath) 'Credentials did not share the resolved maintenance tool.'
    }
    & $copy @launch -NoTts | Out-Null
    $plan = Get-Content -LiteralPath $planPath -Raw | ConvertFrom-Json
    Assert ($plan.Model -ceq 'Canary' -and $plan.Deferred) 'NoTts prepared unused Qwen files.'
    Assert (@([IO.File]::ReadAllLines($resolved)).Count -eq 2) 'Repeated same-shell launch skipped its maintenance freshness check.'
    $external = Join-Path $data 'external.key'
    [IO.File]::WriteAllText($external, ('b' * 64))
    & $copy @launch -CanaryKeyFile $external | Out-Null
    $plan = Get-Content -LiteralPath $planPath -Raw | ConvertFrom-Json
    Assert ($plan.Model -ceq 'QwenTts') 'External Canary prepared unused recognition files.'
    [IO.File]::Delete($planPath)
    & $copy @launch -CanaryKeyFile $external -TtsKeyFile $external | Out-Null
    & $copy @launch -NoSpeechModels | Out-Null
    Assert (-not [IO.File]::Exists($planPath) -and @([IO.File]::ReadAllLines($resolved)).Count -eq 3) 'Text-only or externally managed speech performed automatic model/tool setup.'
    & $copy -DataDirectory $data -SpeechModelsOnly | Out-Null
    $plan = Get-Content -LiteralPath $planPath -Raw | ConvertFrom-Json
    Assert ($plan.Model -ceq 'All' -and -not $plan.Deferred) 'Explicit provisioning stopped fully verifying both engines.'
}

if ($Case -cin @('All', 'Launcher')) {
    $original = [Environment]::CurrentDirectory
    & $copy @launch | Out-Null
    $record = Get-Content -LiteralPath $receipt -Raw | ConvertFrom-Json
    $arguments = @($record.Arguments)
    Assert ($arguments -cnotcontains '--use-repacked-ptq1') 'Default launch silently changed the model packing.'
    Assert ($record.Executable -ceq $chat -and [IO.Directory]::Exists($record.WorkingDirectory)) 'Native app lost its external working directory.'
    Assert ([Environment]::CurrentDirectory -ceq $original) 'Launcher changed the caller process working directory.'
    foreach ($option in @('--canary-key-file', '--canary-executable', '--canary-model', '--tts-key-file', '--tts-executable', '--tts-model', '--tts-codec')) {
        Assert ($arguments -ccontains $option) "Missing automatic speech option $option."
    }
    Assert (-not ($arguments | Where-Object { $_ -match 'python|\.py$' })) 'Native launch retained a Python dependency.'
    Assert ([IO.File]::ReadAllText((Join-Path $data 'native-build-called')) -ceq 'All') 'Ordinary launch did not build both native engines.'
    & $copy @launch -TtsQuantization q4_k_m -TtsThreads 10 | Out-Null
    $tuned = @((Get-Content -LiteralPath $receipt -Raw | ConvertFrom-Json).Arguments)
    $quantizationAt = [Array]::IndexOf($tuned, '--tts-quantization')
    $threadsAt = [Array]::IndexOf($tuned, '--tts-threads')
    Assert ($quantizationAt -ge 0 -and $tuned[$quantizationAt + 1] -ceq 'Q4_K_M' -and
        $threadsAt -ge 0 -and $tuned[$threadsAt + 1] -ceq '10') 'Owned TTS lost its explicit profile or thread budget.'
    $external = Join-Path $data 'external.key'
    [IO.File]::WriteAllText($external, ('b' * 64))
    [IO.File]::Delete((Join-Path $data 'native-build-called'))
    & $copy @launch -CanaryKeyFile $external -TtsKeyFile $external | Out-Null
    $arguments = @((Get-Content -LiteralPath $receipt -Raw | ConvertFrom-Json).Arguments)
    Assert (-not [IO.File]::Exists((Join-Path $data 'native-build-called'))) 'External services triggered compilation.'
    Assert ($arguments -ccontains '--tts-key-file' -and $arguments -cnotcontains '--tts-executable' -and
        $arguments -cnotcontains '--canary-executable') 'External services were replaced with owned processes.'
    & $copy @launch -NoTts | Out-Null
    $arguments = @((Get-Content -LiteralPath $receipt -Raw | ConvertFrom-Json).Arguments)
    Assert ($arguments -ccontains '--canary-executable' -and $arguments -cnotcontains '--tts-executable') 'NoTts disabled input or retained output.'
    Assert ([IO.File]::ReadAllText((Join-Path $data 'native-build-called')) -ceq 'Canary') 'NoTts built an unused synthesis engine.'
    [IO.File]::Delete((Join-Path $data 'native-build-called'))
    & $copy @launch -NoSpeechModels | Out-Null
    $arguments = @((Get-Content -LiteralPath $receipt -Raw | ConvertFrom-Json).Arguments)
    Assert (-not [IO.File]::Exists((Join-Path $data 'native-build-called')) -and
        $arguments -cnotcontains '--tts-executable' -and $arguments -cnotcontains '--canary-executable') 'Text-only launch built or configured speech.'
    & $copy @launch -NoSpeechModels -UseRepackedPtq1 -NoAbliterateLora | Out-Null
    $arguments = @((Get-Content -LiteralPath $receipt -Raw | ConvertFrom-Json).Arguments)
    Assert ($arguments -ccontains '--use-repacked-ptq1' -and $arguments -ccontains '--no-abliterate-lora') 'Explicit PTQ1 and adapter choices were not forwarded together.'
}
if ($Case -cin @('All', 'DownloadOnly')) {
    [IO.File]::Delete($receipt)
    [IO.File]::Delete((Join-Path $data 'native-build-called'))
    & $copy -DataDirectory $data -SpeechModelsOnly | Out-Null
    Assert ([IO.File]::Exists((Join-Path $data 'download-called')) -and -not [IO.File]::Exists($receipt) -and
        -not [IO.File]::Exists((Join-Path $data 'native-build-called'))) 'Download-only launched or compiled a program.'
    Assert-Failure { & $copy -DataDirectory $data -SpeechModelsOnly -NoSpeechModels } 'Contradictory switches were accepted.'
}
if ($Case -cin @('All', 'Rejections')) {
    [IO.File]::Delete($receipt)
    [IO.File]::WriteAllText((Join-Path $data 'native-build-failure'), '')
    Assert-Failure { & $copy @launch } 'Native compiler failure did not prevent launch.'
    Assert (-not [IO.File]::Exists($receipt)) 'Unavailable native engines reached the UI.'
    [IO.File]::Delete((Join-Path $data 'native-build-failure'))
    Assert-Failure { & $copy @launch -CanaryPort 8767 } 'Colliding speech ports were accepted.'
    [IO.File]::WriteAllText(($receipt + '.fail'), '')
    Assert-Failure { & $copy @launch -NoSpeechModels } 'Nonzero app exit was accepted.'
}
if ($Case -cin @('All', 'Cache')) {
    $module = Import-Module (Join-Path $repo 'scripts/Bonsai.ChatLaunch.psm1') -Force -PassThru
    & $module {
        $script:builds = [Collections.Generic.List[object]]::new()
        $script:exitCode = 0
        function script:Get-Command {
            [CmdletBinding()]
            [OutputType([pscustomobject])]
            param([string]$Name, [string]$CommandType)
            if ($Name -cne 'cargo' -or $CommandType -cne 'Application') { throw 'Unexpected compiler lookup.' }
            return [pscustomobject]@{ Source = 'cargo.exe' }
        }
        function script:Invoke-BonsaiNativeProcess {
            [CmdletBinding()]
            [OutputType([pscustomobject])]
            param([string]$Executable, [string[]]$ArgumentList, [string]$WorkingDirectory, [int]$TimeoutSeconds, [string]$LogPath)
            $target = $ArgumentList[[array]::IndexOf($ArgumentList, '--target-dir') + 1]
            $existed = [Collections.Generic.List[bool]]::new()
            for ($index = 0; $index -lt $ArgumentList.Count; $index++) {
                if ($ArgumentList[$index] -cne '--package') { continue }
                $name = $ArgumentList[$index + 1]
                $path = Join-Path -Path $target -ChildPath 'aarch64-pc-windows-msvc/release' -AdditionalChildPath ($name + '.exe')
                $existed.Add([IO.File]::Exists($path))
                if ([IO.File]::Exists($path) -or $script:exitCode) { continue }
                [void][IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($path))
                $stream = [IO.FileStream]::new($path, [IO.FileMode]::CreateNew)
                $writer = [IO.BinaryWriter]::new($stream)
                try {
                    $stream.SetLength(512)
                    $writer.Write([uint16]0x5a4d)
                    $stream.Position = 0x3c
                    $writer.Write([int]0x80)
                    $stream.Position = 0x80
                    $writer.Write([uint32]0x00004550)
                    $writer.Write([uint16]0xaa64)
                } finally { $writer.Dispose() }
            }
            $script:builds.Add([pscustomobject]@{ Executable = $Executable; TimeoutSeconds = $TimeoutSeconds; Arguments = $ArgumentList; WorkingDirectory = $WorkingDirectory; LogPath = $LogPath; Existed = $existed.ToArray() })
            return [pscustomobject]@{ ExitCode = $script:exitCode }
        }
    }
    $working = Join-Path $scratch 'cache-checks'
    $parameters = @{ DataDirectory = $data; WorkingDirectory = $working; RepositoryRoot = $repo; Package = @('bonsai-chat', 'bonsai-broker') }
    $first = Invoke-ChatBuild @parameters
    $second = Invoke-ChatBuild @parameters
    $builds = @(& $module { $script:builds.ToArray() })
    Assert ($first.TargetDirectory -ceq $second.TargetDirectory -and $first.ChatPath -ceq $second.ChatPath) 'Repeated launch discarded the reusable Cargo target.'
    Assert ($builds.Count -eq 2 -and $builds[0].Existed -notcontains $true -and $builds[1].Existed -notcontains $false) 'Unchanged binaries were rebuilt or Cargo freshness was skipped.'
    foreach ($build in $builds) {
        Assert ($build.Executable -ceq 'cargo.exe' -and $build.TimeoutSeconds -eq 3600 -and $build.Arguments -contains '--locked' -and $build.Arguments -contains '--release' -and
            $build.Arguments -contains '--manifest-path' -and $build.Arguments[0] -match '^\+nightly-\d{4}-\d{2}-\d{2}-aarch64-pc-windows-msvc$' -and
            $build.WorkingDirectory -ceq $working -and $build.LogPath.StartsWith($working + '\', [StringComparison]::OrdinalIgnoreCase)) 'Cache reuse lost Cargo freshness, the pinned compiler or external diagnostics.'
    }
    $digest = (Get-FileHash -LiteralPath $first.ChatPath -Algorithm SHA256).Hash
    [IO.File]::WriteAllText($first.ChatPath, 'changed executable')
    $null = Invoke-ChatBuild @parameters
    Assert ((Get-FileHash -LiteralPath $first.ChatPath -Algorithm SHA256).Hash -ceq $digest) 'A changed cached executable was trusted.'
    $receiptPath = Join-Path $first.TargetDirectory 'bonsai-chat.sha256'
    [IO.File]::Delete($receiptPath)
    $null = Invoke-ChatBuild @parameters
    $builds = @(& $module { $script:builds.ToArray() })
    Assert (-not $builds[2].Existed[1] -and -not $builds[3].Existed[1]) 'Changed or unreceipted outputs reached Cargo as trusted executables.'
    $lock = [IO.FileStream]::new((Join-Path $first.TargetDirectory 'build.lock'), [IO.FileMode]::Open, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
    try { Assert-Failure { Invoke-ChatBuild @parameters } 'Concurrent cache mutation was accepted.' }
    finally { $lock.Dispose() }
    & $module { $script:exitCode = 7 }
    Assert-Failure { Invoke-ChatBuild @parameters } 'A failed freshness check returned executable paths.'
    Assert-Failure { Invoke-ChatBuild -DataDirectory $repo -WorkingDirectory $working -RepositoryRoot $repo -Package bonsai-chat } 'A chat build wrote inside Git.'
    $alias = Join-Path $scratch 'cache-alias'
    $null = New-Item -ItemType Junction -Path $alias -Target $data
    Assert-Failure { Invoke-ChatBuild -DataDirectory $alias -WorkingDirectory $working -RepositoryRoot $repo -Package bonsai-chat } 'A reparse cache path was accepted.'
}
Write-Output "Native chat speech $Case checks passed. Evidence: $scratch"
