#requires -Version 7.6
<#
.SYNOPSIS
Installs or verifies the pinned prebuilt Prism Inference executables.
.DESCRIPTION
Downloads only the exact locked Windows ARM64 release, verifies its archive and
every file, and publishes a new external installation without replacing files.
Ordinary provisioning invokes no Rust toolchain, source build, model loading,
test or benchmark. IncludeCapabilities additionally runs a bounded model-free
probe through a trusted maintenance tool; ToolPath selects it or the resolver
builds the pinned maintenance source in external storage. The probe accepts
only this checkout's compiled release lock and retains verification evidence.
#>
[CmdletBinding()]
[OutputType([pscustomobject])]
param([Parameter(Mandatory)][string]$DataDirectory, [string]$LockPath, [switch]$VerifyOnly,
      [switch]$IncludeCapabilities, [string]$ToolPath)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Inference.psm1') -Force
if ($IncludeCapabilities -and $LockPath) { throw 'IncludeCapabilities uses the compiled release lock; omit LockPath.' }
if ($ToolPath -and -not $IncludeCapabilities) { throw 'ToolPath requires IncludeCapabilities.' }
$parameters = @{ DataDirectory = $DataDirectory; VerifyOnly = $VerifyOnly }
if ($LockPath) { $parameters.LockPath = $LockPath }
$installed = Install-BonsaiInference @parameters
if ($IncludeCapabilities) {
    Import-Module (Join-Path $PSScriptRoot 'Bonsai.Tools.psm1')
    $discovery = Invoke-BonsaiTool -Command 'inference-capabilities' -ToolPath $ToolPath -Request @{
        schemaVersion = 1; dataDirectory = [IO.Path]::GetFullPath($DataDirectory)
    }
    Assert-BonsaiInferenceCapabilityReceipt -Lock (Read-BonsaiInferenceLock) -Installed $installed -Receipt $discovery -DataDirectory $DataDirectory
    $installed | Add-Member -NotePropertyName Capabilities -NotePropertyValue $discovery.Capabilities
    $installed | Add-Member -NotePropertyName CapabilityVerification -NotePropertyValue $discovery
}
$installed
