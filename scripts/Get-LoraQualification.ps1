#requires -Version 7.6
<#
.SYNOPSIS
Reports mandatory LoRA gates from captured evidence and an external human review.
.DESCRIPTION
Read-only and never starts a model or enables a serving profile. Review records
must bind each exact response digest and attest direct human review. Missing
correctness evidence or reviews remain pending; known regressions are rejected.
#>
[CmdletBinding()]
[OutputType([pscustomobject])]
param([Parameter(Mandatory)][string]$EvidencePath, [string]$ReviewPath)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1') -Force
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Qualification.psm1') -Force
function Read-QualificationJson {
    [CmdletBinding()]
    [OutputType([Collections.IDictionary])]
    param([string]$Path)
    $full = Resolve-ExternalPath $Path
    $guard = [IO.File]::Open($full, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
    try {
        if ($guard.Length -gt 16MB) { throw 'Qualification JSON exceeds its bounded input size.' }
        $reader = [IO.StreamReader]::new($guard, [Text.UTF8Encoding]::new($false, $true), $true, 4096, $true)
        try { $text = $reader.ReadToEnd() } finally { $reader.Dispose() }
        $value = $text | ConvertFrom-Json -AsHashtable -DateKind String -Depth 32
        if ($value -isnot [Collections.IDictionary]) { throw 'Qualification input must be a JSON object.' }
        return $value
    } finally { $guard.Dispose() }
}
$evidence = Read-QualificationJson $EvidencePath
$review = if ($ReviewPath) { Read-QualificationJson $ReviewPath } else { $null }
Test-BonsaiLoraQualification -Evidence $evidence -Review $review
