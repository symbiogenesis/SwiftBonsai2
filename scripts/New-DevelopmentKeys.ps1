#requires -Version 7.6
<#
.SYNOPSIS
Creates separate owner-private credentials for the explicit development broker.
.DESCRIPTION
Creates a new random child directory below an existing external local directory.
The directory and both new files have protected Windows ACLs granting only the
current owner and LocalSystem full control. Each file contains 32 cryptographic
random bytes encoded as 64 lowercase ASCII hex characters, without a newline.
Returns paths only. Does not launch processes, enable chat or modify existing
ACLs. An owner-controlled parent is required: file ACLs cannot prevent a parent
owner or administrator from replacing a directory. Same-user processes can read
these credentials; this is not backend bypass prevention or process isolation.
#>
[CmdletBinding(SupportsShouldProcess)]
[OutputType([pscustomobject])]
param([Parameter(Mandatory)][string]$ParentDirectory, [string]$ToolPath)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
if (-not $IsWindows) { throw 'Development key creation requires Windows ACL support.' }
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1') -Force
if ($ParentDirectory -notmatch '^[A-Za-z]:[\\/]') { throw 'Use an expanded absolute local drive parent directory.' }
$drive = [IO.DriveInfo]::new($ParentDirectory.Substring(0,3))
if ($drive.DriveType -notin @([IO.DriveType]::Fixed, [IO.DriveType]::Removable, [IO.DriveType]::Ram)) {
    throw 'Key storage requires a recognized local disk.'
}
$parent = Resolve-ExternalPath $ParentDirectory
if (-not [IO.Directory]::Exists($parent)) { throw 'The selected parent directory must already exist.' }
$directory = Resolve-ExternalPath (Join-Path $parent ('SwiftBonsai-Keys-' + [guid]::NewGuid().ToString('N')))
if (-not $PSCmdlet.ShouldProcess($directory, 'Create two owner-private development credentials')) { return }

Import-Module (Join-Path $PSScriptRoot 'Bonsai.Tools.psm1')
$result = Invoke-BonsaiTool -Command credentials -Request @{ parentDirectory = $parent } -ToolPath $ToolPath
[pscustomobject]$result