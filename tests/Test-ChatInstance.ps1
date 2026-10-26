#requires -Version 7.6
<#
.SYNOPSIS
Checks launcher singleton admission and cleanup with synthetic child processes.
.DESCRIPTION
Uses an isolated guard path and mocked provisioning/build/native operations.
No model, compiler, UI window, full test suite or benchmark is executed.
#>
[CmdletBinding()]
[OutputType([string])]
param()
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
if (-not $IsWindows) { throw 'Singleton launch contracts target Windows.' }
$repo = Split-Path $PSScriptRoot -Parent
Import-Module (Join-Path $PSScriptRoot 'Bonsai.TestSupport.psm1')
$scratch = Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-ChatInstance-' + [guid]::NewGuid().ToString('N'))
$scripts = Join-Path $scratch 'scripts'
$data = Join-Path $scratch 'data'
[void][IO.Directory]::CreateDirectory($scripts)
[void][IO.Directory]::CreateDirectory($data)
$receipt = Join-Path $scratch 'launch.json'
$copy = Join-Path $scripts 'Start-Chat.ps1'
$source = [IO.File]::ReadAllText((Join-Path $repo 'scripts/Start-Chat.ps1'))
# Keep every copied launcher out of the actual user's singleton scope without
# changing inherited Windows profile or system environment variables.
$instanceAssignment = '$instanceDirectory = Resolve-ExternalPath -Path (Join-Path $env:LOCALAPPDATA ''SwiftBonsai2'')'
Assert ($source.Contains($instanceAssignment)) 'The launcher singleton fixture needs its isolated lock path.'
$instanceStorage = Join-Path $scratch 'single-instance'
$source = $source.Replace($instanceAssignment, ('$instanceDirectory = Resolve-ExternalPath -Path ''' + $instanceStorage.Replace("'", "''") + "'"))
$tokens = $null
$errors = $null
$ast = [Management.Automation.Language.Parser]::ParseInput($source, [ref]$tokens, [ref]$errors)
Assert ($errors.Count -eq 0) 'Launcher parse failure.'
$cargo = (Join-Path $scratch 'cargo.exe').Replace("'", "''")
$source = $source.Insert($ast.ParamBlock.Extent.EndOffset, "`nfunction Get-Command { [pscustomobject]@{ Source = '$cargo' } }`n")
[IO.File]::WriteAllText($copy, $source)
Copy-Item -LiteralPath (Join-Path $repo 'scripts/Bonsai.Common.psm1') -Destination $scripts
$toolResolver = @'
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Native.psm1')
function Resolve-BonsaiRustTool {
    [CmdletBinding()]
    [OutputType([string])]
    param([string]$Name, [string]$ManifestRelativePath, [string]$ToolPath, [switch]$ForceBuild)
    [IO.File]::AppendAllText('__RESOLVED__', "resolved`n")
    [IO.File]::WriteAllText('__RESOLVED__.force', [string][bool]$ForceBuild)
    if ($ToolPath) { return $ToolPath }
    return '__TOOL__'
}
function Invoke-BonsaiTool {
    [CmdletBinding()]
    [OutputType([Collections.IDictionary])]
    param([string]$Command, [Collections.IDictionary]$Request, [string]$ToolPath, [int]$TimeoutSeconds)
    if ($Command -cne 'chat-launch') { throw 'Unexpected maintenance command.' }
    if ($Request.Contains('SpeechModelsOnly') -and $Request.SpeechModelsOnly) {
        [IO.File]::WriteAllText((Join-Path $Request.DataDirectory 'download-called'), '')
        return @{}
    }
    if ([IO.File]::Exists((Join-Path $Request.DataDirectory 'native-build-failure'))) { throw 'Deliberate native build failure.' }
    [IO.File]::WriteAllText((Join-Path $Request.DataDirectory 'download-called'), '')
    $result = Invoke-BonsaiNativeProcess -Executable $Request.ChatPath -ArgumentList @() -WorkingDirectory $Request.WorkingDirectory -LogPath (Join-Path $Request.WorkingDirectory 'chat.log')
    if ($result.ExitCode) { throw 'Deliberate native app failure.' }
    return @{}
}
Export-ModuleMember -Function Resolve-BonsaiRustTool, Invoke-BonsaiTool
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
[pscustomobject]@{ CanaryModelPath = 'canary.gguf'; TtsModelPath = 'talker.gguf'; TtsCodecPath = 'codec.gguf'; TtsVoiceModelPath = 'voice-talker.gguf'; TotalBytes = 1 }
'@
[IO.File]::WriteAllText((Join-Path $scripts 'Get-NativeSpeechModels.ps1'), $download)
$build = @'
function Invoke-NativeSpeechBuild {
    [CmdletBinding()]
    [OutputType([pscustomobject])]
    param([string]$DataDirectory, [string]$WorkingDirectory, [string]$RepositoryRoot, [string]$Model, [string]$ToolPath, [switch]$ForceBuild)
    [IO.File]::WriteAllText((Join-Path $DataDirectory 'native-build-called'), $Model)
    [IO.File]::WriteAllText((Join-Path $DataDirectory 'build-tool'), $ToolPath)
    [IO.File]::WriteAllText((Join-Path $DataDirectory 'build-force'), [string][bool]$ForceBuild)
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

& {
    # Real launcher processes race while fixtures hold preparation before a
    # window exists, then hold a simulated UI without loading models or a GPU.
    $control = Join-Path $scratch 'singleton-control'
    [void][IO.Directory]::CreateDirectory($control)
    [IO.File]::Delete($receipt + '.fail')
    $barrier = @'
    if ($env:BONSAI_TEST_CHAT_PHASES -and '__PHASE__' -in $env:BONSAI_TEST_CHAT_PHASES.Split(',')) {
        $control = '__CONTROL__'
        [IO.File]::WriteAllText((Join-Path $control ('__PHASE__-' + $PID)), '')
        $timeout = [Diagnostics.Stopwatch]::StartNew()
        while (-not [IO.File]::Exists((Join-Path $control 'release-__PHASE__'))) {
            if ($timeout.Elapsed.TotalSeconds -ge 15) { throw 'Singleton fixture barrier timed out.' }
            Start-Sleep -Milliseconds 20
        }
    }
'@
    $barrier = $barrier.Replace('__CONTROL__', $control.Replace("'", "''"))
    $resolver = $toolResolver.Replace('    [IO.File]::AppendAllText', ($barrier.Replace('__PHASE__', 'startup') + [Environment]::NewLine + '    [IO.File]::AppendAllText'))
    $resolver = $resolver.Replace('__RESOLVED__', (Join-Path $data 'tool-resolved').Replace("'", "''")).Replace('__TOOL__', (Join-Path $data 'bonsai-tools.exe').Replace("'", "''"))
    [IO.File]::WriteAllText((Join-Path $scripts 'Bonsai.Tools.psm1'), $resolver)
    $uiEntry = '    [IO.File]::WriteAllText((Join-Path ''' + $control.Replace("'", "''") + ''' (''launch-'' + $PID)), '''')'
    $ui = $capture.Replace('    $record =', ($uiEntry + [Environment]::NewLine + '    $record ='))
    $ui = $ui.Replace('    [IO.File]::WriteAllText($receipt,', ($barrier.Replace('__PHASE__', 'ui') + [Environment]::NewLine + '    [IO.File]::WriteAllText($receipt,'))
    $ui = $ui.Replace('__RECEIPT__', $receipt.Replace("'", "''"))
    [IO.File]::WriteAllText((Join-Path $scripts 'Bonsai.Native.psm1'), $ui)
    [IO.File]::Delete($receipt)
    [IO.File]::Delete((Join-Path $data 'tool-resolved'))
    $children = [Collections.Generic.List[object]]::new()

    function Start-ChatFixture {
        [Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseShouldProcessForStateChangingFunctions', '', Justification = 'Starts only the owned synthetic launcher fixture and always cleans it up.')]
        [CmdletBinding()]
        [OutputType([pscustomobject])]
        param([string]$Phases = '', [string]$FixtureData = $data,
              [AllowEmptyCollection()][string[]]$Arguments = @())
        $start = [Diagnostics.ProcessStartInfo]::new([Environment]::ProcessPath)
        $start.WorkingDirectory = $scratch
        $start.UseShellExecute = $false
        $start.CreateNoWindow = $true
        $start.RedirectStandardOutput = $true
        $start.RedirectStandardError = $true
        foreach ($argument in (@('-NoLogo', '-NoProfile', '-NonInteractive', '-File', $copy,
                '-DataDirectory', $FixtureData, '-ChatPath', $chat, '-BrokerPath', $broker) + $Arguments)) {
            $start.ArgumentList.Add($argument)
        }
        $start.Environment['BONSAI_TEST_CHAT_PHASES'] = $Phases
        $process = [Diagnostics.Process]::Start($start)
        $child = [pscustomobject]@{ Process = $process; Output = $process.StandardOutput.ReadToEndAsync(); Error = $process.StandardError.ReadToEndAsync() }
        $children.Add($child)
        return $child
    }
    function Complete-ChatFixture {
        [CmdletBinding()]
        [OutputType([pscustomobject])]
        param([Parameter(Mandatory)][pscustomobject]$Child)
        Assert ($Child.Process.WaitForExit(15000)) 'Singleton launcher did not exit promptly.'
        Assert ($Child.Output.Wait(5000) -and $Child.Error.Wait(5000)) 'Singleton launcher did not close its output streams.'
        $output = $Child.Output.GetAwaiter().GetResult()
        $errorOutput = $Child.Error.GetAwaiter().GetResult()
        [IO.File]::WriteAllText((Join-Path $control ($Child.Process.Id.ToString() + '.log')), $output + $errorOutput)
        return [pscustomobject]@{ ExitCode = $Child.Process.ExitCode; Output = $output; Error = $errorOutput;
            Launched = [IO.File]::Exists((Join-Path $control ('launch-' + $Child.Process.Id))) }
    }
    function Wait-ChatFixture {
        [CmdletBinding()]
        [OutputType([void])]
        param([Parameter(Mandatory)][scriptblock]$Condition, [Parameter(Mandatory)][string]$Message)
        $timeout = [Diagnostics.Stopwatch]::StartNew()
        while (-not (& $Condition)) {
            Assert ($timeout.Elapsed.TotalSeconds -lt 15) $Message
            Start-Sleep -Milliseconds 20
        }
    }
    try {
        $first = Start-ChatFixture -Phases 'startup,ui'
        $second = Start-ChatFixture -Phases 'startup,ui'
        Wait-ChatFixture { @(Get-ChildItem -LiteralPath $control -Filter 'startup-*').Count -ge 1 } 'Neither competing launcher reached startup.'
        $owner = if ([IO.File]::Exists((Join-Path $control ('startup-' + $first.Process.Id)))) { $first } else { $second }
        $duplicate = if ($owner -eq $first) { $second } else { $first }
        $result = Complete-ChatFixture $duplicate
        Assert ($result.ExitCode -eq 0 -and -not $result.Launched) ('A simultaneous duplicate did not return successfully: ' + $result.Error)
        Assert (@(Get-ChildItem -LiteralPath $control -Filter 'startup-*').Count -eq 1 -and
            -not [IO.File]::Exists($receipt) -and -not $owner.Process.HasExited) 'A duplicate performed startup work before the first UI loaded.'
        [IO.File]::WriteAllText((Join-Path $control 'release-startup'), '')
        Wait-ChatFixture { [IO.File]::Exists((Join-Path $control ('ui-' + $owner.Process.Id))) } 'The accepted launcher did not reach its UI.'

        $alternateData = Join-Path $scratch 'alternate-data'
        [void][IO.Directory]::CreateDirectory($alternateData)
        $result = Complete-ChatFixture (Start-ChatFixture -FixtureData $alternateData -Arguments @('-FrontendPort', '6173', '-BackendPort', '6181', '-NoSpeechModels'))
        Assert ($result.ExitCode -eq 0 -and -not $result.Launched -and -not [IO.File]::Exists($receipt)) 'Different data or ports bypassed the running UI singleton.'
        Assert (@([IO.File]::ReadAllLines((Join-Path $data 'tool-resolved'))).Count -eq 1) 'A rejected duplicate repeated maintenance preparation.'
        [IO.File]::Delete((Join-Path $data 'download-called'))
        $result = Complete-ChatFixture (Start-ChatFixture -Arguments @('-SpeechModelsOnly'))
        Assert ($result.ExitCode -eq 0 -and -not $result.Launched -and [IO.File]::Exists((Join-Path $data 'download-called'))) 'The singleton blocked download-only maintenance.'

        [IO.File]::WriteAllText((Join-Path $control 'release-ui'), '')
        $result = Complete-ChatFixture $owner
        Assert ($result.ExitCode -eq 0 -and $result.Launched -and [IO.File]::Exists($receipt)) 'The accepted UI did not complete successfully.'
        $result = Complete-ChatFixture (Start-ChatFixture -Arguments @('-NoSpeechModels'))
        Assert ($result.ExitCode -eq 0 -and $result.Launched) 'Normal UI exit did not permit a later launch.'
        Assert ([IO.File]::Exists((Join-Path $instanceStorage 'chat.lock'))) 'Singleton cleanup deleted the reusable lock file.'

        [IO.File]::WriteAllText($receipt + '.fail', '')
        $result = Complete-ChatFixture (Start-ChatFixture -Arguments @('-NoSpeechModels'))
        Assert ($result.ExitCode -ne 0 -and $result.Launched) 'A failed native app was reported as successful.'
        [IO.File]::Delete($receipt + '.fail')
        $result = Complete-ChatFixture (Start-ChatFixture -Arguments @('-NoSpeechModels'))
        Assert ($result.ExitCode -eq 0 -and $result.Launched) 'Failed UI exit left the singleton locked.'

        [IO.File]::WriteAllText((Join-Path $data 'native-build-failure'), '')
        $result = Complete-ChatFixture (Start-ChatFixture)
        Assert ($result.ExitCode -ne 0 -and -not $result.Launched) 'A preparation failure reached the native app.'
        [IO.File]::Delete((Join-Path $data 'native-build-failure'))
        $result = Complete-ChatFixture (Start-ChatFixture -Arguments @('-NoSpeechModels'))
        Assert ($result.ExitCode -eq 0 -and $result.Launched) 'Preparation failure left the singleton locked.'

        [IO.File]::Delete((Join-Path $control 'release-ui'))
        $crash = Start-ChatFixture -Phases 'ui' -Arguments @('-NoSpeechModels')
        Wait-ChatFixture { [IO.File]::Exists((Join-Path $control ('ui-' + $crash.Process.Id))) } 'The crash fixture did not acquire the singleton.'
        $crash.Process.Kill($true)
        $null = Complete-ChatFixture $crash
        $result = Complete-ChatFixture (Start-ChatFixture -Arguments @('-NoSpeechModels'))
        Assert ($result.ExitCode -eq 0 -and $result.Launched) 'A terminated launcher left a stale singleton lock.'

        # A storage error must fail rather than masquerade as a duplicate.
        $lockPath = Join-Path $instanceStorage 'chat.lock'
        [IO.File]::Delete($lockPath)
        [void][IO.Directory]::CreateDirectory($lockPath)
        try {
            $result = Complete-ChatFixture (Start-ChatFixture -Arguments @('-NoSpeechModels'))
            Assert ($result.ExitCode -ne 0 -and -not $result.Launched) 'An unusable lock path was treated as an existing instance.'
        } finally { [IO.Directory]::Delete($lockPath) }
    } finally {
        foreach ($child in $children) {
            try {
                if (-not $child.Process.HasExited) { $child.Process.Kill($true); $null = $child.Process.WaitForExit(5000) }
            } finally { $child.Process.Dispose() }
        }
    }
}
Write-Output "Native chat single-instance checks passed. Evidence: $scratch"
