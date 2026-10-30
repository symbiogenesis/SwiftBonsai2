#requires -Version 7.6
<# .SYNOPSIS
Builds the exact row-worker checker on hosted ARM64 CI without models.
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
    throw 'Row capture preparation requires this repository main on hosted native Windows ARM64 CI.'
}
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1')
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Native.psm1')
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$pinsPath = Join-Path $repo 'tools/row-parallel/pins.json'
if ((Get-Item -LiteralPath $pinsPath).Length -gt 65536) { throw 'Source pins exceed their pre-read bound.' }
$pins = Get-Content -LiteralPath $pinsPath -Raw | ConvertFrom-Json -DateKind String
$source = $pins.rustSource
if ($source.commit -cnotmatch '^[a-f0-9]{40}$' -or $source.sha256 -cnotmatch '^[a-f0-9]{64}$' -or
    $source.bytes -lt 1 -or $source.bytes -gt 67108864 -or
    $source.url -cne "https://github.com/symbiogenesis/prism-inference/archive/$($source.commit).zip") {
    throw 'Reviewed exact Prism source archive pins are required.'
}
$output = Resolve-ExternalPath $OutputDirectory
if (Test-Path -LiteralPath $output) {
    if (-not [IO.Directory]::Exists($output) -or @(Get-ChildItem -LiteralPath $output -Force).Count) {
        throw 'Capture bundle output must be a new or empty external directory.'
    }
}
[void][IO.Directory]::CreateDirectory($output)
$scratch = Resolve-ExternalPath (Join-Path ([IO.Path]::GetTempPath()) ('Bonsai-HostedRowBuild-' + [guid]::NewGuid().ToString('N')))
[void][IO.Directory]::CreateDirectory($scratch)
$evidence = Join-Path $scratch 'evidence'
[void][IO.Directory]::CreateDirectory($evidence)
$env:BONSAI_ROW_BUILD_EVIDENCE = $evidence
if ($env:GITHUB_ENV) { Add-Content -LiteralPath $env:GITHUB_ENV -Value "BONSAI_ROW_BUILD_EVIDENCE=$evidence" -Encoding utf8NoBOM }
$channel = 'nightly-2026-10-22-aarch64-pc-windows-msvc'
$buildReceipt = [ordered]@{
    SchemaVersion = 1; Scope = 'hosted-row-checker-build-only'; RunnerRevision = $env:GITHUB_SHA
    RustSource = $source; RustToolchain = $channel; BuildFeatures = @('parallel-rows')
    Phase = 'download'; Status = 'running'; ModelProcessStarted = $false; ToolImages = @()
    DescendantOwnership = 'kill-on-close-job; reject-and-stop-leftover-descendants'
}
function Save-RowBuildEvidence {
    <# .SYNOPSIS
    Retains only a small public-source receipt and bounded compiler/build log tails.
    #>
    [CmdletBinding()]
    [OutputType([void])]
    param()
    foreach ($name in @('cargo-path.txt', 'rustc-path.txt', 'rustc.txt', 'build.txt')) {
        $path = Join-Path $scratch $name
        if (-not [IO.File]::Exists($path)) { continue }
        $log = [IO.FileStream]::new($path, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
        try {
            $count = [int][Math]::Min(65536, $log.Length)
            [void]$log.Seek(-$count, [IO.SeekOrigin]::End)
            $tail = [byte[]]::new($count)
            $log.ReadExactly($tail, 0, $tail.Length)
            [IO.File]::WriteAllBytes((Join-Path $evidence $name), $tail)
        } finally { $log.Dispose() }
    }
    $json = [Text.UTF8Encoding]::new($false).GetBytes(($buildReceipt | ConvertTo-Json -Depth 12) + "`n")
    if ($json.Length -gt 65536) { throw 'Hosted row build receipt exceeds its fixed bound.' }
    [IO.File]::WriteAllBytes((Join-Path $evidence 'build-receipt.json'), $json)
}
Save-RowBuildEvidence
try {
    $archive = Join-Path $scratch ($source.commit + '.zip')
    Receive-LockedArtifact -Uri $source.url -Path $archive -Bytes $source.bytes -Sha256 $source.sha256 -TimeoutSeconds 180 | Out-Null
    Expand-Archive -LiteralPath $archive -DestinationPath (Join-Path $scratch 'source')
    $manifest = Join-Path $scratch ('source/prism-inference-' + $source.commit + '/Cargo.toml')
    $buildReceipt.Phase = 'toolchain'
    $rustup = @(Get-Command rustup -CommandType Application -ErrorAction Stop)[0].Source
    $resolvedCargo = Invoke-BonsaiNativeProcess -Executable $rustup -WorkingDirectory $scratch -TimeoutSeconds 30 -LogPath (Join-Path $scratch 'cargo-path.txt') -ArgumentList @('which', '--toolchain', $channel, 'cargo')
    $resolvedRustc = Invoke-BonsaiNativeProcess -Executable $rustup -WorkingDirectory $scratch -TimeoutSeconds 30 -LogPath (Join-Path $scratch 'rustc-path.txt') -ArgumentList @('which', '--toolchain', $channel, 'rustc')
    if ($resolvedCargo.ExitCode -ne 0 -or $resolvedRustc.ExitCode -ne 0) { throw 'The exact pinned Rust executables are unavailable.' }
    $cargo = Resolve-ExternalPath ($resolvedCargo.StandardOutput.Trim())
    $rustc = Resolve-ExternalPath ($resolvedRustc.StandardOutput.Trim())
    $toolchainBin = [IO.Path]::GetDirectoryName($rustc)
    if ($cargo -cne (Join-Path $toolchainBin 'cargo.exe') -or [IO.Path]::GetFileName($rustc) -cne 'rustc.exe') {
        throw 'Cargo and rustc must belong to the same exact pinned toolchain.'
    }
    $toolchainRoot = [IO.Path]::GetDirectoryName($toolchainBin)
    $linker = Join-Path $toolchainRoot 'lib/rustlib/aarch64-pc-windows-msvc/bin/rust-lld.exe'
    $buildReceipt.ToolImages = @(foreach ($path in @($cargo, $rustc, $linker)) {
        Assert-Arm64PE $path
        [ordered]@{ Name = [IO.Path]::GetFileName($path); Bytes = (Get-Item -LiteralPath $path).Length
            Sha256 = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant() }
    })
    $compiler = Invoke-BonsaiNativeProcess -Executable $rustc -WorkingDirectory $scratch -TimeoutSeconds 30 -LogPath (Join-Path $scratch 'rustc.txt') -ArgumentList @('-vV')
    if ($compiler.ExitCode -ne 0 -or $compiler.StandardOutput -notmatch 'host: aarch64-pc-windows-msvc') {
        throw 'The pinned native ARM64 Rust compiler is unavailable.'
    }
    $target = Join-Path $scratch 'build'
    $arguments = @('build', '--locked', '--release', '--manifest-path', $manifest,
        '-p', 'prism-inference', '--features', 'parallel-rows', '--example', 'swift_model_check', '--target-dir', $target)
    # Invoke real toolchain images rather than rustup proxies, and keep linking in
    # the owned compiler tree. LLD writes PDBs itself rather than using the MSVC
    # linker's reusable PDB server. The shared helper must still reject leftovers.
    $rustFlags = @('-C', "linker=$linker", '-C', 'linker-flavor=lld-link')
    $buildReceipt.LinkerFlavor = 'lld-link'
    $buildReceipt.Phase = 'compile'
    Save-RowBuildEvidence
    $built = Invoke-BonsaiNativeProcess -Executable $cargo -WorkingDirectory $scratch -TimeoutSeconds 1200 -OwnDescendants -LogPath (Join-Path $scratch 'build.txt') -ArgumentList $arguments -Environment @{
        RUSTC = $rustc; RUSTC_WRAPPER = ''; RUSTC_WORKSPACE_WRAPPER = ''
        CARGO_ENCODED_RUSTFLAGS = $rustFlags -join [char]0x1f
    }
    if ($built.ExitCode -ne 0) { throw "Hosted row checker build failed; inspect $scratch" }
    $checker = Join-Path $target 'release/examples/swift_model_check.exe'
    $buildReceipt.Phase = 'package'
    Assert-Arm64PE $checker
    $destination = Join-Path $output 'swift_model_check.exe'
    [IO.File]::Copy($checker, $destination, $false)
    $receipt = [ordered]@{
        SchemaVersion = 1; RunnerRevision = $env:GITHUB_SHA; RustSource = $source
        RustToolchain = $channel; BuildFeatures = @('parallel-rows')
        Files = @([ordered]@{ Name = 'swift_model_check.exe'; Bytes = (Get-Item -LiteralPath $destination).Length
            Sha256 = (Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash.ToLowerInvariant() })
    }
    $receiptPath = Join-Path $output 'capture-tools.json'
    $stream = [IO.FileStream]::new($receiptPath, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
    try {
        $bytes = [Text.UTF8Encoding]::new($false).GetBytes(($receipt | ConvertTo-Json -Depth 12) + "`n")
        if ($bytes.Length -gt 65536) { throw 'Capture tool receipt exceeds its fixed bound.' }
        $stream.Write($bytes, 0, $bytes.Length)
        $stream.Flush($true)
    } finally { $stream.Dispose() }
    $buildReceipt.Status = 'completed'
    $buildReceipt.Phase = 'completed'
} catch {
    $buildReceipt.Status = 'failed'
    $message = $_.Exception.Message
    $buildReceipt.Failure = $message.Substring(0, [Math]::Min(4096, $message.Length))
    throw
} finally { Save-RowBuildEvidence }
[pscustomobject]@{ Status = 'hosted-row-capture-tools-built'; OutputDirectory = $output; ReceiptPath = $receiptPath
    EvidenceDirectory = $evidence; ModelProcessStarted = $false }
