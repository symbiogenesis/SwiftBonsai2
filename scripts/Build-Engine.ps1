#requires -Version 7.6
<##
Build the locked CPU engine on Windows ARM64; never runs inference or benchmarks.
Sources checked before implementation:
https://github.com/ggml-org/llama.cpp/blob/620d2225cc436ab0b001e3200e590e3b286ef9ab/docs/build.md
https://github.com/ggml-org/llama.cpp/blob/620d2225cc436ab0b001e3200e590e3b286ef9ab/ggml/src/ggml-cpu/CMakeLists.txt#L109
https://cmake.org/cmake/help/latest/generator/Visual%20Studio%2018%202026.html
MSVC ARM64 requires GGML_NATIVE=ON: upstream probes this host's supported ISA.
Remote UI, TLS dependency fetching, OpenMP, subprocess tools and other backends are disabled.
The resulting server has no full embedded chat UI; those assets need separate pinning.
##>
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$DataDirectory,
    [Parameter(Mandatory)][string]$GitPath,
    [Parameter(Mandatory)][string]$CMakePath,
    [ValidateSet('Visual Studio 18 2026', 'Visual Studio 17 2022')][string]$Generator = 'Visual Studio 18 2026',
    [ValidateRange(1, 12)][int]$Parallel = 2,
    [switch]$FetchSource
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if (-not $IsWindows -or [Runtime.InteropServices.RuntimeInformation]::OSArchitecture -ne 'Arm64' -or [Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture -ne 'Arm64') { throw 'Build in native ARM64 PowerShell on a Windows ARM64 host.' }
function Full-Path([string]$Path) {
    if ($Path -match '%[^%]+%' -or $Path -notmatch '^[A-Za-z]:[\\/]' -or $Path.Substring(2).Contains(':') -or -not [IO.Path]::IsPathFullyQualified($Path)) { throw "Use an absolute local drive path without device syntax, streams or unresolved variables: $Path" }
    [IO.Path]::GetFullPath($Path).TrimEnd('\', '/')
}
function Assert-PlainPath([string]$Path) {
    for ($item = $Path; $item; $item = [IO.Path]::GetDirectoryName($item)) {
        if ((Test-Path -LiteralPath $item) -and ((Get-Item -Force -LiteralPath $item).Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw "Reparse paths are unsupported: $item" }
    }
}
$DataDirectory = Full-Path $DataDirectory
if ($DataDirectory -eq [IO.Path]::GetPathRoot($DataDirectory).TrimEnd('\')) { throw 'Choose a dedicated data directory, not a drive root.' }
Assert-PlainPath $DataDirectory
for ($item = $DataDirectory; $item; $item = [IO.Path]::GetDirectoryName($item)) {
    if ((Split-Path $item -Leaf) -eq '.git' -or (Test-Path -LiteralPath (Join-Path $item '.git'))) { throw 'DataDirectory must be outside Git checkouts and Git metadata.' }
}
foreach ($name in 'GitPath', 'CMakePath') {
    $path = Full-Path (Get-Variable $name -ValueOnly)
    if (-not (Test-Path -LiteralPath $path -PathType Leaf) -or [IO.Path]::GetExtension($path) -ne '.exe') { throw "$name must name an existing executable." }
    Set-Variable $name $path
}
$injected = Get-ChildItem Env: | Where-Object Name -Match '^(GIT_(DIR|WORK_TREE|COMMON_DIR|INDEX_FILE|OBJECT_DIRECTORY|ALTERNATE_OBJECT_DIRECTORIES|CONFIG.*)|CMAKE_.*|CC|CXX|CL|_CL_|LINK|CFLAGS|CXXFLAGS|LDFLAGS)$'
if ($injected) { throw ('Build override environment is unsupported; no environment was changed: ' + ($injected.Name -join ', ')) }
$lock = Get-Content -Raw -LiteralPath (Join-Path $PSScriptRoot '../config/runtime.lock.json') | ConvertFrom-Json -DateKind String
$commit = [string]$lock.runtime.commit
$repository = [string]$lock.runtime.repository
if ($commit -cnotmatch '^[0-9a-f]{40}$' -or $repository -cne 'https://github.com/ggml-org/llama.cpp.git') { throw 'Unsupported runtime lock identity.' }
$scratch = Join-Path ([IO.Path]::GetTempPath()) ('swiftbonsai-build-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $scratch | Out-Null
$emptyHooks = New-Item -ItemType Directory -Path (Join-Path $scratch 'empty-hooks')
$script:commandNumber = 0
function Invoke-Native([string]$Executable, [string[]]$Arguments, [int]$TimeoutSeconds = 3600) {
    $script:commandNumber++
    $info = [Diagnostics.ProcessStartInfo]::new($Executable)
    $info.WorkingDirectory = $scratch
    $info.UseShellExecute = $false; $info.CreateNoWindow = $true
    $info.RedirectStandardOutput = $true; $info.RedirectStandardError = $true
    foreach ($argument in $Arguments) { $info.ArgumentList.Add($argument) }
    $process = [Diagnostics.Process]::new(); $process.StartInfo = $info
    try {
        if (-not $process.Start()) { throw "Could not start $Executable" }
        $stdout = $process.StandardOutput.ReadToEndAsync(); $stderr = $process.StandardError.ReadToEndAsync()
        if (-not $process.WaitForExit($TimeoutSeconds * 1000)) { $process.Kill($true); $process.WaitForExit(); throw "Command timed out: $Executable" }
        $output = $stdout.GetAwaiter().GetResult() + $stderr.GetAwaiter().GetResult()
        $log = Join-Path $scratch ('command-{0:D2}.log' -f $script:commandNumber)
        [IO.File]::WriteAllText($log, $output)
        if ($process.ExitCode -ne 0) { throw "Command failed ($($process.ExitCode)): $Executable; see $log" }
        $output.Trim()
    } finally { $process.Dispose() }
}
function Invoke-Git([string[]]$Arguments) { Invoke-Native $GitPath (@('-c', "core.hooksPath=$($emptyHooks.FullName)", '-c', 'submodule.recurse=false', '-c', 'core.longpaths=true') + $Arguments) }
$gitVersion = Invoke-Git @('--version')
$cmakeVersion = Invoke-Native $CMakePath @('--version')
$capabilities = Invoke-Native $CMakePath @('-E', 'capabilities') | ConvertFrom-Json -DateKind String
if ($Generator -notin $capabilities.generators.name) { throw "Installed CMake does not support $Generator." }
$source = Join-Path $DataDirectory -ChildPath 'sources', $commit
$build = Join-Path $DataDirectory -ChildPath 'builds', $commit, 'cpu'
Assert-PlainPath $source; Assert-PlainPath $build
if (-not (Test-Path -LiteralPath $source)) {
    if (-not $FetchSource) { throw 'Source is absent. Use -FetchSource to explicitly fetch the locked commit.' }
    $null = New-Item -ItemType Directory -Path (Split-Path $source) -Force
    $null = Invoke-Git @('init', '--quiet', "--template=$($emptyHooks.FullName)", $source)
    $null = Invoke-Git @('-C', $source, 'fetch', '--no-tags', '--depth=1', $repository, $commit)
    $null = Invoke-Git @('-C', $source, 'checkout', '--quiet', '--detach', $commit)
}
if (-not (Test-Path -LiteralPath (Join-Path $source '.git') -PathType Container)) { throw 'Existing source must be a dedicated Git clone; it will not be overwritten.' }
if ((Full-Path (Invoke-Git @('-C', $source, 'rev-parse', '--show-toplevel'))) -ne $source -or (Invoke-Git @('-C', $source, 'rev-parse', 'HEAD')) -cne $commit) { throw 'Existing source is not the exact locked checkout.' }
if (Invoke-Git @('-C', $source, 'status', '--porcelain', '--untracked-files=all', '--ignored=matching')) { throw 'Existing source contains changes or extra files; it will not be modified.' }
# Only the server target is built. LLAMA_BUILD_TOOLS=ON is required to define it.
$options = @('GGML_CPU=ON', 'GGML_NATIVE=ON', 'GGML_BACKEND_DL=OFF', 'GGML_CPU_ALL_VARIANTS=OFF',
    'GGML_OPENMP=OFF', 'GGML_OPENMP_FETCH=OFF', 'GGML_CPU_KLEIDIAI=OFF', 'GGML_BLAS=OFF', 'GGML_LLAMAFILE=OFF',
    'GGML_CUDA=OFF', 'GGML_HIP=OFF', 'GGML_MUSA=OFF', 'GGML_CANN=OFF', 'GGML_VULKAN=OFF', 'GGML_OPENCL=OFF', 'GGML_HEXAGON=OFF', 'GGML_RPC=OFF',
    'GGML_SYCL=OFF', 'GGML_METAL=OFF', 'GGML_OPENVINO=OFF', 'GGML_WEBGPU=OFF', 'GGML_BUILD_TESTS=OFF',
    'GGML_ACCELERATE=OFF', 'GGML_ZDNN=OFF', 'GGML_VIRTGPU=OFF', 'GGML_VIRTGPU_BACKEND=OFF', 'GGML_ET=OFF', 'GGML_ET_SYSEMU=OFF', 'GGML_ZENDNN=OFF',
    'GGML_BUILD_EXAMPLES=OFF', 'GGML_CCACHE=OFF', 'LLAMA_BUILD_COMMON=ON', 'LLAMA_BUILD_TOOLS=ON',
    'LLAMA_BUILD_SERVER=ON', 'LLAMA_BUILD_APP=OFF', 'LLAMA_BUILD_TESTS=OFF', 'LLAMA_BUILD_EXAMPLES=OFF',
    'LLAMA_BUILD_UI=OFF', 'LLAMA_USE_PREBUILT_UI=OFF', 'LLAMA_OPENSSL=OFF', 'LLAMA_BUILD_BORINGSSL=OFF',
    'LLAMA_BUILD_LIBRESSL=OFF', 'LLAMA_SUBPROCESS=OFF', 'LLAMA_LLGUIDANCE=OFF', 'FETCHCONTENT_FULLY_DISCONNECTED=ON')
$configuration = [ordered]@{ repository = $repository; commit = $commit; source = $source; generator = $Generator; platform = 'ARM64'; configuration = 'Release'; options = $options }
$identity = $configuration | ConvertTo-Json -Depth 4 -Compress
$marker = Join-Path $build 'swiftbonsai-build.json'
if ((Test-Path -LiteralPath $build) -and (Get-ChildItem -Force -LiteralPath $build)) {
    if (-not (Test-Path -LiteralPath $marker) -or (Get-Content -Raw -LiteralPath $marker).Trim() -cne $identity) { throw 'Build directory has no matching ownership/configuration record; choose a fresh data directory.' }
}
$null = New-Item -ItemType Directory -Path $build -Force
[IO.File]::WriteAllText($marker, $identity)
$configure = @('-S', $source, '-B', $build, '-G', $Generator, '-A', 'ARM64', "-DGIT_EXECUTABLE=$GitPath") + @($options | ForEach-Object { '-D' + $_ })
$null = Invoke-Native $CMakePath $configure
$cache = Get-Content -Raw -LiteralPath (Join-Path $build 'CMakeCache.txt')
if ($cache -notmatch '(?m)^CMAKE_GENERATOR_PLATFORM:INTERNAL=ARM64\r?$' -or $cache -notmatch ('(?m)^CMAKE_GENERATOR:INTERNAL=' + [regex]::Escape($Generator) + '\r?$')) { throw 'Configured generator/platform mismatch.' }
$null = Invoke-Native $CMakePath @('--build', $build, '--config', 'Release', '--target', 'llama-server', '--parallel', [string]$Parallel)
if (Invoke-Git @('-C', $source, 'status', '--porcelain', '--untracked-files=all', '--ignored=matching')) { throw 'Build changed the source checkout; inspect it before reuse.' }
$engine = Join-Path $build -ChildPath 'bin', 'Release', 'llama-server.exe'
$reader = [IO.BinaryReader]::new([IO.File]::OpenRead($engine))
try {
    if ($reader.ReadUInt16() -ne 0x5A4D -or $reader.BaseStream.Length -lt 64) { throw 'Output is not a PE executable.' }
    $reader.BaseStream.Position = 0x3C; $offset = $reader.ReadInt32()
    if ($offset -lt 64 -or $offset -gt $reader.BaseStream.Length - 6) { throw 'Invalid PE header offset.' }
    $reader.BaseStream.Position = $offset
    if ($reader.ReadUInt32() -ne 0x4550 -or $reader.ReadUInt16() -ne 0xAA64) { throw 'Output is not native ARM64 PE.' }
} finally { $reader.Dispose() }
$engineVersion = Invoke-Native $engine @('--version') 30
$reportedCommit = [regex]::Match($engineVersion, '\bcommit ([a-f0-9]{7,40})\)').Groups[1].Value
if (-not $reportedCommit -or -not $commit.StartsWith($reportedCommit)) { throw 'Engine version does not identify the locked commit.' }
$compilerFiles = @(Get-ChildItem -Path (Join-Path $build 'CMakeFiles/*/CMakeCXXCompiler.cmake') | ForEach-Object { @{ path = $_.FullName; content = (Get-Content -Raw -LiteralPath $_.FullName) } })
$receipt = [ordered]@{ schemaVersion = 1; builtUtc = [DateTime]::UtcNow.ToString('o'); configuration = $configuration; git = @{ path = $GitPath; version = $gitVersion }; cmake = @{ path = $CMakePath; version = $cmakeVersion }; compiler = $compilerFiles; engine = @{ path = $engine; sha256 = (Get-FileHash -LiteralPath $engine -Algorithm SHA256).Hash.ToLowerInvariant(); version = $engineVersion }; cacheSha256 = (Get-FileHash -LiteralPath (Join-Path $build 'CMakeCache.txt') -Algorithm SHA256).Hash.ToLowerInvariant(); logs = $scratch; qualification = 'build-and-version-only; inference, UI and performance unqualified' }
$receiptPath = Join-Path $build ('receipt-' + [guid]::NewGuid().ToString('N') + '.json')
$receipt | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $receiptPath -Encoding utf8
Write-Output "Built ARM64 CPU Release server: $engine"
Write-Output "Build receipt: $receiptPath"
