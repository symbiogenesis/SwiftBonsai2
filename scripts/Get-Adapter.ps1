#requires -Version 7.6
<#
.SYNOPSIS
Provisions or verifies the fixed, owner-approved pretrained Bonsai LoRA artifact.
.DESCRIPTION
Uses the repository's immutable adapter lock and bounded size/SHA-256-verified
download helper. Stores only beneath an existing external DataDirectory, without
overwriting a mismatched artifact. Does not train, load a model, enable a profile,
or establish behavioral compatibility with Swift Bonsai.
#>
[CmdletBinding()]
[OutputType([pscustomobject])]
param([Parameter(Mandatory)][string]$DataDirectory, [switch]$VerifyOnly)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1') -Force
$lock = Get-Content -LiteralPath (Join-Path $PSScriptRoot '../config/swift-lora.lock.json') -Raw |
    ConvertFrom-Json -DateKind String -AsHashtable
$adapter = $lock.adapter
if ($lock.schemaVersion -ne 1 -or
    $adapter.repository -cne 'Continuum-AI-Corp/OrcaBonsai-27B-Uncensored' -or
    $adapter.revision -cnotmatch '^[a-f0-9]{40}$' -or
    $adapter.path -cne 'gguf/bonsai-abliterate-lora.gguf' -or
    $adapter.filename -cne 'bonsai-abliterate-lora.gguf' -or
    ($adapter.bytes -isnot [long] -and $adapter.bytes -isnot [int]) -or $adapter.bytes -le 0 -or
    $adapter.sha256 -cnotmatch '^[a-f0-9]{64}$') {
    throw 'Invalid or unsupported immutable adapter lock.'
}
$uri = "https://raw.githubusercontent.com/$($adapter.repository)/$($adapter.revision)/$($adapter.path)"
if ($adapter.url -cne $uri) { throw 'Adapter repository, revision, path and source URL disagree.' }
$root = Resolve-ExternalPath $DataDirectory
if (-not [IO.Directory]::Exists($root)) { throw 'The external DataDirectory must already exist.' }
$path = Resolve-ExternalPath (Join-Path $root -ChildPath 'adapters', $adapter.revision, $adapter.filename)
if ($VerifyOnly) { Test-LockedArtifact -Path $path -Bytes $adapter.bytes -Sha256 $adapter.sha256 }
else {
    Receive-LockedArtifact -Uri ([uri]$uri) -Path $path -Bytes $adapter.bytes -Sha256 $adapter.sha256 -TimeoutSeconds 300 | Out-Null
}
[pscustomobject]@{
    Status = 'verified'; AdapterPath = $path; Repository = $adapter.repository
    Revision = $adapter.revision; Sha256 = $adapter.sha256
    Qualification = 'pretrained artifact integrity only; Swift behavior, memory and performance remain unqualified'
}
