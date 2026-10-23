#requires -Version 7.6
<#
.SYNOPSIS
Reports mandatory LoRA gates from captured evidence and an external human review.
.DESCRIPTION
Read-only and never starts a model or enables a serving profile. Review records
must bind each exact response digest and attest direct human review. Missing
correctness evidence or reviews remain pending; known regressions are rejected.
Evidence and reviews must use standard JSON. UTF-8 and BOM-identified Unicode
encodings retain the previous reader's decoding behavior.
#>
[CmdletBinding()]
[OutputType([pscustomobject])]
param([Parameter(Mandatory)][string]$EvidencePath, [string]$ReviewPath, [string]$ToolPath)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Tools.psm1')
# Rust reads and validates the original bounded files directly; no PowerShell
# JSON parse/serialize roundtrip can alter their numeric or Unicode contents.
$request = @{ operation = 'file-score'; evidencePath = $EvidencePath
    reviewPath = $(if ($ReviewPath) { $ReviewPath } else { $null }) }
[pscustomobject](Invoke-BonsaiTool -Command qualification -Request $request -ToolPath $ToolPath)
