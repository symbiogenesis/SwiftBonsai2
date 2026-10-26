#requires -Version 7.6
[CmdletBinding()]
[OutputType([string])]
param([ValidateSet('All', 'Models', 'Sources', 'Build', 'Reload')][string]$Case = 'All', [string]$ToolPath)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
if (-not $IsWindows) { throw 'Native speech provisioning contracts target Windows.' }
if ($Case -eq 'All' -and $env:GITHUB_ACTIONS -ne 'true') {
    throw 'The full native speech suite runs only in CI. Select one focused -Case locally.'
}
$repo = Split-Path $PSScriptRoot -Parent
Import-Module (Join-Path $PSScriptRoot 'Bonsai.TestSupport.psm1')
$module = Import-Module (Join-Path $repo 'scripts/Bonsai.NativeSpeech.psm1') -Force -PassThru
$scratch = Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-NativeSpeechTests-' + [guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($scratch)

if ($Case -in @('All', 'Reload')) {
    $fixture = Join-Path $scratch 'module-reload'
    [void][IO.Directory]::CreateDirectory($fixture)
    $fixtureModulePath = Join-Path $fixture 'Bonsai.NativeSpeech.psm1'
    $oldModule = @'
function Install-NativeSpeechModels {
    [CmdletBinding()]
    [OutputType([pscustomobject])]
    param([string]$DataDirectory, [string]$Model, [switch]$VerifyOnly, [string]$ToolPath)
    throw 'The obsolete cached speech module was invoked.'
}
Export-ModuleMember -Function Install-NativeSpeechModels
'@
    $newModule = @'
function Install-NativeSpeechModels {
    [CmdletBinding()]
    [OutputType([pscustomobject])]
    param([string]$DataDirectory, [string]$Model, [string]$TtsQuantization,
          [switch]$VerifyOnly, [string]$ToolPath)
    [pscustomobject]@{ Quantization = $TtsQuantization; Model = $Model; Verified = [bool]$VerifyOnly }
}
Export-ModuleMember -Function Install-NativeSpeechModels
'@
    [IO.File]::WriteAllText($fixtureModulePath, $oldModule)
    Import-Module $fixtureModulePath -Force
    try {
        [IO.File]::WriteAllText($fixtureModulePath, $newModule)
        Assert (-not (Get-Command Install-NativeSpeechModels).Parameters.ContainsKey('TtsQuantization')) 'Fixture did not retain the obsolete module.'
        $wrapper = Join-Path $fixture 'Get-NativeSpeechModels.ps1'
        Copy-Item -LiteralPath (Join-Path $repo 'scripts/Get-NativeSpeechModels.ps1') -Destination $wrapper
        $result = & $wrapper -DataDirectory $scratch -Model QwenTts -TtsQuantization Q4_K_M -VerifyOnly
        Assert ($result.Quantization -ceq 'Q4_K_M' -and $result.Model -ceq 'QwenTts' -and $result.Verified) 'Model provisioning did not reload the changed parameter contract.'
    } finally {
        Get-Module | Where-Object Path -EQ $fixtureModulePath | Remove-Module -Force
        Import-Module (Join-Path $repo 'scripts/Bonsai.NativeSpeech.psm1') -Force
    }
}

