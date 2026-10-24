#requires -Version 7.6
<#
.SYNOPSIS
Runs safe Rust assertions against pinned Prism and the Rust runtime.
.DESCRIPTION
All source, build, request and result files remain in fresh external evidence.
The full catalog runs only in main-branch GitHub Actions. A named adapter may
run locally. Expected Rust conformance failures are data; compiler, reference,
timeout, malformed-report and baseline regressions fail the check.
#>
[CmdletBinding()]
[OutputType([Collections.IDictionary])]
param(
    [Parameter(Mandatory)][string]$SourceArchive,
    [Parameter(Mandatory)][string]$EvidenceDirectory,
    [Parameter(Mandatory)][string]$TargetDirectory,
    [ValidateSet('All', 'Pq2RowShapes', 'Ptq1ElementMap', 'Unicode', 'MtpCatchupBatch', 'Jinja', 'MtmdImpl', 'GrammarParser', 'ReasoningBudget', 'Col2im', 'Dfly', 'Rope', 'LlamaGrammar', 'GrammarIntegration', 'Quantize', 'PtqCuda', 'Log', 'MoeReduce', 'PegBasic', 'PegGbnf', 'PegJson', 'PegSerialization', 'PegPythonDict', 'PegUnicode', 'PegParser', 'BatchAllocator', 'Allocator', 'ChatTemplate', 'MtmdCApi', 'Sampling', 'ChatPeg', 'Gguf', 'ChatAuto', 'Tokenizer0')][string]$Case = 'All',
    [string]$CMakePath,
    [ValidateSet('Ninja', 'Visual Studio 18 2026', 'Visual Studio 17 2022')][string]$Generator = 'Ninja',
    [ValidateSet('ARM64', 'x64')][string]$NativePlatform = 'ARM64',
    [string]$Toolchain = 'nightly-2026-10-22',
    [string]$CompilerLauncher,
    [ValidateRange(1, 12)][int]$Parallel = 2
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
if ($Case -eq 'All' -and ($env:GITHUB_ACTIONS -cne 'true' -or $env:GITHUB_REF -cne 'refs/heads/main')) {
    throw 'The complete Prism catalog runs only in GitHub Actions on main; select one named local adapter.'
}
$repo = Split-Path $PSScriptRoot -Parent
Import-Module (Join-Path $repo 'scripts/Bonsai.Common.psm1')
Import-Module (Join-Path $repo 'scripts/Bonsai.Native.psm1')
Import-Module (Join-Path $repo 'scripts/Bonsai.PrismBaseline.psm1')
$archive = Resolve-ExternalPath $SourceArchive
$evidence = Resolve-ExternalPath $EvidenceDirectory
$target = Resolve-ExternalPath $TargetDirectory
if ($CompilerLauncher -and ($Generator -ne 'Ninja' -or
    -not [IO.Path]::IsPathFullyQualified($CompilerLauncher) -or $CompilerLauncher -match '%[^%]+%' -or
    $CompilerLauncher.Contains(';') -or -not [IO.File]::Exists($CompilerLauncher))) {
    throw 'CompilerLauncher requires Ninja and one existing expanded absolute executable without CMake list separators.'
}
if (Test-Path -LiteralPath $evidence) { throw 'Use a fresh evidence directory for each Prism import.' }
[void][IO.Directory]::CreateDirectory($evidence)
$lock = Get-Content -LiteralPath (Join-Path $repo 'tools/prism/source.lock.json') -Raw | ConvertFrom-Json -AsHashtable
$guard = [IO.FileStream]::new($archive, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
try {
    if ($guard.Length -ne $lock.ArchiveBytes -or
        [Convert]::ToHexStringLower([Security.Cryptography.SHA256]::HashData($guard)) -cne $lock.ArchiveSha256) {
        throw 'Prism source archive differs from its immutable pin.'
    }
    Expand-Archive -LiteralPath $archive -DestinationPath (Join-Path $evidence 'source')
} finally { $guard.Dispose() }
$source = Join-Path (Join-Path $evidence 'source') $lock.ArchiveRoot
$upstream = Join-Path $repo 'tools/prism/upstream'
$catalog = Import-BonsaiPrismCatalog -Path (Join-Path $upstream 'catalog.json') -SourceDirectory $source
$receipt = [ordered]@{
    SchemaVersion = 1
    SourceCommit = $lock.Commit
    SourceArchiveSha256 = $lock.ArchiveSha256
    ProjectRevision = $env:GITHUB_SHA
    RecordedAtUtc = [DateTime]::UtcNow.ToString('o')
    Platform = [Runtime.InteropServices.RuntimeInformation]::OSDescription
    Architecture = [Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture.ToString()
    Toolchain = $Toolchain
    Selection = $Case
    Generator = $Generator
    NativePlatform = $NativePlatform
    SourceDirectory = $source
    NativeToolset = if ($Generator -ne 'Ninja' -and $NativePlatform -eq 'ARM64') { 'ClangCL' } else { $null }
    CompilerLauncher = $CompilerLauncher
    ParallelJobs = $Parallel
    Status = 'building'
}
$receiptPath = Join-Path $evidence 'receipt.json'
[IO.File]::WriteAllText($receiptPath, ($receipt | ConvertTo-Json -Depth 8))
if (-not $CMakePath) { $CMakePath = (Get-Command cmake -CommandType Application -ErrorAction Stop).Source }
if (-not [IO.Path]::IsPathFullyQualified($CMakePath) -or $CMakePath -match '%[^%]+%' -or -not [IO.File]::Exists($CMakePath)) {
    throw 'CMakePath must name an existing expanded absolute executable.'
}
if ($Generator -ne 'Ninja' -and -not $IsWindows) { throw 'Visual Studio generators require Windows.' }
$cargo = (Get-Command cargo -CommandType Application -ErrorAction Stop).Source
$script:upstreamCommandNumber = 0
function Invoke-UpstreamCommand {
    [CmdletBinding()]
    [OutputType([pscustomobject])]
    param([string]$Executable, [string[]]$Arguments, [int]$TimeoutSeconds = 900)
    $script:upstreamCommandNumber++
    $log = Join-Path $evidence ('command-{0:D2}.log' -f $script:upstreamCommandNumber)
    $result = Invoke-BonsaiNativeProcess -Executable $Executable -ArgumentList $Arguments -WorkingDirectory $evidence -TimeoutSeconds $TimeoutSeconds -Environment $compilerEnvironment -LogPath $log
    if ($result.ExitCode -ne 0) { throw "Prism import command failed with exit $($result.ExitCode); see $log" }
    return $result
}
$compilerEnvironment = @{}
$generatorArguments = @('-G', $Generator)
if ($Generator -ne 'Ninja') {
    $generatorArguments += @('-A', $NativePlatform)
    if ($NativePlatform -eq 'ARM64') { $generatorArguments += @('-T', 'ClangCL') }
}
if ($CompilerLauncher) {
    $generatorArguments += @("-DCMAKE_C_COMPILER_LAUNCHER=$CompilerLauncher", "-DCMAKE_CXX_COMPILER_LAUNCHER=$CompilerLauncher")
    $compilerEnvironment['CCACHE_BASEDIR'] = $evidence
}
$adapters = @{
    Pq2RowShapes = @{ Name = 'test-pq2-row-shapes'; Family = 'ggml' }
    Ptq1ElementMap = @{ Name = 'test-ptq1_0-element-map'; Family = 'ggml' }
    Unicode = @{ Name = 'test-unicode'; Family = 'common' }
    MtpCatchupBatch = @{ Name = 'test-mtp-catchup-batch'; Family = 'common' }
    Jinja = @{ Name = 'test-jinja'; Family = 'common' }
    MtmdImpl = @{ Name = 'test-mtmd-impl'; Family = 'common' }
    GrammarParser = @{ Name = 'test-grammar-parser'; Family = 'common' }
    ReasoningBudget = @{ Name = 'test-reasoning-budget'; Family = 'common' }
    Col2im = @{ Name = 'test-col2im-1d'; Family = 'common' }
    Dfly = @{ Name = 'test-dfly-fusion'; Family = 'common' }
    Rope = @{ Name = 'test-rope'; Family = 'common' }
    LlamaGrammar = @{ Name = 'test-llama-grammar'; Family = 'common' }
    GrammarIntegration = @{ Name = 'test-grammar-integration'; Family = 'common' }
    Quantize = @{ Name = 'test-quantize-fns'; Family = 'common' }
    PtqCuda = @{ Name = 'test-ptq1_0-cuda-dot'; Family = 'common' }
    Log = @{ Name = 'test-log'; Family = 'common' }
    MoeReduce = @{ Name = 'test-moe-weighted-reduce'; Family = 'common' }
    PegBasic = @{ Name = 'peg-basic'; Id = 'tests/peg-parser/test-basic.cpp'; Family = 'common' }
    PegGbnf = @{ Name = 'peg-gbnf'; Id = 'tests/peg-parser/test-gbnf-generation.cpp'; Family = 'common' }
    PegJson = @{ Name = 'peg-json'; Id = 'tests/peg-parser/test-json-parser.cpp'; Family = 'common' }
    PegSerialization = @{ Name = 'peg-serialization'; Id = 'tests/peg-parser/test-json-serialization.cpp'; Family = 'common' }
    PegPythonDict = @{ Name = 'peg-python-dict'; Id = 'tests/peg-parser/test-python-dict-parser.cpp'; Family = 'common' }
    PegUnicode = @{ Name = 'peg-unicode'; Id = 'tests/peg-parser/test-unicode.cpp'; Family = 'common' }
    PegParser = @{ Name = 'test-peg-parser'; Family = 'common' }
    BatchAllocator = @{ Name = 'test-batch-alloc'; Family = 'common' }
    Allocator = @{ Name = 'test-alloc'; Family = 'common' }
    ChatTemplate = @{ Name = 'test-chat-template'; Family = 'common' }
    MtmdCApi = @{ Name = 'test-mtmd-c-api'; Id = 'tests/test-mtmd-c-api.c'; Family = 'common' }
    Sampling = @{ Name = 'test-sampling'; Family = 'common' }
    ChatPeg = @{ Name = 'test-chat-peg-parser'; Family = 'common' }
    Gguf = @{ Name = 'test-gguf'; Family = 'common' }
    ChatAuto = @{ Name = 'test-chat-auto-parser'; Family = 'common' }
    Tokenizer0 = @{ Name = 'test-tokenizer-0'; Family = 'common' }
}
$selected = if ($Case -eq 'All') { @('Pq2RowShapes', 'Ptq1ElementMap', 'Unicode', 'MtpCatchupBatch', 'Jinja', 'MtmdImpl', 'GrammarParser', 'ReasoningBudget', 'Col2im', 'Dfly', 'Rope', 'LlamaGrammar', 'GrammarIntegration', 'Quantize', 'PtqCuda', 'Log', 'MoeReduce', 'PegBasic', 'PegGbnf', 'PegJson', 'PegSerialization', 'PegPythonDict', 'PegUnicode', 'PegParser', 'BatchAllocator', 'Allocator', 'ChatTemplate', 'MtmdCApi', 'Sampling', 'ChatPeg', 'Gguf', 'ChatAuto', 'Tokenizer0') } else { @($Case) }
$bridgeFamilies = @{
    ggml = @{ Source = $upstream; Target = 'prism-upstream-bridge'; Option = '--bridge' }
    common = @{ Source = (Join-Path $upstream 'common'); Target = 'prism-upstream-common-bridge'; Option = '--common-bridge' }
}
$bridges = @{}
$nativeBuilds = [Collections.Generic.List[object]]::new()
$extension = if ($IsWindows) { '.exe' } else { '' }
foreach ($family in @($selected | ForEach-Object { $adapters[$_].Family } | Select-Object -Unique)) {
    $definition = $bridgeFamilies[$family]
    $build = Join-Path $evidence ("build-$family")
    $configureArguments = @('-S', $definition.Source, '-B', $build, "-DPRISM_SOURCE_DIR=$source", '-DCMAKE_BUILD_TYPE=Release',
        '-DLLAMA_BUILD_NUMBER=0', "-DLLAMA_BUILD_COMMIT=$($lock.Commit)") + $generatorArguments
    $null = Invoke-UpstreamCommand -Executable $CMakePath -Arguments $configureArguments
    $null = Invoke-UpstreamCommand -Executable $CMakePath -Arguments @('--build', $build, '--config', 'Release', '--parallel', [string]$Parallel, '--target', $definition.Target)
    $executables = @(Get-ChildItem -LiteralPath $build -Recurse -File -Filter ($definition.Target + $extension))
    if ($executables.Count -ne 1) { throw "Expected exactly one $family production-call bridge." }
    $bridges[$family] = $executables[0].FullName
    if ($IsWindows -and $NativePlatform -eq 'ARM64') { Assert-Arm64PE -Path $bridges[$family] }
    $nativeBuilds.Add([ordered]@{
        Family = $family
        ConfigureArguments = $configureArguments
        Target = $definition.Target
        CacheSha256 = (Get-FileHash -LiteralPath (Join-Path $build 'CMakeCache.txt') -Algorithm SHA256).Hash.ToLowerInvariant()
        Executable = $bridges[$family]
        ExecutableSha256 = (Get-FileHash -LiteralPath $bridges[$family] -Algorithm SHA256).Hash.ToLowerInvariant()
        VerifiedPeMachine = if ($IsWindows -and $NativePlatform -eq 'ARM64') { '0xAA64' } else { $null }
    })
}
$receipt.NativeBuilds = $nativeBuilds.ToArray()
[IO.File]::WriteAllText($receiptPath, ($receipt | ConvertTo-Json -Depth 10))
$compiled = Invoke-UpstreamCommand -Executable $cargo -Arguments @("+$Toolchain", 'build', '--manifest-path', (Join-Path $repo 'Cargo.toml'), '--package', 'bonsai-prism-runtime', '--example', 'upstream_prism', '--locked', '--offline', '--release', '--target-dir', $target, '--message-format=json')
$executables = @($compiled.StandardOutput -split "`n" | Where-Object { $_.StartsWith('{') } | ForEach-Object {
    $record = $_ | ConvertFrom-Json -AsHashtable
    if ($record.reason -eq 'compiler-artifact' -and $record.target.name -eq 'upstream_prism' -and $record.executable) { $record.executable }
})
if ($executables.Count -ne 1) { throw 'Expected exactly one compiled shared-assertion Rust harness.' }
$harness = $executables[0]
if ($IsWindows -and $NativePlatform -eq 'ARM64') { Assert-Arm64PE -Path $harness }
$receipt.HarnessExecutable = $harness
$receipt.HarnessSha256 = (Get-FileHash -LiteralPath $harness -Algorithm SHA256).Hash.ToLowerInvariant()
$receipt.HarnessVerifiedPeMachine = if ($IsWindows -and $NativePlatform -eq 'ARM64') { '0xAA64' } else { $null }
$referenceCases = [Collections.Generic.List[object]]::new()
$rustCases = [Collections.Generic.List[object]]::new()
# Rust owns each selected test's inputs, reference calculations and assertions.
# The native bridge dispatches production engine calls without importing tests.
foreach ($selection in $selected) {
    $adapter = $adapters[$selection]
    $caseName = $adapter.Name
    $caseId = if ($adapter.ContainsKey('Id')) { $adapter.Id } else { "tests/$caseName.cpp" }
    foreach ($engine in @('cpp', 'rust')) {
        $output = Join-Path $evidence "$caseName-$engine.json"
        $arguments = @('--target', $engine, '--case', $caseId, '--source-directory', $source, '--output', $output)
        if ($engine -eq 'cpp') {
            $arguments += @($bridgeFamilies[$adapter.Family].Option, $bridges[$adapter.Family], '--evidence-directory', (Join-Path $evidence "$caseName-bridge"))
        }
        $timeout = if ($selection -in @('Jinja', 'GrammarIntegration', 'Quantize', 'PtqCuda', 'ChatPeg', 'Gguf', 'ChatAuto', 'Tokenizer0')) { 300 } else { 120 }
        $null = Invoke-UpstreamCommand -Executable $harness -Arguments $arguments -TimeoutSeconds $timeout
        $result = Get-Content -LiteralPath $output -Raw | ConvertFrom-Json -AsHashtable
        if ($result.SchemaVersion -ne 1 -or $result.UpstreamCommit -cne $lock.Commit -or $result.Target -cne $engine -or
            @($result.Cases).Count -ne 1 -or $result.Cases[0].Id -cne $caseId) {
            throw 'Shared assertion harness returned an unexpected case identity.'
        }
        if ($engine -eq 'cpp') {
            $referenceCases.Add($result.Cases[0])
            if ($result.Cases[0].Status -cne 'pass') { throw "Rust assertions failed on pinned Prism for $caseName; see $output" }
        }
        else { $rustCases.Add($result.Cases[0]) }
    }
}
if ($Case -eq 'All') {
    $cacheArguments = @{}
    if ($CompilerLauncher) { $cacheArguments = @{ CompilerLauncher = $CompilerLauncher; CompilerCacheBaseDirectory = $evidence } }
    $raw = & (Join-Path $repo 'scripts/Invoke-PrismCppSuite.ps1') -SourceDirectory $source -EvidenceDirectory (Join-Path $evidence 'raw-cpp') -CMakePath $CMakePath -Generator $Generator -NativePlatform $NativePlatform -Parallel $Parallel @cacheArguments
    if (@($raw.ReferenceResults.Cases).Count -ne @($raw.Targets).Count) { throw 'The raw C++ importer lost an expected result.' }
    foreach ($name in $raw.Targets) {
        if (@($raw.ReferenceResults.Cases | Where-Object { $_.Id -ceq "tests/$name.cpp" }).Count -ne 1) {
            throw 'The raw C++ importer returned a missing or duplicate suite identity.'
        }
    }
    foreach ($result in $raw.ReferenceResults.Cases) {
        if (@($referenceCases | Where-Object { $_.Id -ceq $result.Id }).Count -gt 0) {
            # Keep the original GGUF replay until its Rust replacement has
            # passed hosted acceptance; a duplicate source is counted once.
            if ($result.Status -cne 'pass') { throw "Retained original control failed for $($result.Id)." }
        } else { $referenceCases.Add($result) }
    }
}
$reference = @{ SchemaVersion = 1; UpstreamCommit = $lock.Commit; Cases = $referenceCases.ToArray() }
$rust = @{ SchemaVersion = 1; UpstreamCommit = $lock.Commit; Cases = $rustCases.ToArray() }
[IO.File]::WriteAllText((Join-Path $evidence 'cpp-results.json'), ($reference | ConvertTo-Json -Depth 10))
[IO.File]::WriteAllText((Join-Path $evidence 'rust-results.json'), ($rust | ConvertTo-Json -Depth 10))
if ($Case -eq 'All') {
    $report = New-BonsaiPrismReport -Catalog $catalog -TargetResults $rust -ReferenceResults $reference
    [IO.File]::WriteAllText((Join-Path $evidence 'report.json'), ($report | ConvertTo-Json -Depth 12))
    $gate = Test-BonsaiPrismBaseline -Catalog $catalog -Report $report -BaselinePath (Join-Path $upstream 'baseline.json')
    [IO.File]::WriteAllText((Join-Path $evidence 'gate.json'), ($gate | ConvertTo-Json -Depth 12))
    if (-not $gate.Passed) { throw "Prism baseline regressed; see $(Join-Path $evidence 'gate.json')" }
    $receipt.Summary = $gate.Summary
} else {
    if (@($reference.Cases | Where-Object { $_.Status -cne 'pass' }).Count) { throw 'The focused C++ control failed.' }
    $receipt.Results = $rust
}
$receipt.Status = 'complete'
[IO.File]::WriteAllText($receiptPath, ($receipt | ConvertTo-Json -Depth 12))
return $receipt
