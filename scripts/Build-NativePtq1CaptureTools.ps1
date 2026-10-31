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
$source = $pins.rustSource
$archive = Join-Path $scratch ($source.commit + '.zip')
Receive-LockedArtifact -Uri $source.url -Path $archive -Bytes $source.bytes -Sha256 $source.sha256 -TimeoutSeconds 180 | Out-Null
Expand-Archive -LiteralPath $archive -DestinationPath (Join-Path $scratch $source.commit)
$rust = Join-Path $scratch ($pins.rustSource.commit + '/prism-inference-' + $pins.rustSource.commit)
$collectorSourcePath = 'tools/native-reference/swift_ptq1_reference.rs'
$collectorSource = Join-Path $repo $collectorSourcePath
$collectorSourceSha256 = (Get-FileHash -LiteralPath $collectorSource -Algorithm SHA256).Hash.ToLowerInvariant()
$abiControlPath = Join-Path $repo 'tools/native-reference/fixtures/collector-abi-v1.txt'
$abiProvenancePath = Join-Path $repo 'tools/native-reference/fixtures/collector-abi-v1.provenance.json'
if ((Get-Item -LiteralPath $abiControlPath).Length -gt 65536 -or (Get-Item -LiteralPath $abiProvenancePath).Length -gt 65536) {
    throw 'Native ABI controls exceed their fixed pre-read bound.'
}
$abiProvenance = Get-Content -LiteralPath $abiProvenancePath -Raw | ConvertFrom-Json -DateKind String
$abiControlSha256 = (Get-FileHash -LiteralPath $abiControlPath -Algorithm SHA256).Hash.ToLowerInvariant()
$abiProvenanceSha256 = (Get-FileHash -LiteralPath $abiProvenancePath -Algorithm SHA256).Hash.ToLowerInvariant()
if ($abiProvenance.schema_version -ne 1 -or $abiProvenance.source.revision -cne $pins.nativeSource.commit -or
    $abiProvenance.source.headers.'include/llama.h'.sha256 -cne $pins.nativeSource.headerSha256 -or
    $abiProvenance.fixture.file -cne 'collector-abi-v1.txt' -or
    $abiProvenance.fixture.bytes -ne (Get-Item -LiteralPath $abiControlPath).Length -or
    $abiProvenance.fixture.sha256 -cne $abiControlSha256) {
    throw 'Native ABI controls differ from the reviewed source pins.'
}
$channel = 'nightly-2026-10-22-aarch64-pc-windows-msvc'
$rustup = @(Get-Command rustup -CommandType Application -ErrorAction Stop)[0].Source
$resolvedCargo = Invoke-BonsaiNativeProcess -Executable $rustup -WorkingDirectory $scratch -TimeoutSeconds 30 -ArgumentList @('which', '--toolchain', $channel, 'cargo')
$resolvedRustc = Invoke-BonsaiNativeProcess -Executable $rustup -WorkingDirectory $scratch -TimeoutSeconds 30 -ArgumentList @('which', '--toolchain', $channel, 'rustc')
if ($resolvedCargo.ExitCode -ne 0 -or $resolvedRustc.ExitCode -ne 0) { throw 'The exact pinned Rust executables are unavailable.' }
$cargo = Resolve-ExternalPath ($resolvedCargo.StandardOutput.Trim())
$rustc = Resolve-ExternalPath ($resolvedRustc.StandardOutput.Trim())
$toolchainBin = [IO.Path]::GetDirectoryName($rustc)
if ($cargo -cne (Join-Path $toolchainBin 'cargo.exe') -or [IO.Path]::GetFileName($rustc) -cne 'rustc.exe') {
    throw 'Cargo and rustc must belong to the same exact pinned toolchain.'
}
$toolchainRoot = [IO.Path]::GetDirectoryName($toolchainBin)
$linker = Join-Path $toolchainRoot 'lib/rustlib/aarch64-pc-windows-msvc/bin/rust-lld.exe'
foreach ($path in @($cargo, $rustc, $linker)) { Assert-Arm64PE $path }
$rustVersion = Invoke-BonsaiNativeProcess -Executable $rustc -WorkingDirectory $scratch -TimeoutSeconds 30 -ArgumentList @('-vV')
if ($rustVersion.ExitCode -ne 0 -or $rustVersion.StandardOutput -notmatch 'host: aarch64-pc-windows-msvc') {
    throw 'The pinned native ARM64 Rust compiler is unavailable.'
}
$collector = Join-Path $scratch 'swift-ptq1-reference.exe'
$compilerOptions = @('--edition=2024', '--target', 'aarch64-pc-windows-msvc', '--deny', 'warnings',
    '-C', 'opt-level=2', '-C', 'overflow-checks=on', '-C', 'panic=unwind',
    '-C', "linker=$linker", '-C', 'linker-flavor=lld-link')
