#requires -Version 7.6
[CmdletBinding()]
[OutputType([void])]
param([ValidateSet('All', 'Install', 'Archive', 'Lock', 'ReleaseLock')][string]$Case = 'All', [string]$ToolPath)
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
if ($Case -in @('All', 'ReleaseLock')) {
    $fixture = New-InferenceFixture -Name 'release-lock' -Entries ($names + @('bonsai-gguf.exe', 'LICENSE', 'NOTICE', 'PRISM-LICENSE.txt', 'CAMELID-LICENSE.txt'))
    $archivePath = Join-Path $fixture.Root ('downloads/' + $fixture.Lock.asset.filename)
    $receiptPath = Join-Path $fixture.Root 'receipt.json'
    $sourcePath = Join-Path $fixture.Root 'source.zip'
    $outputPath = Join-Path $fixture.Root 'candidate.json'
    $inventory = @($names + @('bonsai-gguf.exe', 'LICENSE', 'NOTICE', 'PRISM-LICENSE.txt', 'CAMELID-LICENSE.txt') | ForEach-Object {
        @{ Name = $_; Bytes = $image.Length; Sha256 = [Convert]::ToHexStringLower([Security.Cryptography.SHA256]::HashData($image)) }
    })
    $receipt = @{
        SchemaVersion = 1; Repository = $fixture.Lock.repository; SourceRevision = $fixture.Lock.commit
        Version = '0.1.0'; Target = 'aarch64-pc-windows-msvc'; Toolchain = 'nightly-2026-10-22-aarch64-pc-windows-msvc'
        Files = $inventory; RunId = '12345'; RunAttempt = '2'
        Archive = @{ Name = $fixture.Lock.asset.filename; Bytes = $fixture.Lock.asset.bytes; Sha256 = $fixture.Lock.asset.sha256 }
    }
    [IO.File]::WriteAllText($receiptPath, ($receipt | ConvertTo-Json -Depth 8))
    $source = [IO.Compression.ZipArchive]::new([IO.File]::Open($sourcePath, [IO.FileMode]::CreateNew), [IO.Compression.ZipArchiveMode]::Create)
    try {
        foreach ($relative in @('Cargo.toml', 'server/Cargo.toml', 'rust-toolchain.toml')) {
            $entry = $source.CreateEntry("prism-inference-$($fixture.Lock.commit)/$relative")
            # The previous StreamReader path accepted a UTF-8 BOM in each manifest.
            $writer = [IO.StreamWriter]::new($entry.Open(), [Text.UTF8Encoding]::new($true))
            try {
                if ($relative -ceq 'rust-toolchain.toml') { $writer.WriteLine('[toolchain]'); $writer.WriteLine('channel = "nightly-2026-10-22"') }
                else {
                    $packageName = if ($relative -ceq 'Cargo.toml') { 'prism-inference' } else { 'prism-inference-server' }
                    $writer.WriteLine('[package]'); $writer.WriteLine("name = `"$packageName`""); $writer.WriteLine('version = "0.1.0"')
                }
            } finally { $writer.Dispose() }
        }
    } finally { $source.Dispose() }
    $parameters = @{
        ReceiptPath = $receiptPath; ArchivePath = $archivePath; SourceArchivePath = $sourcePath
        SourceRevision = $fixture.Lock.commit; Version = '0.1.0'; RunId = '12345'; RunAttempt = 2; OutputPath = $outputPath
        ToolPath = $ToolPath
    }
    $script = Join-Path $PSScriptRoot '../scripts/New-InferenceLock.ps1'
    $result = & $script @parameters
    $candidate = Read-BonsaiInferenceLock -Path $outputPath
    Assert ($result.Verified -and $candidate.files.Count -eq 9 -and $candidate.commit -ceq $fixture.Lock.commit) 'The reviewed release did not produce a complete candidate lock.'
    Assert ([IO.File]::Exists((Join-Path $result.VerificationDirectory 'candidate.lock.json')) -and
        [IO.File]::Exists((Join-Path $result.VerificationDirectory 'verification.json')) -and
        -not [IO.Directory]::Exists((Join-Path $result.VerificationDirectory 'inference'))) 'Review evidence was missing or the offline review extracted runtime files.'
    Assert ($candidate.Source.Commit -ceq $candidate.commit -and $candidate.Source.Url -ceq "$($candidate.repository)/archive/$($candidate.commit).zip" -and
        $candidate.Source.Bytes -eq ([IO.FileInfo]$sourcePath).Length -and
        $candidate.Source.Sha256 -ceq (Get-FileHash -LiteralPath $sourcePath -Algorithm SHA256).Hash.ToLowerInvariant()) 'Candidate source identity does not match the source archive.'
    $original = [IO.File]::ReadAllText($outputPath)
    Assert-Failure { & $script @parameters } 'The candidate generator overwrote an existing output file.'
    Assert ([IO.File]::ReadAllText($outputPath) -ceq $original) 'Rejected candidate replacement modified the output.'
    foreach ($mutation in @('revision', 'run', 'target', 'missing-license', 'file-hash', 'archive-hash', 'toolchain')) {
        $changed = $receipt | ConvertTo-Json -Depth 8 | ConvertFrom-Json -AsHashtable
        switch ($mutation) {
            'revision' { $changed.SourceRevision = 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa' }
            'run' { $changed.RunAttempt = '1' }
            'target' { $changed.Target = 'x86_64-pc-windows-msvc' }
            'missing-license' { $changed.Files = @($changed.Files | Where-Object Name -CNE 'LICENSE') }
            'file-hash' { $changed.Files[0].Sha256 = '0' * 64 }
            'archive-hash' { $changed.Archive.Sha256 = '0' * 64 }
            'toolchain' { $changed.Toolchain = 'nightly-2026-10-21-aarch64-pc-windows-msvc' }
        }
        [IO.File]::WriteAllText($receiptPath, ($changed | ConvertTo-Json -Depth 8))
        $parameters.OutputPath = Join-Path $fixture.Root "$mutation.json"
        Assert-Failure { & $script @parameters } "Unsafe release receipt accepted: $mutation"
        Assert (-not [IO.File]::Exists($parameters.OutputPath)) "Rejected release produced a candidate: $mutation"
    }
    [IO.File]::WriteAllText($receiptPath, ($receipt | ConvertTo-Json -Depth 8))
    $wrongMachineArchive = Join-Path $fixture.Root 'wrong-machine.zip'
    [IO.File]::Copy($archivePath, $wrongMachineArchive)
    $wrongMachine = [byte[]]$image.Clone()
    $wrongMachine[68] = 0x64; $wrongMachine[69] = 0x86
    $archive = [IO.Compression.ZipArchive]::new([IO.File]::Open($wrongMachineArchive, [IO.FileMode]::Open), [IO.Compression.ZipArchiveMode]::Update)
    try {
        $archive.GetEntry('prism-inference-server.exe').Delete()
        $entry = $archive.CreateEntry('prism-inference-server.exe')
        $stream = $entry.Open()
        try { $stream.Write($wrongMachine, 0, $wrongMachine.Length) } finally { $stream.Dispose() }
    } finally { $archive.Dispose() }
    $changed = $receipt | ConvertTo-Json -Depth 8 | ConvertFrom-Json -AsHashtable
    ($changed.Files | Where-Object Name -CEQ 'prism-inference-server.exe').Sha256 = [Convert]::ToHexStringLower([Security.Cryptography.SHA256]::HashData($wrongMachine))
    $changed.Archive.Bytes = ([IO.FileInfo]$wrongMachineArchive).Length
    $changed.Archive.Sha256 = (Get-FileHash -LiteralPath $wrongMachineArchive -Algorithm SHA256).Hash.ToLowerInvariant()
    [IO.File]::WriteAllText($receiptPath, ($changed | ConvertTo-Json -Depth 8))
    $parameters.ArchivePath = $wrongMachineArchive
    $parameters.OutputPath = Join-Path $fixture.Root 'wrong-machine.json'
    Assert-Failure { & $script @parameters } 'A correctly hashed x64 server was accepted in an ARM64 bundle.'
    Assert (-not [IO.File]::Exists($parameters.OutputPath)) 'Wrong-machine bundle produced a candidate lock.'
    $parameters.ArchivePath = $archivePath
    $parameters.SourceRevision = 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'
    $receipt.SourceRevision = $parameters.SourceRevision
    [IO.File]::WriteAllText($receiptPath, ($receipt | ConvertTo-Json -Depth 8))
    $parameters.OutputPath = Join-Path $fixture.Root 'source-mismatch.json'
    Assert-Failure { & $script @parameters } 'A source archive for a different commit was accepted.'
    Assert (-not [IO.File]::Exists($parameters.OutputPath)) 'Mismatched source produced a candidate lock.'
    $parameters.SourceRevision = $fixture.Lock.commit
    $receipt.SourceRevision = $fixture.Lock.commit
    [IO.File]::WriteAllText($receiptPath, ($receipt | ConvertTo-Json -Depth 8))
    $source = [IO.Compression.ZipArchive]::new([IO.File]::Open($sourcePath, [IO.FileMode]::Open), [IO.Compression.ZipArchiveMode]::Update)
    try {
        $entryName = "prism-inference-$($fixture.Lock.commit)/server/Cargo.toml"
        $source.GetEntry($entryName).Delete()
        $writer = [IO.StreamWriter]::new($source.CreateEntry($entryName).Open())
        try { $writer.WriteLine('[package]'); $writer.WriteLine('name = "prism-inference-server"'); $writer.WriteLine('version = "9.9.9"') }
        finally { $writer.Dispose() }
    } finally { $source.Dispose() }
    $parameters.OutputPath = Join-Path $fixture.Root 'source-version.json'
    Assert-Failure { & $script @parameters } 'An independently mismatched source package version was accepted.'
    Assert (-not [IO.File]::Exists($parameters.OutputPath)) 'Wrong-version source produced a candidate lock.'
    $source = [IO.Compression.ZipArchive]::new([IO.File]::Open($sourcePath, [IO.FileMode]::Open), [IO.Compression.ZipArchiveMode]::Update)
    try {
        $entryName = "prism-inference-$($fixture.Lock.commit)/server/Cargo.toml"
        $source.GetEntry($entryName).Delete()
        $stream = $source.CreateEntry($entryName).Open()
        try { $stream.WriteByte(0xFF) } finally { $stream.Dispose() }
    } finally { $source.Dispose() }
    $parameters.OutputPath = Join-Path $fixture.Root 'source-encoding.json'
    Assert-Failure { & $script @parameters } 'Malformed UTF-8 source was accepted.'
    Assert (-not [IO.File]::Exists($parameters.OutputPath)) 'Malformed UTF-8 source produced a candidate lock.'
}
