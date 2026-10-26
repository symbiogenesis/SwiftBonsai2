#requires -Version 7.6
<# .SYNOPSIS
Reports the bounded PTQ1 decision screen without loading or timing a model.
#>
[CmdletBinding()]
[OutputType([pscustomobject])]
param([Parameter(Mandatory)][string]$EvidencePath, [string]$ToolPath)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Tools.psm1')
[pscustomobject](Invoke-BonsaiTool -Command qualification -ToolPath $ToolPath -Request @{
    operation = 'screen-file-score'; evidencePath = $EvidencePath; reviewPath = $null
})
