#requires -Version 7.6
<#
.SYNOPSIS
Checks source pinning, deterministic overlay output, and rejection without edits.
.DESCRIPTION
Focused source-application checks only. Does not compile, execute native code,
load a model, or benchmark. The real-ggml native regression is a separate CTest.
#>
[CmdletBinding()]
[OutputType([string])]
param([Parameter(Mandatory)][string]$SourceArchive)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$overlay = Join-Path $repo 'tools/prism/Apply-Overlay.ps1'
$lock = Get-Content -LiteralPath (Join-Path $repo 'tools/prism/source.lock.json') -Raw | ConvertFrom-Json -DateKind String
$result = & (Join-Path $repo 'scripts/Build-PrismEngine.ps1') -SourceArchive $SourceArchive -PrepareOnly
$adapter = Join-Path $result.SourceDirectory 'src/llama-adapter.cpp'
$header = Join-Path $result.SourceDirectory 'src/llama-lora-b-share.h'
if ((Get-FileHash -LiteralPath $adapter -Algorithm SHA256).Hash.ToLowerInvariant() -cne $lock.OverlaidAdapterSha256 -or
    (Get-FileHash -LiteralPath $header -Algorithm SHA256).Hash -cne (Get-FileHash -LiteralPath (Join-Path $repo 'tools/prism/llama-lora-b-share.h') -Algorithm SHA256).Hash) {
    throw 'Overlay source output is not deterministic.'
}
$rejected = $false
try { $null = & $overlay -SourceDirectory $result.SourceDirectory }
catch { $rejected = $_.Exception.Message -eq 'Pinned llama-adapter.cpp source hash mismatch.' }
if (-not $rejected) { throw 'Repeat application was not rejected.' }
if ((Get-FileHash -LiteralPath $adapter -Algorithm SHA256).Hash.ToLowerInvariant() -cne $lock.OverlaidAdapterSha256) { throw 'Repeat rejection changed source.' }
$fixture = Join-Path $result.EvidenceDirectory 'drift-fixture'
[IO.Directory]::CreateDirectory((Join-Path $fixture 'src')) | Out-Null
$driftPath = Join-Path $fixture 'src/llama-adapter.cpp'
[IO.File]::WriteAllText($driftPath, '// wrong source revision')
$before = (Get-FileHash -LiteralPath $driftPath -Algorithm SHA256).Hash
$rejected = $false
try { $null = & $overlay -SourceDirectory $fixture }
catch { $rejected = $_.Exception.Message -eq 'Pinned llama-adapter.cpp source hash mismatch.' }
if (-not $rejected -or (Get-FileHash -LiteralPath $driftPath -Algorithm SHA256).Hash -cne $before -or
    [IO.File]::Exists((Join-Path $fixture 'src/llama-lora-b-share.h'))) { throw 'Source drift rejection changed the fixture.' }
Remove-Item -LiteralPath $driftPath -Force
Write-Output "Pinned Prism overlay source/rejection checks passed. Evidence: $($result.EvidenceDirectory)"
