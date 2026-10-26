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

function Read-NativeSpeechBuildLock {
    [CmdletBinding()]
    [OutputType([Collections.IDictionary])]
    param()
    $pin = Get-Content -LiteralPath (Join-Path $PSScriptRoot '../config/native-speech.lock.json') -Raw |
        ConvertFrom-Json -AsHashtable -DateKind String -NoEnumerate
    if ($pin.schemaVersion -ne 1 -or $pin.target -cne 'aarch64-pc-windows-msvc' -or
        $pin.compiler.version -cnotmatch '^\d+\.\d+\.\d+$' -or
        $pin.compiler.directory -cne "llvm-$($pin.compiler.version)-arm64" -or
        $pin.compiler.root -cne "clang+llvm-$($pin.compiler.version)-aarch64-pc-windows-msvc" -or
        $pin.compiler.filename -cne "$($pin.compiler.root).tar.xz") { throw 'Invalid native speech compiler lock.' }
    $compilerUrl = "https://github.com/llvm/llvm-project/releases/download/llvmorg-$($pin.compiler.version)/clang%2Bllvm-$($pin.compiler.version)-aarch64-pc-windows-msvc.tar.xz"
    Assert-NativeSpeechArtifact -Artifact $pin.compiler -ExpectedUrl $compilerUrl
    if ($pin.compiler.executables.Count -ne 2) { throw 'Native compiler executable identities are required.' }
    foreach ($name in @('clang-cl.exe', 'llvm-rc.exe')) {
        $file = $pin.compiler.executables[$name]
        if (($file.bytes -isnot [long] -and $file.bytes -isnot [int]) -or $file.bytes -le 0 -or
            $file.sha256 -cnotmatch '^[a-f0-9]{64}$') { throw 'Invalid native compiler executable hash.' }
    }
    $model = Get-Content -LiteralPath (Join-Path $PSScriptRoot '../src/speech-tts/model.lock.json') -Raw |
        ConvertFrom-Json -AsHashtable -DateKind String -NoEnumerate
    if ($pin.sources.Count -ne 2 -or $model.nativeRuntime.repository -cne "https://github.com/$($pin.sources.qwen.repository)" -or
        $model.nativeRuntime.revision -cne $pin.sources.qwen.revision -or
        $model.nativeRuntime.ggmlRepository -cne "https://github.com/$($pin.sources.ggml.repository)" -or
        $model.nativeRuntime.ggmlRevision -cne $pin.sources.ggml.revision) { throw 'Native source and model runtime revisions disagree.' }
    foreach ($key in @('qwen', 'ggml')) {
        $source = $pin.sources[$key]
        if ($source.repository -cnotmatch '^[\w.-]+/[\w.-]+$' -or $source.revision -cnotmatch '^[a-f0-9]{40}$' -or
            $source.root -cne "$($source.repository.Split('/')[1])-$($source.revision)") { throw 'Invalid native speech source identity.' }
        Assert-NativeSpeechArtifact -Artifact $source -ExpectedUrl "https://codeload.github.com/$($source.repository)/zip/$($source.revision)"
    }
    return $pin
}

