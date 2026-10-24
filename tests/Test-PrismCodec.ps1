#requires -Version 7.6
<#
.SYNOPSIS
Checks exact synthetic PQ2/PTQ1 FP32 parity using the pinned native Prism codecs.
.DESCRIPTION
Requires an already installed, verified Swift assessment runtime, but no weights.
Native code runs only in an owned one-process child with a 1 GiB Job commit cap
and a 30-second deadline. This checks format consumption, not logits or speed.
#>
[CmdletBinding()]
[OutputType([string])]
param([Parameter(Mandatory)][string]$DataDirectory, [string]$ToolPath,
    [string]$ProbeToolPath, [string]$EvidenceDirectory)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
if (-not $IsWindows -or [Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture -ne 'Arm64' -or
    [Runtime.InteropServices.RuntimeInformation]::OSArchitecture -ne 'Arm64') { throw 'The pinned native codec probe requires Windows ARM64 PowerShell.' }
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
Import-Module (Join-Path $repo 'scripts/Bonsai.Common.psm1') -Force
Import-Module (Join-Path $repo 'scripts/Bonsai.Tools.psm1')
$root = Resolve-ExternalPath -Path $DataDirectory
$lockPath = Join-Path $repo 'config/swift-assessment.lock.json'
$lock = Read-BonsaiLock -Path $lockPath
if ($lock.runtime.commit -cne '73bfe6cc485554822f80c473a3c0c4f25e80f41b' -or
    $lock.runtime.asset.sha256 -cne 'cc808b33d35c72a3bd5c69a0d609d422426feb9dce23868e77992b6c33b8ddb3') {
    throw 'The native codec test must be reviewed before changing its pinned ABI or runtime package.'
}
# Verify the complete package, every installed DLL, ARM64 PE headers and version
# before the native child starts. VerifyOnly cannot download models or runtimes.
$engine = & (Join-Path $repo 'scripts/Get-Engine.ps1') -DataDirectory $root -LockPath $lockPath -VerifyOnly
$runtimeDirectory = Resolve-ExternalPath -Path ([IO.Path]::GetDirectoryName($engine.ServerPath))
if (-not $ProbeToolPath -and $env:BONSAI_CODEC_PROBE_PATH) { $ProbeToolPath = $env:BONSAI_CODEC_PROBE_PATH }
$probe = Resolve-BonsaiRustTool -Name 'bonsai-codec-probe' -ManifestRelativePath '../tools/codec-probe/Cargo.toml' -ToolPath $ProbeToolPath
if ($EvidenceDirectory) {
    $scratch = Resolve-ExternalPath -Path $EvidenceDirectory
    # An explicitly selected lane must be new, so stale results cannot count as
    # evidence of a successful child. Existing lanes retain their diagnostics.
    if ([IO.Directory]::Exists($scratch) -or [IO.File]::Exists($scratch)) { throw 'EvidenceDirectory must name a new external directory.' }
} else {
    $scratch = Resolve-ExternalPath -Path (Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-PrismCodec-' + [guid]::NewGuid().ToString('N')))
}
[void][IO.Directory]::CreateDirectory($scratch)
$resultPath = Join-Path $scratch 'result.json'
$arguments = @('--runtime-directory', $runtimeDirectory, '--result', $resultPath)
# Inherit the complete system/profile environment. The absolute external native
# working directory and kill-on-close one-process Job apply from creation.
$supervision = Invoke-BonsaiTool -Command completion -ToolPath $ToolPath -Request @{
    schemaVersion = 1; executable = $probe; arguments = $arguments
    workingDirectory = $scratch; evidenceDirectory = $scratch; artifacts = @()
    memoryLimitBytes = 1GB; requiredAvailableBytes = 0; physicalReserveBytes = 0; timeoutSeconds = 30
    stdoutLimitBytes = $null; stderrLimitBytes = $null; requireNonemptyStdout = $false; ciOnly = $false
}
$peak = $supervision.PeakJobMemoryBytes
if (-not [IO.File]::Exists($resultPath)) { throw "Native codec child produced no result. Evidence: $scratch" }
$result = Get-Content -LiteralPath $resultPath -Raw | ConvertFrom-Json -DateKind String
if ($result.Status -cne 'native-fp32-bit-parity-passed' -or $result.SyntheticBlocks -ne 6670 -or
    $result.Fp32Values -ne 853760 -or $result.NativePq2Type -ne 142 -or $result.NativePtq1Type -ne 143 -or
    $result.TraitsBytes -ne 56 -or $result.ToFloatOffset -ne 40 -or $result.ModelLoaded -ne $false -or
    $result.NativeRuntimeCommit -cne $lock.runtime.commit -or $peak -gt 1GB) {
    throw "Native codec evidence did not match the fixed test contract. Evidence: $scratch"
}
Write-Output "Focused native PQ2/PTQ1 FP32 bit-parity checks passed. Evidence: $scratch"
