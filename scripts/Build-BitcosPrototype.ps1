#requires -Version 7.6
<#
.SYNOPSIS
Builds the experimental ARM64 BITCOS kernels with external build/evidence lanes.
.DESCRIPTION
Build only: no tests, benchmarks, model loading, downloads or inference. Uses
strict repository-owned C++ settings and limits project/compiler workers to two.
Returns absolute executable/evidence paths for focused checks or hosted CI.
An optional external BuildDirectory permits cached CMake compiler discovery and
intermediates. Configuration, build and ARM64 executable checks always run;
evidence and native-command working directories remain unique system-temp lanes.
#>
[CmdletBinding()]
[OutputType([Collections.IDictionary])]
param(
    [string]$CMakePath,
    [string]$BuildDirectory,
    [ValidateSet('Visual Studio 18 2026','Visual Studio 17 2022')][string]$Generator = 'Visual Studio 18 2026'
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
if (-not $IsWindows -or [Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture -ne 'Arm64') { throw 'Use native ARM64 PowerShell on Windows.' }
if (-not $CMakePath) { $CMakePath = (Get-Command -Name cmake -CommandType Application -ErrorAction Stop).Source }
if (-not [IO.Path]::IsPathFullyQualified($CMakePath) -or $CMakePath -match '%[^%]+%' -or -not [IO.File]::Exists($CMakePath)) { throw 'CMakePath must name an existing expanded absolute executable.' }
$overrides = @(Get-ChildItem Env: | Where-Object Name -Match '^(CMAKE_.*|CC|CXX|CL|_CL_|LINK|CFLAGS|CXXFLAGS|LDFLAGS)$')
if ($overrides.Count) { throw ('Build override environment is unsupported: ' + ($overrides.Name -join ', ')) }
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1') -Force
$scratch = Resolve-ExternalPath (Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-BitcosBuild-' + [guid]::NewGuid().ToString('N')))
[void][IO.Directory]::CreateDirectory($scratch)
$build = if ($BuildDirectory) { Resolve-ExternalPath $BuildDirectory } else { Join-Path $scratch 'build' }
$source = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../tools/bitcos'))
$cachePath = Join-Path $build 'CMakeCache.txt'
if ([IO.File]::Exists($cachePath)) {
    # CMake build trees contain absolute source/output paths. Reject a restored
    # tree from another layout before configuring or using any generated file.
    $cacheLines = [IO.File]::ReadAllLines($cachePath)
    foreach ($binding in @(@('CMAKE_HOME_DIRECTORY', $source), @('CMAKE_CACHEFILE_DIR', $build))) {
        $entries = @($cacheLines | Where-Object { $_.StartsWith($binding[0] + ':INTERNAL=') })
        $cachedPath = if ($entries.Count -eq 1) { $entries[0].Split('=', 2)[1] } else { '' }
        if (-not [IO.Path]::IsPathFullyQualified($cachedPath) -or
            -not [string]::Equals([IO.Path]::GetFullPath($cachedPath), $binding[1], [StringComparison]::OrdinalIgnoreCase)) {
            throw 'Cached CMake build directory does not match the current absolute source/build paths.'
        }
    }
}
$script:commandNumber = 0
function Invoke-PrototypeNative {
    [CmdletBinding()]
    [OutputType([string])]
    param([string[]]$Arguments)
    $script:commandNumber++
    $start = [Diagnostics.ProcessStartInfo]::new($CMakePath)
    $start.WorkingDirectory = $scratch
    $start.UseShellExecute = $false; $start.CreateNoWindow = $true
    $start.RedirectStandardOutput = $true; $start.RedirectStandardError = $true
    foreach ($argument in $Arguments) { $start.ArgumentList.Add($argument) }
    $process = [Diagnostics.Process]::new(); $process.StartInfo = $start
    try {
        if (-not $process.Start()) { throw 'Native prototype build command failed to start.' }
        $stdout = $process.StandardOutput.ReadToEndAsync(); $stderr = $process.StandardError.ReadToEndAsync()
        if (-not $process.WaitForExit(180000)) { $process.Kill($true); $process.WaitForExit(); throw 'Native prototype build command exceeded three minutes.' }
        $output = $stdout.GetAwaiter().GetResult() + $stderr.GetAwaiter().GetResult()
        $log = Join-Path $scratch ('command-{0:D2}.log' -f $script:commandNumber)
        [IO.File]::WriteAllText($log, $output)
        if ($process.ExitCode) { throw "Native prototype build failed; see $log" }
        return $output.Trim()
    } finally { $process.Dispose() }
}
$version = Invoke-PrototypeNative @('--version')
$capabilities = Invoke-PrototypeNative @('-E','capabilities') | ConvertFrom-Json -DateKind String
if ($Generator -notin $capabilities.generators.name) { throw 'Installed CMake does not support the selected generator.' }
$dotProduct = [Runtime.Intrinsics.Arm.Dp]::IsSupported
$dotProductOption = if ($dotProduct) { 'ON' } else { 'OFF' }
$null = Invoke-PrototypeNative @('-S',$source,'-B',$build,'-G',$Generator,'-A','ARM64',"-DBITCOS_DOTPROD=$dotProductOption")
$null = Invoke-PrototypeNative @('--build',$build,'--config','Release','--parallel','2','--',
    '/p:UseMultiToolTask=true','/p:EnforceProcessCountAcrossBuilds=true','/p:CL_MPCount=2','/p:MultiProcMaxCount=2')
$tests = Join-Path $build 'Release/bitcos-tests.exe'
$benchmark = Join-Path $build 'Release/bitcos-bench.exe'
foreach ($executable in @($tests,$benchmark)) { Assert-Arm64PE $executable }
$receipt = [ordered]@{
    SchemaVersion = 1
    RecordedAtUtc = [DateTime]::UtcNow.ToString('o')
    CMakeVersion = $version
    Compiler = @(Get-ChildItem -Path (Join-Path $build 'CMakeFiles/*/CMakeCXXCompiler.cmake') | ForEach-Object {
        @(Get-Content -LiteralPath $_.FullName | Where-Object { $_ -match '^set\(CMAKE_CXX_COMPILER(_ID|_VERSION|_ARCHITECTURE_ID|_FRONTEND_VARIANT)? ' })
    })
    Generator = $Generator
    Platform = 'ARM64'
    Configuration = 'Release'
    DotProductInstructions = $dotProduct
    BuildDirectory = $build
    TestExecutable = $tests
    BenchmarkExecutable = $benchmark
    TestExecutableSha256 = (Get-FileHash -LiteralPath $tests -Algorithm SHA256).Hash.ToLowerInvariant()
    BenchmarkExecutableSha256 = (Get-FileHash -LiteralPath $benchmark -Algorithm SHA256).Hash.ToLowerInvariant()
    EvidenceDirectory = $scratch
    Qualification = 'build-only; experimental kernels, no model/runtime performance qualification'
}
[IO.File]::WriteAllText((Join-Path $scratch 'build-receipt.json'), ($receipt | ConvertTo-Json -Depth 5))
return $receipt
