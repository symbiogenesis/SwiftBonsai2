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
param([Parameter(Mandatory)][string]$ParentDirectory)
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

$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
try { $owner = $identity.User } finally { $identity.Dispose() }
if ($null -eq $owner) { throw 'The current Windows user has no security identifier.' }
$system = [Security.Principal.SecurityIdentifier]::new([Security.Principal.WellKnownSidType]::LocalSystemSid, $null)
$principals = @($owner)
if ($owner.Value -cne $system.Value) { $principals += $system }
$directorySecurity = [Security.AccessControl.DirectorySecurity]::new()
$directorySecurity.SetOwner($owner)
$directorySecurity.SetAccessRuleProtection($true, $false)
$fileSecurity = [Security.AccessControl.FileSecurity]::new()
$fileSecurity.SetOwner($owner)
$fileSecurity.SetAccessRuleProtection($true, $false)
foreach ($principal in $principals) {
    $directorySecurity.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new($principal,
        [Security.AccessControl.FileSystemRights]::FullControl,
        ([Security.AccessControl.InheritanceFlags]::ContainerInherit -bor [Security.AccessControl.InheritanceFlags]::ObjectInherit),
        [Security.AccessControl.PropagationFlags]::None, [Security.AccessControl.AccessControlType]::Allow))
    $fileSecurity.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new($principal,
        [Security.AccessControl.FileSystemRights]::FullControl, [Security.AccessControl.AccessControlType]::Allow))
}
if (Test-Path -LiteralPath $directory) { throw 'The new private directory already exists; no credentials were created.' }
[IO.FileSystemAclExtensions]::Create([IO.DirectoryInfo]::new($directory), $directorySecurity)
# Create can return an existing directory. Recheck its resolved path and exact
# protected owner/DACL before generating secrets, rather than trusting that call.
$null = Resolve-ExternalPath $directory
$actual = [IO.FileSystemAclExtensions]::GetAccessControl([IO.DirectoryInfo]::new($directory))
$sections = [Security.AccessControl.AccessControlSections]::Access -bor [Security.AccessControl.AccessControlSections]::Owner
if ($actual.GetSecurityDescriptorSddlForm($sections) -cne $directorySecurity.GetSecurityDescriptorSddlForm($sections)) {
    throw 'The new directory security differs from the requested protected ACL; no credentials were created.'
}
$paths = @{}
foreach ($name in @('frontend', 'backend')) {
    $path = Resolve-ExternalPath (Join-Path $directory ($name + '.key'))
    $stream = [IO.FileSystemAclExtensions]::Create([IO.FileInfo]::new($path), [IO.FileMode]::CreateNew,
        [Security.AccessControl.FileSystemRights]::FullControl, [IO.FileShare]::None, 4096,
        [IO.FileOptions]::None, $fileSecurity)
    $random = $bytes = $null
    try {
        $random = [Security.Cryptography.RandomNumberGenerator]::GetBytes(32)
        $bytes = [Text.Encoding]::ASCII.GetBytes([Convert]::ToHexStringLower($random))
        $stream.Write($bytes, 0, $bytes.Length)
        $stream.Flush($true)
    } finally {
        $stream.Dispose()
        if ($random) { [Array]::Clear($random) }
        if ($bytes) { [Array]::Clear($bytes) }
    }
    $paths[$name] = $path
}
[pscustomobject]@{
    Status = 'development-keys-created'; Directory = $directory
    FrontendKeyFile = $paths.frontend; BackendKeyFile = $paths.backend
    Qualification = 'owner-and-LocalSystem ACLs; same-user access and parent replacement remain possible; no daily isolation claim'
}
