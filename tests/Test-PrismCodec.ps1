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
param([Parameter(Mandatory)][string]$DataDirectory)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
if (-not $IsWindows -or [Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture -ne 'Arm64' -or
    [Runtime.InteropServices.RuntimeInformation]::OSArchitecture -ne 'Arm64') { throw 'The pinned native codec probe requires Windows ARM64 PowerShell.' }
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
Import-Module (Join-Path $repo 'scripts/Bonsai.Common.psm1') -Force
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
if (-not ('SwiftBonsai.OwnedProcess' -as [type])) {
    Add-Type -Path (Join-Path $repo 'scripts/Bonsai.Process.cs') -CompilerOptions @('/nullable:enable', '/checked+', '/unsafe-', '/warn:9999', '/warnaserror+', '/langversion:14.0')
}
$scratch = Resolve-ExternalPath -Path (Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-PrismCodec-' + [guid]::NewGuid().ToString('N')))
[void][IO.Directory]::CreateDirectory($scratch)
$childPath = Join-Path $scratch 'probe.ps1'
$resultPath = Join-Path $scratch 'result.json'
$stdoutPath = Join-Path $scratch 'stdout.txt'
$stderrPath = Join-Path $scratch 'stderr.txt'
$owned = $null
try {
    # Paths are named argv entries, never interpolated into executable source.
    @'
#requires -Version 7.6
[CmdletBinding()]
[OutputType([string])]
param([Parameter(Mandatory)][string]$CodecSource, [Parameter(Mandatory)][string]$ProbeSource,
    [Parameter(Mandatory)][string]$RuntimeDirectory, [Parameter(Mandatory)][string]$ResultPath)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Add-Type -Path @($CodecSource, $ProbeSource) -CompilerOptions @('/nullable:enable', '/checked+', '/unsafe-', '/warn:9999', '/warnaserror+', '/langversion:14.0')
$blocks = [SwiftBonsai.Validation.PrismCodecProbe]::Run($RuntimeDirectory)
$result = [ordered]@{
    Status = 'native-fp32-bit-parity-passed'; SyntheticBlocks = $blocks; Fp32Values = $blocks * 128
    NativePq2Type = 142; NativePtq1Type = 143; TraitsBytes = 56; ToFloatOffset = 40
    NativeRuntimeCommit = '73bfe6cc485554822f80c473a3c0c4f25e80f41b'; ModelLoaded = $false
    Scope = 'Synthetic native dequantization and exact symbol/scale packing only; activation quantization, logits and performance unqualified.'
}
$json = $result | ConvertTo-Json -Depth 4
[IO.File]::WriteAllText($ResultPath, $json)
Write-Output $json
'@ | Set-Content -LiteralPath $childPath
    $arguments = @('-NoLogo', '-NoProfile', '-NonInteractive', '-File', $childPath,
        '-CodecSource', (Join-Path $repo 'scripts/Pq2Ptq1Codec.cs'), '-ProbeSource', (Join-Path $PSScriptRoot 'PrismCodecProbe.cs'),
        '-RuntimeDirectory', $runtimeDirectory, '-ResultPath', $resultPath)
    # Inherit the complete system/profile environment. The absolute external
    # native working directory and kill-on-close Job apply from creation.
    $owned = [SwiftBonsai.OwnedProcess]::Start((Join-Path $PSHOME 'pwsh.exe'), $arguments, $scratch, $stdoutPath, $stderrPath, 1GB, 1)
    $deadline = [Environment]::TickCount64 + 30000
    while (-not $owned.Process.WaitForExit(1000)) {
        $owned.Process.Refresh()
        if ($owned.Process.PrivateMemorySize64 -gt 1GB -or $owned.Process.WorkingSet64 -gt 1GB) {
            throw "Native codec child exceeded its private/working-set watchdog. Evidence: $scratch"
        }
        if ([Environment]::TickCount64 -ge $deadline) { throw "Native codec child exceeded its 30-second deadline. Evidence: $scratch" }
    }
    $exitCode = $owned.Process.ExitCode
    $peak = $owned.GetPeakJobMemoryBytes()
    $owned.Stop(10000)
    if ($exitCode -ne 0) { throw "Native codec child failed with exit code $exitCode. Evidence: $scratch" }
    if (-not [IO.File]::Exists($resultPath)) { throw "Native codec child produced no result. Evidence: $scratch" }
    $result = Get-Content -LiteralPath $resultPath -Raw | ConvertFrom-Json -DateKind String
    if ($result.Status -cne 'native-fp32-bit-parity-passed' -or $result.SyntheticBlocks -ne 6670 -or
        $result.Fp32Values -ne 853760 -or $result.NativePq2Type -ne 142 -or $result.NativePtq1Type -ne 143 -or
        $result.TraitsBytes -ne 56 -or $result.ToFloatOffset -ne 40 -or $result.ModelLoaded -ne $false -or
        $result.NativeRuntimeCommit -cne $lock.runtime.commit -or $peak -gt 1GB) {
        throw "Native codec evidence did not match the fixed test contract. Evidence: $scratch"
    }
    Write-Output "Focused native PQ2/PTQ1 FP32 bit-parity checks passed. Evidence: $scratch"
} finally {
    try {
        if ($null -ne $owned) {
            try { $owned.Stop(10000) } finally { $owned.Dispose() }
        }
    } finally {
        # Remove only this generated source; retain useful stdout/stderr/result.
        if ([IO.File]::Exists($childPath)) { Remove-Item -LiteralPath $childPath -Force }
    }
}
