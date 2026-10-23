#requires -Version 7.6
<#
.SYNOPSIS
Builds the pinned Prism storage overlay in a fresh external source/build lane.
.DESCRIPTION
Requires the exact source ZIP identified by tools/prism/source.lock.json. No
downloads, tests, benchmarks, model loading, installation, or profile promotion.
PrepareOnly returns the verified, patched source for a separate CI build. Probe
builds the focused real-ggml alias test; Engine builds completion and server.
#>
[CmdletBinding()]
[OutputType([Collections.IDictionary])]
param(
    [Parameter(Mandatory)][string]$SourceArchive,
    [string]$CMakePath,
    [ValidateSet('Probe','Engine','All')][string]$BuildMode = 'Probe',
    [ValidateSet('Ninja','Visual Studio 18 2026','Visual Studio 17 2022')][string]$Generator = 'Ninja',
    [switch]$PrepareOnly
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1') -Force
$archive = Resolve-ExternalPath -Path $SourceArchive
$overlay = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../tools/prism'))
$lock = Get-Content -LiteralPath (Join-Path $overlay 'source.lock.json') -Raw | ConvertFrom-Json -DateKind String
$scratch = Resolve-ExternalPath -Path (Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-PrismBuild-' + [guid]::NewGuid().ToString('N')))
[IO.Directory]::CreateDirectory($scratch) | Out-Null
if ($env:GITHUB_ACTIONS -eq 'true' -and $env:GITHUB_ENV) {
    $environmentPath = Resolve-ExternalPath -Path $env:GITHUB_ENV
    if ($scratch.IndexOfAny([char[]]"`r`n") -ge 0) { throw 'The evidence directory cannot contain line breaks.' }
    [IO.File]::AppendAllText($environmentPath, "BONSAI_PRISM_EVIDENCE=$scratch`n")
}
$guard = [IO.FileStream]::new($archive, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
try {
    if ($guard.Length -ne $lock.ArchiveBytes) { throw 'Pinned Prism source ZIP size mismatch.' }
    $archiveHash = [Convert]::ToHexStringLower([Security.Cryptography.SHA256]::HashData($guard))
    if ($archiveHash -cne $lock.ArchiveSha256) { throw 'Pinned Prism source ZIP hash mismatch.' }
    Expand-Archive -LiteralPath $archive -DestinationPath (Join-Path $scratch 'source')
} finally { $guard.Dispose() }
$source = Join-Path $scratch ('source/' + $lock.ArchiveRoot)
$overlayReceipt = & (Join-Path $overlay 'Apply-Overlay.ps1') -SourceDirectory $source
$receipt = [ordered]@{
    SchemaVersion = 1
    RecordedAtUtc = [DateTime]::UtcNow.ToString('o')
    SourceCommit = $lock.Commit
    SourceArchiveSha256 = $archiveHash
    SourceArchiveBytes = $lock.ArchiveBytes
    Overlay = $overlayReceipt
    SourceDirectory = $source
    EvidenceDirectory = $scratch
    ProbeSourceDirectory = $overlay
    BuildMode = $BuildMode
    Generator = $Generator
    Binaries = @()
    BuildConfigurations = @()
    Status = 'prepared'
    Qualification = 'source/build candidate only; no TPS, output-quality, or model qualification'
}
[IO.File]::WriteAllText((Join-Path $scratch 'build-receipt.json'), ($receipt | ConvertTo-Json -Depth 7))
if (-not $PrepareOnly) {
    if (-not $CMakePath) { $CMakePath = (Get-Command -Name cmake -CommandType Application -ErrorAction Stop).Source }
    if (-not [IO.Path]::IsPathFullyQualified($CMakePath) -or $CMakePath -match '%[^%]+%' -or -not [IO.File]::Exists($CMakePath)) { throw 'CMakePath must name an existing expanded absolute executable.' }
    if ($Generator -ne 'Ninja' -and -not $IsWindows) { throw 'Visual Studio generators require Windows.' }
    $script:prismCommandNumber = 0
    function Invoke-PrismBuildCommand {
        [CmdletBinding()]
        [OutputType([string])]
        param([string[]]$Arguments)
        $script:prismCommandNumber++
        $start = [Diagnostics.ProcessStartInfo]::new($CMakePath)
        $start.WorkingDirectory = $scratch
        $start.UseShellExecute = $false; $start.CreateNoWindow = $true
        $start.RedirectStandardOutput = $true; $start.RedirectStandardError = $true
        foreach ($argument in $Arguments) { $start.ArgumentList.Add($argument) }
        $process = [Diagnostics.Process]::new(); $process.StartInfo = $start
        try {
            if (-not $process.Start()) { throw 'Prism build command failed to start.' }
            $stdout = $process.StandardOutput.ReadToEndAsync(); $stderr = $process.StandardError.ReadToEndAsync()
            $deadline = [Environment]::TickCount64 + 900000L
            $timedOut = $false
            while (-not $process.WaitForExit(1000)) {
                if ([Environment]::TickCount64 -ge $deadline) {
                    $timedOut = $true
                    $process.Kill($true); $process.WaitForExit()
                    break
                }
            }
            $output = $stdout.GetAwaiter().GetResult() + $stderr.GetAwaiter().GetResult()
            $log = Join-Path $scratch ('command-{0:D2}.log' -f $script:prismCommandNumber)
            [IO.File]::WriteAllText($log, $output)
            if ($timedOut) { throw "Prism build command exceeded fifteen minutes; see $log" }
            if ($process.ExitCode) { throw "Prism build failed; see $log" }
            return $output.Trim()
        } finally { $process.Dispose() }
    }
    $receipt.CMakeVersion = Invoke-PrismBuildCommand -Arguments @('--version')
    $generatorArguments = @('-G', $Generator)
    if ($Generator -ne 'Ninja') { $generatorArguments += @('-A','ARM64','-T','ClangCL') }
    $modes = if ($BuildMode -eq 'All') { @('Probe','Engine') } else { @($BuildMode) }
    $binaries = [Collections.Generic.List[object]]::new()
    $configurations = [Collections.Generic.List[object]]::new()
    foreach ($mode in $modes) {
        $build = Join-Path $scratch ('build-' + $mode.ToLowerInvariant())
        if ($mode -eq 'Probe') {
            $arguments = @('-S',$overlay,'-B',$build) + $generatorArguments + @("-DPRISM_SOURCE_DIR=$source",'-DCMAKE_BUILD_TYPE=Release','-DFETCHCONTENT_FULLY_DISCONNECTED=ON')
            $targets = @('prism-lora-share-tests')
        } else {
            $arguments = @('-S',$source,'-B',$build) + $generatorArguments + @(
                '-DCMAKE_BUILD_TYPE=Release','-DBUILD_SHARED_LIBS=OFF','-DFETCHCONTENT_FULLY_DISCONNECTED=ON',
                '-DLLAMA_BUILD_NUMBER=0',"-DLLAMA_BUILD_COMMIT=$($lock.Commit)+swift-lora-b-share-v1",
                '-DGGML_NATIVE=OFF','-DGGML_BACKEND_DL=OFF',
                '-DGGML_CPU_ALL_VARIANTS=OFF','-DGGML_OPENMP=OFF','-DGGML_CUDA=OFF',
                '-DGGML_METAL=OFF','-DGGML_VULKAN=OFF','-DGGML_OPENCL=OFF','-DGGML_BLAS=OFF',
                '-DGGML_RPC=OFF','-DLLAMA_BUILD_TESTS=OFF','-DLLAMA_BUILD_APP=OFF',
                '-DLLAMA_BUILD_EXAMPLES=OFF','-DLLAMA_BUILD_TOOLS=ON','-DLLAMA_BUILD_SERVER=ON',
                '-DLLAMA_BUILD_UI=OFF','-DLLAMA_USE_PREBUILT_UI=OFF','-DLLAMA_OPENSSL=OFF',
                '-DLLAMA_BUILD_BORINGSSL=OFF','-DLLAMA_BUILD_LIBRESSL=OFF','-DLLAMA_SUBPROCESS=OFF','-DLLAMA_LLGUIDANCE=OFF')
            $targets = @('llama-completion','llama-server')
        }
        $null = Invoke-PrismBuildCommand -Arguments $arguments
        $null = Invoke-PrismBuildCommand -Arguments (@('--build',$build,'--config','Release','--parallel','2','--target') + $targets)
        $cache = Join-Path $build 'CMakeCache.txt'
        $configurations.Add([ordered]@{
            Mode = $mode
            ConfigureArguments = $arguments
            CacheSha256 = (Get-FileHash -LiteralPath $cache -Algorithm SHA256).Hash.ToLowerInvariant()
            Compiler = @(Get-ChildItem -Path (Join-Path $build 'CMakeFiles/*/CMakeCXXCompiler.cmake') | ForEach-Object {
                @(Get-Content -LiteralPath $_.FullName | Where-Object { $_ -match '^set\(CMAKE_CXX_COMPILER(_ID|_VERSION|_ARCHITECTURE_ID|_FRONTEND_VARIANT|_TARGET)? ' })
            })
            Options = @(Get-Content -LiteralPath $cache | Where-Object { $_ -match '^(CMAKE_BUILD_TYPE|BUILD_SHARED_LIBS|FETCHCONTENT_FULLY_DISCONNECTED|GGML_\w+|LLAMA_\w+):BOOL=' })
        })
        foreach ($target in $targets) {
            $extension = if ($IsWindows) { '.exe' } else { '' }
            $builtFiles = @(Get-ChildItem -LiteralPath $build -Recurse -File -Filter ($target + $extension))
            if ($builtFiles.Count -ne 1) { throw "Expected one built $target executable." }
            $binary = $builtFiles[0].FullName
            $binaries.Add([ordered]@{ Target = $target; Path = $binary; Sha256 = (Get-FileHash -LiteralPath $binary -Algorithm SHA256).Hash.ToLowerInvariant() })
        }
    }
    $receipt.Binaries = $binaries.ToArray()
    $receipt.BuildConfigurations = $configurations.ToArray()
    $receipt.Status = 'built'
}
[IO.File]::WriteAllText((Join-Path $scratch 'build-receipt.json'), ($receipt | ConvertTo-Json -Depth 7))
return $receipt
