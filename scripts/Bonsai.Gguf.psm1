#requires -Version 7.6
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1')
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Native.psm1')

$script:GgufScratch = $null

function Get-BonsaiGgufScratch {
    [CmdletBinding()]
    [OutputType([string])]
    param()
    if (-not $script:GgufScratch) {
        $script:GgufScratch = Resolve-ExternalPath (Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-Gguf-' + [guid]::NewGuid().ToString('N')))
        [void][IO.Directory]::CreateDirectory($script:GgufScratch)
    }
    return $script:GgufScratch
}

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
    Invokes a prebuilt GGUF CLI or incrementally builds the pinned native Rust tool.
    #>
    [CmdletBinding()]
    [OutputType([string])]
    param([string]$GgufToolPath)
    $scratch = Get-BonsaiGgufScratch
    if ($GgufToolPath) {
        if (-not [IO.Path]::IsPathFullyQualified($GgufToolPath) -or $GgufToolPath -match '%[^%]+%' -or
            -not [IO.File]::Exists($GgufToolPath)) {
            throw 'GgufToolPath must name an existing expanded absolute executable.'
        }
        $tool = [IO.Path]::GetFullPath($GgufToolPath)
    } else {
        $cargo = (Get-Command -Name cargo -CommandType Application -ErrorAction Stop).Source
        $manifest = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../tools/gguf/Cargo.toml'))
        $target = Join-Path $scratch 'target'
        # Always ask Cargo to check source freshness; the session target preserves
        # incremental work without trusting a stale cached executable.
        $buildArguments = @('+1.99.1-aarch64-pc-windows-msvc', 'build', '--locked', '--release', '--manifest-path', $manifest, '--target-dir', $target)
        $log = Join-Path $scratch ('cargo-build-' + [guid]::NewGuid().ToString('N') + '.log')
        $null = Invoke-BonsaiGgufProcess -Executable $cargo -ArgumentList $buildArguments -WorkingDirectory $scratch -LogPath $log
        $tool = Join-Path $target 'release/bonsai-gguf.exe'
        if (-not [IO.File]::Exists($tool)) { throw "The GGUF build did not produce its executable. Build log: $log" }
    }
    return $tool
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
    $scratch = Get-BonsaiGgufScratch
    $json = Invoke-BonsaiGgufProcess -Executable $tool -ArgumentList $Arguments -WorkingDirectory $scratch
    $result = ConvertFrom-Json -InputObject $json -Depth 30 -AsHashtable -NoEnumerate -DateKind String
    if ($result -isnot [Collections.IDictionary]) { throw 'The GGUF tool did not return a JSON object.' }
    return $result
}

Export-ModuleMember -Function Resolve-GgufTool, Invoke-GgufTool
