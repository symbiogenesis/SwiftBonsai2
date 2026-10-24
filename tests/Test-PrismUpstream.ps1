#requires -Version 7.6
<#
.SYNOPSIS
Imports the pinned Prism tests and compares shared assertions on C++ and Rust.
.DESCRIPTION
All source, build, fixture and result files remain in fresh external evidence.
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
    [ValidateSet('All', 'Pq2RowShapes', 'Ptq1ElementMap')][string]$Case = 'All',
    [string]$CMakePath,
    [ValidateSet('Ninja', 'Visual Studio 18 2026', 'Visual Studio 17 2022')][string]$Generator = 'Ninja',
    [ValidateSet('ARM64', 'x64')][string]$NativePlatform = 'ARM64',
    [string]$Toolchain = 'nightly-2026-10-22',
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
    Status = 'building'
}
$receiptPath = Join-Path $evidence 'receipt.json'
[IO.File]::WriteAllText($receiptPath, ($receipt | ConvertTo-Json -Depth 8))
if (-not $CMakePath) { $CMakePath = (Get-Command cmake -CommandType Application -ErrorAction Stop).Source }
$cargo = (Get-Command cargo -CommandType Application -ErrorAction Stop).Source
$script:upstreamCommandNumber = 0
function Invoke-UpstreamCommand {
    [CmdletBinding()]
    [OutputType([pscustomobject])]
    param([string]$Executable, [string[]]$Arguments, [int]$TimeoutSeconds = 900)
    $script:upstreamCommandNumber++
    $log = Join-Path $evidence ('command-{0:D2}.log' -f $script:upstreamCommandNumber)
    $result = Invoke-BonsaiNativeProcess -Executable $Executable -ArgumentList $Arguments -WorkingDirectory $evidence -TimeoutSeconds $TimeoutSeconds -LogPath $log
    if ($result.ExitCode -ne 0) { throw "Prism import command failed with exit $($result.ExitCode); see $log" }
    return $result
}
$build = Join-Path $evidence 'build'
$generatorArguments = @('-G', $Generator)
if ($Generator -ne 'Ninja') { $generatorArguments += @('-A', $NativePlatform) }
$null = Invoke-UpstreamCommand -Executable $CMakePath -Arguments (@('-S', $upstream, '-B', $build, "-DPRISM_SOURCE_DIR=$source", '-DCMAKE_BUILD_TYPE=Release') + $generatorArguments)
$null = Invoke-UpstreamCommand -Executable $CMakePath -Arguments @('--build', $build, '--config', 'Release', '--parallel', [string]$Parallel)
$compiled = Invoke-UpstreamCommand -Executable $cargo -Arguments @("+$Toolchain", 'build', '--manifest-path', (Join-Path $repo 'Cargo.toml'), '--package', 'bonsai-prism-runtime', '--example', 'upstream_prism', '--locked', '--offline', '--release', '--target-dir', $target, '--message-format=json')
$executables = @($compiled.StandardOutput -split "`n" | Where-Object { $_.StartsWith('{') } | ForEach-Object {
    $record = $_ | ConvertFrom-Json -AsHashtable
    if ($record.reason -eq 'compiler-artifact' -and $record.target.name -eq 'upstream_prism' -and $record.executable) { $record.executable }
})
if ($executables.Count -ne 1) { throw 'Expected exactly one compiled shared-assertion Rust harness.' }
$harness = $executables[0]
$referenceCases = [Collections.Generic.List[object]]::new()
$rustCases = [Collections.Generic.List[object]]::new()
# Adapter commands below are intentionally explicit: upstream includes model
# downloaders and benchmarks that cannot be safely selected by a filename glob.
$selected = if ($Case -eq 'All') { @('Pq2RowShapes', 'Ptq1ElementMap') } else { @($Case) }
foreach ($selection in $selected) {
    $caseName = if ($selection -eq 'Pq2RowShapes') { 'test-pq2-row-shapes' } else { 'test-ptq1_0-element-map' }
    $binaryName = if ($selection -eq 'Pq2RowShapes') { 'prism-upstream-pq2-row-shapes' } else { 'prism-upstream-ptq1-element-map' }
    $extension = if ($IsWindows) { '.exe' } else { '' }
    $binaries = @(Get-ChildItem -LiteralPath $build -Recurse -File -Filter ($binaryName + $extension))
    if ($binaries.Count -ne 1) { throw "Expected exactly one native $binaryName executable." }
    $null = Invoke-UpstreamCommand -Executable $binaries[0].FullName -Arguments @() -TimeoutSeconds 120
    $fixtureName = if ($selection -eq 'Pq2RowShapes') { 'pq2' } else { 'ptq1' }
    $fixture = Join-Path $evidence "$fixtureName-fixtures.bin"
    $exporters = @(Get-ChildItem -LiteralPath $build -Recurse -File -Filter ("prism-upstream-$fixtureName-fixtures" + $extension))
    if ($exporters.Count -ne 1) { throw 'Expected exactly one shared-fixture exporter.' }
    $null = Invoke-UpstreamCommand -Executable $exporters[0].FullName -Arguments @($fixture) -TimeoutSeconds 120
    foreach ($engine in @('cpp', 'rust')) {
        $output = Join-Path $evidence "$caseName-$engine.json"
        $arguments = @('--target', $engine, '--case', $caseName, '--output', $output)
        $arguments += @("--$fixtureName-fixtures", $fixture)
        $null = Invoke-UpstreamCommand -Executable $harness -Arguments $arguments -TimeoutSeconds 120
        $result = Get-Content -LiteralPath $output -Raw | ConvertFrom-Json -AsHashtable
        if ($result.SchemaVersion -ne 1 -or $result.UpstreamCommit -cne $lock.Commit -or $result.Target -cne $engine -or
            @($result.Cases).Count -ne 1 -or $result.Cases[0].Id -cne "tests/$caseName.cpp") {
            throw 'Shared assertion harness returned an unexpected case identity.'
        }
        if ($engine -eq 'cpp') { $referenceCases.Add($result.Cases[0]) }
        else { $rustCases.Add($result.Cases[0]) }
    }
}
if ($Case -eq 'All') {
    $raw = & (Join-Path $repo 'scripts/Invoke-PrismCppSuite.ps1') -SourceDirectory $source -EvidenceDirectory (Join-Path $evidence 'raw-cpp') -CMakePath $CMakePath -Parallel $Parallel
    if (@($raw.ReferenceResults.Cases).Count -ne @($raw.Targets).Count) { throw 'The raw C++ importer lost an expected result.' }
    foreach ($name in $raw.Targets) {
        if (@($raw.ReferenceResults.Cases | Where-Object { $_.Id -ceq "tests/$name.cpp" }).Count -ne 1) {
            throw 'The raw C++ importer returned a missing or duplicate suite identity.'
        }
    }
    foreach ($result in $raw.ReferenceResults.Cases) { $referenceCases.Add($result) }
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
