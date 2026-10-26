#requires -Version 7.6
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1')
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Native.psm1')
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Tools.psm1')

function Install-NativeSpeechModels {
    <#
    .SYNOPSIS
    Downloads or verifies pinned native Canary and Qwen VoiceDesign GGUF artifacts.
    .DESCRIPTION
    Rust plans artifact identities from the inference crates' compiled model locks.
    Reuses exact verified caches under DataDirectory/speech/native. Never loads
    a model, starts a service, or accesses an audio device.
    DeferCachedVerification checks existing file lengths without reading model
    contents. It is reserved for launchers whose native speech services verify
    compiled SHA-256 pins and hold verified files before loading them.
    #>
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseShouldProcessForStateChangingFunctions', '', Justification = 'Explicit artifact provisioning command; verified cache writes are its requested purpose.')]
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseSingularNouns', '', Justification = 'Public launcher API provisions a selected set of speech models.')]
    [CmdletBinding()]
    [OutputType([pscustomobject])]
    param([Parameter(Mandatory)][string]$DataDirectory,
          [ValidateSet('All', 'Canary', 'QwenTts')][string]$Model = 'All',
          [ValidateSet('Q8_0', 'Q4_K_M')][string]$TtsQuantization = 'Q8_0',
          [switch]$VerifyOnly, [switch]$DeferCachedVerification, [string]$ToolPath)
    if ($VerifyOnly -and $DeferCachedVerification) {
        throw 'VerifyOnly cannot be combined with DeferCachedVerification.'
    }
    $TtsQuantization = if ($TtsQuantization -ieq 'Q8_0') { 'Q8_0' } else { 'Q4_K_M' }
    $result = Invoke-BonsaiTool -Command speech-models -ToolPath $ToolPath -Request @{
        dataDirectory = Resolve-ExternalPath -Path $DataDirectory; model = $Model; ttsQuantization = $TtsQuantization
    }
    foreach ($file in $result.Plans) {
        $path = Resolve-ExternalPath -Path $file.Path
        if ($DeferCachedVerification -and [IO.File]::Exists($path)) {
            # The companion verifies the same compiled lock before loading and
            # retains its file guard. Avoid reading gigabytes before the UI opens.
            $stream = [IO.FileStream]::new($path, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
            try {
                if ($stream.Length -ne $file.Bytes) { throw "Artifact byte length mismatch: $path" }
            } finally { $stream.Dispose() }
        }
        elseif ($VerifyOnly) { Test-LockedArtifact -Path $path -Bytes $file.Bytes -Sha256 $file.Sha256 }
        else { [void](Receive-LockedArtifact -Uri $file.Uri -Path $file.Path -Bytes $file.Bytes -Sha256 $file.Sha256) }
    }
    [void]$result.Remove('Plans')
    return [pscustomobject]$result
}

function Install-NativeSpeechCompiler {
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseShouldProcessForStateChangingFunctions', '', Justification = 'Private pinned compiler provisioning for an explicitly requested build.')]
    [CmdletBinding()]
    [OutputType([string])]
    param([Parameter(Mandatory)][string]$DataDirectory, [Parameter(Mandatory)][string]$WorkingDirectory,
          [Parameter(Mandatory)][Collections.IDictionary]$Pin, [string]$ToolPath)
    $directory = Resolve-ExternalPath -Path (Join-Path -Path $DataDirectory -ChildPath 'tools' -AdditionalChildPath $Pin.directory)
    $archive = Join-Path $directory $Pin.filename
    $root = Resolve-ExternalPath -Path (Join-Path $directory $Pin.root)
    if (-not [IO.Directory]::Exists($root)) {
        # The archive is needed only for extraction. Installed compiler binaries
        # are still checked against their exact pins below on every build.
        [void](Receive-LockedArtifact -Uri $Pin.url -Path $archive -Bytes $Pin.bytes -Sha256 $Pin.sha256)
        $tar = Join-Path $env:SystemRoot 'System32/tar.exe'
        $listing = Invoke-BonsaiNativeProcess -Executable $tar -ArgumentList @('-tvf', $archive) -WorkingDirectory $WorkingDirectory -TimeoutSeconds 600
        if ($listing.ExitCode -ne 0) { throw 'Cannot inspect the pinned LLVM archive.' }
        foreach ($line in ($listing.StandardOutput -split '\r?\n')) {
            if ($line -and $line[0] -notin @('-', 'd')) { throw 'LLVM archive contains a link or special file.' }
        }
        $names = Invoke-BonsaiNativeProcess -Executable $tar -ArgumentList @('-tf', $archive) -WorkingDirectory $WorkingDirectory -TimeoutSeconds 600
        if ($names.ExitCode -ne 0) { throw 'Cannot inspect LLVM archive paths.' }
        $null = Invoke-BonsaiTool -Command speech-build -ToolPath $ToolPath -Request @{
            operation = 'validate-names'; archiveRoot = $Pin.root; names = @($names.StandardOutput -split '\r?\n' | Where-Object { $_ })
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
        Assert-Arm64PE -Path $file
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
    param([Parameter(Mandatory)][string]$DataDirectory, [Parameter(Mandatory)][string]$WorkingDirectory,
          [string]$ToolPath)
    if (-not $IsWindows -or [Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture -ne 'Arm64') {
        throw 'Native speech builds require native ARM64 PowerShell on Windows.'
    }
    $data = Resolve-ExternalPath -Path $DataDirectory
    $working = Resolve-ExternalPath -Path $WorkingDirectory
    [void][IO.Directory]::CreateDirectory($working)
    $pin = Invoke-BonsaiTool -Command speech-build -ToolPath $ToolPath -Request @{ operation = 'plan' }
    $compiler = Install-NativeSpeechCompiler -DataDirectory $data -WorkingDirectory $working -Pin $pin.compiler -ToolPath $ToolPath
    $cache = Resolve-ExternalPath -Path (Join-Path $data 'tools/native-speech')
    [void][IO.Directory]::CreateDirectory($cache)
    $guard = [IO.FileStream]::new((Join-Path $cache 'source-provision.lock'), [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
    try {
        $source = Resolve-ExternalPath -Path (Join-Path -Path $cache -ChildPath 'sources' -AdditionalChildPath $pin.sources.qwen.root)
        foreach ($key in @('qwen', 'ggml')) {
            $artifact = $pin.sources[$key]
            $archive = Join-Path -Path $cache -ChildPath 'downloads' -AdditionalChildPath $artifact.filename
            [void](Receive-LockedArtifact -Uri $artifact.url -Path $archive -Bytes $artifact.bytes -Sha256 $artifact.sha256)
        }
        $null = Invoke-BonsaiTool -Command speech-build -TimeoutSeconds 120 -ToolPath $ToolPath -Request @{
            operation = 'stage-sources'; sourceDirectory = $source
            qwenArchivePath = Join-Path -Path $cache -ChildPath 'downloads' -AdditionalChildPath $pin.sources.qwen.filename
            ggmlArchivePath = Join-Path -Path $cache -ChildPath 'downloads' -AdditionalChildPath $pin.sources.ggml.filename
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
    $variables['BONSAI_TTS_COMPILER'] = Join-Path $bin 'clang-cl.exe'
    $variables['BONSAI_CMAKE'] = $cmake
    $variables['BONSAI_CMAKE_GENERATOR'] = 'Ninja Multi-Config'
    return [pscustomobject]@{ Environment = $variables; SourceDirectory = $source; CompilerPath = Join-Path $bin 'clang-cl.exe'; CMakePath = $cmake }
}

function Invoke-NativeSpeechBuild {
    <#
    .SYNOPSIS
    Builds selected native speech daemons in Release and verifies ARM64 PE outputs.
    .DESCRIPTION
    Returns AsrPath and TtsPath; unselected paths are null. Uses the repository's
    pinned Rust toolchain and Cargo.lock. Cargo reuses DataDirectory/build/native-speech;
    process working directories and logs remain in the external WorkingDirectory.
    No model is loaded during builds.
    #>
    [CmdletBinding()]
    [OutputType([pscustomobject])]
    param([Parameter(Mandatory)][string]$DataDirectory, [Parameter(Mandatory)][string]$WorkingDirectory,
          [Parameter(Mandatory)][string]$RepositoryRoot,
          [ValidateSet('All', 'Canary', 'QwenTts')][string]$Model = 'All', [string]$ToolPath)
    $working = Resolve-ExternalPath -Path $WorkingDirectory
    if (-not [IO.Path]::IsPathFullyQualified($RepositoryRoot) -or $RepositoryRoot -match '%[^%]+%') { throw 'RepositoryRoot must be an expanded absolute path.' }
    $repository = [IO.Path]::GetFullPath($RepositoryRoot)
    foreach ($file in @('Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml')) { if (-not [IO.File]::Exists((Join-Path $repository $file))) { throw "Native build input is missing: $file" } }
    $toolchain = Get-Content -LiteralPath (Join-Path $repository 'rust-toolchain.toml') -Raw
    if ($toolchain -notmatch '(?m)^channel\s*=\s*"(nightly-\d{4}-\d{2}-\d{2})"\s*$') { throw 'Native build requires the repository-pinned Rust toolchain.' }
    $channel = $Matches[1] + '-aarch64-pc-windows-msvc'
    $native = Initialize-NativeSpeechBuild -DataDirectory $DataDirectory -WorkingDirectory $working -ToolPath $ToolPath
    $target = Resolve-ExternalPath -Path (Join-Path $DataDirectory 'build/native-speech')
    $result = [ordered]@{ AsrPath = $null; TtsPath = $null }
    foreach ($kind in @('Canary', 'QwenTts')) {
        if ($Model -ne 'All' -and $Model -ne $kind) { continue }
        $package = if ($kind -eq 'Canary') { 'bonsai-speech-asr' } else { 'bonsai-speech-tts' }
        $arguments = @("+$channel", 'build', '--locked', '--release', '--manifest-path', (Join-Path $repository 'Cargo.toml'), '--package', $package,
            '--target', 'aarch64-pc-windows-msvc', '--target-dir', $target)
        $log = Join-Path $working ($package + '-build.log')
        $environment = @{}
        foreach ($name in $native.Environment.Keys) { $environment[$name] = $native.Environment[$name] }
        $build = Invoke-BonsaiNativeProcess -Executable 'cargo.exe' -ArgumentList $arguments -WorkingDirectory $working -Environment $environment -TimeoutSeconds 3600 -LogPath $log
        if ($build.ExitCode -ne 0) { throw "Native $kind build failed; see $log" }
        $executable = Resolve-ExternalPath -Path (Join-Path -Path $target -ChildPath 'aarch64-pc-windows-msvc/release' -AdditionalChildPath ($package + '.exe'))
        Assert-Arm64PE -Path $executable
        if ($kind -eq 'Canary') { $result.AsrPath = $executable } else { $result.TtsPath = $executable }
    }
    return [pscustomobject]$result
}

Export-ModuleMember -Function Install-NativeSpeechModels, Initialize-NativeSpeechBuild, Invoke-NativeSpeechBuild
