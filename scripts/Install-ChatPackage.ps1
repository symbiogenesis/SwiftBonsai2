#requires -Version 7.6
<#
.SYNOPSIS
Installs and activates a reviewed portable package, or selects the previous version.
.DESCRIPTION
Supply a manifest SHA-256 retained independently of the package. Versions are
copied and checked before an atomic active/previous pointer update. Existing
versions, model artifacts and conversation recovery are preserved. Rollback is
explicit and verifies the retained previous version again. Close chat before
changing versions; active processes continue using their originally selected files.
#>
[CmdletBinding(DefaultParameterSetName = 'Install')]
[OutputType([pscustomobject])]
param([Parameter(Mandatory)][string]$InstallationDirectory,
      [Parameter(Mandatory, ParameterSetName = 'Install')][string]$PackageDirectory,
      [Parameter(Mandatory, ParameterSetName = 'Install')][ValidatePattern('^[a-f0-9]{64}$')][string]$ManifestSha256,
      [Parameter(Mandatory, ParameterSetName = 'Rollback')][switch]$Rollback)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.ChatPackage.psm1')
Set-ChatPackage @PSBoundParameters
