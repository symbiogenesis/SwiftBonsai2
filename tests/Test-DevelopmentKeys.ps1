#requires -Version 7.6
[CmdletBinding()]
[OutputType([string])]
param([Parameter(Mandatory)][ValidateSet('Creation','Paths')][string]$Case)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true

function Assert {
    [CmdletBinding()]
    [OutputType([void])]
    param([bool]$Condition, [string]$Message)
    if (-not $Condition) { throw $Message }
}
function Assert-Failure {
    [CmdletBinding()]
    [OutputType([void])]
    param([scriptblock]$Action)
    $failed = $false
    try { & $Action | Out-Null } catch { $failed = $true }
    Assert $failed 'Unsafe key storage was accepted.'
}

$script = Join-Path $PSScriptRoot '../scripts/New-DevelopmentKeys.ps1'
$scratch = Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-KeyTests-' + [guid]::NewGuid().ToString('N'))
if (-not [IO.Path]::IsPathFullyQualified($scratch) -or $scratch -match '%[^%]+%') { throw 'Expected expanded absolute scratch.' }
[IO.Directory]::CreateDirectory($scratch) | Out-Null
if ($Case -eq 'Paths') {
    Assert-Failure { & $script -ParentDirectory '.' }
    Assert-Failure { & $script -ParentDirectory '%TEMP%\keys' }
    Assert-Failure { & $script -ParentDirectory '\\localhost\share' }
    Assert-Failure { & $script -ParentDirectory (Join-Path $scratch 'absent') }
    Assert-Failure { & $script -ParentDirectory ([IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))) }
    $planned = @(& $script -ParentDirectory $scratch -WhatIf)
    Assert ($planned.Count -eq 0 -and [IO.Directory]::GetFileSystemEntries($scratch).Length -eq 0) 'WhatIf created key material.'
} else {
    $before = [Environment]::GetEnvironmentVariables()
    $result = & $script -ParentDirectory $scratch
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    try { $owner = $identity.User.Value } finally { $identity.Dispose() }
    $allowed = @($owner, 'S-1-5-18') | Select-Object -Unique
    foreach ($path in @($result.Directory, $result.FrontendKeyFile, $result.BackendKeyFile)) {
        $item = Get-Item -LiteralPath $path -Force
        Assert (-not ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) 'Key storage is a reparse point.'
        $acl = Get-Acl -LiteralPath $path
        Assert $acl.AreAccessRulesProtected 'Key ACL inheritance was not disabled.'
        Assert ($acl.GetOwner([Security.Principal.SecurityIdentifier]).Value -ceq $owner) 'Wrong key owner.'
        $rules = @($acl.GetAccessRules($true, $true, [Security.Principal.SecurityIdentifier]))
        Assert ($rules.Count -eq @($allowed).Count) 'Unexpected key ACL entry count.'
        foreach ($rule in $rules) {
            Assert ($rule.IdentityReference.Value -cin $allowed -and -not $rule.IsInherited -and
                $rule.AccessControlType -eq [Security.AccessControl.AccessControlType]::Allow -and
                $rule.FileSystemRights -eq [Security.AccessControl.FileSystemRights]::FullControl) 'Unexpected key ACL access.'
        }
    }
    $frontend = [IO.File]::ReadAllText($result.FrontendKeyFile)
    $backend = [IO.File]::ReadAllText($result.BackendKeyFile)
    Assert ($frontend -cmatch '^[0-9a-f]{64}$' -and $backend -cmatch '^[0-9a-f]{64}$' -and $frontend -cne $backend) 'Invalid or shared credentials.'
    $serialized = $result | ConvertTo-Json
    Assert (-not $serialized.Contains($frontend) -and -not $serialized.Contains($backend)) 'Returned key values in diagnostics.'
    $second = & $script -ParentDirectory $scratch
    Assert ($second.Directory -cne $result.Directory -and [IO.File]::ReadAllText($result.FrontendKeyFile) -ceq $frontend) 'Repeated setup overwrote existing credentials.'
    $after = [Environment]::GetEnvironmentVariables()
    Assert ($before.Count -eq $after.Count) 'Key setup changed the inherited environment.'
    foreach ($name in $before.Keys) { Assert ($before[$name] -ceq $after[$name]) 'Key setup changed an inherited environment value.' }
}
Write-Output "Development key $Case checks passed. Evidence: $scratch"
