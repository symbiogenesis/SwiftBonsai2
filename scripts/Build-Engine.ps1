#requires -Version 7.6
<##
Build the locked CPU engine on Windows ARM64; never runs inference or benchmarks.
Sources checked before implementation:
https://github.com/ggml-org/llama.cpp/blob/620d2225cc436ab0b001e3200e590e3b286ef9ab/docs/build.md
https://github.com/ggml-org/llama.cpp/blob/620d2225cc436ab0b001e3200e590e3b286ef9ab/ggml/src/ggml-cpu/CMakeLists.txt#L109
https://cmake.org/cmake/help/latest/generator/Visual%20Studio%2018%202026.html
https://devblogs.microsoft.com/cppblog/improved-parallelism-in-msbuild/
MSVC ARM64 requires GGML_NATIVE=ON: upstream probes this host's supported ISA.
Remote UI, TLS dependency fetching, OpenMP, subprocess tools and other backends are disabled.
The resulting server has no full embedded chat UI; those assets need separate pinning.
##>
[CmdletBinding()]
[OutputType([string])]
param(
    [Parameter(Mandatory)][string]$DataDirectory,
    [Parameter(Mandatory)][string]$GitPath,
    [Parameter(Mandatory)][string]$CMakePath,
    [ValidateSet('Visual Studio 18 2026', 'Visual Studio 17 2022')][string]$Generator = 'Visual Studio 18 2026',
    # Zero chooses a bounded CPU/memory budget; 1..12 explicitly overrides it.
    [ValidateRange(0, 12)][int]$Parallel = 0,
    [switch]$FetchSource
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Native.psm1')
if (-not $IsWindows -or [Runtime.InteropServices.RuntimeInformation]::OSArchitecture -ne 'Arm64' -or [Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture -ne 'Arm64') { throw 'Build in native ARM64 PowerShell on a Windows ARM64 host.' }
function Resolve-BuildParallelism {
    [CmdletBinding()]
    [OutputType([int])]
    param([int]$Requested, [int]$ProcessorCount, [long]$AvailableBytes)
    if ($Requested -lt 0 -or $Requested -gt 12 -or $ProcessorCount -lt 1 -or $AvailableBytes -lt -1) { throw 'Invalid build parallelism input.' }
    if ($Requested -gt 0) { return $Requested }
    # A starting budget, not a measured optimum: retain two CPU threads and 2 GiB
    # for interactive work, then allow at most one compiler per remaining GiB.
    $cpuBudget = [Math]::Min(8, [Math]::Max(1, $ProcessorCount - 2))
    if ($AvailableBytes -eq -1) { return [Math]::Min(2, $cpuBudget) }
    $memoryBudget = [Math]::Max(1.0, [Math]::Floor(($AvailableBytes - 2GB) / 1GB))
    [int][Math]::Min([double]$cpuBudget, $memoryBudget)
}
function Get-EngineBuildArgument {
    [CmdletBinding()]
    [OutputType([string], [array])]
    param([string]$BuildDirectory, [int]$Jobs)
    if ($Jobs -lt 1 -or $Jobs -gt 12) { throw 'Build jobs must be between 1 and 12.' }
    # CMake's --parallel limits MSBuild project workers. The pinned upstream
    # enables MultiToolTask, whose compiler limit otherwise defaults to all CPUs.
    # Share the same limit across projects instead of multiplying worker counts.
    @('--build', $BuildDirectory, '--config', 'Release', '--target', 'llama-server', '--parallel', [string]$Jobs,
        '--', '/p:UseMultiToolTask=true', '/p:EnforceProcessCountAcrossBuilds=true', "/p:CL_MPCount=$Jobs", "/p:MultiProcMaxCount=$Jobs")
}
function Resolve-BuildPath {
    [CmdletBinding()]
    [OutputType([string])]
    param([string]$Path)
    if ($Path -match '%[^%]+%' -or $Path -notmatch '^[A-Za-z]:[\\/]' -or $Path.Substring(2).Contains(':') -or -not [IO.Path]::IsPathFullyQualified($Path)) { throw "Use an absolute local drive path without device syntax, streams or unresolved variables: $Path" }
    [IO.Path]::GetFullPath($Path).TrimEnd('\', '/')
}
function Assert-PlainPath {
    [CmdletBinding()]
    [OutputType([void])]
    param([string]$Path)
    for ($item = $Path; $item; $item = [IO.Path]::GetDirectoryName($item)) {
        if ((Test-Path -LiteralPath $item) -and ((Get-Item -Force -LiteralPath $item).Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw "Reparse paths are unsupported: $item" }
    }
}
$DataDirectory = Resolve-BuildPath $DataDirectory
if ($DataDirectory -eq [IO.Path]::GetPathRoot($DataDirectory).TrimEnd('\')) { throw 'Choose a dedicated data directory, not a drive root.' }
Assert-PlainPath $DataDirectory
for ($item = $DataDirectory; $item; $item = [IO.Path]::GetDirectoryName($item)) {
    if ((Split-Path $item -Leaf) -eq '.git' -or (Test-Path -LiteralPath (Join-Path $item '.git'))) { throw 'DataDirectory must be outside Git checkouts and Git metadata.' }
}
foreach ($name in 'GitPath', 'CMakePath') {
    $path = Resolve-BuildPath (Get-Variable $name -ValueOnly)
    if (-not (Test-Path -LiteralPath $path -PathType Leaf) -or [IO.Path]::GetExtension($path) -ne '.exe') { throw "$name must name an existing executable." }
    Set-Variable $name $path
}
$injected = Get-ChildItem Env: | Where-Object Name -Match '^(GIT_(DIR|WORK_TREE|COMMON_DIR|INDEX_FILE|OBJECT_DIRECTORY|ALTERNATE_OBJECT_DIRECTORIES|CONFIG.*)|CMAKE_.*|CC|CXX|CL|_CL_|LINK|CFLAGS|CXXFLAGS|LDFLAGS)$'
if ($injected) { throw ('Build override environment is unsupported; no environment was changed: ' + ($injected.Name -join ', ')) }
$availableBytes = -1L
if ($Parallel -eq 0) {
    try {
        $osMemory = Get-CimInstance -ClassName Win32_OperatingSystem -Property FreePhysicalMemory -ErrorAction Stop
        if ($null -ne $osMemory.FreePhysicalMemory -and $osMemory.FreePhysicalMemory -ge 0) { $availableBytes = [long]$osMemory.FreePhysicalMemory * 1KB }
    } catch { Write-Verbose 'Available memory query failed; automatic parallelism uses the conservative fallback.' }
}
$parallelJobs = Resolve-BuildParallelism -Requested $Parallel -ProcessorCount ([Environment]::ProcessorCount) -AvailableBytes $availableBytes
Write-Output "Engine build parallel jobs: $parallelJobs (project and shared compiler limits)."
$lock = Get-Content -Raw -LiteralPath (Join-Path $PSScriptRoot '../config/runtime.lock.json') | ConvertFrom-Json -DateKind String
$commit = [string]$lock.runtime.commit
$repository = [string]$lock.runtime.repository
if ($commit -cnotmatch '^[0-9a-f]{40}$' -or $repository -cne 'https://github.com/ggml-org/llama.cpp.git') { throw 'Unsupported runtime lock identity.' }
$scratch = Join-Path ([IO.Path]::GetTempPath()) ('swiftbonsai-build-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $scratch | Out-Null
$emptyHooks = New-Item -ItemType Directory -Path (Join-Path $scratch 'empty-hooks')
$script:commandNumber = 0
function Invoke-Native {
    [CmdletBinding()]
    [OutputType([string])]
    param([string]$Executable, [string[]]$Arguments, [int]$TimeoutSeconds = 3600)
    $script:commandNumber++
    $log = Join-Path $scratch ('command-{0:D2}.log' -f $script:commandNumber)
    $result = Invoke-BonsaiNativeProcess -Executable $Executable -ArgumentList $Arguments -WorkingDirectory $scratch -TimeoutSeconds $TimeoutSeconds -LogPath $log
    if ($result.ExitCode -ne 0) { throw "Command failed ($($result.ExitCode)): $Executable; see $log" }
    ($result.StandardOutput + $result.StandardError).Trim()
}
function Invoke-Git {
    [CmdletBinding()]
    [OutputType([string])]
    param([string[]]$Arguments)
    Invoke-Native $GitPath (@('-c', "core.hooksPath=$($emptyHooks.FullName)", '-c', 'submodule.recurse=false', '-c', 'core.longpaths=true') + $Arguments)
}
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
if ((Resolve-BuildPath (Invoke-Git @('-C', $source, 'rev-parse', '--show-toplevel'))) -ne $source -or (Invoke-Git @('-C', $source, 'rev-parse', 'HEAD')) -cne $commit) { throw 'Existing source is not the exact locked checkout.' }
if (Invoke-Git @('-C', $source, 'status', '--porcelain', '--untracked-files=all', '--ignored=matching')) { throw 'Existing source contains changes or extra files; it will not be modified.' }
# Only the server target is built. LLAMA_BUILD_TOOLS=ON is required to define it.
$options = @('GGML_CPU=ON', 'GGML_NATIVE=ON', 'GGML_BACKEND_DL=OFF', 'GGML_CPU_ALL_VARIANTS=OFF',
    'CMAKE_C_STANDARD=11', 'CMAKE_C_STANDARD_REQUIRED=ON', 'CMAKE_C_EXTENSIONS=OFF',
    'CMAKE_CXX_STANDARD=17', 'CMAKE_CXX_STANDARD_REQUIRED=ON', 'CMAKE_CXX_EXTENSIONS=OFF',
    # The immutable upstream source currently emits MSVC C4244 in llama-graph.cpp.
    # Keep its diagnostics visible; first-party warning-as-error policies apply
    # to repository-owned Rust, PowerShell and C#, not this external dependency.
    'CMAKE_COMPILE_WARNING_AS_ERROR=OFF', 'LLAMA_ALL_WARNINGS=ON', 'LLAMA_FATAL_WARNINGS=OFF',
    'CMAKE_CXX_FLAGS_INIT=/permissive- /sdl', 'CMAKE_C_FLAGS_INIT=/sdl',
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
$null = Invoke-Native $CMakePath (Get-EngineBuildArgument $build $parallelJobs)
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
$engineVersion = Invoke-Native -Executable $engine -Arguments @('--version') -TimeoutSeconds 30
$reportedCommit = [regex]::Match($engineVersion, '\bcommit ([a-f0-9]{7,40})\)').Groups[1].Value
if (-not $reportedCommit -or -not $commit.StartsWith($reportedCommit)) { throw 'Engine version does not identify the locked commit.' }
$compilerFiles = @(Get-ChildItem -Path (Join-Path $build 'CMakeFiles/*/CMakeCXXCompiler.cmake') | ForEach-Object { @{ path = $_.FullName; content = (Get-Content -Raw -LiteralPath $_.FullName) } })
$receipt = [ordered]@{ schemaVersion = 1; builtUtc = [DateTime]::UtcNow.ToString('o'); configuration = $configuration; scheduling = @{ requestedParallel = $Parallel; parallelJobs = $parallelJobs; availableMemoryBytes = $availableBytes }; git = @{ path = $GitPath; version = $gitVersion }; cmake = @{ path = $CMakePath; version = $cmakeVersion }; compiler = $compilerFiles; engine = @{ path = $engine; sha256 = (Get-FileHash -LiteralPath $engine -Algorithm SHA256).Hash.ToLowerInvariant(); version = $engineVersion }; cacheSha256 = (Get-FileHash -LiteralPath (Join-Path $build 'CMakeCache.txt') -Algorithm SHA256).Hash.ToLowerInvariant(); logs = $scratch; qualification = 'build-and-version-only; inference, UI and performance unqualified' }
$receiptPath = Join-Path $build ('receipt-' + [guid]::NewGuid().ToString('N') + '.json')
$receipt | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $receiptPath -Encoding utf8
Write-Output "Built ARM64 CPU Release server: $engine"
Write-Output "Build receipt: $receiptPath"
