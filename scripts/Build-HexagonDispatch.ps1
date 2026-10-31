#requires -Version 7.6
<#
.SYNOPSIS
Builds source-bound, model-free Hexagon diagnostics on hosted Windows ARM64 CI.
.DESCRIPTION
Compiles standalone Rust diagnostics; no inference libraries are linked at build
time. Self checks never load driver or inference DLLs. Device execution is a
separate, explicitly bounded operation. The receipt identifies source and compiler
inputs; trusted same-run artifact intake remains the caller's responsibility.
#>
[CmdletBinding()]
[OutputType([Collections.Specialized.OrderedDictionary])]
param([Parameter(Mandatory)][string]$OutputDirectory)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1')
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Native.psm1')
if (-not $IsWindows -or [Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture -cne 'Arm64' -or
    $env:GITHUB_ACTIONS -cne 'true' -or $env:RUNNER_ENVIRONMENT -cne 'github-hosted') {
    throw 'Hexagon dispatch preparation requires hosted Windows ARM64 CI; do not build a device trial on the owner laptop.'
}
if ($env:GITHUB_SHA -cnotmatch '^[a-f0-9]{40}$' -or $env:GITHUB_RUN_ID -cnotmatch '^[0-9]{1,32}$') {
    throw 'Hosted preparation requires an exact source revision and run identity.'
}
$output = Resolve-ExternalPath -Path $OutputDirectory
if ([IO.Directory]::Exists($output) -or [IO.File]::Exists($output)) { throw 'OutputDirectory must be new external storage.' }
[void][IO.Directory]::CreateDirectory($output)
$bundle = Join-Path $output 'bundle'
[void][IO.Directory]::CreateDirectory($bundle)
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$rustup = (Get-Command -Name rustup.exe -CommandType Application -ErrorAction Stop).Source
$toolchain = 'nightly-2026-10-22-aarch64-pc-windows-msvc'
$source = Join-Path $repo 'tools/hexagon/dispatch_fixture.rs'
$manifest = Join-Path $repo 'tools/hexagon/diagnostic.manifest'
$lock = Join-Path $repo 'config/hexagon-dispatch.lock.json'
$compiler = Invoke-BonsaiNativeProcess -Executable $rustup -ArgumentList @('run', $toolchain, 'rustc', '-vV') -WorkingDirectory $output -TimeoutSeconds 20 -OwnDescendants
if ($compiler.ExitCode -ne 0 -or $compiler.StandardOutput -cnotmatch '(?m)^host: aarch64-pc-windows-msvc\r?$') { throw 'Native compiler identity did not match the pinned target.' }
$compilerDigest = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($compiler.StandardOutput))).ToLowerInvariant()
$compiledExe = Join-Path $output 'hexagon-dispatch-fixture.exe'
$exe = Join-Path $bundle 'hexagon-dispatch-fixture.exe'
$arguments = @('run', $toolchain, 'rustc', '--edition=2024', '--target', 'aarch64-pc-windows-msvc', '-C', 'opt-level=2', '-C', 'overflow-checks=yes', '-C', 'link-arg=/MANIFEST:EMBED', '-C', ('link-arg=/MANIFESTINPUT:' + $manifest), '-D', 'warnings', $source, '-o', $compiledExe)
$build = Invoke-BonsaiNativeProcess -Executable $rustup -ArgumentList $arguments -WorkingDirectory $output -TimeoutSeconds 120 -OwnDescendants -LogPath (Join-Path $output 'dispatch-build.txt')
if ($build.ExitCode -ne 0) { throw 'Hexagon dispatch fixture compilation failed.' }
Copy-Item -LiteralPath $compiledExe -Destination $exe
$check = Invoke-BonsaiNativeProcess -Executable $exe -ArgumentList @('--self-test') -WorkingDirectory $output -TimeoutSeconds 15 -OwnDescendants
if ($check.ExitCode -ne 0 -or $check.StandardOutput.Length -gt 65536 -or $check.StandardError.Length -gt 65536) { throw 'Model-free dispatch control failed.' }
[IO.File]::WriteAllText((Join-Path $output 'dispatch-self-test.json'), $check.StandardOutput)
$receipt = [ordered]@{
    SchemaVersion = 1; RunnerRevision = $env:GITHUB_SHA
    RunId = $env:GITHUB_RUN_ID; RunAttempt = $env:GITHUB_RUN_ATTEMPT
    RustToolchain = $toolchain; Target = 'aarch64-pc-windows-msvc'; HostedBuild = $true
    CompilerIdentitySha256 = $compilerDigest
    SourceSha256 = (Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash.ToLowerInvariant()
    ManifestSha256 = (Get-FileHash -LiteralPath $manifest -Algorithm SHA256).Hash.ToLowerInvariant()
    LockSha256 = (Get-FileHash -LiteralPath $lock -Algorithm SHA256).Hash.ToLowerInvariant()
    Executable = [ordered]@{ Name = 'hexagon-dispatch-fixture.exe'; Bytes = ([IO.FileInfo]$exe).Length; Sha256 = (Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash.ToLowerInvariant() }
    DeviceCodeLoaded = $false; DeviceExecutionQualified = $false
}
$receipt | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $bundle 'fixture-build.json') -Encoding utf8NoBOM
if (@([IO.Directory]::EnumerateFileSystemEntries($bundle)).Count -ne 2) { throw 'The source-bound fixture bundle must contain exactly two entries.' }
# The access bundle intentionally contains exactly the fixture and its receipt.
# Keep compilation evidence and the separate direct-driver controls outside it.
$nativeOutput = Join-Path $output 'native-rpc'
[void][IO.Directory]::CreateDirectory($nativeOutput)
$nativeSource = Join-Path $repo 'tools/hexagon/native_rpc'
$nativeLibrary = Join-Path $nativeOutput 'libhexagon_native_rpc.rlib'
$nativeExe = Join-Path $nativeOutput 'native-hexagon-query.exe'
$nativeTest = Join-Path $nativeOutput 'native-hexagon-contract.exe'
foreach ($buildCase in @(
    @{ Name = 'library'; Arguments = @('--crate-type','lib','--crate-name','hexagon_native_rpc',(Join-Path $nativeSource 'lib.rs'),'-o',$nativeLibrary) },
    @{ Name = 'query'; Arguments = @((Join-Path $nativeSource 'query.rs'),'--extern',('hexagon_native_rpc=' + $nativeLibrary),'-o',$nativeExe) },
    @{ Name = 'contract'; Arguments = @('--test',(Join-Path $nativeSource 'lib.rs'),'-o',$nativeTest) }
)) {
    $compileArguments = @('run',$toolchain,'rustc','--edition=2024','--target','aarch64-pc-windows-msvc','-C','opt-level=2','-C','overflow-checks=yes','-C','link-arg=/MANIFEST:EMBED','-C',('link-arg=/MANIFESTINPUT:' + $manifest),'-D','warnings') + $buildCase.Arguments
    $nativeBuild = Invoke-BonsaiNativeProcess -Executable $rustup -ArgumentList $compileArguments -WorkingDirectory $output -TimeoutSeconds 120 -OwnDescendants -LogPath (Join-Path $nativeOutput ($buildCase.Name + '-build.txt'))
    if ($nativeBuild.ExitCode -ne 0) { throw 'Direct Rust native capability control failed to compile.' }
}
$nativeCheck = Invoke-BonsaiNativeProcess -Executable $nativeExe -ArgumentList @('--self-test') -WorkingDirectory $output -TimeoutSeconds 15 -OwnDescendants
if ($nativeCheck.ExitCode -ne 0) { throw 'Direct Rust model-free capability self check failed.' }
[IO.File]::WriteAllText((Join-Path $nativeOutput 'self-test.json'),$nativeCheck.StandardOutput)
$contractCheck = Invoke-BonsaiNativeProcess -Executable $nativeTest -WorkingDirectory $output -TimeoutSeconds 15 -OwnDescendants
[IO.File]::WriteAllText((Join-Path $nativeOutput 'contract-tests.txt'),$contractCheck.StandardOutput + $contractCheck.StandardError)
if ($contractCheck.ExitCode -ne 0) { throw 'Direct Rust capability contract regressions failed.' }
$nativeInputs = @(foreach ($name in @('lib.rs','windows.rs','mapping.rs','session.rs','session_native.rs','query.rs','sources.json')) {
    $inputPath = Join-Path $nativeSource $name
    [ordered]@{ Name = $name; Sha256 = (Get-FileHash -LiteralPath $inputPath -Algorithm SHA256).Hash.ToLowerInvariant() }
})
[ordered]@{
    SchemaVersion = 1; RunnerRevision = $env:GITHUB_SHA
    RunId = $env:GITHUB_RUN_ID; RustToolchain = $toolchain; Target = 'aarch64-pc-windows-msvc'
    CompilerIdentitySha256 = $compilerDigest; SourceFiles = $nativeInputs
    ExecutableSha256 = (Get-FileHash -LiteralPath $nativeExe -Algorithm SHA256).Hash.ToLowerInvariant()
    DeviceCodeLoaded = $false; DspCodeLoaded = $false; ProfileEligible = $false
} | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $nativeOutput 'native-build.json') -Encoding utf8NoBOM
$packedOutput = Join-Path $output 'packed-projection'
[void][IO.Directory]::CreateDirectory($packedOutput)
$packedSource = Join-Path $repo 'tools/hexagon/packed_projection.rs'
$packedTest = Join-Path $packedOutput 'packed-projection-contract.exe'
$packedBuild = Invoke-BonsaiNativeProcess -Executable $rustup -ArgumentList @('run',$toolchain,'rustc','--edition=2024','--target','aarch64-pc-windows-msvc','--test','-C','opt-level=2','-C','overflow-checks=yes','-D','warnings',$packedSource,'-o',$packedTest) -WorkingDirectory $output -TimeoutSeconds 120 -OwnDescendants -LogPath (Join-Path $packedOutput 'build.txt')
if ($packedBuild.ExitCode -ne 0) { throw 'The CPU packed-projection control failed to compile.' }
$packedCheck = Invoke-BonsaiNativeProcess -Executable $packedTest -WorkingDirectory $output -TimeoutSeconds 30 -OwnDescendants
[IO.File]::WriteAllText((Join-Path $packedOutput 'contract-tests.txt'),$packedCheck.StandardOutput + $packedCheck.StandardError)
if ($packedCheck.ExitCode -ne 0) { throw 'The CPU packed-projection contract regressions failed.' }
$packedInputs = @(foreach ($name in @('packed_projection.rs','packed-sources.json','verify_packed_fixture.py','fixtures/swift-q8k-parity.bin','fixtures/swift-q8k-parity.json')) {
    [ordered]@{ Name = $name; Sha256 = (Get-FileHash -LiteralPath (Join-Path $repo ('tools/hexagon/' + $name)) -Algorithm SHA256).Hash.ToLowerInvariant() }
})
[ordered]@{
    SchemaVersion = 1; RunnerRevision = $env:GITHUB_SHA; RunId = $env:GITHUB_RUN_ID
    CompilerIdentitySha256 = $compilerDigest; Target = 'aarch64-pc-windows-msvc'
    SourceSha256 = (Get-FileHash -LiteralPath $packedSource -Algorithm SHA256).Hash.ToLowerInvariant()
    SourceFiles = $packedInputs
    ExecutableSha256 = (Get-FileHash -LiteralPath $packedTest -Algorithm SHA256).Hash.ToLowerInvariant()
    ExecutionBackend = 'CPU-control-only'; DspCodeLoaded = $false; ProfileEligible = $false
} | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $packedOutput 'build.json') -Encoding utf8NoBOM
Write-Output $receipt
