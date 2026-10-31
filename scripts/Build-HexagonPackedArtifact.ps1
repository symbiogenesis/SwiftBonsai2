#requires -Version 7.6
<# .SYNOPSIS
Builds the narrow verified Swift projection extractor against pinned Prism on hosted ARM64.
#>
[CmdletBinding()]
[OutputType([Collections.Specialized.OrderedDictionary])]
param([Parameter(Mandatory)][string]$GgufSourceDirectory, [Parameter(Mandatory)][string]$OutputDirectory)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1')
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Native.psm1')
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Hexagon.psm1')
if (-not $IsWindows -or [Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture -cne 'Arm64' -or
    $env:GITHUB_ACTIONS -cne 'true' -or $env:RUNNER_ENVIRONMENT -cne 'github-hosted') {
    throw 'Projection-extractor preparation requires hosted Windows ARM64; do not build an owner-device trial locally.'
}
if ($env:GITHUB_SHA -cnotmatch '^[a-f0-9]{40}$' -or $env:GITHUB_RUN_ID -cnotmatch '^[0-9]{1,32}$' -or $env:GITHUB_RUN_ATTEMPT -cnotmatch '^[0-9]{1,8}$') {
    throw 'Hosted preparation requires exact source and run identities.'
}
$GgufSourceDirectory = Get-BonsaiHexagonLocalPath -Path $GgufSourceDirectory
$output = Resolve-ExternalPath -Path $OutputDirectory
if (Test-Path -LiteralPath $output) { throw 'OutputDirectory must be new external storage.' }
[void][IO.Directory]::CreateDirectory($output)
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$pin = '7aca2251f09acb345d4afb53d5ce411f81017745'
$git = (Get-Command git.exe -CommandType Application -ErrorAction Stop | Select-Object -First 1).Source
$root = Invoke-BonsaiNativeProcess -Executable $git -ArgumentList @('-C',$GgufSourceDirectory,'rev-parse','--show-toplevel') -WorkingDirectory $output -TimeoutSeconds 10 -OwnDescendants
if ($root.ExitCode -ne 0 -or
    -not [IO.Path]::TrimEndingDirectorySeparator([IO.Path]::GetFullPath($root.StandardOutput.Trim())).Equals([IO.Path]::TrimEndingDirectorySeparator($GgufSourceDirectory), [StringComparison]::OrdinalIgnoreCase)) {
    throw 'The GGUF source path must be the exact pinned checkout root.'
}
$revision = Invoke-BonsaiNativeProcess -Executable $git -ArgumentList @('-C',$GgufSourceDirectory,'rev-parse','HEAD') -WorkingDirectory $output -TimeoutSeconds 10 -OwnDescendants
$status = Invoke-BonsaiNativeProcess -Executable $git -ArgumentList @('-C',$GgufSourceDirectory,'status','--porcelain') -WorkingDirectory $output -TimeoutSeconds 10 -OwnDescendants
if ($revision.ExitCode -ne 0 -or $revision.StandardOutput.Trim() -cne $pin -or $status.ExitCode -ne 0 -or $status.StandardOutput.Trim()) { throw 'Prism dependency checkout differs from the exact clean reviewed revision.' }
$rustup = (Get-Command rustup.exe -CommandType Application -ErrorAction Stop | Select-Object -First 1).Source
$toolchain = 'nightly-2026-10-22-aarch64-pc-windows-msvc'
$compiler = Invoke-BonsaiNativeProcess -Executable $rustup -ArgumentList @('run',$toolchain,'rustc','-vV') -WorkingDirectory $output -TimeoutSeconds 20 -OwnDescendants
if ($compiler.ExitCode -ne 0 -or $compiler.StandardOutput -cnotmatch '(?m)^host: aarch64-pc-windows-msvc\r?$') { throw 'Compiler identity differs from the pinned ARM64 target.' }
$resolvedCargo = Invoke-BonsaiNativeProcess -Executable $rustup -ArgumentList @('which','--toolchain',$toolchain,'cargo') -WorkingDirectory $output -TimeoutSeconds 20 -OwnDescendants
$resolvedRustc = Invoke-BonsaiNativeProcess -Executable $rustup -ArgumentList @('which','--toolchain',$toolchain,'rustc') -WorkingDirectory $output -TimeoutSeconds 20 -OwnDescendants
if ($resolvedCargo.ExitCode -ne 0 -or $resolvedRustc.ExitCode -ne 0) { throw 'Pinned toolchain images are unavailable.' }
$cargo = Resolve-ExternalPath ($resolvedCargo.StandardOutput.Trim())
$rustc = Resolve-ExternalPath ($resolvedRustc.StandardOutput.Trim())
$toolchainBin = [IO.Path]::GetDirectoryName($rustc)
if ($cargo -cne (Join-Path $toolchainBin 'cargo.exe')) { throw 'Cargo and rustc must belong to the same pinned toolchain.' }
$linker = Join-Path ([IO.Path]::GetDirectoryName($toolchainBin)) 'lib/rustlib/aarch64-pc-windows-msvc/bin/rust-lld.exe'
$images = @(foreach ($path in @($cargo,$rustc,$linker)) {
    Assert-Arm64PE $path
    [ordered]@{ Name=[IO.Path]::GetFileName($path); Bytes=([IO.FileInfo]$path).Length; Sha256=(Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant() }
})
$target = Join-Path $output 'target'
$source = Join-Path $repo 'tools/hexagon/packed_artifact.rs'
$manifest = Join-Path $repo 'tools/hexagon/diagnostic.manifest'
# Compile the exact source as a temporary example in the clean pinned reader.
# Cargo owns split metadata/link discovery; no tracked dependency source changes.
$examples = Join-Path $GgufSourceDirectory 'tools/gguf/examples'
[void][IO.Directory]::CreateDirectory($examples)
$example = Join-Path $examples 'packed_artifact.rs'
[IO.File]::Copy($source,$example,$false)
$sourceDigest = (Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash.ToLowerInvariant()
if ((Get-FileHash -LiteralPath $example -Algorithm SHA256).Hash.ToLowerInvariant() -cne $sourceDigest) { throw 'The temporary example differs from the reviewed extractor source.' }
# Use the same owned LLD convention as the hosted native-row tooling. MSVC's
# reusable PDB server must not outlive an otherwise completed compiler.
$rustFlags = @('-C',('linker=' + $linker),'-C','linker-flavor=lld-link','-C','link-arg=/MANIFEST:EMBED','-C',('link-arg=/MANIFESTINPUT:' + $manifest))
$environment = @{
    RUSTC=$rustc; RUSTC_WRAPPER=''; RUSTC_WORKSPACE_WRAPPER=''
    CARGO_ENCODED_RUSTFLAGS=$rustFlags -join [char]0x1f
    CARGO_TARGET_AARCH64_PC_WINDOWS_MSVC_LINKER=$linker
}
$arguments = @('--locked','--release','--no-default-features','--manifest-path',(Join-Path $GgufSourceDirectory 'Cargo.toml'),'--target-dir',$target,'-p','bonsai-gguf','--example','packed_artifact')
$built = Invoke-BonsaiNativeProcess -Executable $cargo -ArgumentList (@('build','--message-format=json-render-diagnostics') + $arguments) -WorkingDirectory $output -TimeoutSeconds 180 -OwnDescendants -LogPath (Join-Path $output 'extractor-build.txt') -Environment $environment
if ($built.ExitCode -ne 0) { throw 'The source-bound projection extractor did not compile.' }
$executables = [Collections.Generic.List[string]]::new()
$dependencies = [Collections.Generic.List[object]]::new()
foreach ($line in $built.StandardOutput -split '\r?\n') {
    if (-not $line) { continue }
    $message = ConvertFrom-Json -InputObject $line -AsHashtable -NoEnumerate -Depth 32
    if ($message.reason -cne 'compiler-artifact') { continue }
    if ($message.target.name -ceq 'packed_artifact' -and $message.target.kind -ccontains 'example') {
        if ([IO.Path]::GetFullPath($message.manifest_path) -ine [IO.Path]::GetFullPath((Join-Path $GgufSourceDirectory 'tools/gguf/Cargo.toml')) -or $message.executable -isnot [string]) { throw 'Cargo compiled an unrelated extractor target.' }
        $executables.Add([IO.Path]::GetFullPath($message.executable))
    }
    if ($message.target.kind -ccontains 'lib' -and $message.target.name -cin @('bonsai_gguf','sha2')) {
        foreach ($path in $message.filenames) {
            $full = [IO.Path]::GetFullPath($path)
            if (-not $full.StartsWith($target + [IO.Path]::DirectorySeparatorChar,[StringComparison]::OrdinalIgnoreCase)) { throw 'Dependency artifact escaped the owned target directory.' }
            $dependencies.Add([ordered]@{ Crate=$message.target.name; Name=[IO.Path]::GetFileName($full); Sha256=(Get-FileHash -LiteralPath $full -Algorithm SHA256).Hash.ToLowerInvariant() })
        }
    }
}
if ($executables.Count -ne 1 -or -not $executables[0].StartsWith($target + [IO.Path]::DirectorySeparatorChar,[StringComparison]::OrdinalIgnoreCase)) { throw 'The source-bound extractor executable is ambiguous.' }
Assert-Arm64PE $executables[0]
$bundle = Join-Path $output 'bundle'
[void][IO.Directory]::CreateDirectory($bundle)
$exe = Join-Path $bundle 'hexagon-packed-artifact.exe'
[IO.File]::Copy($executables[0],$exe,$false)
$checked = Invoke-BonsaiNativeProcess -Executable $exe -ArgumentList @('--self-test') -WorkingDirectory $output -TimeoutSeconds 15 -OwnDescendants
[IO.File]::WriteAllText((Join-Path $output 'self-test.txt'),$checked.StandardOutput + $checked.StandardError)
if ($checked.ExitCode -ne 0) { throw 'The model-free extraction self-check failed.' }
$tested = Invoke-BonsaiNativeProcess -Executable $cargo -ArgumentList (@('test') + $arguments + @('--','--exact','tests::fixed_transform_sign_selection_and_malformed_control')) -WorkingDirectory $output -TimeoutSeconds 120 -OwnDescendants -LogPath (Join-Path $output 'contract-tests.txt') -Environment $environment
if ($tested.ExitCode -ne 0 -or $tested.StandardOutput -cnotmatch 'test result: ok\. 1 passed;') { throw 'The hosted extraction regression did not execute and pass exactly one model-free test.' }
$unchanged = Invoke-BonsaiNativeProcess -Executable $git -ArgumentList @('-C',$GgufSourceDirectory,'diff','--exit-code') -WorkingDirectory $output -TimeoutSeconds 10 -OwnDescendants
if ($unchanged.ExitCode -ne 0 -or (Get-FileHash -LiteralPath $example -Algorithm SHA256).Hash.ToLowerInvariant() -cne $sourceDigest) { throw 'Pinned tracked source or temporary example changed during preparation.' }
$receipt = [ordered]@{
    SchemaVersion = 1; RunnerRevision = $env:GITHUB_SHA; RunId = $env:GITHUB_RUN_ID; RunAttempt = $env:GITHUB_RUN_ATTEMPT
    HostedBuild = $true; Target = 'aarch64-pc-windows-msvc'; RustToolchain = $toolchain; GgufRevision = $pin
    GgufDefaultFeatures = $false; LinkerFlavor = 'lld-link'; ToolImages = $images; DependencyArtifacts = $dependencies.ToArray()
    CompilerIdentitySha256 = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($compiler.StandardOutput))).ToLowerInvariant()
    SourceSha256 = $sourceDigest; AddedExampleSourceSha256 = $sourceDigest; PinnedTrackedSourceUnchanged = $true
    ManifestSha256 = (Get-FileHash -LiteralPath $manifest -Algorithm SHA256).Hash.ToLowerInvariant()
    DependencyCargoLockSha256 = (Get-FileHash -LiteralPath (Join-Path $GgufSourceDirectory 'Cargo.lock') -Algorithm SHA256).Hash.ToLowerInvariant()
    Executable = [ordered]@{ Name='hexagon-packed-artifact.exe'; Bytes=([IO.FileInfo]$exe).Length; Sha256=(Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash.ToLowerInvariant() }
    ModelLoaded = $false; DeviceCodeLoaded = $false; ProfileEligible = $false
}
$receipt | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $bundle 'artifact-build.json') -Encoding utf8NoBOM
return $receipt
