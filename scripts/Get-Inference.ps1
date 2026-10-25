#requires -Version 7.6
<#
.SYNOPSIS
Installs or verifies the pinned prebuilt Prism Inference executables.
.DESCRIPTION
Downloads only the exact locked Windows ARM64 release, verifies its archive and
every file, and publishes a new external installation without replacing files.
No Rust toolchain, source build, model loading, test or benchmark is invoked.
#>
[CmdletBinding()]
[OutputType([pscustomobject])]
param([Parameter(Mandatory)][string]$DataDirectory, [string]$LockPath, [switch]$VerifyOnly)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Inference.psm1') -Force
$parameters = @{ DataDirectory = $DataDirectory; VerifyOnly = $VerifyOnly }
if ($LockPath) { $parameters.LockPath = $LockPath }
Install-BonsaiInference @parameters
