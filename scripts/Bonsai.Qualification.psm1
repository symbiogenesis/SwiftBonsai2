#requires -Version 7.6
# Fixed qualification policy and scoring live in the packaged safe Rust tool.
# These adapters retain the existing in-memory PowerShell contracts. Scoring
# never launches inference, manufactures human reviews, or enables a profile.
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Tools.psm1')

function Get-BonsaiQualificationDigest {
    <# .SYNOPSIS
    Hashes exact UTF-8 text without normalization or a byte-order mark.
    #>
    [CmdletBinding()]
    [OutputType([string])]
    param([Parameter(Mandatory)][AllowEmptyString()][string]$Text)
    return [Convert]::ToHexStringLower([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($Text)))
}

function Get-BonsaiLoraQualificationPlan {
    <# .SYNOPSIS
    Returns the packaged fixed contract, LF-normalized identity, and paired schedule.
    #>
    [CmdletBinding()]
    [OutputType([pscustomobject])]
    param([string]$ToolPath)
    $result = Invoke-BonsaiTool -Command qualification -Request @{ operation = 'plan' } -ToolPath $ToolPath
    return [pscustomobject]$result
}

function Get-BonsaiLoraControlDigest {
    <# .SYNOPSIS
    Validates fixed controls and returns the legacy canonical UTF-8 identity.
    #>
    [CmdletBinding()]
    [OutputType([string])]
    param([Parameter(Mandatory)][Collections.IDictionary]$Controls, [string]$ToolPath)
    $result = Invoke-BonsaiTool -Command qualification -Request @{ operation = 'controls'; controls = $Controls } -ToolPath $ToolPath
    return [string]$result.Sha256
}

function Test-BonsaiLoraQualification {
    <#
    .SYNOPSIS
    Scores matched evidence and independently supplied external human reviews.
    .DESCRIPTION
    Malformed evidence throws, known regressions reject, and missing proofs or
    complete human reviews remain pending. The Rust scorer preserves the fixed
    gate policy and canonical identities. This adapter serializes each input once.
    #>
    [CmdletBinding()]
    [OutputType([pscustomobject])]
    param(
        [Parameter(Mandatory)][Collections.IDictionary]$Evidence,
        [AllowNull()][Collections.IDictionary]$Review,
        [string]$ToolPath
    )
    $result = Invoke-BonsaiTool -Command qualification -Request @{ operation = 'score'; evidence = $Evidence; review = $Review } -ToolPath $ToolPath
    return [pscustomobject]$result
}

Export-ModuleMember -Function Get-BonsaiQualificationDigest, Get-BonsaiLoraQualificationPlan, Get-BonsaiLoraControlDigest, Test-BonsaiLoraQualification
