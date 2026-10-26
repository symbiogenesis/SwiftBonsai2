#requires -Version 7.6
[CmdletBinding()]
[OutputType([string])]
param([ValidateSet('All', 'Models', 'Sources', 'Build')][string]$Case = 'All')
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
    $all = Install-NativeSpeechModels -DataDirectory $data
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
    $canary = Install-NativeSpeechModels -DataDirectory $data -Model Canary -VerifyOnly
    $qwen = Install-NativeSpeechModels -DataDirectory $data -Model QwenTts -VerifyOnly
    $counts = & $module { @($script:received.Count, $script:verified.Count) }
    Assert ($counts[0] -eq 3 -and $counts[1] -eq 3 -and $null -eq $canary.TtsModelPath -and
        $null -eq $canary.TtsCodecPath -and $null -eq $qwen.CanaryModelPath) 'Model selection or verify-only downloaded an unrelated model.'
    foreach ($invalid in @('relative', '%LOCALAPPDATA%\native', $repo, (Join-Path $scratch 'speech:stream'))) {
        Assert-Failure { Install-NativeSpeechModels -DataDirectory $invalid } 'Unsafe native model storage was accepted.'
    }
    Assert (-not (Test-Path -LiteralPath $data)) 'Mocked native model provisioning wrote storage.'
}
if ($Case -in @('All', 'Sources')) {
    function New-TestSourceZip {
        [Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseShouldProcessForStateChangingFunctions', '', Justification = 'Creates a disposable ZIP fixture beneath the unique test scratch directory.')]
        [CmdletBinding()]
        [OutputType([string])]
        param([Parameter(Mandatory)][string[]]$Names, [switch]$Link)
        $path = Join-Path $scratch ([guid]::NewGuid().ToString('N') + '.zip')
        $zip = [IO.Compression.ZipFile]::Open($path, [IO.Compression.ZipArchiveMode]::Create)
        try {
            foreach ($name in $Names) {
                $entry = $zip.CreateEntry($name)
                if ($Link) { $entry.ExternalAttributes = -1610612736 }
                $writer = [IO.StreamWriter]::new($entry.Open())
                try { $writer.Write('fixture') } finally { $writer.Dispose() }
            }
        } finally { $zip.Dispose() }
        return $path
    }
    $safe = New-TestSourceZip -Names @('source/src/engine.cpp', 'source/CMakeLists.txt')
    $destination = Join-Path $scratch 'source'
    $files = @(& $module { param($a, $d) Expand-NativeSpeechZip -Archive $a -Destination $d -ArchiveRoot source } $safe $destination)
    Assert ($files.Count -eq 2 -and (Get-Content -LiteralPath $files[0] -Raw) -ceq 'fixture') 'Verified ZIP extraction lost source bytes.'
    & $module { param($a, $d) Expand-NativeSpeechZip -Archive $a -Destination $d -ArchiveRoot source } $safe $destination | Out-Null
    [IO.File]::WriteAllText($files[0], 'changed')
    Assert-Failure { & $module { param($a, $d) Expand-NativeSpeechZip -Archive $a -Destination $d -ArchiveRoot source } $safe $destination } 'A changed source cache was accepted.'
    $index = 0
    foreach ($names in @(
        @('source/../outside.cpp'), @('source/sub/../../outside.cpp'), @('/absolute.cpp'),
        @('source/file:stream.cpp'), @('source/CON.cpp'), @('source/file. '),
        @('source/src/engine.cpp', 'source/SRC/engine.cpp')
    )) {
        $archive = New-TestSourceZip -Names $names
        $invalid = Join-Path $scratch ('rejected-' + $index++)
        Assert-Failure { & $module { param($a, $d) Expand-NativeSpeechZip -Archive $a -Destination $d -ArchiveRoot source } $archive $invalid } 'An unsafe or duplicate archive path was accepted.'
        Assert (-not (Test-Path -LiteralPath $invalid)) 'Unsafe archive validation wrote partial source bytes.'
    }
    $link = New-TestSourceZip -Names @('source/doc/link.md') -Link
    $linkFiles = @(& $module { param($a, $d) Expand-NativeSpeechZip -Archive $a -Destination $d -ArchiveRoot source } $link (Join-Path $scratch 'link'))
    Assert ($linkFiles.Count -eq 1 -and -not ((Get-Item -LiteralPath $linkFiles[0]).Attributes -band [IO.FileAttributes]::ReparsePoint) -and
        (Get-Content -LiteralPath $linkFiles[0] -Raw) -ceq 'fixture') 'A ZIP link was followed instead of materialized as ordinary bytes.'
    & $module { Read-NativeSpeechBuildLock } | Out-Null
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
    Assert ($builds[0].Environment.CC -ceq 'clang-cl.exe' -and $builds[0].Environment.CMAKE_GENERATOR -ceq 'Ninja' -and
        $builds[1].Environment.CC -ceq 'cl.exe' -and $builds[1].Environment.CMAKE_GENERATOR -ceq 'Visual Studio 18 2026') 'Native engines used incompatible compiler environments.'
    foreach ($build in $builds) {
        Assert ($build.Arguments -contains '--locked' -and $build.Arguments -contains '--release' -and
            $build.Arguments[0] -match '^\+nightly-\d{4}-\d{2}-\d{2}-aarch64-pc-windows-msvc$' -and
            $build.WorkingDirectory -ceq $working -and $build.Environment.Sentinel -ceq 'inherited') 'Native build lost its pinned compiler, external working directory or environment overlay.'
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
