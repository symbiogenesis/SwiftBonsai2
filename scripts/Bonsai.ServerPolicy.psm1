#requires -Version 7.6
# Nonlaunching M1 policy preparation; this module is not an HTTP or OS boundary.
# Literature and pre-implementation sanity checks: docs/implementation-research.md.
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1')
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Tools.psm1')

function Assert-BonsaiServerEnvironment {
    <#
    .SYNOPSIS
    Rejects inherited model/backend overrides without exposing their values.
    #>
    [CmdletBinding()]
    [OutputType([void])]
    param([Collections.IDictionary]$Variables = [Environment]::GetEnvironmentVariables(), [string]$ToolPath)
    # Names only cross the bridge: values can contain credentials or prompts.
    $null = Invoke-BonsaiTool -Command 'server-policy' -ToolPath $ToolPath -Request @{
        operation = 'environment'; names = @($Variables.Keys | ForEach-Object { [string]$_ })
    }
}

function New-BonsaiServerPlan {
    <#
    .SYNOPSIS
    Returns a fixed, nonlaunching development server plan after policy validation.
    #>
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseShouldProcessForStateChangingFunctions', '', Justification = 'Model/key provisioning and inference remain disabled; the maintenance bridge uses external tool/request storage.')]
    [CmdletBinding()]
    [OutputType([pscustomobject])]
    param(
        [Parameter(Mandatory)][string]$DataDirectory,
        [ValidateRange(1024,65535)][int]$Port = 8080,
        [Alias('Profile')][Collections.IDictionary]$ServerProfile = (Get-Content -Raw -LiteralPath (Join-Path $PSScriptRoot '../config/profiles/reference-cpu.json') | ConvertFrom-Json -DateKind String -AsHashtable),
        [string]$ToolPath
    )
    # Reject UNC before filesystem probing; this plan promises local/offline paths.
    if ($DataDirectory -notmatch '^[A-Za-z]:[\\/]') { throw 'Use an absolute local drive data directory.' }
    $drive = [IO.DriveInfo]::new($DataDirectory.Substring(0,3))
    if ($drive.DriveType -notin @([IO.DriveType]::Fixed, [IO.DriveType]::Removable, [IO.DriveType]::Ram)) {
        throw 'Data directory must be on a recognized local disk, not a mapped network or unavailable drive.'
    }
    $data = Resolve-ExternalPath $DataDirectory
    if ($data.TrimEnd('\','/') -eq [IO.Path]::GetPathRoot($data).TrimEnd('\','/')) {
        throw 'Choose a dedicated data directory, not a drive root.'
    }
    $lock = Read-BonsaiLock
    # JSON would otherwise coerce unsupported .NET types (decimal/short/char,
    # for example) into an accepted integer/string. Preserve the existing type
    # contract here; Rust owns the field set, values and complete launch policy.
    foreach ($name in $ServerProfile.Keys) {
        $value = $ServerProfile[$name]
        if ($value -is [array]) {
            if (@($value | Where-Object { $_ -isnot [string] }).Count) {
                throw "Unreviewed reference profile value/type: $name"
            }
        } elseif ($value -isnot [string] -and $value -isnot [bool] -and $value -isnot [int] -and $value -isnot [long]) {
            throw "Unreviewed reference profile value/type: $name"
        }
    }
    $model = Resolve-ExternalPath (Join-Path $data -ChildPath 'models', $lock.model.revision, $lock.model.filename)
    $keyFile = Resolve-ExternalPath (Join-Path $data 'private/server.key')
    $plan = Invoke-BonsaiTool -Command 'server-policy' -ToolPath $ToolPath -Request @{
        operation = 'plan'; profile = $ServerProfile; modelPath = $model; keyFile = $keyFile; port = $Port
        runtimeCommit = $lock.runtime.commit; modelRevision = $lock.model.revision; modelBytes = $lock.model.bytes
        environmentNames = @([Environment]::GetEnvironmentVariables().Keys | ForEach-Object { [string]$_ })
    }
    $plan.Arguments = [string[]]$plan.Arguments
    return [pscustomobject]$plan
}

function Assert-BonsaiServerHelp {
    <#
    .SYNOPSIS
    Verifies the exact planned switches are advertised by pinned server help.
    #>
    [CmdletBinding()]
    [OutputType([void])]
    param([Parameter(Mandatory)][string]$HelpText, [Parameter(Mandatory)][string[]]$Arguments)
    $flags = @($Arguments | Where-Object { $_ -cmatch '^--[a-z]' } | Select-Object -Unique)
    if ($flags.Count -eq 0) { throw 'No server switches to verify.' }
    foreach ($flag in $flags) {
        if ($HelpText -cnotmatch ('(?<![\w-])' + [regex]::Escape($flag) + '(?![\w-])')) {
            throw "Pinned server help does not advertise required switch: $flag"
        }
    }
}

Export-ModuleMember -Function Assert-BonsaiServerEnvironment, New-BonsaiServerPlan, Assert-BonsaiServerHelp
