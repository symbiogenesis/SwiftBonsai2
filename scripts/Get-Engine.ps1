#requires -Version 7.6
[CmdletBinding()]
[OutputType([pscustomobject])]
param([Parameter(Mandatory)][string]$DataDirectory, [switch]$VerifyOnly, [string]$LockPath, [string]$ToolPath, [switch]$IncludeHelp)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1') -Force
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Tools.psm1')
$lock = if ($LockPath) { Read-BonsaiLock -Path $LockPath } else { Read-BonsaiLock }
$root = Resolve-ExternalPath $DataDirectory
$asset = $lock.runtime.asset
$archivePath = Join-Path $root -ChildPath 'downloads', $asset.filename
if (-not $VerifyOnly) { Receive-LockedArtifact -Uri $asset.url -Path $archivePath -Bytes $asset.bytes -Sha256 $asset.sha256 | Out-Null }
$destination = Resolve-ExternalPath (Join-Path $root -ChildPath 'runtime', $lock.runtime.commit)
if (-not (Test-Path -LiteralPath $destination)) {
    if ($VerifyOnly) { throw 'Runtime is not installed.' }
    # Installation still validates entry paths before creating any staging files.
    # The shared Rust verifier checks the complete result before any version probe.
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
        $staging = Resolve-ExternalPath ($destination + '.staging-' + [Guid]::NewGuid().ToString('N'))
        [void][IO.Directory]::CreateDirectory($staging)
        foreach ($entry in $entries) {
            $target = Resolve-ExternalPath (Join-Path $staging $entry.FullName)
            [void][IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($target))
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
    } finally {
        $archive.Dispose()
        if ($staging) { Write-Warning "Incomplete runtime installation retained at $staging" }
    }
}
$result = Invoke-BonsaiTool -Command runtime-verify -ToolPath $ToolPath -Request @{
    schemaVersion = 1; commit = $lock.runtime.commit; includeHelp = $IncludeHelp.IsPresent
    runtime = @{ archivePath = $archivePath; runtimeDirectory = $destination
        archiveBytes = [long]$asset.bytes; archiveSha256 = $asset.sha256 }
}
return [pscustomobject]$result
