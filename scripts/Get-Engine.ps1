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
    # Extraction and offline verification use one Rust archive policy. Directory
    # publication retains .NET's no-replacement behavior; runtime-verify below
    # guards the published files for probes.
    $staged = Invoke-BonsaiTool -Command runtime-stage -ToolPath $ToolPath -Request @{
        schemaVersion = 1
        runtime = @{ archivePath = $archivePath; runtimeDirectory = $destination
            archiveBytes = [long]$asset.bytes; archiveSha256 = $asset.sha256 }
    }
    $staging = Resolve-ExternalPath $staged.StagingDirectory
    try {
        [IO.Directory]::Move($staging, $destination)
        $staging = $null
    } finally {
        if ($staging) { Write-Warning "Incomplete runtime installation retained at $staging" }
    }
}
$result = Invoke-BonsaiTool -Command runtime-verify -ToolPath $ToolPath -Request @{
    schemaVersion = 1; commit = $lock.runtime.commit; includeHelp = $IncludeHelp.IsPresent
    runtime = @{ archivePath = $archivePath; runtimeDirectory = $destination
        archiveBytes = [long]$asset.bytes; archiveSha256 = $asset.sha256 }
}
return [pscustomobject]$result
