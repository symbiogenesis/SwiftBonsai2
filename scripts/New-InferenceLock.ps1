#requires -Version 7.6
<#
.SYNOPSIS
Verifies a reviewed Prism release bundle and writes a new candidate runtime lock.
.DESCRIPTION
Use artifacts from the reviewed successful main-branch Foundation run. Supply
the expected source revision, version and run identity independently of the
receipt, plus the source ZIP downloaded from that exact GitHub commit. This
command checks bytes and identities; the receipt does not prove CI acceptance
or source-to-binary provenance. Review the originating run before promotion.
The Rust review operation performs no network access, model loading or runtime execution.
Select a trusted maintenance binary with ToolPath or BONSAI_TOOLS_PATH; otherwise
the resolver builds the pinned maintenance tool in external temporary storage.
The candidate and verification evidence must be outside Git; existing files
are never replaced. Publishing a release and promoting the pin are separate.
#>
[CmdletBinding()]
[OutputType([pscustomobject])]
param(
    [Parameter(Mandatory)][string]$ReceiptPath,
    [Parameter(Mandatory)][string]$ArchivePath,
    [Parameter(Mandatory)][string]$SourceArchivePath,
    [Parameter(Mandatory)][ValidatePattern('^[a-f0-9]{40}$')][string]$SourceRevision,
    [Parameter(Mandatory)][ValidatePattern('^[0-9]+\.[0-9]+\.[0-9]+$')][string]$Version,
    [Parameter(Mandatory)][ValidatePattern('^[1-9][0-9]*$')][string]$RunId,
    [ValidateRange(1, 2147483647)][int]$RunAttempt = 1,
    [Parameter(Mandatory)][string]$OutputPath,
    [string]$ToolPath
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1')
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Tools.psm1')
$result = Invoke-BonsaiTool -Command 'inference-lock' -ToolPath $ToolPath -TimeoutSeconds 120 -Request @{
    receiptPath = Resolve-ExternalPath $ReceiptPath
    archivePath = Resolve-ExternalPath $ArchivePath
    sourceArchivePath = Resolve-ExternalPath $SourceArchivePath
    sourceRevision = $SourceRevision; version = $Version; runId = $RunId; runAttempt = $RunAttempt
    outputPath = Resolve-ExternalPath $OutputPath
}
[pscustomobject]$result