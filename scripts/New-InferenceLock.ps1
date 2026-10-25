#requires -Version 7.6
<#
.SYNOPSIS
Verifies a reviewed Prism release bundle and writes a new candidate runtime lock.
.DESCRIPTION
Use artifacts from the reviewed successful main-branch Foundation run. Supply
the expected source revision, version and run identity independently of the
receipt, plus the source ZIP downloaded from that exact GitHub commit. This
command checks bytes and identities; the receipt does not prove CI acceptance
or source-to-binary provenance. Review the originating run before promotion.
No network access, source build, model loading or binary execution occurs.
The candidate and verification evidence must be outside Git; existing files
are never replaced. Publishing a release and promoting the pin are separate.
#>
[CmdletBinding()]
[OutputType([pscustomobject])]
param(
    [Parameter(Mandatory)][string]$ReceiptPath,
    [Parameter(Mandatory)][string]$ArchivePath,
    [Parameter(Mandatory)][string]$SourceArchivePath,
    [Parameter(Mandatory)][ValidatePattern('^[a-f0-9]{40}$')][string]$SourceRevision,
    [Parameter(Mandatory)][ValidatePattern('^[0-9]+\.[0-9]+\.[0-9]+$')][string]$Version,
    [Parameter(Mandatory)][ValidatePattern('^[1-9][0-9]*$')][string]$RunId,
    [ValidateRange(1, 2147483647)][int]$RunAttempt = 1,
    [Parameter(Mandatory)][string]$OutputPath
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1')
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Inference.psm1')

$receiptFile = Resolve-ExternalPath $ReceiptPath
$bundleFile = Resolve-ExternalPath $ArchivePath
$sourceFile = Resolve-ExternalPath $SourceArchivePath
$candidateFile = Resolve-ExternalPath $OutputPath
if (Test-Path -LiteralPath $candidateFile) { throw 'Use a new candidate lock output file.' }
$receiptStream = [IO.FileStream]::new($receiptFile, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
try {
    if ($receiptStream.Length -le 0 -or $receiptStream.Length -gt 1048576) { throw 'Release receipt exceeded its size bound.' }
    $reader = [IO.StreamReader]::new($receiptStream, [Text.UTF8Encoding]::new($false, $true), $true, 4096, $true)
    try { $receipt = $reader.ReadToEnd() | ConvertFrom-Json -AsHashtable -NoEnumerate -DateKind String }
    finally { $reader.Dispose() }
} finally { $receiptStream.Dispose() }
$repository = 'https://github.com/symbiogenesis/prism-inference'
$target = 'aarch64-pc-windows-msvc'
$filename = "prism-inference-$Version-$target.zip"
if ($receipt -isnot [Collections.IDictionary] -or $receipt.SchemaVersion -ne 1 -or
    $receipt.Repository -cne $repository -or $receipt.SourceRevision -cne $SourceRevision -or
    $receipt.Version -cne $Version -or $receipt.Target -cne $target -or
    [string]$receipt.RunId -cne $RunId -or [string]$receipt.RunAttempt -cne [string]$RunAttempt -or
    $receipt.Toolchain -cnotmatch '^nightly-[0-9]{4}-[0-9]{2}-[0-9]{2}-aarch64-pc-windows-msvc$' -or
    $receipt.Files -isnot [array] -or $receipt.Archive.Name -cne $filename) {
    throw 'The release receipt differs from the independently selected build identity.'
}
$required = @('prism-inference.exe', 'bonsai-token-worker.exe', 'bonsai-swift-worker.exe',
    'prism-inference-server.exe', 'bonsai-gguf.exe', 'LICENSE', 'NOTICE', 'PRISM-LICENSE.txt', 'CAMELID-LICENSE.txt')
if ($receipt.Files.Count -ne $required.Count -or
    @($required | Where-Object { $_ -cnotin $receipt.Files.Name }).Count) {
    throw 'The release receipt must include all five executables and four license files exactly once.'
}
$lock = [ordered]@{
    schemaVersion = 1; package = 'prism-inference'; version = $Version
    repository = $repository; commit = $SourceRevision; target = $target; release = "v$Version"
    asset = [ordered]@{
        filename = $filename; url = "$repository/releases/download/v$Version/$filename"
        bytes = $receipt.Archive.Bytes; sha256 = $receipt.Archive.Sha256
    }
    files = @($receipt.Files | ForEach-Object { [ordered]@{ name = $_.Name; bytes = $_.Bytes; sha256 = $_.Sha256 } })
}

# The source ZIP is never extracted. Check its exact commit root, bounded
# inventory, package versions and compiler pin before retaining its byte identity.
$sourceStream = [IO.FileStream]::new($sourceFile, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
try {
    if ($sourceStream.Length -le 0 -or $sourceStream.Length -gt 268435456) { throw 'Source archive exceeded its size bound.' }
    $sourceHash = [Convert]::ToHexStringLower([Security.Cryptography.SHA256]::HashData($sourceStream))
    $sourceStream.Position = 0
    $sourceArchive = [IO.Compression.ZipArchive]::new($sourceStream, [IO.Compression.ZipArchiveMode]::Read, $true)
    try {
        if ($sourceArchive.Entries.Count -eq 0 -or $sourceArchive.Entries.Count -gt 65536) { throw 'Invalid source archive inventory.' }
        $prefix = "prism-inference-$SourceRevision/"
        $seen = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
        [long]$sourceBytes = 0
        foreach ($entry in $sourceArchive.Entries) {
            if (-not $entry.FullName.StartsWith($prefix, [StringComparison]::Ordinal) -or
                $entry.FullName.Contains('\') -or $entry.FullName.Contains(':') -or
                $entry.FullName -match '(^|/)\.{1,2}(/|$)' -or -not $seen.Add($entry.FullName) -or
                (($entry.ExternalAttributes -shr 16) -band 0xF000) -eq 0xA000 -or
                $entry.Length -gt 536870912) { throw 'Unexpected, duplicate or linked source archive entry.' }
            $sourceBytes += $entry.Length
            if ($sourceBytes -gt 1073741824) { throw 'Source archive contents exceeded their size bound.' }
        }
        foreach ($relative in @('Cargo.toml', 'server/Cargo.toml', 'rust-toolchain.toml')) {
            $entry = $sourceArchive.GetEntry($prefix + $relative)
            if ($null -eq $entry -or $entry.Length -le 0 -or $entry.Length -gt 1048576) { throw "Missing or oversized source manifest: $relative" }
            $reader = [IO.StreamReader]::new($entry.Open(), [Text.UTF8Encoding]::new($false, $true))
            try { $text = $reader.ReadToEnd() } finally { $reader.Dispose() }
            if ($relative -ceq 'rust-toolchain.toml') {
                $channel = [regex]::Match($text, '(?m)^channel\s*=\s*"(?<Value>nightly-[0-9]{4}-[0-9]{2}-[0-9]{2})"\s*\r?$')
                if (-not $channel.Success -or $receipt.Toolchain -cne "$($channel.Groups['Value'].Value)-$target") { throw 'The source compiler pin differs from the release receipt.' }
            } else {
                $package = [regex]::Match($text, '(?ms)^\[package\]\s*\r?\n(?<Value>.*?)(?=^\[|\z)')
                $declared = [regex]::Match($package.Groups['Value'].Value, '(?m)^version\s*=\s*"(?<Value>[0-9]+\.[0-9]+\.[0-9]+)"\s*\r?$')
                $name = [regex]::Match($package.Groups['Value'].Value, '(?m)^name\s*=\s*"(?<Value>[a-z0-9-]+)"\s*\r?$')
                $expectedName = if ($relative -ceq 'Cargo.toml') { 'prism-inference' } else { 'prism-inference-server' }
                if (-not $package.Success -or -not $declared.Success -or $declared.Groups['Value'].Value -cne $Version -or
                    -not $name.Success -or $name.Groups['Value'].Value -cne $expectedName) { throw "The source package identity differs from the release receipt: $relative" }
            }
        }
    } finally { $sourceArchive.Dispose() }
    $lock.Source = [ordered]@{
        Repository = $repository; Commit = $SourceRevision; Url = "$repository/archive/$SourceRevision.zip"
        Bytes = $sourceStream.Length; Sha256 = $sourceHash
    }
} finally { $sourceStream.Dispose() }

$lane = Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-LockReview-' + [guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($lane)
$stagedLock = Join-Path $lane 'candidate.lock.json'
[IO.File]::WriteAllText($stagedLock, ($lock | ConvertTo-Json -Depth 8), [Text.UTF8Encoding]::new($false))
Read-BonsaiInferenceLock -Path $stagedLock | Out-Null
$downloads = Join-Path $lane 'downloads'
[void][IO.Directory]::CreateDirectory($downloads)
$bundleStream = [IO.FileStream]::new($bundleFile, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
try {
    if ($bundleStream.Length -ne $lock.asset.bytes -or
        [Convert]::ToHexStringLower([Security.Cryptography.SHA256]::HashData($bundleStream)) -cne $lock.asset.sha256) { throw 'The bundle archive differs from the release receipt.' }
    $bundleStream.Position = 0
    $copy = [IO.FileStream]::new((Join-Path $downloads $filename), [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
    try { $bundleStream.CopyTo($copy); $copy.Flush($true) } finally { $copy.Dispose() }
} finally { $bundleStream.Dispose() }
# Reuse production extraction, entry hashing and PE checks; the already verified
# local archive satisfies provisioning without any HTTP request or execution.
Install-BonsaiInference -DataDirectory $lane -LockPath $stagedLock | Out-Null
[void][IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($candidateFile))
$output = [IO.FileStream]::new($candidateFile, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
try {
    $bytes = [Text.UTF8Encoding]::new($false).GetBytes(($lock | ConvertTo-Json -Depth 8) + [Environment]::NewLine)
    $output.Write($bytes, 0, $bytes.Length)
    $output.Flush($true)
} finally { $output.Dispose() }
[pscustomobject]@{
    Path = $candidateFile; Version = $Version; Commit = $SourceRevision; Target = $target
    RunId = $RunId; RunAttempt = $RunAttempt; Verified = $true; VerificationDirectory = $lane
}
