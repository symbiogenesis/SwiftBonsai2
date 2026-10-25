#requires -Version 7.6
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1')
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Native.psm1')
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Tools.psm1')
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Inference.psm1')

function Invoke-BonsaiGgufProcess {
    [CmdletBinding()]
    [OutputType([string])]
    param(
        [Parameter(Mandatory)][string]$Executable,
        [Parameter(Mandatory)][string[]]$ArgumentList,
        [Parameter(Mandatory)][string]$WorkingDirectory,
        [string]$LogPath
    )
    $result = Invoke-BonsaiNativeProcess -Executable $Executable -ArgumentList $ArgumentList -WorkingDirectory $WorkingDirectory -LogPath $LogPath
    if ($result.ExitCode -ne 0) {
        $evidence = if ($LogPath) { " Build log: $LogPath" } else { '' }
        throw "GGUF tool exited with code $($result.ExitCode).$evidence`n$($result.StandardError)$($result.StandardOutput)"
    }
    if ($result.StandardError) { Write-Verbose -Message $result.StandardError.TrimEnd() }
    return $result.StandardOutput
}

function Resolve-GgufTool {
    <#
    .SYNOPSIS
    Selects an explicitly supplied GGUF tool or verifies the installed prebuilt release.
    #>
    [CmdletBinding()]
    [OutputType([string])]
    param([string]$GgufToolPath)
    if (-not $GgufToolPath -and $env:BONSAI_GGUF_TOOL) { $GgufToolPath = $env:BONSAI_GGUF_TOOL }
    if ($GgufToolPath) {
        if (-not [IO.Path]::IsPathFullyQualified($GgufToolPath) -or $GgufToolPath -match '%[^%]+%' -or
            -not [IO.File]::Exists($GgufToolPath)) {
            throw 'GgufToolPath must name an existing expanded absolute executable.'
        }
        return [IO.Path]::GetFullPath($GgufToolPath)
    }
    $dataDirectory = if ($env:BONSAI_DATA_DIRECTORY) { $env:BONSAI_DATA_DIRECTORY } else { Join-Path $env:LOCALAPPDATA 'SwiftBonsai2' }
    $installation = Install-BonsaiInference -DataDirectory $dataDirectory -VerifyOnly
    if (-not $installation.Gguf) { throw 'The pinned inference release has no GGUF tool; provision a release containing bonsai-gguf.exe.' }
    return $installation.Gguf
}

function Invoke-GgufTool {
    <#
    .SYNOPSIS
    Invokes the Rust GGUF CLI and returns its single JSON receipt or report.
    #>
    [CmdletBinding()]
    [OutputType([Collections.IDictionary])]
    param(
        [Parameter(Mandatory)][string[]]$Arguments,
        [string]$GgufToolPath
    )
    $tool = Resolve-GgufTool -GgufToolPath $GgufToolPath
    $scratch = Get-BonsaiToolScratch -Name 'bonsai-gguf'
    $json = Invoke-BonsaiGgufProcess -Executable $tool -ArgumentList $Arguments -WorkingDirectory $scratch
    $result = ConvertFrom-Json -InputObject $json -Depth 30 -AsHashtable -NoEnumerate -DateKind String
    if ($result -isnot [Collections.IDictionary]) { throw 'The GGUF tool did not return a JSON object.' }
    return $result
}

Export-ModuleMember -Function Resolve-GgufTool, Invoke-GgufTool