if ($Case -in @('All', 'Models')) {
    & $module {
        $script:received = [Collections.Generic.List[object]]::new()
        $script:verified = [Collections.Generic.List[object]]::new()
        function script:Receive-LockedArtifact {
            [CmdletBinding()]
            [OutputType([string])]
            param([uri]$Uri, [string]$Path, [long]$Bytes, [string]$Sha256)
            $script:received.Add([pscustomobject]@{ Uri = $Uri; Path = $Path; Bytes = $Bytes; Sha256 = $Sha256 })
            return $Path
        }
        function script:Test-LockedArtifact {
            [CmdletBinding()]
            [OutputType([void])]
            param([string]$Path, [long]$Bytes, [string]$Sha256)
            $script:verified.Add([pscustomobject]@{ Path = $Path; Bytes = $Bytes; Sha256 = $Sha256 })
        }
    }
    $data = Join-Path $scratch 'uncreated data [literal]'
    $all = Install-NativeSpeechModels -DataDirectory $data -ToolPath $ToolPath
    $actual = @(& $module { $script:received.ToArray() })
    Assert ($actual.Count -eq 3) 'Native provisioning omitted or added model artifacts.'
    $asr = Get-Content -LiteralPath (Join-Path $repo 'src/speech-asr/model.lock.json') -Raw | ConvertFrom-Json
    $tts = Get-Content -LiteralPath (Join-Path $repo 'src/speech-tts/model.lock.json') -Raw | ConvertFrom-Json
    $pins = @($asr, $tts.files.talker, $tts.files.codec)
    for ($index = 0; $index -lt $pins.Count; $index++) {
        $pin = $pins[$index]
        $file = $actual[$index]
        Assert ($file.Uri.AbsoluteUri -ceq $pin.url -and $file.Bytes -eq $pin.bytes -and
            $file.Sha256 -ceq $pin.sha256 -and [IO.Path]::GetFileName($file.Path) -ceq $pin.filename) 'Provisioning lost a per-crate artifact lock.'
    }
    Assert ($all.CanaryModelPath -ceq $actual[0].Path -and $all.TtsModelPath -ceq $actual[1].Path -and
        $all.TtsCodecPath -ceq $actual[2].Path -and $all.TotalBytes -eq ($pins.bytes | Measure-Object -Sum).Sum) 'Native model results lost their paths or byte total.'
    $canary = Install-NativeSpeechModels -DataDirectory $data -Model Canary -VerifyOnly -ToolPath $ToolPath
    $qwen = Install-NativeSpeechModels -DataDirectory $data -Model QwenTts -VerifyOnly -ToolPath $ToolPath
    $counts = & $module { @($script:received.Count, $script:verified.Count) }
    Assert ($counts[0] -eq 3 -and $counts[1] -eq 3 -and $null -eq $canary.TtsModelPath -and
        $null -eq $canary.TtsCodecPath -and $null -eq $qwen.CanaryModelPath) 'Model selection or verify-only downloaded an unrelated model.'
    $q4 = Install-NativeSpeechModels -DataDirectory $data -Model QwenTts -TtsQuantization Q4_K_M -VerifyOnly -ToolPath $ToolPath
    $q4Pin = Get-Content -LiteralPath (Join-Path $repo 'src/speech-tts/model-q4.lock.json') -Raw | ConvertFrom-Json
    $q4Files = @(& $module { $script:verified.ToArray() }) | Select-Object -Last 2
    Assert ($q4Files[0].Sha256 -ceq $q4Pin.files.talker.sha256 -and $q4Files[0].Bytes -eq $q4Pin.files.talker.bytes -and
        $q4.TtsModelPath -ceq $q4Files[0].Path -and $q4.TtsModelPath -cne $qwen.TtsModelPath -and
        $q4.TtsCodecPath -ceq $qwen.TtsCodecPath) 'Opt-in Q4 lost its separate talker identity or shared pinned Q8 codec.'
    foreach ($invalid in @('relative', '%LOCALAPPDATA%\native', $repo, (Join-Path $scratch 'speech:stream'))) {
        Assert-Failure { Install-NativeSpeechModels -DataDirectory $invalid -ToolPath $ToolPath } 'Unsafe native model storage was accepted.'
    }
    Assert (-not (Test-Path -LiteralPath $data)) 'Mocked native model provisioning wrote storage.'
}
if ($Case -in @('All', 'Sources')) {
    # Rust owns hostile ZIP, cache, path-alias and reparse regressions. This
    # exercises the production combined operation through the packaged bridge.
    $request = @{ operation = 'stage-sources'; sourceDirectory = Join-Path $scratch 'sources'
        qwenArchivePath = Join-Path $scratch 'qwen.zip'; ggmlArchivePath = Join-Path $scratch 'ggml.zip' }
    $failure = $null
    try { & $module { param($r, $t) Invoke-BonsaiTool -Command speech-build -Request $r -ToolPath $t } $request $ToolPath }
    catch { $failure = $_.Exception.Message }
    Assert ($failure -and $failure.Contains('Native source file cannot be opened safely.')) 'The combined source staging bridge did not reach the pinned archive boundary.'
    Assert (-not (Test-Path -LiteralPath $request.sourceDirectory)) 'Missing source archives wrote cache storage.'
    $request.sourceDirectory = $repo
    Assert-Failure { & $module { param($r, $t) Invoke-BonsaiTool -Command speech-build -Request $r -ToolPath $t } $request $ToolPath } 'Source staging accepted a Git checkout.'
}
if ($Case -in @('All', 'Build')) {
    & $module {
        $script:builds = [Collections.Generic.List[object]]::new()
        $script:machine = [uint16]0xaa64
        function script:Initialize-NativeSpeechBuild {
            [CmdletBinding()]
            [OutputType([pscustomobject])]
            param([string]$DataDirectory, [string]$WorkingDirectory)
            [void](Resolve-ExternalPath -Path $DataDirectory)
            [void][IO.Directory]::CreateDirectory($WorkingDirectory)
            return [pscustomobject]@{ Environment = @{ CC = 'clang-cl.exe'; CXX = 'clang-cl.exe'; CMAKE_GENERATOR = 'Ninja'; Sentinel = 'inherited' } }
        }
        function script:Invoke-BonsaiNativeProcess {
            [CmdletBinding()]
            [OutputType([pscustomobject])]
            param([string]$Executable, [string[]]$ArgumentList, [string]$WorkingDirectory,
                  [Collections.IDictionary]$Environment, [int]$TimeoutSeconds, [string]$LogPath)
            $script:builds.Add([pscustomobject]@{ Executable = $Executable; Arguments = $ArgumentList; WorkingDirectory = $WorkingDirectory;
                Environment = $Environment; TimeoutSeconds = $TimeoutSeconds; LogPath = $LogPath })
            $package = $ArgumentList[[array]::IndexOf($ArgumentList, '--package') + 1]
            $target = $ArgumentList[[array]::IndexOf($ArgumentList, '--target-dir') + 1]
            $path = Join-Path -Path $target -ChildPath 'aarch64-pc-windows-msvc/release' -AdditionalChildPath ($package + '.exe')
            [void][IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($path))
            $stream = [IO.FileStream]::new($path, [IO.FileMode]::Create)
            $writer = [IO.BinaryWriter]::new($stream)
            try {
                $stream.SetLength(512)
                $writer.Write([uint16]0x5a4d)
                $stream.Position = 0x3c
                $writer.Write([int]0x80)
                $stream.Position = 0x80
                $writer.Write([uint32]0x00004550)
                $writer.Write($script:machine)
            } finally { $writer.Dispose() }
            return [pscustomobject]@{ ExitCode = 0; StandardOutput = ''; StandardError = '' }
        }
    }
    $working = Join-Path $scratch 'build'
    $all = Invoke-NativeSpeechBuild -DataDirectory (Join-Path $scratch 'data') -WorkingDirectory $working -RepositoryRoot $repo
    $builds = @(& $module { $script:builds.ToArray() })
    Assert ($builds.Count -eq 2 -and $all.AsrPath.EndsWith('bonsai-speech-asr.exe') -and $all.TtsPath.EndsWith('bonsai-speech-tts.exe')) 'Native build omitted an owned speech daemon.'
    $target = Join-Path -Path $scratch -ChildPath 'data' -AdditionalChildPath 'build/native-speech'
    Assert ($all.AsrPath.StartsWith($target + '\', [StringComparison]::OrdinalIgnoreCase) -and
        $all.TtsPath.StartsWith($target + '\', [StringComparison]::OrdinalIgnoreCase)) 'Native daemon builds discarded the persistent external Cargo cache.'
    Assert ($builds[0].Environment.CC -ceq 'clang-cl.exe' -and $builds[0].Environment.CMAKE_GENERATOR -ceq 'Ninja' -and
        $builds[1].Environment.CC -ceq 'cl.exe' -and $builds[1].Environment.CMAKE_GENERATOR -ceq 'Visual Studio 18 2026') 'Native engines used incompatible compiler environments.'
    foreach ($build in $builds) {
        Assert ($build.Arguments -contains '--locked' -and $build.Arguments -contains '--release' -and
            $build.Arguments[0] -match '^\+nightly-\d{4}-\d{2}-\d{2}-aarch64-pc-windows-msvc$' -and
            $build.WorkingDirectory -ceq $working -and $build.Environment.Sentinel -ceq 'inherited') 'Native build lost its pinned compiler, external working directory or environment overlay.'
        Assert ($build.Arguments[[array]::IndexOf($build.Arguments, '--target-dir') + 1] -ceq $target) 'Native build placed compiler artifacts in a disposable launch directory.'
        foreach ($name in @('SystemDrive', 'SystemRoot', 'windir', 'ProgramData', 'LOCALAPPDATA', 'APPDATA')) {
            Assert (-not $build.Environment.Contains($name)) 'Native build redirected a system or profile environment variable.'
        }
    }
    $only = Invoke-NativeSpeechBuild -DataDirectory (Join-Path $scratch 'data') -WorkingDirectory $working -RepositoryRoot $repo -Model Canary
    Assert ($null -eq $only.TtsPath -and $null -ne $only.AsrPath) 'Canary selection forced a TTS build.'
    & $module { $script:machine = [uint16]0x8664 }
    Assert-Failure { Invoke-NativeSpeechBuild -DataDirectory (Join-Path $scratch 'data') -WorkingDirectory $working -RepositoryRoot $repo -Model QwenTts } 'An emulated x64 executable passed native ARM64 acceptance.'
    Assert-Failure { Invoke-NativeSpeechBuild -DataDirectory (Join-Path $scratch 'data') -WorkingDirectory $repo -RepositoryRoot $repo } 'A native build wrote into the Git checkout.'
}
"Native speech $Case contracts passed. Evidence: $scratch"