$compilerArguments = $compilerOptions + @($collectorSource, '-o', $collector)
$built = Invoke-BonsaiNativeProcess -Executable $rustc -WorkingDirectory $scratch -TimeoutSeconds 120 -OwnDescendants -LogPath (Join-Path $scratch 'collector-build.txt') -ArgumentList $compilerArguments
if ($built.ExitCode -ne 0) { throw "Hosted Rust reference collector build failed; inspect $scratch" }
Assert-Arm64PE $collector
$collectorTests = Join-Path $scratch 'collector-tests.exe'
$builtTests = Invoke-BonsaiNativeProcess -Executable $rustc -WorkingDirectory $scratch -TimeoutSeconds 120 -OwnDescendants -LogPath (Join-Path $scratch 'collector-test-build.txt') -ArgumentList ($compilerOptions + @('--test', $collectorSource, '-o', $collectorTests))
if ($builtTests.ExitCode -ne 0) { throw "Hosted Rust collector test build failed; inspect $scratch" }
$tested = Invoke-BonsaiNativeProcess -Executable $collectorTests -WorkingDirectory $scratch -TimeoutSeconds 30 -OwnDescendants -LogPath (Join-Path $scratch 'collector-tests.txt') -ArgumentList @('--test-threads=1')
if ($tested.ExitCode -ne 0) { throw "Hosted Rust collector controls failed; inspect $scratch" }
if ((Get-FileHash -LiteralPath $collectorSource -Algorithm SHA256).Hash.ToLowerInvariant() -cne $collectorSourceSha256 -or
    (Get-FileHash -LiteralPath $abiControlPath -Algorithm SHA256).Hash.ToLowerInvariant() -cne $abiControlSha256 -or
    (Get-FileHash -LiteralPath $abiProvenancePath -Algorithm SHA256).Hash.ToLowerInvariant() -cne $abiProvenanceSha256) {
    throw 'Collector source or ABI controls changed during compilation.'
}
$target = Join-Path $scratch 'rust-build'
$built = Invoke-BonsaiNativeProcess -Executable $cargo -WorkingDirectory $scratch -TimeoutSeconds 1200 -OwnDescendants -LogPath (Join-Path $scratch 'rust-build.txt') -ArgumentList @(
    'build', '--locked', '--release', '--manifest-path', (Join-Path $rust 'Cargo.toml'),
    '-p', 'prism-inference', '--example', 'swift_model_check', '--target-dir', $target) -Environment @{
        RUSTC = $rustc; RUSTC_WRAPPER = ''; RUSTC_WORKSPACE_WRAPPER = ''
        CARGO_ENCODED_RUSTFLAGS = @('-C', "linker=$linker", '-C', 'linker-flavor=lld-link') -join [char]0x1f
    }
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
    SchemaVersion = 2; RunnerRevision = $env:GITHUB_SHA; NativeSource = $pins.nativeSource; RustSource = $pins.rustSource
    CollectorLanguage = 'rust'; CollectorSourcePath = $collectorSourcePath; CollectorSourceSha256 = $collectorSourceSha256
    AbiControlSha256 = $abiControlSha256; AbiProvenanceSha256 = $abiProvenanceSha256; RustToolchain = $channel
    Compiler = [ordered]@{ Path = $rustc; Sha256 = (Get-FileHash -LiteralPath $rustc -Algorithm SHA256).Hash.ToLowerInvariant()
        LinkerSha256 = (Get-FileHash -LiteralPath $linker -Algorithm SHA256).Hash.ToLowerInvariant(); CollectorArguments = $compilerArguments
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
