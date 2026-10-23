#requires -Version 7.6
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot '../scripts/Bonsai.Gguf.psm1') -Force
$script:GgufTool = Resolve-GgufTool -GgufToolPath $env:BONSAI_GGUF_TOOL

function Read-TestGguf {
    <#
    .SYNOPSIS
    Inspects an independently hashed synthetic fixture through the Rust CLI.
    #>
    [CmdletBinding()]
    [OutputType([Collections.IDictionary])]
    param([string]$Path, [string]$Dialect = 'Stock', [bool]$Census = $false)
    $arguments = @('inspect', '--model', $Path, '--expected-bytes', [string](Get-Item -LiteralPath $Path).Length,
        '--expected-sha256', (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash, '--dialect', $Dialect)
    if ($Census) { $arguments += '--census-pq2' }
    Invoke-GgufTool -GgufToolPath $script:GgufTool -Arguments $arguments
}

function Export-TestGguf {
    <#
    .SYNOPSIS
    Exports an independently hashed synthetic fixture through the Rust CLI.
    #>
    [CmdletBinding()]
    [OutputType([Collections.IDictionary])]
    param([string]$Path, [string]$OutputPath, [int]$TimeoutSeconds)
    Invoke-GgufTool -GgufToolPath $script:GgufTool -Arguments @('export-ptq1', '--model', $Path,
        '--output', $OutputPath, '--expected-bytes', [string](Get-Item -LiteralPath $Path).Length,
        '--expected-sha256', (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash,
        '--timeout-seconds', [string]$TimeoutSeconds)
}

Export-ModuleMember -Function Read-TestGguf, Export-TestGguf
