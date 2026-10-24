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
    [ValidateSet('All', 'Pq2RowShapes', 'Ptq1ElementMap', 'Unicode', 'MtpCatchupBatch', 'Jinja', 'MtmdImpl', 'GrammarParser', 'ReasoningBudget', 'Col2im', 'Dfly', 'Rope', 'LlamaGrammar', 'GrammarIntegration', 'Quantize', 'PtqCuda', 'Log', 'MoeReduce', 'PegBasic', 'PegGbnf', 'PegJson', 'PegSerialization', 'PegPythonDict', 'PegUnicode', 'PegParser', 'BatchAllocator', 'Allocator', 'Barrier', 'ChatTemplate', 'MtmdCApi', 'Sampling', 'ChatPeg', 'Gguf', 'ChatAuto', 'Tokenizer0', 'ChatAnalysis', 'PublicC', 'DsparkLogSnr', 'GgufMetadata', 'QuantSelection', 'Architectures', 'RecurrentRollback', 'KvMeanCenter', 'Optimizer', 'DoubleFloat', 'GbnfValidator', 'ArgParser', 'Chat', 'ModelResolution', 'TokenizerSpm', 'TokenizerBpe', 'JsonSchema', 'Autorelease', 'ModelLoadCancel', 'SaveLoadState', 'StateRestoreFragmented', 'ThreadSafety')][string]$Case = 'All',
    [string]$CMakePath,
    [ValidateSet('Ninja', 'Visual Studio 18 2026', 'Visual Studio 17 2022')][string]$Generator = 'Ninja',
    [ValidateSet('ARM64', 'x64')][string]$NativePlatform = 'ARM64',
    [string]$Toolchain = 'nightly-2026-10-22',
    [string]$CompilerLauncher,
    [ValidateRange(1, 12)][int]$Parallel = 2,
    [string]$NativeBuildDirectory
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
Import-Module (Join-Path $repo 'scripts/Bonsai.PrismNativeCache.psm1')
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
$nativeBuildRoot = $evidence
$nativeSource = $source
if ($NativeBuildDirectory) {
    $nativeBuildRoot = Resolve-ExternalPath $NativeBuildDirectory
    foreach ($other in @($evidence, $target)) {
        $prefix = $other.TrimEnd([IO.Path]::DirectorySeparatorChar) + [IO.Path]::DirectorySeparatorChar
        $nativePrefix = $nativeBuildRoot.TrimEnd([IO.Path]::DirectorySeparatorChar) + [IO.Path]::DirectorySeparatorChar
        if ($nativeBuildRoot -eq $other -or $nativeBuildRoot.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase) -or
            $other.StartsWith($nativePrefix, [StringComparison]::OrdinalIgnoreCase)) {
            throw 'Native build cache, Rust targets and uploaded evidence must use separate directories.'
        }
    }
    $nativeSource = Initialize-BonsaiPrismNativeSource -SourceDirectory $source -BuildDirectory $nativeBuildRoot
}
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
    ProcessorIdentifier = $env:PROCESSOR_IDENTIFIER
    LogicalProcessors = [Environment]::ProcessorCount
    Toolchain = $Toolchain
    Selection = $Case
    Generator = $Generator
    NativePlatform = $NativePlatform
    SourceDirectory = $source
    NativeSourceDirectory = $nativeSource
    NativeBuildDirectory = $nativeBuildRoot
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
    Barrier = @{ Name = 'test-barrier'; Family = 'common' }
    ChatTemplate = @{ Name = 'test-chat-template'; Family = 'common' }
    MtmdCApi = @{ Name = 'test-mtmd-c-api'; Id = 'tests/test-mtmd-c-api.c'; Family = 'common' }
    Sampling = @{ Name = 'test-sampling'; Family = 'common' }
    ChatPeg = @{ Name = 'test-chat-peg-parser'; Family = 'common' }
    Gguf = @{ Name = 'test-gguf'; Family = 'common' }
    ChatAuto = @{ Name = 'test-chat-auto-parser'; Family = 'common' }
    Tokenizer0 = @{ Name = 'test-tokenizer-0'; Family = 'common' }
    JsonSchema = @{ Name = 'test-json-schema-to-grammar'; Family = 'common' }
    ChatAnalysis = @{ Name = 'test-chat-analysis'; Family = 'common' }
    PublicC = @{ Name = 'test-c'; Id = 'tests/test-c.c'; Family = 'common' }
    DsparkLogSnr = @{ Name = 'test-dspark-logsnr-meta'; Family = 'common' }
    GgufMetadata = @{ Name = 'test-gguf-model-data'; Family = 'independent' }
    QuantSelection = @{ Name = 'test-quant-type-selection'; Family = 'common' }
    Architectures = @{ Name = 'test-llama-archs'; Family = 'common' }
    RecurrentRollback = @{ Name = 'test-recurrent-state-rollback'; Family = 'common' }
    KvMeanCenter = @{ Name = 'test-kv-mean-center'; Family = 'common' }
    Optimizer = @{ Name = 'test-opt'; Family = 'common' }
    DoubleFloat = @{ Name = 'test-double-float'; Family = 'independent' }
    GbnfValidator = @{ Name = 'test-gbnf-validator'; Family = 'common' }
    ArgParser = @{ Name = 'test-arg-parser'; Family = 'common' }
    Chat = @{ Name = 'test-chat'; Family = 'common' }
    ModelResolution = @{ Name = 'test-model-resolution'; Family = 'common' }
    TokenizerSpm = @{ Name = 'test-tokenizer-1-spm'; Family = 'common' }
    TokenizerBpe = @{ Name = 'test-tokenizer-1-bpe'; Family = 'common' }
    Autorelease = @{ Name = 'test-autorelease'; Family = 'common' }
    ModelLoadCancel = @{ Name = 'test-model-load-cancel'; Family = 'common' }
    SaveLoadState = @{ Name = 'test-save-load-state'; Family = 'common' }
    StateRestoreFragmented = @{ Name = 'test-state-restore-fragmented'; Family = 'common' }
    ThreadSafety = @{ Name = 'test-thread-safety'; Family = 'common' }
}
$selected = if ($Case -eq 'All') { @('Pq2RowShapes', 'Ptq1ElementMap', 'Unicode', 'MtpCatchupBatch', 'Jinja', 'MtmdImpl', 'GrammarParser', 'ReasoningBudget', 'Col2im', 'Dfly', 'Rope', 'LlamaGrammar', 'GrammarIntegration', 'Quantize', 'PtqCuda', 'Log', 'MoeReduce', 'PegBasic', 'PegGbnf', 'PegJson', 'PegSerialization', 'PegPythonDict', 'PegUnicode', 'PegParser', 'BatchAllocator', 'Allocator', 'Barrier', 'ChatTemplate', 'MtmdCApi', 'Sampling', 'ChatPeg', 'Gguf', 'ChatAuto', 'Tokenizer0', 'ChatAnalysis', 'PublicC', 'DsparkLogSnr', 'GgufMetadata', 'QuantSelection', 'Architectures', 'RecurrentRollback', 'KvMeanCenter', 'Optimizer', 'DoubleFloat', 'GbnfValidator', 'ArgParser', 'Chat', 'ModelResolution', 'TokenizerSpm', 'TokenizerBpe', 'JsonSchema', 'Autorelease', 'ModelLoadCancel', 'SaveLoadState', 'StateRestoreFragmented', 'ThreadSafety') } else { @($Case) }
$modelSelections = @('Autorelease', 'ModelLoadCancel', 'SaveLoadState', 'StateRestoreFragmented', 'ThreadSafety')
$assetDirectory = $null
if (@($selected | Where-Object { $_ -in $modelSelections }).Count) {
    $assetDirectory = Join-Path $evidence 'model-assets'
    $modelLock = Get-Content -LiteralPath (Join-Path $upstream 'model-assets.lock.json') -Raw | ConvertFrom-Json -AsHashtable
    if ($modelLock.SchemaVersion -ne 1 -or $modelLock.Assets.Count -ne 1) { throw 'Unexpected model asset lock schema.' }
    $modelReceipts = [Collections.Generic.List[object]]::new()
    foreach ($asset in $modelLock.Assets) {
        if ($asset.Path -cne 'tinyllamas/stories15M-q4_0.gguf' -or
            $asset.Url -notlike 'https://huggingface.co/*' -or $asset.Sha256 -cnotmatch '^[0-9a-f]{64}$') {
            throw 'Unexpected or unpinned model asset.'
        }
        $destination = Join-Path $assetDirectory $asset.Path
        [void][IO.Directory]::CreateDirectory((Split-Path $destination -Parent))
        Invoke-WebRequest -Uri $asset.Url -OutFile $destination -TimeoutSec 180
        $modelGuard = [IO.FileStream]::new($destination, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
        try {
            $digest = [Convert]::ToHexStringLower([Security.Cryptography.SHA256]::HashData($modelGuard))
            if ($modelGuard.Length -ne $asset.Bytes -or $digest -cne $asset.Sha256) { throw 'Model asset differs from immutable byte identity.' }
            $modelReceipts.Add(@{ Path = $destination; Bytes = $modelGuard.Length; Sha256 = $digest; Url = $asset.Url })
        } finally { $modelGuard.Dispose() }
    }
    $receipt.ModelAssets = $modelReceipts.ToArray()
}
if (@($selected | Where-Object { $_ -in @('GgufMetadata', 'QuantSelection') }).Count) {
    Import-Module (Join-Path $repo 'scripts/Bonsai.PrismAssets.psm1')
    if (-not $assetDirectory) { $assetDirectory = Join-Path $evidence 'model-assets' }
    $receipt.MetadataAssets = Save-BonsaiPrismMetadataAsset -Repository $repo -Directory $assetDirectory
    [IO.File]::WriteAllText($receiptPath, ($receipt | ConvertTo-Json -Depth 12))
}
$bridgeFamilies = @{
    ggml = @{ Source = $upstream; Target = 'prism-upstream-bridge'; Option = '--bridge' }
    common = @{ Source = (Join-Path $upstream 'common'); Target = 'prism-upstream-common-bridge'; Option = '--common-bridge' }
}
$bridges = @{}
$nativeBuilds = [Collections.Generic.List[object]]::new()
$extension = if ($IsWindows) { '.exe' } else { '' }
foreach ($family in @($selected | ForEach-Object { $adapters[$_].Family } | Select-Object -Unique)) {
    if ($family -eq 'independent') { continue }
    $definition = $bridgeFamilies[$family]
    $build = Join-Path $nativeBuildRoot ("build-$family")
    $null = Resolve-ExternalPath $build
    $configureArguments = @('-S', $definition.Source, '-B', $build, "-DPRISM_SOURCE_DIR=$nativeSource", '-DCMAKE_BUILD_TYPE=Release',
        '-DLLAMA_BUILD_NUMBER=0', "-DLLAMA_BUILD_COMMIT=$($lock.Commit)") + $generatorArguments
    try {
        $null = Invoke-UpstreamCommand -Executable $CMakePath -Arguments $configureArguments
        $null = Invoke-UpstreamCommand -Executable $CMakePath -Arguments @('--build', $build, '--config', 'Release', '--parallel', [string]$Parallel, '--target', $definition.Target)
    } finally {
        if ($NativeBuildDirectory) {
            $diagnostics = Join-Path $evidence ("native-$family")
            [void][IO.Directory]::CreateDirectory($diagnostics)
            foreach ($diagnostic in @('CMakeCache.txt', 'CMakeFiles/CMakeConfigureLog.yaml')) {
                $path = Join-Path $build $diagnostic
                if ([IO.File]::Exists($path)) { [IO.File]::Copy($path, (Join-Path $diagnostics ([IO.Path]::GetFileName($path))), $false) }
            }
        }
    }
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
        $arguments = @('--target', $engine, '--case', $caseId, '--source-directory', $source, '--output', $output,
            '--evidence-directory', (Join-Path $evidence "$caseName-$engine-bridge"))
        if ($assetDirectory) { $arguments += @('--asset-directory', $assetDirectory) }
        if ($engine -eq 'cpp' -and $adapter.Family -ne 'independent') {
            $arguments += @($bridgeFamilies[$adapter.Family].Option, $bridges[$adapter.Family])
        }
        $timeout = if ($selection -eq 'Barrier') { 360 } elseif ($selection -eq 'Architectures') { 1800 } elseif ($selection -eq 'RecurrentRollback') { 600 } elseif ($selection -in @('KvMeanCenter','Optimizer')) { 600 } elseif ($selection -eq 'Chat') { 1200 } elseif ($selection -eq 'ModelResolution') { 360 } elseif ($selection -eq 'DoubleFloat') { 900 } elseif ($selection -eq 'TokenizerBpe') { 2100 } elseif ($selection -eq 'TokenizerSpm') { 600 } elseif ($selection -in $modelSelections) { 360 } elseif ($selection -in @('Jinja', 'GrammarIntegration', 'Quantize', 'PtqCuda', 'ChatPeg', 'Gguf', 'ChatAuto', 'Tokenizer0', 'ChatAnalysis', 'PublicC', 'DsparkLogSnr', 'GgufMetadata', 'QuantSelection', 'Architectures', 'RecurrentRollback', 'KvMeanCenter', 'Optimizer', 'DoubleFloat', 'GbnfValidator', 'ArgParser', 'Chat', 'ModelResolution', 'TokenizerSpm', 'TokenizerBpe', 'JsonSchema', 'Autorelease', 'ModelLoadCancel', 'SaveLoadState', 'StateRestoreFragmented', 'ThreadSafety')) { 300 } else { 120 }
        $null = Invoke-UpstreamCommand -Executable $harness -Arguments $arguments -TimeoutSeconds $timeout
        $result = Get-Content -LiteralPath $output -Raw | ConvertFrom-Json -AsHashtable
        if ($result.SchemaVersion -ne 1 -or $result.UpstreamCommit -cne $lock.Commit -or $result.Target -cne $engine -or
            @($result.Cases).Count -ne 1 -or $result.Cases[0].Id -cne $caseId) {
            throw 'Shared assertion harness returned an unexpected case identity.'
        }
        if ($engine -eq 'cpp') {
            $referenceCases.Add($result.Cases[0])
        }
        else { $rustCases.Add($result.Cases[0]) }
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
    $receipt.Summary = $gate.Summary
    if (-not $gate.Passed) {
        $receipt.Status = 'failed'
        $receipt.Issues = $gate.Issues
        [IO.File]::WriteAllText($receiptPath, ($receipt | ConvertTo-Json -Depth 12))
        throw "Prism baseline regressed; all source contract failures are retained in $(Join-Path $evidence 'gate.json')"
    }
} else {
    $receipt.Results = $rust
    if (@($reference.Cases | Where-Object { $_.Status -cne 'pass' }).Count) {
        $receipt.Status = 'failed'
        [IO.File]::WriteAllText($receiptPath, ($receipt | ConvertTo-Json -Depth 12))
        throw 'The focused C++ reference contract failed; both target result files are retained.'
    }
}
$receipt.Status = 'complete'
[IO.File]::WriteAllText($receiptPath, ($receipt | ConvertTo-Json -Depth 12))
return $receipt