function Assert-NativeSpeechArchiveName {
    [CmdletBinding()]
    [OutputType([void])]
    param([Parameter(Mandatory)][string]$Name, [Parameter(Mandatory)][string]$ArchiveRoot)
    if ($Name.Contains('\') -or -not $Name.StartsWith($ArchiveRoot + '/', [StringComparison]::Ordinal) -or
        $Name -match '[:<>"|?*\x00-\x1f]' -or $Name.Contains('//')) { throw 'Native archive contains an unsafe path.' }
    foreach ($part in $Name.TrimEnd('/').Split('/')) {
        if ($part -match '[. ]$' -or $part -match '^(CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])(\.|$)') {
            throw 'Native archive contains an ambiguous Windows path.'
        }
    }
}

function Expand-NativeSpeechZip {
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseShouldProcessForStateChangingFunctions', '', Justification = 'Private verified source extraction for explicit provisioning.')]
    [CmdletBinding()]
    [OutputType([string])]
    param([Parameter(Mandatory)][string]$Archive, [Parameter(Mandatory)][string]$Destination,
          [Parameter(Mandatory)][string]$ArchiveRoot)
    $Destination = Resolve-ExternalPath -Path $Destination
    $zip = [IO.Compression.ZipFile]::OpenRead($Archive)
    try {
        $entries = [Collections.Generic.List[object]]::new()
        $seen = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
        [long]$expanded = 0
        foreach ($entry in $zip.Entries) {
            $name = $entry.FullName
            # Entries stay within one reviewed root. Unix symlink bytes are
            # materialized as ordinary text files; no filesystem link is made
            # or followed (the GGML archive has one documentation link).
            Assert-NativeSpeechArchiveName -Name $name -ArchiveRoot $ArchiveRoot
            $relative = $name.Substring($ArchiveRoot.Length + 1)
            if (-not $relative) { continue }
            $mode = ($entry.ExternalAttributes -shr 16) -band 0xf000
            $directory = $name.EndsWith('/', [StringComparison]::Ordinal)
            if ($mode -notin @(0, 0x8000, 0x4000, 0xa000) -or ($directory -and $mode -in @(0x8000, 0xa000)) -or
                (-not $directory -and $mode -eq 0x4000) -or -not $seen.Add($relative.TrimEnd('/'))) {
                throw 'Native source ZIP contains a link, device, or duplicate path.'
            }
            $expanded += $entry.Length
            if ($expanded -gt 1GB -or $zip.Entries.Count -gt 20000) { throw 'Native source ZIP exceeds its expansion limit.' }
            $path = Resolve-ExternalPath -Path (Join-Path $Destination $relative)
            if (-not $path.StartsWith($Destination.TrimEnd('\') + '\', [StringComparison]::OrdinalIgnoreCase)) {
                throw 'Native source ZIP escaped its destination.'
            }
            $entries.Add(@{ Entry = $entry; Path = $path; Directory = $directory })
        }
        # Validate every entry before writing any source bytes.
        foreach ($item in $entries) {
            if ($item.Directory) { [void][IO.Directory]::CreateDirectory($item.Path); continue }
            [void][IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($item.Path))
            $inputStream = $item.Entry.Open()
            try {
                if ([IO.File]::Exists($item.Path)) {
                    $expected = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($inputStream))
                    Test-LockedArtifact -Path $item.Path -Bytes $item.Entry.Length -Sha256 $expected
                } else {
                    $output = [IO.FileStream]::new($item.Path, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
                    try { $inputStream.CopyTo($output) } finally { $output.Dispose() }
                }
            } finally { $inputStream.Dispose() }
            $item.Path
        }
    } finally { $zip.Dispose() }
}

function Assert-NativeSpeechArm64 {
    [CmdletBinding()]
    [OutputType([void])]
    param([Parameter(Mandatory)][string]$Path)
    $stream = [IO.FileStream]::new($Path, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
    $reader = [IO.BinaryReader]::new($stream)
    try {
        if ($stream.Length -lt 64 -or $reader.ReadUInt16() -ne 0x5a4d) { throw 'Native speech executable lacks a DOS header.' }
        $stream.Position = 0x3c
        $offset = $reader.ReadInt32()
        if ($offset -lt 64 -or $offset -gt $stream.Length - 6) { throw 'Native speech executable has an invalid PE offset.' }
        $stream.Position = $offset
        if ($reader.ReadUInt32() -ne 0x00004550 -or $reader.ReadUInt16() -ne 0xaa64) {
            throw 'Native speech executables must be native ARM64 PE images.'
        }
    } finally { $reader.Dispose() }
}

function Install-NativeSpeechCompiler {
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseShouldProcessForStateChangingFunctions', '', Justification = 'Private pinned compiler provisioning for an explicitly requested build.')]
    [CmdletBinding()]
    [OutputType([string])]
    param([Parameter(Mandatory)][string]$DataDirectory, [Parameter(Mandatory)][string]$WorkingDirectory,
          [Parameter(Mandatory)][Collections.IDictionary]$Pin)
    $directory = Resolve-ExternalPath -Path (Join-Path -Path $DataDirectory -ChildPath 'tools' -AdditionalChildPath $Pin.directory)
    $archive = Join-Path $directory $Pin.filename
    [void](Receive-LockedArtifact -Uri $Pin.url -Path $archive -Bytes $Pin.bytes -Sha256 $Pin.sha256)
    $root = Resolve-ExternalPath -Path (Join-Path $directory $Pin.root)
    if (-not [IO.Directory]::Exists($root)) {
        $tar = Join-Path $env:SystemRoot 'System32/tar.exe'
        $listing = Invoke-BonsaiNativeProcess -Executable $tar -ArgumentList @('-tvf', $archive) -WorkingDirectory $WorkingDirectory -TimeoutSeconds 600
        if ($listing.ExitCode -ne 0) { throw 'Cannot inspect the pinned LLVM archive.' }
        foreach ($line in ($listing.StandardOutput -split '\r?\n')) {
            if ($line -and $line[0] -notin @('-', 'd')) { throw 'LLVM archive contains a link or special file.' }
        }
        $names = Invoke-BonsaiNativeProcess -Executable $tar -ArgumentList @('-tf', $archive) -WorkingDirectory $WorkingDirectory -TimeoutSeconds 600
        if ($names.ExitCode -ne 0) { throw 'Cannot inspect LLVM archive paths.' }
        foreach ($name in ($names.StandardOutput -split '\r?\n')) {
            if ($name) { Assert-NativeSpeechArchiveName -Name $name -ArchiveRoot $Pin.root }
        }
        $stage = Resolve-ExternalPath -Path (Join-Path $directory ('extract-' + [guid]::NewGuid().ToString('N')))
        [void][IO.Directory]::CreateDirectory($stage)
        $extract = Invoke-BonsaiNativeProcess -Executable $tar -ArgumentList @('-xf', $archive, '-C', $stage) -WorkingDirectory $WorkingDirectory -TimeoutSeconds 1200
        if ($extract.ExitCode -ne 0) { throw "Pinned LLVM extraction failed; evidence remains in $stage" }
        $extracted = Resolve-ExternalPath -Path (Join-Path $stage $Pin.root)
        [IO.Directory]::Move($extracted, $root)
    }
    foreach ($name in $Pin.executables.Keys) {
        $file = Resolve-ExternalPath -Path (Join-Path -Path $root -ChildPath 'bin' -AdditionalChildPath $name)
        Test-LockedArtifact -Path $file -Bytes $Pin.executables[$name].bytes -Sha256 $Pin.executables[$name].sha256
        Assert-NativeSpeechArm64 -Path $file
    }
    return $root
}

function Initialize-NativeSpeechBuild {
    <#
    .SYNOPSIS
    Provisions verified native sources and LLVM, returning a child build environment.
    .DESCRIPTION
    Requires installed Visual Studio ARM64 C++ tools, SDK, CMake, and Ninja.
    Returns Environment (IDictionary), SourceDirectory, CompilerPath, CMakePath.
    Does not change the host process environment. All caches and child working
    directories are external to Git. Cargo.lock pins the bundled ASR source.
    #>
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseShouldProcessForStateChangingFunctions', '', Justification = 'Explicit build initialization provisions only verified external artifacts.')]
    [CmdletBinding()]
    [OutputType([pscustomobject])]
    param([Parameter(Mandatory)][string]$DataDirectory, [Parameter(Mandatory)][string]$WorkingDirectory)
    if (-not $IsWindows -or [Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture -ne 'Arm64') {
        throw 'Native speech builds require native ARM64 PowerShell on Windows.'
    }
    $data = Resolve-ExternalPath -Path $DataDirectory
    $working = Resolve-ExternalPath -Path $WorkingDirectory
    [void][IO.Directory]::CreateDirectory($working)
    $pin = Read-NativeSpeechBuildLock
    $compiler = Install-NativeSpeechCompiler -DataDirectory $data -WorkingDirectory $working -Pin $pin.compiler
    $cache = Resolve-ExternalPath -Path (Join-Path $data 'tools/native-speech')
    [void][IO.Directory]::CreateDirectory($cache)
    $guard = [IO.FileStream]::new((Join-Path $cache 'source-provision.lock'), [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
    try {
        $source = Resolve-ExternalPath -Path (Join-Path -Path $cache -ChildPath 'sources' -AdditionalChildPath $pin.sources.qwen.root)
        $expected = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
        foreach ($key in @('qwen', 'ggml')) {
            $artifact = $pin.sources[$key]
            $archive = Join-Path -Path $cache -ChildPath 'downloads' -AdditionalChildPath $artifact.filename
            [void](Receive-LockedArtifact -Uri $artifact.url -Path $archive -Bytes $artifact.bytes -Sha256 $artifact.sha256)
            $destination = if ($key -eq 'qwen') { $source } else { Join-Path $source 'ggml' }
            foreach ($file in (Expand-NativeSpeechZip -Archive $archive -Destination $destination -ArchiveRoot $artifact.root)) { [void]$expected.Add($file) }
        }
        foreach ($file in (Get-ChildItem -LiteralPath $source -Recurse -Force)) {
            [void](Resolve-ExternalPath -Path $file.FullName)
            if (-not $file.PSIsContainer -and -not $expected.Contains($file.FullName)) { throw "Native source cache contains an unpinned file: $($file.FullName)" }
        }
    } finally { $guard.Dispose() }
    $vswhere = Join-Path ([Environment]::GetFolderPath([Environment+SpecialFolder]::ProgramFilesX86)) 'Microsoft Visual Studio/Installer/vswhere.exe'
    $found = Invoke-BonsaiNativeProcess -Executable $vswhere -ArgumentList @('-latest', '-products', '*', '-property', 'installationPath') -WorkingDirectory $working -TimeoutSeconds 60
    if ($found.ExitCode -ne 0 -or -not [IO.Path]::IsPathFullyQualified($found.StandardOutput.Trim())) { throw 'Install Visual Studio with native ARM64 C++ tools.' }
    $visualStudio = $found.StandardOutput.Trim()
    $devcmd = Join-Path $visualStudio 'Common7/Tools/VsDevCmd.bat'
    $cmake = Join-Path $visualStudio 'Common7/IDE/CommonExtensions/Microsoft/CMake/CMake/bin/cmake.exe'
    $ninja = Join-Path $visualStudio 'Common7/IDE/CommonExtensions/Microsoft/CMake/Ninja/ninja.exe'
    foreach ($file in @($devcmd, $cmake, $ninja)) { if (-not [IO.File]::Exists($file)) { throw "Visual Studio build prerequisite is missing: $file" } }
    if ($devcmd.Contains('"') -or $devcmd.Contains('%') -or $devcmd.Contains("`r") -or $devcmd.Contains("`n")) { throw 'Unsupported Visual Studio installation path.' }
    $batch = Join-Path $working ('visual-studio-env-' + [guid]::NewGuid().ToString('N') + '.cmd')
    [IO.File]::WriteAllText($batch, "@call `"$devcmd`" -no_logo -arch=arm64 -host_arch=arm64 >nul`r`n@if errorlevel 1 exit /b 1`r`n@set`r`n")
    $setup = Invoke-BonsaiNativeProcess -Executable (Join-Path $env:SystemRoot 'System32/cmd.exe') -ArgumentList @('/d', '/c', $batch) -WorkingDirectory $working -TimeoutSeconds 60
    if ($setup.ExitCode -ne 0) { throw 'Visual Studio ARM64 SDK environment initialization failed.' }
    $variables = @{}
    foreach ($line in ($setup.StandardOutput -split '\r?\n')) {
        if ($line -match '^(PATH|INCLUDE|LIB|LIBPATH|VSINSTALLDIR|VCINSTALLDIR|VCToolsInstallDir|WindowsSdkDir|WindowsSDKVersion|UniversalCRTSdkDir|UCRTVersion|VSCMD_ARG_TGT_ARCH|VSCMD_ARG_HOST_ARCH)=(.*)$') {
            $variables[$Matches[1]] = $Matches[2]
        }
    }
    if ($variables['VSCMD_ARG_TGT_ARCH'] -ne 'arm64' -or -not $variables['INCLUDE'] -or -not $variables['LIB']) { throw 'Visual Studio did not provide ARM64 compiler and SDK libraries.' }
    $bin = Join-Path $compiler 'bin'
    $variables['PATH'] = $bin + ';' + [IO.Path]::GetDirectoryName($cmake) + ';' + [IO.Path]::GetDirectoryName($ninja) + ';' + $variables['PATH']
    foreach ($name in @('CC', 'CXX', 'CMAKE_C_COMPILER', 'CMAKE_CXX_COMPILER')) { $variables[$name] = Join-Path $bin 'clang-cl.exe' }
    foreach ($name in @('CMAKE_C_COMPILER_TARGET', 'CMAKE_CXX_COMPILER_TARGET')) { $variables[$name] = $pin.target }
    $variables['CMAKE_GENERATOR'] = 'Ninja'
    $variables['CMAKE'] = $cmake
    $variables['RC'] = Join-Path $bin 'llvm-rc.exe'
    $variables['CL'] = '/EHsc'
    $variables['CARGO_BUILD_JOBS'] = '4'
    $variables['CMAKE_BUILD_PARALLEL_LEVEL'] = '4'
    $variables['TRANSCRIBE_CMAKE_ARGS'] = '-DGGML_NATIVE=OFF -DGGML_OPENMP=OFF -DTRANSCRIBE_USE_OPENMP=OFF -DTRANSCRIBE_USE_SYSTEM_BLAS=OFF -DCMAKE_C_COMPILER_TARGET=aarch64-pc-windows-msvc -DCMAKE_CXX_COMPILER_TARGET=aarch64-pc-windows-msvc'
    $variables['BONSAI_TTS_SOURCE_DIR'] = $source
    $variables['BONSAI_CMAKE'] = $cmake
    $variables['BONSAI_CMAKE_GENERATOR'] = 'Visual Studio 18 2026'
    return [pscustomobject]@{ Environment = $variables; SourceDirectory = $source; CompilerPath = Join-Path $bin 'clang-cl.exe'; CMakePath = $cmake }
}

function Invoke-NativeSpeechBuild {
    <#
    .SYNOPSIS
    Builds selected native speech daemons in Release and verifies ARM64 PE outputs.
    .DESCRIPTION
    Returns AsrPath and TtsPath; unselected paths are null. Uses the repository's
    pinned Rust toolchain and Cargo.lock. Logs and target artifacts are beneath
    the explicitly external WorkingDirectory. No model is loaded during builds.
    #>
    [CmdletBinding()]
    [OutputType([pscustomobject])]
    param([Parameter(Mandatory)][string]$DataDirectory, [Parameter(Mandatory)][string]$WorkingDirectory,
          [Parameter(Mandatory)][string]$RepositoryRoot,
          [ValidateSet('All', 'Canary', 'QwenTts')][string]$Model = 'All')
    $working = Resolve-ExternalPath -Path $WorkingDirectory
    if (-not [IO.Path]::IsPathFullyQualified($RepositoryRoot) -or $RepositoryRoot -match '%[^%]+%') { throw 'RepositoryRoot must be an expanded absolute path.' }
    $repository = [IO.Path]::GetFullPath($RepositoryRoot)
    foreach ($file in @('Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml')) { if (-not [IO.File]::Exists((Join-Path $repository $file))) { throw "Native build input is missing: $file" } }
    $toolchain = Get-Content -LiteralPath (Join-Path $repository 'rust-toolchain.toml') -Raw
    if ($toolchain -notmatch '(?m)^channel\s*=\s*"(nightly-\d{4}-\d{2}-\d{2})"\s*$') { throw 'Native build requires the repository-pinned Rust toolchain.' }
    $channel = $Matches[1] + '-aarch64-pc-windows-msvc'
    $native = Initialize-NativeSpeechBuild -DataDirectory $DataDirectory -WorkingDirectory $working
    $target = Resolve-ExternalPath -Path (Join-Path $working 'native-speech-target')
    $result = [ordered]@{ AsrPath = $null; TtsPath = $null }
    foreach ($kind in @('Canary', 'QwenTts')) {
        if ($Model -ne 'All' -and $Model -ne $kind) { continue }
        $package = if ($kind -eq 'Canary') { 'bonsai-speech-asr' } else { 'bonsai-speech-tts' }
        $arguments = @("+$channel", 'build', '--locked', '--release', '--manifest-path', (Join-Path $repository 'Cargo.toml'), '--package', $package,
            '--target', 'aarch64-pc-windows-msvc', '--target-dir', $target)
        $log = Join-Path $working ($package + '-build.log')
        $environment = @{}
        foreach ($name in $native.Environment.Keys) { $environment[$name] = $native.Environment[$name] }
        if ($kind -eq 'QwenTts') {
            foreach ($name in @('CC', 'CXX', 'CMAKE_C_COMPILER', 'CMAKE_CXX_COMPILER')) { $environment[$name] = 'cl.exe' }
            $environment['RC'] = 'rc.exe'
            $environment['CMAKE_GENERATOR'] = 'Visual Studio 18 2026'
        }
        $build = Invoke-BonsaiNativeProcess -Executable 'cargo.exe' -ArgumentList $arguments -WorkingDirectory $working -Environment $environment -TimeoutSeconds 3600 -LogPath $log
        if ($build.ExitCode -ne 0) { throw "Native $kind build failed; see $log" }
        $executable = Resolve-ExternalPath -Path (Join-Path -Path $target -ChildPath 'aarch64-pc-windows-msvc/release' -AdditionalChildPath ($package + '.exe'))
        Assert-NativeSpeechArm64 -Path $executable
        if ($kind -eq 'Canary') { $result.AsrPath = $executable } else { $result.TtsPath = $executable }
    }
    return [pscustomobject]$result
}

Export-ModuleMember -Function Install-NativeSpeechModels, Initialize-NativeSpeechBuild, Invoke-NativeSpeechBuild
