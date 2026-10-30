#requires -Version 7.6
<# .SYNOPSIS
Builds the reviewed Shell32-free publisher candidate on hosted ARM64 CI without models.
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
    $env:GITHUB_REPOSITORY -cne 'symbiogenesis/SwiftBonsai2Runner' -or $env:GITHUB_SHA -cnotmatch '^[a-f0-9]{40}$') {
    throw 'Headless publisher preparation requires this repository main on hosted native Windows ARM64 CI.'
}
foreach ($name in @('ACTIONS_RUNTIME_TOKEN', 'ACTIONS_ID_TOKEN_REQUEST_TOKEN', 'GITHUB_TOKEN', 'GH_TOKEN', 'ACTIONS_RUNNER_INPUT_TOKEN')) {
    if (Test-Path -LiteralPath ('Env:' + $name)) { throw 'Use a credential-free preparation child before any supervised worker probe.' }
}
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1')
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Native.psm1')
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Tools.psm1')
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$output = Resolve-ExternalPath $OutputDirectory
if (Test-Path -LiteralPath $output) { throw 'Use a fresh external candidate output directory; existing runtime bundles are never modified.' }
[void][IO.Directory]::CreateDirectory($output)
$scratch = Resolve-ExternalPath (Join-Path ([IO.Path]::GetTempPath()) ('Bonsai-HostedHeadless-' + [guid]::NewGuid().ToString('N')))
[void][IO.Directory]::CreateDirectory($scratch)
$manifestPath = Join-Path $repo 'docs/runtime-candidates/headless-ucrt-argv.json'
$manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json -DateKind String
$prior = Get-Content -LiteralPath (Join-Path $repo 'docs/runtime-candidates/headless-ucrt-argv.evidence.json') -Raw | ConvertFrom-Json -DateKind String
$patch = Join-Path $repo ('docs/runtime-candidates/' + $manifest.patch.filename)
if ((Get-Item -LiteralPath $patch).Length -ne $manifest.patch.bytes -or
    (Get-FileHash -LiteralPath $patch -Algorithm SHA256).Hash.ToLowerInvariant() -cne $manifest.patch.sha256) { throw 'Reviewed headless patch identity changed.' }
