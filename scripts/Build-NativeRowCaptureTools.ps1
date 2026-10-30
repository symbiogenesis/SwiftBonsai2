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
$archive = Join-Path $scratch ($source.commit + '.zip')
Receive-LockedArtifact -Uri $source.url -Path $archive -Bytes $source.bytes -Sha256 $source.sha256 -TimeoutSeconds 180 | Out-Null
Expand-Archive -LiteralPath $archive -DestinationPath (Join-Path $scratch 'source')
$manifest = Join-Path $scratch ('source/prism-inference-' + $source.commit + '/Cargo.toml')
$channel = 'nightly-2026-10-22-aarch64-pc-windows-msvc'
$cargo = @(Get-Command cargo -CommandType Application -ErrorAction Stop)[0].Source
$rustc = @(Get-Command rustc -CommandType Application -ErrorAction Stop)[0].Source
$compiler = Invoke-BonsaiNativeProcess -Executable $rustc -WorkingDirectory $scratch -TimeoutSeconds 30 -LogPath (Join-Path $scratch 'rustc.txt') -ArgumentList @("+$channel", '-vV')
if ($compiler.ExitCode -ne 0 -or $compiler.StandardOutput -notmatch 'host: aarch64-pc-windows-msvc') {
    throw 'The pinned native ARM64 Rust compiler is unavailable.'
}
$target = Join-Path $scratch 'build'
$arguments = @("+$channel", 'build', '--locked', '--release', '--manifest-path', $manifest,
    '-p', 'prism-inference', '--features', 'parallel-rows', '--example', 'swift_model_check', '--target-dir', $target)
$built = Invoke-BonsaiNativeProcess -Executable $cargo -WorkingDirectory $scratch -TimeoutSeconds 1200 -OwnDescendants -LogPath (Join-Path $scratch 'build.txt') -ArgumentList $arguments
if ($built.ExitCode -ne 0) { throw "Hosted row checker build failed; inspect $scratch" }
$checker = Join-Path $target 'release/examples/swift_model_check.exe'
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
[pscustomobject]@{ Status = 'hosted-row-capture-tools-built'; OutputDirectory = $output; ReceiptPath = $receiptPath
    EvidenceDirectory = $scratch; ModelProcessStarted = $false }
