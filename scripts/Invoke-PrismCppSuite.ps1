#requires -Version 7.6
<#
.SYNOPSIS
Imports unchanged model-free Prism C++ correctness suites from verified source.
.DESCRIPTION
The caller must extract the archive verified against tools/prism/source.lock.json.
Selected original translation units are rechecked against the pinned catalog.
Only the explicit allowlist is built and run: upstream CTest also registers
benchmarks and model downloads and must not be executed without filtering.
Full execution is CI-only. BuildOnly permits local builds; Case permits one
focused local original suite. Build, start, and timeout failures are fatal;
ordinary assertion/test failures become red reference records with retained logs.
#>
[CmdletBinding()]
[OutputType([Collections.IDictionary])]
param(
    [Parameter(Mandatory)][string]$SourceDirectory,
    [Parameter(Mandatory)][string]$EvidenceDirectory,
    [string]$CMakePath,
    [ValidateSet('Ninja','Visual Studio 18 2026','Visual Studio 17 2022')][string]$Generator = 'Ninja',
    [ValidateRange(1,12)][int]$Parallel = 2,
    [string]$CompilerLauncher,
    [string]$Case,
    [switch]$BuildOnly
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1') -Force
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Native.psm1')

$source = Resolve-ExternalPath -Path $SourceDirectory
$evidence = Resolve-ExternalPath -Path $EvidenceDirectory
if (-not [IO.Directory]::Exists($source)) { throw 'The verified source directory is missing.' }
if ([IO.Directory]::Exists($evidence) -or [IO.File]::Exists($evidence)) { throw 'Use a fresh external evidence directory.' }
if ($evidence.StartsWith($source.TrimEnd([IO.Path]::DirectorySeparatorChar) + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
    throw 'Evidence must be separate from the verified source tree.'
}
$overlay = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../tools/prism'))
$lock = Get-Content -LiteralPath (Join-Path $overlay 'source.lock.json') -Raw | ConvertFrom-Json -DateKind String
$catalog = Get-Content -LiteralPath (Join-Path $overlay 'upstream/catalog.json') -Raw | ConvertFrom-Json -DateKind String
if ($catalog.SchemaVersion -ne 1 -or $catalog.UpstreamCommit -cne $lock.Commit -or $catalog.ArchiveSha256 -cne $lock.ArchiveSha256) {
    throw 'The upstream catalog does not identify the pinned source archive.'
}
if ((Get-FileHash -LiteralPath (Join-Path $source 'CMakeLists.txt') -Algorithm SHA256).Hash.ToLowerInvariant() -cne $lock.CMakeSourceSha256) {
    throw 'The original upstream CMake source changed; use an unmodified extraction.'
}

# All are original, argument-free correctness suites. In particular exclude
# sampling, barrier, chat-peg-parser, peg-parser and quantize-perf: their
# default paths execute benchmarks. Model/download-dependent targets stay red
# in the catalog until an explicit qualified adapter is added.
$targets = @(
    'test-alloc', 'test-arg-parser', 'test-batch-alloc', 'test-col2im-1d',
    'test-dfly-fusion', 'test-gguf', 'test-grammar-integration',
    'test-grammar-parser', 'test-jinja', 'test-llama-grammar', 'test-log',
    'test-moe-weighted-reduce', 'test-mtp-catchup-batch',
    'test-ptq1_0-cuda-dot', 'test-quantize-fns', 'test-reasoning-budget',
    'test-rope', 'test-unicode'
)
if ($Case) {
    if ($targets -cnotcontains $Case) { throw 'Case must name exactly one allowlisted original C++ suite.' }
    $targets = @($Case)
} elseif (-not $BuildOnly -and $env:GITHUB_ACTIONS -ne 'true') {
    throw 'Complete upstream suite selections run only in CI. Use BuildOnly or one focused Case locally.'
}
foreach ($target in $targets) {
    $id = 'tests/' + $target + '.cpp'
    $entries = @($catalog.Cases | Where-Object { $_.Id -ceq $id })
    if ($entries.Count -ne 1 -or $entries[0].SourceSha256 -cnotmatch '^[a-f0-9]{64}$') {
        throw "The pinned catalog must contain exactly one source hash for $id."
    }
    if ((Get-FileHash -LiteralPath (Join-Path $source $id) -Algorithm SHA256).Hash.ToLowerInvariant() -cne $entries[0].SourceSha256) {
        throw "Original upstream test source hash mismatch: $id"
    }
}
if (-not $CMakePath) { $CMakePath = (Get-Command -Name cmake -CommandType Application -ErrorAction Stop).Source }
if (-not [IO.Path]::IsPathFullyQualified($CMakePath) -or $CMakePath -match '%[^%]+%' -or -not [IO.File]::Exists($CMakePath)) {
    throw 'CMakePath must name one existing expanded absolute executable.'
}
if ($Generator -ne 'Ninja' -and -not $IsWindows) { throw 'Visual Studio generators require Windows.' }
if ($CompilerLauncher) {
    if ($Generator -ne 'Ninja' -or -not [IO.Path]::IsPathFullyQualified($CompilerLauncher) -or
        $CompilerLauncher -match '%[^%]+%' -or $CompilerLauncher.Contains(';') -or -not [IO.File]::Exists($CompilerLauncher)) {
        throw 'CompilerLauncher requires Ninja and one existing expanded absolute executable without CMake list separators.'
    }
}
[void][IO.Directory]::CreateDirectory($evidence)
$build = Join-Path $evidence 'build'
$wrapper = Join-Path $overlay 'upstream/reference'
$configure = @('-S', $wrapper, '-B', $build, '-G', $Generator,
    "-DPRISM_SOURCE_DIR=$source", '-DCMAKE_BUILD_TYPE=Release',
    '-DLLAMA_BUILD_NUMBER=0', "-DLLAMA_BUILD_COMMIT=$($lock.Commit)")
if ($Generator -ne 'Ninja') { $configure += @('-A','ARM64','-T','ClangCL') }
$environment = @{}
if ($CompilerLauncher) {
    $configure += @("-DCMAKE_C_COMPILER_LAUNCHER=$CompilerLauncher", "-DCMAKE_CXX_COMPILER_LAUNCHER=$CompilerLauncher")
    $environment['CCACHE_BASEDIR'] = $evidence
}
$receiptPath = Join-Path $evidence 'build-receipt.json'
$receipt = [ordered]@{
    SchemaVersion = 1
    SourceCommit = $lock.Commit
    ArchiveSha256 = $lock.ArchiveSha256
    SourceDirectory = $source
    EvidenceDirectory = $evidence
    ConfigureArguments = $configure
    ParallelJobs = $Parallel
    Targets = $targets
    AssertionsEnabled = $true
    Status = 'configuring'
    Qualification = 'Original CPU correctness suites only; no models, benchmarks or Rust parity claim.'
}
[IO.File]::WriteAllText($receiptPath, ($receipt | ConvertTo-Json -Depth 6))
$configured = Invoke-BonsaiNativeProcess -Executable $CMakePath -ArgumentList $configure -WorkingDirectory $evidence -TimeoutSeconds 900 -Environment $environment -LogPath (Join-Path $evidence 'configure.log')
if ($configured.ExitCode) { throw "Original Prism suite configure failed; see $evidence/configure.log" }
$receipt.Status = 'building'
[IO.File]::WriteAllText($receiptPath, ($receipt | ConvertTo-Json -Depth 6))
$built = Invoke-BonsaiNativeProcess -Executable $CMakePath -ArgumentList (@('--build', $build, '--config', 'Release', '--parallel', [string]$Parallel, '--target') + $targets) -WorkingDirectory $evidence -TimeoutSeconds 1200 -Environment $environment -LogPath (Join-Path $evidence 'build.log')
if ($built.ExitCode) { throw "Original Prism suite build failed; see $evidence/build.log" }
$receipt.Status = 'built'
$receipt.CacheSha256 = (Get-FileHash -LiteralPath (Join-Path $build 'CMakeCache.txt') -Algorithm SHA256).Hash.ToLowerInvariant()
[IO.File]::WriteAllText($receiptPath, ($receipt | ConvertTo-Json -Depth 6))
$cases = [Collections.Generic.List[object]]::new()
$resultPath = Join-Path $evidence 'reference-results.json'
if (-not $BuildOnly) {
    foreach ($target in $targets) {
        $extension = if ($IsWindows) { '.exe' } else { '' }
        $executables = @(Get-ChildItem -LiteralPath (Join-Path $build 'bin') -Recurse -File -Filter ($target + $extension))
        if ($executables.Count -ne 1) { throw "Expected exactly one built $target executable." }
        $working = Join-Path $evidence ('run-' + $target)
        [void][IO.Directory]::CreateDirectory($working)
        $log = Join-Path $evidence ($target + '.log')
        $tested = Invoke-BonsaiNativeProcess -Executable $executables[0].FullName -WorkingDirectory $working -TimeoutSeconds 300 -LogPath $log
        $status = if ($tested.ExitCode -eq 0) { 'pass' } else { 'fail' }
        $cases.Add([ordered]@{
            Id = 'tests/' + $target + '.cpp'
            Status = $status
            Detail = "Original unmodified C++ suite exited $($tested.ExitCode); log: $log"
        })
        [IO.File]::WriteAllText($resultPath, ([ordered]@{ SchemaVersion = 1; Cases = $cases.ToArray() } | ConvertTo-Json -Depth 5))
    }
    $receipt.Status = 'tested'
    [IO.File]::WriteAllText($receiptPath, ($receipt | ConvertTo-Json -Depth 6))
}
return [ordered]@{
    EvidenceDirectory = $evidence
    BuildDirectory = $build
    ResultPath = if ($BuildOnly) { $null } else { $resultPath }
    Targets = $targets
    ReferenceResults = [ordered]@{ SchemaVersion = 1; Cases = $cases.ToArray() }
}