$receipt = [ordered]@{
    SchemaVersion=1; Scope='hosted-headless-publisher-build-and-model-free-startup-only'
    RunnerRevision=$env:GITHUB_SHA; RunId=$env:GITHUB_RUN_ID; RunAttempt=$env:GITHUB_RUN_ATTEMPT
    ImageOS=$env:ImageOS; ImageVersion=$env:ImageVersion
    Source=$manifest.base; Patch=$manifest.patch; Passed=$false; Phase='download'
    ModelLoaded=$false; NumericalOrPerformanceQualified=$false; ProductionRuntimePinChanged=$false
    WorkerPolicyRelaxed=$false; PackageFiles=@(); Probes=@()
}
$save = { $receipt | ConvertTo-Json -Depth 18 | Set-Content -LiteralPath (Join-Path $output 'build-receipt.json') -Encoding utf8NoBOM }
& $save
function Invoke-CandidateTool {
    [CmdletBinding()]
    [OutputType([pscustomobject])]
    param([string]$Executable, [string[]]$Arguments, [string]$LogName, [int]$TimeoutSeconds=120)
    $result = Invoke-BonsaiNativeProcess -Executable $Executable -ArgumentList $Arguments -WorkingDirectory $scratch -TimeoutSeconds $TimeoutSeconds -OwnDescendants -LogPath (Join-Path $scratch $LogName)
    $logs = Join-Path $output 'logs'
    [void][IO.Directory]::CreateDirectory($logs)
    foreach ($stream in @{stdout=$result.StandardOutput; stderr=$result.StandardError}.GetEnumerator()) {
        $text = [string]$stream.Value
        if ($text.Length -gt 16384) { $text = '[truncated to final 16384 characters]' + "`n" + $text.Substring($text.Length - 16384) }
        [IO.File]::WriteAllText((Join-Path $logs ($LogName + '.' + $stream.Key + '.txt')), $text, [Text.UTF8Encoding]::new($false))
    }
    if ($result.ExitCode -ne 0) { throw ('Hosted candidate tool failed: ' + $LogName) }
    return $result
}
try {
    $archive = Join-Path $scratch 'source.zip'
    Receive-LockedArtifact -Uri $manifest.base.url -Path $archive -Bytes $manifest.base.bytes -Sha256 $manifest.base.sha256 -TimeoutSeconds 180 | Out-Null
    Expand-Archive -LiteralPath $archive -DestinationPath (Join-Path $scratch 'source')
    $source = Join-Path $scratch ('source/llama.cpp-' + $manifest.base.commit)
    foreach ($file in $manifest.files) {
        $path = Join-Path $source $file.path
        if ($file.baseSha256 -and (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant() -cne $file.baseSha256) { throw 'Pristine source differs from reviewed patch base.' }
        if (-not $file.baseSha256 -and (Test-Path -LiteralPath $path)) { throw 'New patch target already exists.' }
    }
    $git = (Get-Command git -CommandType Application | Select-Object -First 1).Source
    Invoke-CandidateTool -Executable $git -Arguments @('-c', 'core.autocrlf=false', '-c', 'core.eol=lf', '-C', $source, 'apply', '--check', $patch) -LogName 'patch-check.txt' | Out-Null
    Invoke-CandidateTool -Executable $git -Arguments @('-c', 'core.autocrlf=false', '-c', 'core.eol=lf', '-C', $source, 'apply', $patch) -LogName 'patch-apply.txt' | Out-Null
    foreach ($file in $manifest.files) {
        if ((Get-FileHash -LiteralPath (Join-Path $source $file.path) -Algorithm SHA256).Hash.ToLowerInvariant() -cne $file.candidateSha256) { throw 'Patched source identity changed.' }
    }
    if ((Get-FileHash -LiteralPath (Join-Path $source 'LICENSE') -Algorithm SHA256).Hash.ToLowerInvariant() -cne $manifest.license.sha256) { throw 'Publisher license identity changed.' }
    $llvmPin = $prior.compiler
    $llvmArchive = Join-Path $scratch $llvmPin.archive
    Receive-LockedArtifact -Uri ('https://github.com/llvm/llvm-project/releases/download/' + $llvmPin.release + '/' + $llvmPin.archive) -Path $llvmArchive -Bytes $llvmPin.archiveBytes -Sha256 $llvmPin.archiveSha256 -TimeoutSeconds 600 | Out-Null
    $llvmRoot = Join-Path $scratch 'llvm'
    [void][IO.Directory]::CreateDirectory($llvmRoot)
    $tar = (Get-Command tar -CommandType Application | Select-Object -First 1).Source
    Invoke-CandidateTool -Executable $tar -Arguments @('-xf', $llvmArchive, '-C', $llvmRoot) -LogName 'llvm-extract.txt' -TimeoutSeconds 300 | Out-Null
    $llvmDirectories = @(Get-ChildItem -LiteralPath $llvmRoot -Directory)
    if ($llvmDirectories.Count -ne 1) { throw 'Expected one pinned LLVM directory.' }
    $llvm = Join-Path $llvmDirectories[0].FullName 'bin'
    $compilerPins = @($llvmPin.executables | Where-Object file -In @('clang-cl.exe', 'lld-link.exe'))
    if ($compilerPins.Count -ne 2) { throw 'Expected the two reviewed native LLVM compiler identities.' }
    foreach ($file in $compilerPins) {
        $path = Join-Path $llvm $file.file
        Assert-Arm64PE $path
        if ((Get-Item -LiteralPath $path).Length -ne $file.bytes -or (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant() -cne $file.sha256) { throw 'Pinned native LLVM compiler identity changed.' }
    }
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
    $vs = (& $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.ARM64 -property installationPath | Out-String).Trim()
    if (-not $vs) { throw 'Installed ARM64 Visual C++ libraries are required.' }
    $vcVersion = (Get-Content -LiteralPath (Join-Path $vs 'VC/Auxiliary/Build/Microsoft.VCToolsVersion.default.txt') -Raw).Trim()
    $vc = Join-Path $vs ('VC/Tools/MSVC/' + $vcVersion)
    $sdk = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits/10'
    $sdkVersions = @(Get-ChildItem -LiteralPath (Join-Path $sdk 'Include') -Directory | Where-Object {
        $_.Name -match '^10\.0\.[0-9]+\.[0-9]+$' -and [IO.File]::Exists((Join-Path $sdk ('Lib/' + $_.Name + '/ucrt/arm64/ucrt.lib'))) -and
        [IO.File]::Exists((Join-Path $sdk ('Lib/' + $_.Name + '/um/arm64/kernel32.lib'))) -and
        [IO.File]::Exists((Join-Path $sdk ('Bin/' + $_.Name + '/arm64/rc.exe'))) -and [IO.File]::Exists((Join-Path $sdk ('Bin/' + $_.Name + '/arm64/mt.exe')))
    } | Sort-Object { [version]$_.Name } -Descending)
    if (-not $sdkVersions.Count) { throw 'An installed ARM64 Windows SDK is required.' }
    $sdkVersion = $sdkVersions[0].Name
    $env:INCLUDE = "$vc/include;$sdk/Include/$sdkVersion/ucrt;$sdk/Include/$sdkVersion/shared;$sdk/Include/$sdkVersion/um"
    $env:LIB = "$vc/lib/arm64;$sdk/Lib/$sdkVersion/ucrt/arm64;$sdk/Lib/$sdkVersion/um/arm64"
    $sdkBin = Join-Path $sdk ('Bin/' + $sdkVersion + '/arm64')
    $env:PATH = $llvm + ';' + (Join-Path $vc 'bin/Hostarm64/arm64') + ';' + $sdkBin + ';' + $env:PATH
    $compiler = Join-Path $llvm 'clang-cl.exe'
    $linker = Join-Path $llvm 'lld-link.exe'
    $cmake = (Get-Command cmake -CommandType Application | Select-Object -First 1).Source
    $python = (Get-Command python -CommandType Application | Select-Object -First 1).Source
    $build = Join-Path $scratch 'build'
    $flags = @('-S', $source, '-B', $build, '-G', 'Ninja', '-DCMAKE_BUILD_TYPE=Release',
        "-DCMAKE_C_COMPILER=$compiler", "-DCMAKE_CXX_COMPILER=$compiler", "-DCMAKE_LINKER=$linker",
        ('-DCMAKE_RC_COMPILER=' + (Join-Path $sdkBin 'rc.exe')), ('-DCMAKE_MT=' + (Join-Path $sdkBin 'mt.exe')),
        '-DCMAKE_C_FLAGS=/clang:-march=armv8.7-a /clang:-fvectorize /clang:-ffp-model=fast /clang:-fno-finite-math-only',
        '-DCMAKE_CXX_FLAGS=/clang:-march=armv8.7-a /clang:-fvectorize /clang:-ffp-model=fast /clang:-fno-finite-math-only',
        '-DBUILD_SHARED_LIBS=ON', '-DGGML_CPU=ON', '-DGGML_NATIVE=OFF', '-DGGML_BACKEND_DL=OFF', '-DGGML_CPU_ALL_VARIANTS=OFF',
        '-DGGML_OPENMP=OFF', '-DGGML_CPU_KLEIDIAI=OFF', '-DGGML_LLAMAFILE=OFF', '-DGGML_RPC=OFF',
        '-DGGML_CUDA=OFF', '-DGGML_VULKAN=OFF', '-DGGML_OPENCL=OFF', '-DGGML_HEXAGON=OFF',
        '-DLLAMA_BUILD_TESTS=ON', '-DGGML_BUILD_TESTS=OFF', '-DLLAMA_BUILD_EXAMPLES=OFF', '-DLLAMA_BUILD_TOOLS=ON',
        '-DLLAMA_BUILD_SERVER=ON', '-DLLAMA_BUILD_APP=OFF', '-DLLAMA_BUILD_UI=OFF', '-DLLAMA_OPENSSL=OFF', '-DLLAMA_BUILD_BORINGSSL=OFF',
        '-DLLAMA_SUBPROCESS=OFF', '-DLLAMA_LLGUIDANCE=OFF', '-DLLAMA_BUILD_COMMIT=73bfe6c', '-DLLAMA_BUILD_NUMBER=10787')
    $receipt.Compiler = @{ LLVM=$llvmPin; VisualCppToolsVersion=$vcVersion; WindowsSdkVersion=$sdkVersion
        ClangVersion=(Invoke-CandidateTool -Executable $compiler -Arguments @('--version') -LogName 'compiler-version.txt').StandardOutput.Trim()
        CMakeVersion=(Invoke-CandidateTool -Executable $cmake -Arguments @('--version') -LogName 'cmake-version.txt').StandardOutput.Trim() }
    $receipt.Supervisor = @{ SourceId=$env:BONSAI_TOOLS_SOURCE_ID; Sha256=(Get-FileHash -LiteralPath $env:BONSAI_TOOLS_PATH -Algorithm SHA256).Hash.ToLowerInvariant()
        WorkerPolicy='ordinary-required-headless'; MemoryLimitBytes=536870912; TimeoutSeconds=10; OutputLimitBytesPerStream=65536 }
    $receipt.CMakeArguments=$flags; $receipt.BuildTargets=@('llama-completion', 'llama-server', 'llama-bench', 'test-headless-argv')
    $receipt.Phase='configure'; & $save
    Invoke-CandidateTool -Executable $cmake -Arguments $flags -LogName 'configure.txt' -TimeoutSeconds 180 | Out-Null
    $receipt.Phase='compile'; & $save
    Invoke-CandidateTool -Executable $cmake -Arguments (@('--build', $build, '--parallel', '4', '--target') + $receipt.BuildTargets) -LogName 'compile.txt' -TimeoutSeconds 1500 | Out-Null
    foreach ($file in $manifest.files) {
        if ((Get-FileHash -LiteralPath (Join-Path $source $file.path) -Algorithm SHA256).Hash.ToLowerInvariant() -cne $file.candidateSha256) { throw 'Reviewed source changed during compilation.' }
    }
    $receipt.CMakeCacheSha256=(Get-FileHash -LiteralPath (Join-Path $build 'CMakeCache.txt') -Algorithm SHA256).Hash.ToLowerInvariant()
    $bundle = Join-Path $output 'bundle'
    [void][IO.Directory]::CreateDirectory($bundle)
    foreach ($file in Get-ChildItem -LiteralPath (Join-Path $build 'bin') -File) {
        if ($file.Extension -ine '.dll' -and $file.Name -cnotin @('llama-completion.exe', 'llama-server.exe', 'llama-bench.exe', 'test-headless-argv.exe')) { continue }
        if ($file.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Redirected candidate output.' }
        Assert-Arm64PE $file.FullName
        Copy-Item -LiteralPath $file.FullName -Destination (Join-Path $bundle $file.Name)
    }
    foreach ($name in @('llama-completion.exe', 'llama-server.exe', 'llama-bench.exe', 'test-headless-argv.exe')) {
        if (-not [IO.File]::Exists((Join-Path $bundle $name))) { throw 'A required candidate comparator or fixture is missing.' }
    }
    Copy-Item -LiteralPath (Join-Path $source 'LICENSE') -Destination (Join-Path $bundle 'LICENSE')
    $receipt.Phase='static-import-closure'; & $save
    Invoke-CandidateTool -Executable $python -Arguments @('-B', (Join-Path $repo 'tools/runtime-candidates/pe_closure.py'), '--bundle', $bundle, '--system', (Join-Path $env:WINDIR 'System32'), '--output', (Join-Path $output 'imports.json')) -LogName 'static-imports.txt' | Out-Null
    $files = @(Get-ChildItem -LiteralPath $bundle -File | ForEach-Object { [ordered]@{ Name=$_.Name; Bytes=$_.Length; Sha256=(Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant() } })
    $receipt.PackageFiles=$files
    $artifacts = @($files | ForEach-Object { @{path=(Join-Path $bundle $_.Name); bytes=$_.Bytes; sha256=$_.Sha256} })
    $probes = [Collections.Generic.List[object]]::new()
    $receipt.Phase='required-headless-model-free-probes'; & $save
    $cases = @(@{name='completion-version';exe='llama-completion.exe';arguments=@('--version')},
        @{name='server-version';exe='llama-server.exe';arguments=@('--version')}, @{name='bench-help';exe='llama-bench.exe';arguments=@('--help')})
    [IO.File]::WriteAllText((Join-Path $scratch 'literal-wildcard-fixture.gguf'), 'Synthetic argv wildcard sentinel; not a model.')
    $vectors = @('', '日本語 🪴 café', 'C:\notes dir\ "quoted" tail\\', "line1`tline2`nline3", '*.gguf', '  leading and trailing  ', 'slashes\\\"quote')
    for ($index=0; $index -lt $vectors.Count; $index++) { $cases += @{name=('argv-' + $index);exe='test-headless-argv.exe';arguments=@('--model', '模型/test.gguf', '--seed', [string]$index, '--prompt', $vectors[$index])} }
    foreach ($case in $cases) {
        $evidence = Join-Path $output ('probes/' + $case.name)
        [void][IO.Directory]::CreateDirectory($evidence)
        $result = Invoke-BonsaiTool -Command completion -TimeoutSeconds 30 -Request @{
            schemaVersion=1; executable=(Join-Path $bundle $case.exe); arguments=$case.arguments
            workingDirectory=$scratch; evidenceDirectory=$evidence; artifacts=$artifacts
            memoryLimitBytes=536870912; requiredAvailableBytes=536870912; physicalReserveBytes=0
            # ciOnly also enables model timing/output capture. These exact
            # hosted-guarded invocations contain no model or timing output.
            timeoutSeconds=10; stdoutLimitBytes=65536; stderrLimitBytes=65536; requireNonemptyStdout=$false; ciOnly=$false
        }
        $probes.Add(@{Name=$case.name; Arguments=$case.arguments; Result=$result; ModelLoaded=$false})
        $receipt.Probes=@($probes); & $save
        if ($result.ExitCode -ne 0) { throw 'A required headless model-free probe failed.' }
    }
    $receipt.Phase='completed'; $receipt.Passed=$true; & $save
} finally { & $save }
[pscustomobject]@{ Status='hosted-headless-candidate-built'; OutputDirectory=$output; ModelLoaded=$false; RuntimePinChanged=$false }
