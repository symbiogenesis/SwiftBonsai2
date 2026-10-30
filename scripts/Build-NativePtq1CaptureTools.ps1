#requires -Version 7.6
<# .SYNOPSIS
Builds a source-bound ARM64 numerical capture bundle on hosted CI without models.
.DESCRIPTION
The owner-device smoke consumes these two prepared executables and their digest
receipt. This helper never downloads model/runtime artifacts or loads a model.
Source archives and compilation stay in separate external temporary storage.
#>
[CmdletBinding()]
[OutputType([pscustomobject])]
param([Parameter(Mandatory)][string]$OutputDirectory)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
if (-not $IsWindows -or [Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture -ne 'Arm64' -or
    [Runtime.InteropServices.RuntimeInformation]::OSArchitecture -ne 'Arm64' -or
    $env:GITHUB_ACTIONS -cne 'true' -or $env:RUNNER_ENVIRONMENT -cne 'github-hosted' -or
    $env:RUNNER_ARCH -cne 'ARM64' -or $env:GITHUB_REF -cne 'refs/heads/main' -or
    $env:GITHUB_REPOSITORY -cne 'symbiogenesis/SwiftBonsai2Runner' -or
    $env:GITHUB_SHA -cnotmatch '^[a-f0-9]{40}$') {
    throw 'Native PTQ1 capture preparation requires this repository main on hosted native Windows ARM64 CI.'
}
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1')
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Native.psm1')
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$output = Resolve-ExternalPath $OutputDirectory
if (Test-Path -LiteralPath $output) {
    if (-not [IO.Directory]::Exists($output) -or @(Get-ChildItem -LiteralPath $output -Force).Count) {
        throw 'Capture bundle output must be a new or empty external directory.'
    }
}
[void][IO.Directory]::CreateDirectory($output)
$scratch = Resolve-ExternalPath (Join-Path ([IO.Path]::GetTempPath()) ('Bonsai-HostedPtq1Build-' + [guid]::NewGuid().ToString('N')))
[void][IO.Directory]::CreateDirectory($scratch)
$pins = Get-Content -LiteralPath (Join-Path $repo 'tools/native-reference/pins.json') -Raw | ConvertFrom-Json -DateKind String
foreach ($source in @($pins.nativeSource, $pins.rustSource)) {
    $archive = Join-Path $scratch ($source.commit + '.zip')
    Receive-LockedArtifact -Uri $source.url -Path $archive -Bytes $source.bytes -Sha256 $source.sha256 -TimeoutSeconds 180 | Out-Null
    Expand-Archive -LiteralPath $archive -DestinationPath (Join-Path $scratch $source.commit)
}
$native = Join-Path $scratch ($pins.nativeSource.commit + '/llama.cpp-' + $pins.nativeSource.commit)
$rust = Join-Path $scratch ($pins.rustSource.commit + '/prism-inference-' + $pins.rustSource.commit)
$header = Join-Path $native 'include/llama.h'
if ((Get-FileHash -LiteralPath $header -Algorithm SHA256).Hash.ToLowerInvariant() -cne $pins.nativeSource.headerSha256) {
    throw 'Pinned native public header identity changed.'
}
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
$vs = (& $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.ARM64 -property installationPath | Out-String).Trim()
if (-not $vs) { throw 'Hosted preparation requires installed native ARM64 Visual C++ tools.' }
$version = (Get-Content -LiteralPath (Join-Path $vs 'VC/Auxiliary/Build/Microsoft.VCToolsVersion.default.txt') -Raw).Trim()
$vc = Join-Path $vs ('VC/Tools/MSVC/' + $version)
$compilerDirectory = Join-Path $vc 'bin/Hostarm64/arm64'
$compiler = Join-Path $compilerDirectory 'cl.exe'
if (-not [IO.File]::Exists($compiler)) { throw 'Native Hostarm64/arm64 cl.exe is required; no emulated compiler fallback.' }
Assert-Arm64PE $compiler
$sdk = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits/10'
$sdkVersions = @(Get-ChildItem -LiteralPath (Join-Path $sdk 'Include') -Directory | Where-Object {
    $_.Name -match '^10\.0\.[0-9]+\.[0-9]+$' -and
    [IO.File]::Exists((Join-Path $_.FullName 'um/Windows.h')) -and
    [IO.File]::Exists((Join-Path $sdk ('Lib/' + $_.Name + '/ucrt/arm64/ucrt.lib'))) -and
    [IO.File]::Exists((Join-Path $sdk ('Lib/' + $_.Name + '/um/arm64/kernel32.lib')))
} | Sort-Object { [version]$_.Name } -Descending)
if (-not $sdkVersions.Count) { throw 'An installed Windows SDK with ARM64 UCRT and system libraries is required.' }
$sdkVersion = $sdkVersions[0].Name
$collectorSource = Join-Path $repo 'tools/native-reference/swift_ptq1_reference.cpp'
$collectorSourceSha256 = (Get-FileHash -LiteralPath $collectorSource -Algorithm SHA256).Hash.ToLowerInvariant()
$collector = Join-Path $scratch 'swift-ptq1-reference.exe'
$compilerArguments = @('/nologo', '/std:c++17', '/MD', '/W4', '/WX', '/O2', '/EHsc',
    '/I', (Join-Path $native 'include'), '/I', (Join-Path $native 'ggml/include'),
    $collectorSource, "/Fe:$collector", ('/Fo:' + (Join-Path $scratch 'reference.obj')), '/link', '/incremental:no')
$savedEnvironment = @{}
foreach ($name in @('INCLUDE', 'LIB', 'PATH')) { $savedEnvironment[$name] = [Environment]::GetEnvironmentVariable($name) }
try {
    $env:INCLUDE = "$vc/include;$sdk/Include/$sdkVersion/ucrt;$sdk/Include/$sdkVersion/shared;$sdk/Include/$sdkVersion/um"
    $env:LIB = "$vc/lib/arm64;$sdk/Lib/$sdkVersion/ucrt/arm64;$sdk/Lib/$sdkVersion/um/arm64"
    $env:PATH = $compilerDirectory + ';' + $env:PATH
    $built = Invoke-BonsaiNativeProcess -Executable $compiler -WorkingDirectory $scratch -TimeoutSeconds 120 -OwnDescendants -LogPath (Join-Path $scratch 'native-build.txt') -ArgumentList $compilerArguments
    if ($built.ExitCode -ne 0) { throw "Hosted native collector build failed; inspect $scratch" }
} finally {
    foreach ($name in $savedEnvironment.Keys) { [Environment]::SetEnvironmentVariable($name, $savedEnvironment[$name]) }
}
Assert-Arm64PE $collector
if ((Get-FileHash -LiteralPath $collectorSource -Algorithm SHA256).Hash.ToLowerInvariant() -cne $collectorSourceSha256) {
    throw 'Collector source changed during compilation.'
}
$channel = 'nightly-2026-10-22-aarch64-pc-windows-msvc'
$cargo = @(Get-Command cargo -CommandType Application -ErrorAction Stop)[0].Source
$rustc = @(Get-Command rustc -CommandType Application -ErrorAction Stop)[0].Source
$rustVersion = Invoke-BonsaiNativeProcess -Executable $rustc -WorkingDirectory $scratch -TimeoutSeconds 30 -ArgumentList @("+$channel", '-vV')
if ($rustVersion.ExitCode -ne 0 -or $rustVersion.StandardOutput -notmatch 'host: aarch64-pc-windows-msvc') {
    throw 'The pinned native ARM64 Rust compiler is unavailable.'
}
$target = Join-Path $scratch 'rust-build'
$built = Invoke-BonsaiNativeProcess -Executable $cargo -WorkingDirectory $scratch -TimeoutSeconds 1200 -OwnDescendants -LogPath (Join-Path $scratch 'rust-build.txt') -ArgumentList @(
    "+$channel", 'build', '--locked', '--release', '--manifest-path', (Join-Path $rust 'Cargo.toml'),
    '-p', 'prism-inference', '--example', 'swift_model_check', '--target-dir', $target)
if ($built.ExitCode -ne 0) { throw "Hosted Rust numerical checker build failed; inspect $scratch" }
$qualifier = Join-Path $target 'release/examples/swift_model_check.exe'
Assert-Arm64PE $qualifier
$files = foreach ($path in @($collector, $qualifier)) {
    $name = [IO.Path]::GetFileName($path)
    $destination = Join-Path $output $name
    [IO.File]::Copy($path, $destination, $false)
    [ordered]@{ Name = $name; Bytes = (Get-Item -LiteralPath $destination).Length
        Sha256 = (Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash.ToLowerInvariant() }
}
$receipt = [ordered]@{
    SchemaVersion = 1; RunnerRevision = $env:GITHUB_SHA; NativeSource = $pins.nativeSource; RustSource = $pins.rustSource
    CollectorSourceSha256 = $collectorSourceSha256; RustToolchain = $channel
    Compiler = [ordered]@{ Path = $compiler; Sha256 = (Get-FileHash -LiteralPath $compiler -Algorithm SHA256).Hash.ToLowerInvariant()
        VisualCppToolsVersion = $version; WindowsSdkVersion = $sdkVersion; CollectorArguments = $compilerArguments
        RustcVerbose = $rustVersion.StandardOutput.Trim() }
    Files = @($files)
}
$receiptPath = Join-Path $output 'capture-tools.json'
$stream = [IO.FileStream]::new($receiptPath, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
try {
    $bytes = [Text.UTF8Encoding]::new($false).GetBytes(($receipt | ConvertTo-Json -Depth 20) + "`n")
    if ($bytes.Length -gt 65536) { throw 'Capture bundle receipt exceeds its fixed bound.' }
    $stream.Write($bytes, 0, $bytes.Length)
    $stream.Flush($true)
} finally { $stream.Dispose() }
[pscustomobject]@{ Status = 'hosted-capture-tools-built'; OutputDirectory = $output; ReceiptPath = $receiptPath
    EvidenceDirectory = $scratch; ModelProcessStarted = $false }
