#requires -Version 7.6
[CmdletBinding()]
[OutputType([pscustomobject])]
param([Parameter(Mandatory)][string]$DataDirectory, [switch]$VerifyOnly)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1') -Force
$lock = Read-BonsaiLock
$root = Resolve-ExternalPath $DataDirectory
$model = $lock.model
$path = Join-Path $root -ChildPath 'models', $model.revision, $model.filename
if ($VerifyOnly) { Test-LockedArtifact $path $model.bytes $model.sha256 }
else {
    $uri = "https://huggingface.co/$($model.repository)/resolve/$($model.revision)/$($model.filename)"
    Receive-LockedArtifact -Uri $uri -Path $path -Bytes $model.bytes -Sha256 $model.sha256 | Out-Null
}
[pscustomobject]@{ Status = 'verified'; Model = $model.repository; Revision = $model.revision; Path = $path }
