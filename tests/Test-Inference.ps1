#requires -Version 7.6
[CmdletBinding()]
[OutputType([void])]
param([ValidateSet('All', 'Install', 'Archive', 'Lock')][string]$Case = 'All')
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
if (-not $IsWindows) { throw 'Inference provisioning targets Windows ARM64.' }
if ($Case -eq 'All' -and $env:GITHUB_ACTIONS -ne 'true') { throw 'The complete suite runs only in CI. Select one focused case locally.' }
Import-Module (Join-Path $PSScriptRoot '../scripts/Bonsai.Inference.psm1') -Force
Import-Module (Join-Path $PSScriptRoot 'Bonsai.TestSupport.psm1')
$scratch = Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-InferenceTest-' + [guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($scratch)
$image = [byte[]]::new(128)
$image[0] = 0x4D; $image[1] = 0x5A; $image[60] = 64
$image[64] = 0x50; $image[65] = 0x45; $image[68] = 0x64; $image[69] = 0xAA
$names = @('prism-inference.exe', 'bonsai-token-worker.exe', 'bonsai-swift-worker.exe', 'prism-inference-server.exe')

function New-InferenceFixture {
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseShouldProcessForStateChangingFunctions', '', Justification = 'Creates isolated synthetic test fixtures in the unique external test lane.')]
    [CmdletBinding()]
    [OutputType([pscustomobject])]
    param([Parameter(Mandatory)][string]$Name, [string[]]$Entries = $names)
    $root = Join-Path $scratch $Name
    $downloads = Join-Path $root 'downloads'
    [void][IO.Directory]::CreateDirectory($downloads)
    $filename = 'prism-inference-0.1.0-aarch64-pc-windows-msvc.zip'
    $archivePath = Join-Path $downloads $filename
    $stream = [IO.FileStream]::new($archivePath, [IO.FileMode]::CreateNew, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
    try {
        $archive = [IO.Compression.ZipArchive]::new($stream, [IO.Compression.ZipArchiveMode]::Create, $true)
        try {
            foreach ($name in $Entries) {
                $entry = $archive.CreateEntry($name)
                $output = $entry.Open()
                try { $output.Write($image, 0, $image.Length) } finally { $output.Dispose() }
            }
        } finally { $archive.Dispose() }
    } finally { $stream.Dispose() }
    $sha256 = [Convert]::ToHexStringLower([Security.Cryptography.SHA256]::HashData($image))
    $lock = @{
        schemaVersion = 1; package = 'prism-inference'; version = '0.1.0'
        repository = 'https://github.com/symbiogenesis/prism-inference'; commit = '1234567890123456789012345678901234567890'
        target = 'aarch64-pc-windows-msvc'; release = 'v0.1.0'
        asset = @{ filename = $filename; url = "https://github.com/symbiogenesis/prism-inference/releases/download/v0.1.0/$filename"
            bytes = ([IO.FileInfo]$archivePath).Length; sha256 = (Get-FileHash -LiteralPath $archivePath -Algorithm SHA256).Hash.ToLowerInvariant() }
        files = @($names | ForEach-Object { @{ name = $_; bytes = $image.Length; sha256 = $sha256 } })
    }
    $lockPath = Join-Path $root 'lock.json'
    [IO.File]::WriteAllText($lockPath, (ConvertTo-Json -InputObject $lock -Depth 8))
    return [pscustomobject]@{ Root = $root; LockPath = $lockPath; Lock = $lock }
}

if ($Case -in @('All', 'Install')) {
    $fixture = New-InferenceFixture -Name 'install'
    Assert-Failure { Install-BonsaiInference -DataDirectory $fixture.Root -LockPath $fixture.LockPath -VerifyOnly } 'Verify-only installed a missing release.'
    $installed = Install-BonsaiInference -DataDirectory $fixture.Root -LockPath $fixture.LockPath
    Assert ($installed.Verified -and [IO.File]::Exists($installed.Executable)) 'The pinned prebuilt inference package was not installed.'
    $verified = & (Join-Path $PSScriptRoot '../scripts/Get-Inference.ps1') -DataDirectory $fixture.Root -LockPath $fixture.LockPath -VerifyOnly
    Assert ($verified.Directory -ceq $installed.Directory) 'An installed release was not reused by verify-only provisioning.'
    [IO.File]::WriteAllBytes($installed.SwiftWorker, [byte[]]@(1, 2, 3))
    Assert-Failure { Install-BonsaiInference -DataDirectory $fixture.Root -LockPath $fixture.LockPath } 'Modified installed worker bytes were accepted.'
}
if ($Case -in @('All', 'Archive')) {
    foreach ($entryNames in @(
        @('../escape.exe', 'bonsai-token-worker.exe', 'bonsai-swift-worker.exe', 'prism-inference-server.exe'),
        @('prism-inference.exe', 'prism-inference.exe', 'bonsai-swift-worker.exe', 'prism-inference-server.exe'),
        @('prism-inference.exe', 'nested/bonsai-token-worker.exe', 'bonsai-swift-worker.exe', 'prism-inference-server.exe')
    )) {
        $fixture = New-InferenceFixture -Name ([guid]::NewGuid().ToString('N')) -Entries $entryNames
        Assert-Failure { Install-BonsaiInference -DataDirectory $fixture.Root -LockPath $fixture.LockPath } 'An unsafe archive inventory was accepted.'
        Assert (-not [IO.File]::Exists((Join-Path $fixture.Root 'escape.exe'))) 'Archive traversal wrote outside staging.'
    }
}
if ($Case -in @('All', 'Lock')) {
    $fixture = New-InferenceFixture -Name 'lock'
    foreach ($mutation in @('repository', 'commit', 'target', 'duplicate', 'url')) {
        $lock = ConvertFrom-Json -InputObject (ConvertTo-Json -InputObject $fixture.Lock -Depth 8) -AsHashtable
        switch ($mutation) {
            'repository' { $lock.repository = 'https://github.com/other/prism-inference' }
            'commit' { $lock.commit = 'main' }
            'target' { $lock.target = 'x86_64-pc-windows-msvc' }
            'duplicate' { $lock.files[1].name = $lock.files[0].name }
            'url' { $lock.asset.url += '?latest=true' }
        }
        [IO.File]::WriteAllText($fixture.LockPath, (ConvertTo-Json -InputObject $lock -Depth 8))
        Assert-Failure { Read-BonsaiInferenceLock -Path $fixture.LockPath } "Unsafe inference lock accepted: $mutation"
    }
}
