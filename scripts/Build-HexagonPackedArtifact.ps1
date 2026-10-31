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
$target = Join-Path $output 'target'
$library = Invoke-BonsaiNativeProcess -Executable $rustup -ArgumentList @('run',$toolchain,'cargo','build','--locked','--release','--target','aarch64-pc-windows-msvc','--manifest-path',(Join-Path $GgufSourceDirectory 'Cargo.toml'),'--target-dir',$target,'-p','bonsai-gguf','--lib') -WorkingDirectory $output -TimeoutSeconds 180 -OwnDescendants -LogPath (Join-Path $output 'gguf-build.txt')
if ($library.ExitCode -ne 0) { throw 'The pinned GGUF library did not build.' }
$release = Join-Path $target 'aarch64-pc-windows-msvc/release'
$dependencyDirectory = Join-Path $release 'deps'
$gguf = Join-Path $release 'bonsai_gguf.rlib'
if (-not (Test-Path -LiteralPath $gguf)) { $gguf = Join-Path $release 'libbonsai_gguf.rlib' }
$sha = @([IO.Directory]::EnumerateFiles($dependencyDirectory, 'libsha2-*.rlib'))
if (-not [IO.File]::Exists($gguf) -or $sha.Count -ne 1) { throw 'Pinned GGUF dependency library selection is ambiguous.' }
$source = Join-Path $repo 'tools/hexagon/packed_artifact.rs'
$manifest = Join-Path $repo 'tools/hexagon/diagnostic.manifest'
$bundle = Join-Path $output 'bundle'
[void][IO.Directory]::CreateDirectory($bundle)
$exe = Join-Path $bundle 'hexagon-packed-artifact.exe'
$test = Join-Path $output 'packed-artifact-contract.exe'
$base = @('run',$toolchain,'rustc','--edition=2024','--target','aarch64-pc-windows-msvc','-C','opt-level=2','-C','overflow-checks=yes','-D','warnings','--extern',('bonsai_gguf=' + $gguf),'--extern',('sha2=' + $sha[0]),'-L',('dependency=' + $dependencyDirectory),'-C','link-arg=/MANIFEST:EMBED','-C',('link-arg=/MANIFESTINPUT:' + $manifest))
foreach ($case in @(@{ Name='extractor'; Arguments=@($source,'-o',$exe) },@{ Name='contract'; Arguments=@('--test',$source,'-o',$test) })) {
    $built = Invoke-BonsaiNativeProcess -Executable $rustup -ArgumentList ($base + $case.Arguments) -WorkingDirectory $output -TimeoutSeconds 120 -OwnDescendants -LogPath (Join-Path $output ($case.Name + '-build.txt'))
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
    CompilerIdentitySha256 = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($compiler.StandardOutput))).ToLowerInvariant()
    SourceSha256 = (Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash.ToLowerInvariant()
    ManifestSha256 = (Get-FileHash -LiteralPath $manifest -Algorithm SHA256).Hash.ToLowerInvariant()
    GgufLibrarySha256 = (Get-FileHash -LiteralPath $gguf -Algorithm SHA256).Hash.ToLowerInvariant()
    Sha2LibrarySha256 = (Get-FileHash -LiteralPath $sha[0] -Algorithm SHA256).Hash.ToLowerInvariant()
    DependencyCargoLockSha256 = (Get-FileHash -LiteralPath (Join-Path $GgufSourceDirectory 'Cargo.lock') -Algorithm SHA256).Hash.ToLowerInvariant()
    Executable = [ordered]@{ Name='hexagon-packed-artifact.exe'; Bytes=([IO.FileInfo]$exe).Length; Sha256=(Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash.ToLowerInvariant() }
    ModelLoaded = $false; DeviceCodeLoaded = $false; ProfileEligible = $false
}
$receipt | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $bundle 'artifact-build.json') -Encoding utf8NoBOM
return $receipt
