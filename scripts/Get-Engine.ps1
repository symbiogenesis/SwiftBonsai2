#requires -Version 7.6
[CmdletBinding()]
[OutputType([pscustomobject])]
param([Parameter(Mandatory)][string]$DataDirectory, [switch]$VerifyOnly, [string]$LockPath)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1') -Force
$lock = if ($LockPath) { Read-BonsaiLock -Path $LockPath } else { Read-BonsaiLock }
$root = Resolve-ExternalPath $DataDirectory
$asset = $lock.runtime.asset
$archivePath = Join-Path $root -ChildPath 'downloads', $asset.filename
if ($VerifyOnly) { Test-LockedArtifact $archivePath $asset.bytes $asset.sha256 }
else { Receive-LockedArtifact -Uri $asset.url -Path $archivePath -Bytes $asset.bytes -Sha256 $asset.sha256 | Out-Null }
$destination = Resolve-ExternalPath (Join-Path $root -ChildPath 'runtime', $lock.runtime.commit)
$archive = [IO.Compression.ZipFile]::OpenRead($archivePath)
$staging = $null
try {
    $entries = @($archive.Entries | Where-Object { $_.Name })
    $names = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
    [long]$expanded = 0
    foreach ($entry in $entries) {
        if ($entry.FullName -notmatch '^[\w. /-]+$' -or $entry.FullName -match '(^|/)\.\.?(/|$)' -or
            $entry.FullName.StartsWith('/') -or -not $names.Add($entry.FullName)) { throw 'Unsafe or duplicate archive entry.' }
        $expanded += $entry.Length
        if ($expanded -gt 512MB) { throw 'Runtime archive exceeds the extraction budget.' }
    }
    $servers = @($entries | Where-Object Name -EQ 'llama-server.exe')
    if ($servers.Count -ne 1) { throw 'Expected exactly one llama-server.exe in the locked archive.' }
    if (-not (Test-Path -LiteralPath $destination)) {
        if ($VerifyOnly) { throw 'Runtime is not installed.' }
        $staging = Resolve-ExternalPath ($destination + '.staging-' + [Guid]::NewGuid().ToString('N'))
        [IO.Directory]::CreateDirectory($staging) | Out-Null
        foreach ($entry in $entries) {
            $target = Resolve-ExternalPath (Join-Path $staging $entry.FullName)
            [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($target)) | Out-Null
            $sourceStream = $targetStream = $null
            try {
                $sourceStream = $entry.Open()
                $targetStream = [IO.File]::Open($target, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
                $sourceStream.CopyTo($targetStream)
            } finally {
                if ($targetStream) { $targetStream.Dispose() }
                if ($sourceStream) { $sourceStream.Dispose() }
            }
            if ([IO.Path]::GetExtension($target) -in @('.exe','.dll')) { Assert-Arm64PE $target }
        }
        [IO.Directory]::Move($staging, $destination)
        $staging = $null
    }
    # Compare every installed file directly to the verified archive, not a mutable receipt.
    $installed = @(Get-ChildItem -LiteralPath $destination -File -Recurse -Force)
    if ($installed.Count -ne $entries.Count) { throw 'Runtime installation contains missing or unexpected files.' }
    foreach ($entry in $entries) {
        $target = Resolve-ExternalPath (Join-Path $destination $entry.FullName)
        $sourceStream = $entry.Open()
        try { $expectedHash = (Get-FileHash -InputStream $sourceStream -Algorithm SHA256).Hash }
        finally { $sourceStream.Dispose() }
        Test-LockedArtifact $target $entry.Length $expectedHash
        if ([IO.Path]::GetExtension($target) -in @('.exe','.dll')) { Assert-Arm64PE $target }
    }
    $serverPath = Join-Path $destination $servers[0].FullName
    $version = Assert-EngineVersion $serverPath $lock.runtime.commit
    [pscustomobject]@{ Status = 'verified'; Commit = $lock.runtime.commit
        ServerPath = $serverPath; ArchivePath = $archivePath; ReportedVersion = $version }
} finally {
    $archive.Dispose()
    # Preserve a failed staging lane for inspection; never recursively delete computed storage.
    if ($staging) { Write-Warning "Incomplete runtime installation retained at $staging" }
}
