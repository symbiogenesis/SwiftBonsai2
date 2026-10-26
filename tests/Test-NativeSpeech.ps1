#requires -Version 7.6
[CmdletBinding()]
[OutputType([string])]
param([ValidateSet('All', 'Models', 'Sources', 'Build', 'Reload', 'Cache')][string]$Case = 'All', [string]$ToolPath)
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
          [switch]$VerifyOnly, [switch]$DeferCachedVerification, [string]$ToolPath)
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
        $module = Import-Module (Join-Path $repo 'scripts/Bonsai.NativeSpeech.psm1') -Force -PassThru
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
if ($Case -in @('All', 'Cache')) {
    # The preceding provisioning case installs mocks that skip actual hashes.
    $module = Import-Module (Join-Path $repo 'scripts/Bonsai.NativeSpeech.psm1') -Force -PassThru
    & $module {
        $script:downloads = [Collections.Generic.List[string]]::new()
        $script:cachePlan = $null
        function script:Invoke-BonsaiTool {
            [CmdletBinding()]
            [OutputType([hashtable])]
            param([string]$Command, [Collections.IDictionary]$Request, [string]$ToolPath)
            if ($Command -cne 'speech-models' -or -not $Request['dataDirectory'] -or $ToolPath) { throw 'Unexpected maintenance operation.' }
            return @{ Plans = @($script:cachePlan); CanaryModelPath = $script:cachePlan.Path }
        }
        function script:Receive-LockedArtifact {
            [CmdletBinding()]
            [OutputType([string])]
            param([uri]$Uri, [string]$Path, [long]$Bytes, [string]$Sha256)
            if ($Uri.Scheme -cne 'https') { throw 'Unexpected download scheme.' }
            $script:downloads.Add($Path)
            if (-not [IO.File]::Exists($Path)) { [IO.File]::WriteAllText($Path, 'abc') }
            Test-LockedArtifact -Path $Path -Bytes $Bytes -Sha256 $Sha256
            return $Path
        }
        function script:Assert-Arm64PE {
            [CmdletBinding()]
            [OutputType([void])]
            param([string]$Path)
            if (-not [IO.File]::Exists($Path)) { throw 'Missing compiler fixture.' }
        }
    }
    $cache = Join-Path $scratch 'model-cache'
    [void][IO.Directory]::CreateDirectory($cache)
    $modelPath = Join-Path $cache 'model.gguf'
    $hash = 'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad'
    $plan = @{ Path = $modelPath; Bytes = 3L; Sha256 = $hash; Uri = 'https://example.invalid/model.gguf' }
    & $module { param($p) $script:cachePlan = $p } $plan
    $null = Install-NativeSpeechModels -DataDirectory $cache -DeferCachedVerification
    Assert ((& $module { $script:downloads.Count }) -eq 1) 'Deferred setup bypassed the verified download operation for a missing artifact.'
    [IO.File]::WriteAllText($modelPath, 'bad')
    $null = Install-NativeSpeechModels -DataDirectory $cache -DeferCachedVerification
    Assert ((& $module { $script:downloads.Count }) -eq 1) 'Cached launch read or redownloaded an existing model.'
    Assert-Failure { Install-NativeSpeechModels -DataDirectory $cache } 'Ordinary provisioning stopped verifying cached model digests.'
    Assert-Failure { Install-NativeSpeechModels -DataDirectory $cache -VerifyOnly } 'VerifyOnly stopped verifying cached model digests.'
    Assert-Failure { Install-NativeSpeechModels -DataDirectory $cache -VerifyOnly -DeferCachedVerification } 'Contradictory verification modes were accepted.'
    [IO.File]::WriteAllText($modelPath, 'short')
    Assert-Failure { Install-NativeSpeechModels -DataDirectory $cache -DeferCachedVerification } 'A wrong-length cached model reached the launcher.'
    $unsafe = @('%LOCALAPPDATA%\model.gguf', 'relative.gguf', (Join-Path $repo 'model.gguf'))
    foreach ($path in $unsafe) {
        & $module { param($p) $script:cachePlan.Path = $p } $path
        Assert-Failure { Install-NativeSpeechModels -DataDirectory $cache -DeferCachedVerification } 'Deferred setup accepted unsafe model storage.'
    }
    $alias = Join-Path $scratch 'model-cache-alias'
    $null = New-Item -ItemType Junction -Path $alias -Target $cache
    & $module { param($p) $script:cachePlan.Path = $p } (Join-Path $alias 'model.gguf')
    Assert-Failure { Install-NativeSpeechModels -DataDirectory $cache -DeferCachedVerification } 'Deferred setup accepted a reparse model path.'
    $compilerDirectory = Join-Path -Path $cache -ChildPath 'tools' -AdditionalChildPath 'test-compiler'
    $compilerRoot = Join-Path $compilerDirectory 'test-root'
    $compilerBin = Join-Path $compilerRoot 'bin'
    [void][IO.Directory]::CreateDirectory($compilerBin)
    foreach ($name in @('clang-cl.exe', 'llvm-rc.exe')) {
        [IO.File]::WriteAllText((Join-Path $compilerBin $name), 'abc')
    }
    $pin = @{ directory = 'test-compiler'; root = 'test-root'; filename = 'missing.tar.xz'
        url = 'https://example.invalid/compiler.tar.xz'; bytes = 3L; sha256 = $hash
        executables = @{ 'clang-cl.exe' = @{ bytes = 3L; sha256 = $hash }; 'llvm-rc.exe' = @{ bytes = 3L; sha256 = $hash } } }
    $downloadCount = & $module { $script:downloads.Count }
    $resolved = & $module {
        param($d, $p) Install-NativeSpeechCompiler -DataDirectory $d -WorkingDirectory $d -Pin $p
    } $cache $pin
    Assert ($resolved -ceq $compilerRoot -and (& $module { $script:downloads.Count }) -eq $downloadCount -and
        -not [IO.File]::Exists((Join-Path $compilerDirectory 'missing.tar.xz'))) 'An installed compiler required or read its distribution archive.'
    [IO.File]::WriteAllText((Join-Path $compilerBin 'clang-cl.exe'), 'bad')
    Assert-Failure {
        & $module { param($d, $p) Install-NativeSpeechCompiler -DataDirectory $d -WorkingDirectory $d -Pin $p } $cache $pin
    } 'Skipping the compiler archive bypassed the installed executable digest.'
    $module = Import-Module (Join-Path $repo 'scripts/Bonsai.NativeSpeech.psm1') -Force -PassThru
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
        function script:Clear-BonsaiWorkspaceBuild {
            [CmdletBinding()]
            [OutputType([void])]
            param([string]$RepositoryRoot, [string]$CargoPath, [string]$Channel, [string]$TargetDirectory, [string]$WorkingDirectory, [string]$BuildTarget)
            $script:invalidation = $PSBoundParameters
        }
        $script:initializations = 0
        function script:Initialize-NativeSpeechBuild {
            [CmdletBinding()]
            [OutputType([pscustomobject])]
            param([string]$DataDirectory, [string]$WorkingDirectory, [string]$ToolPath)
            $script:buildToolPath = $ToolPath
            $script:initializations++
            [void](Resolve-ExternalPath -Path $DataDirectory)
            [void][IO.Directory]::CreateDirectory($WorkingDirectory)
            return [pscustomobject]@{ Environment = @{ CC = 'clang-cl.exe'; CXX = 'clang-cl.exe'; CMAKE_GENERATOR = 'Ninja'
                BONSAI_TTS_COMPILER = 'C:\pinned-llvm\bin\clang-cl.exe'; BONSAI_CMAKE_GENERATOR = 'Ninja Multi-Config'; Sentinel = 'inherited' } }
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
    $selectedTool = Join-Path $scratch 'selected-tools.exe'
    [IO.File]::WriteAllText($selectedTool, 'selected maintenance executable fixture')
    $all = Invoke-NativeSpeechBuild -DataDirectory (Join-Path $scratch 'data') -WorkingDirectory $working -RepositoryRoot $repo -ToolPath $selectedTool
    Assert ((& $module { $script:buildToolPath }) -ceq $selectedTool) 'Native builds discarded the selected maintenance executable.'
    $builds = @(& $module { $script:builds.ToArray() })
    Assert ($builds.Count -eq 2 -and $all.AsrPath.EndsWith('bonsai-speech-asr.exe') -and $all.TtsPath.EndsWith('bonsai-speech-tts.exe')) 'Native build omitted an owned speech daemon.'
    $target = $builds[0].Arguments[[array]::IndexOf($builds[0].Arguments, '--target-dir') + 1]
    $cacheRoot = Join-Path -Path $scratch -ChildPath 'data' -AdditionalChildPath 'build/native-speech'
    Assert ($target.StartsWith($cacheRoot + '\', [StringComparison]::OrdinalIgnoreCase) -and
        [IO.Path]::GetFileName($target) -cmatch '^[a-f0-9]{64}$' -and
        $all.AsrPath.StartsWith($target + '\', [StringComparison]::OrdinalIgnoreCase) -and
        $all.TtsPath.StartsWith($target + '\', [StringComparison]::OrdinalIgnoreCase)) 'Native daemon builds lost their checkout-specific persistent Cargo cache.'
    $warm = Invoke-NativeSpeechBuild -DataDirectory (Join-Path $scratch 'data') -WorkingDirectory $working -RepositoryRoot $repo -ToolPath $selectedTool
    Assert ($warm.AsrPath -ceq $all.AsrPath -and $warm.TtsPath -ceq $all.TtsPath -and
        (& $module { $script:builds.Count }) -eq 2 -and (& $module { $script:initializations }) -eq 1) 'Warm native launch invoked Cargo or provisioned compiler/source inputs.'
    Assert ($builds[0].Environment.CC -ceq 'clang-cl.exe' -and $builds[0].Environment.CMAKE_GENERATOR -ceq 'Ninja' -and
        $builds[1].Environment.CC -ceq 'clang-cl.exe' -and $builds[1].Environment.CMAKE_GENERATOR -ceq 'Ninja' -and
        $builds[1].Environment.BONSAI_TTS_COMPILER -ceq 'C:\pinned-llvm\bin\clang-cl.exe' -and
        $builds[1].Environment.BONSAI_CMAKE_GENERATOR -ceq 'Ninja Multi-Config') 'Native TTS lost the pinned SIMD-capable compiler environment.'
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
    $digest = (Get-FileHash -LiteralPath $all.AsrPath -Algorithm SHA256).Hash
    [IO.File]::WriteAllText($all.AsrPath, 'changed native executable')
    $null = Invoke-NativeSpeechBuild -DataDirectory (Join-Path $scratch 'data') -WorkingDirectory $working -RepositoryRoot $repo -ToolPath $selectedTool
    Assert ((Get-FileHash -LiteralPath $all.AsrPath -Algorithm SHA256).Hash -ceq $digest -and
        (& $module { $script:initializations }) -gt 1) 'A modified speech executable reached launch without rebuilding.'
    $guard = [IO.FileStream]::new((Join-Path $target 'build.lock'), [IO.FileMode]::Open, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
    try {
        Assert-Failure { Invoke-NativeSpeechBuild -DataDirectory (Join-Path $scratch 'data') -WorkingDirectory $working -RepositoryRoot $repo } 'Concurrent native cache mutation was accepted.'
    } finally { $guard.Dispose() }
    & $module { $script:machine = [uint16]0x8664 }
    Assert-Failure { Invoke-NativeSpeechBuild -DataDirectory (Join-Path $scratch 'data') -WorkingDirectory $working -RepositoryRoot $repo -Model QwenTts -ForceBuild } 'An emulated x64 executable passed native ARM64 acceptance.'
    Assert-Failure { Invoke-NativeSpeechBuild -DataDirectory (Join-Path $scratch 'data') -WorkingDirectory $repo -RepositoryRoot $repo } 'A native build wrote into the Git checkout.'
}
"Native speech $Case contracts passed. Evidence: $scratch"
