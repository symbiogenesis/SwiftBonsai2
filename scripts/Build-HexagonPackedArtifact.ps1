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
# Use the same owned LLD build convention as the hosted native-row tooling.
# MSVC's reusable PDB server must not outlive an otherwise completed compiler.
$rustFlags = @('-C',('linker=' + $linker),'-C','linker-flavor=lld-link')
$library = Invoke-BonsaiNativeProcess -Executable $cargo -ArgumentList @('build','--locked','--release','--no-default-features','--message-format=json-render-diagnostics','--manifest-path',(Join-Path $GgufSourceDirectory 'Cargo.toml'),'--target-dir',$target,'-p','bonsai-gguf','--lib') -WorkingDirectory $output -TimeoutSeconds 180 -OwnDescendants -LogPath (Join-Path $output 'gguf-build.txt') -Environment @{
    RUSTC=$rustc; RUSTC_WRAPPER=''; RUSTC_WORKSPACE_WRAPPER=''
    CARGO_ENCODED_RUSTFLAGS=$rustFlags -join [char]0x1f
    CARGO_TARGET_AARCH64_PC_WINDOWS_MSVC_LINKER=$linker
}
if ($library.ExitCode -ne 0) { throw 'The pinned GGUF library did not build.' }
# Bind Cargo's actual artifact paths instead of guessing host/target subdirectories.
$ggufPaths = [Collections.Generic.List[string]]::new()
$shaPaths = [Collections.Generic.List[string]]::new()
$ggufMetadata = [Collections.Generic.List[string]]::new()
$shaMetadata = [Collections.Generic.List[string]]::new()
$searchDirectories = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
foreach ($line in $library.StandardOutput -split '\r?\n') {
    if (-not $line) { continue }
    $message = ConvertFrom-Json -InputObject $line -AsHashtable -NoEnumerate -Depth 32
    if ($message.reason -cne 'compiler-artifact' -or $message.target.kind -cnotcontains 'lib') { continue }
    foreach ($path in $message.filenames) {
        $extension = [IO.Path]::GetExtension($path)
        if ($extension -cnotin @('.rlib','.rmeta')) { continue }
        $full = [IO.Path]::GetFullPath($path)
        if (-not $full.StartsWith($target + [IO.Path]::DirectorySeparatorChar,[StringComparison]::OrdinalIgnoreCase)) { throw 'Cargo artifact escaped the owned target directory.' }
        [void]$searchDirectories.Add([IO.Path]::GetDirectoryName($full))
        if ($message.target.name -ceq 'bonsai_gguf') {
            if ([IO.Path]::GetFullPath($message.manifest_path) -ine [IO.Path]::GetFullPath((Join-Path $GgufSourceDirectory 'tools/gguf/Cargo.toml'))) { throw 'Cargo compiled an unrelated GGUF source manifest.' }
            if ($extension -ceq '.rmeta') { $ggufMetadata.Add($full) } else { $ggufPaths.Add($full) }
        } elseif ($message.target.name -ceq 'sha2' -and $message.package_id -cmatch '#sha2@0\.11\.1$') {
            if ($extension -ceq '.rmeta') { $shaMetadata.Add($full) } else { $shaPaths.Add($full) }
        }
    }
}
if ($ggufPaths.Count -ne 1 -or $shaPaths.Count -ne 1 -or $ggufMetadata.Count -ne 1 -or $shaMetadata.Count -ne 1) { throw 'Pinned GGUF dependency library selection is ambiguous.' }
$gguf = $ggufPaths[0]
$sha = @($shaPaths[0])
$searchArguments = @(foreach ($directory in $searchDirectories) { '-L'; 'dependency=' + $directory })
$source = Join-Path $repo 'tools/hexagon/packed_artifact.rs'
$manifest = Join-Path $repo 'tools/hexagon/diagnostic.manifest'
$bundle = Join-Path $output 'bundle'
[void][IO.Directory]::CreateDirectory($bundle)
$exe = Join-Path $bundle 'hexagon-packed-artifact.exe'
$test = Join-Path $output 'packed-artifact-contract.exe'
$base = @('--edition=2024','--target','aarch64-pc-windows-msvc','-C','opt-level=2','-C','overflow-checks=yes','-C',('linker=' + $linker),'-C','linker-flavor=lld-link','-D','warnings','--extern',('bonsai_gguf=' + $ggufMetadata[0]),'--extern',('sha2=' + $shaMetadata[0]),'-C','link-arg=/MANIFEST:EMBED','-C',('link-arg=/MANIFESTINPUT:' + $manifest)) + $searchArguments
foreach ($case in @(@{ Name='extractor'; Arguments=@($source,'-o',$exe) },@{ Name='contract'; Arguments=@('--test',$source,'-o',$test) })) {
    $built = Invoke-BonsaiNativeProcess -Executable $rustc -ArgumentList ($base + $case.Arguments) -WorkingDirectory $output -TimeoutSeconds 120 -OwnDescendants -LogPath (Join-Path $output ($case.Name + '-build.txt'))
    if ($built.ExitCode -ne 0) { throw 'The narrow projection extractor did not compile.' }
}
foreach ($case in @(@{ Name='self-test'; Executable=$exe; Arguments=@('--self-test') },@{ Name='contract-tests'; Executable=$test; Arguments=@() })) {
    $checked = Invoke-BonsaiNativeProcess -Executable $case.Executable -ArgumentList $case.Arguments -WorkingDirectory $output -TimeoutSeconds 15 -OwnDescendants
    [IO.File]::WriteAllText((Join-Path $output ($case.Name + '.txt')), $checked.StandardOutput + $checked.StandardError)
    if ($checked.ExitCode -ne 0) { throw 'The model-free extraction controls failed.' }
}
$receipt = [ordered]@{
    SchemaVersion = 1; RunnerRevision = $env:GITHUB_SHA; RunId = $env:GITHUB_RUN_ID; RunAttempt = $env:GITHUB_RUN_ATTEMPT
    HostedBuild = $true; Target = 'aarch64-pc-windows-msvc'; RustToolchain = $toolchain; GgufRevision = $pin
    GgufDefaultFeatures = $false; LinkerFlavor = 'lld-link'; ToolImages = $images
    CompilerIdentitySha256 = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($compiler.StandardOutput))).ToLowerInvariant()
    SourceSha256 = (Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash.ToLowerInvariant()
    ManifestSha256 = (Get-FileHash -LiteralPath $manifest -Algorithm SHA256).Hash.ToLowerInvariant()
    GgufMetadataSha256 = (Get-FileHash -LiteralPath $ggufMetadata[0] -Algorithm SHA256).Hash.ToLowerInvariant()
    Sha2MetadataSha256 = (Get-FileHash -LiteralPath $shaMetadata[0] -Algorithm SHA256).Hash.ToLowerInvariant()
    GgufLibrarySha256 = (Get-FileHash -LiteralPath $gguf -Algorithm SHA256).Hash.ToLowerInvariant()
    Sha2LibrarySha256 = (Get-FileHash -LiteralPath $sha[0] -Algorithm SHA256).Hash.ToLowerInvariant()
    DependencyCargoLockSha256 = (Get-FileHash -LiteralPath (Join-Path $GgufSourceDirectory 'Cargo.lock') -Algorithm SHA256).Hash.ToLowerInvariant()
    Executable = [ordered]@{ Name='hexagon-packed-artifact.exe'; Bytes=([IO.FileInfo]$exe).Length; Sha256=(Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash.ToLowerInvariant() }
    ModelLoaded = $false; DeviceCodeLoaded = $false; ProfileEligible = $false
}
$receipt | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $bundle 'artifact-build.json') -Encoding utf8NoBOM
return $receipt
