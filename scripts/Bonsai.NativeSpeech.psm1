#requires -Version 7.6
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Tools.psm1')

function Install-NativeSpeechModels {
    <# .SYNOPSIS
    Provisions pinned speech files through Rust; no model or audio device is opened.
    #>
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseShouldProcessForStateChangingFunctions', '', Justification = 'Explicit artifact provisioning through the checked Rust maintenance command.')]
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseSingularNouns', '', Justification = 'Public API provisions the selected set of speech models.')]
    [CmdletBinding()]
    [OutputType([pscustomobject])]
    param([Parameter(Mandatory)][string]$DataDirectory,
          [ValidateSet('All', 'Canary', 'QwenTts')][string]$Model = 'All',
          [ValidateSet('Q8_0', 'Q4_K_M')][string]$TtsQuantization = 'Q8_0',
          [switch]$VerifyOnly, [switch]$DeferCachedVerification, [string]$ToolPath)
    $request = @{ operation = 'models'; dataDirectory = $DataDirectory; model = $Model;
        ttsQuantization = $TtsQuantization.ToUpperInvariant(); verifyOnly = [bool]$VerifyOnly;
        deferCachedVerification = [bool]$DeferCachedVerification; workingDirectory = Get-BonsaiToolScratch -Name 'native-speech' }
    [pscustomobject](Invoke-BonsaiTool -Command speech-build -Request $request -ToolPath $ToolPath -TimeoutSeconds 0)
}

function Initialize-NativeSpeechBuild {
    <# .SYNOPSIS
    Returns Rust's verified compiler/source plan and child-only build environment.
    #>
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseShouldProcessForStateChangingFunctions', '', Justification = 'Explicit native build preparation delegated to Rust.')]
    [CmdletBinding()]
    [OutputType([pscustomobject])]
    param([Parameter(Mandatory)][string]$DataDirectory, [Parameter(Mandatory)][string]$WorkingDirectory, [string]$ToolPath)
    [pscustomobject](Invoke-BonsaiTool -Command speech-build -Request @{
        operation = 'environment'; dataDirectory = $DataDirectory; workingDirectory = $WorkingDirectory
    } -ToolPath $ToolPath -TimeoutSeconds 0)
}

function Invoke-NativeSpeechBuild {
    <# .SYNOPSIS
    Reuses verified native speech builds or builds their pinned sources in Rust.
    #>
    [CmdletBinding()]
    [OutputType([pscustomobject])]
    param([Parameter(Mandatory)][string]$DataDirectory, [Parameter(Mandatory)][string]$WorkingDirectory,
          [Parameter(Mandatory)][string]$RepositoryRoot,
          [ValidateSet('All', 'Canary', 'QwenTts')][string]$Model = 'All', [string]$ToolPath, [switch]$ForceBuild)
    [pscustomobject](Invoke-BonsaiTool -Command speech-build -Request @{
        operation = 'build'; dataDirectory = $DataDirectory; workingDirectory = $WorkingDirectory;
        repositoryRoot = $RepositoryRoot; model = $Model; forceBuild = [bool]$ForceBuild
    } -ToolPath $ToolPath -TimeoutSeconds 0)
}

Export-ModuleMember -Function Install-NativeSpeechModels, Initialize-NativeSpeechBuild, Invoke-NativeSpeechBuild
