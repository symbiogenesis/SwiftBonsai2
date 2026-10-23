#requires -Version 7.6
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1')

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
    $start = [Diagnostics.ProcessStartInfo]::new($Executable)
    $start.WorkingDirectory = $WorkingDirectory
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    foreach ($argument in $ArgumentList) { $start.ArgumentList.Add($argument) }
    # Inherit the host environment. Only the build target and native cwd move to temp.
    $process = [Diagnostics.Process]::new()
    $process.StartInfo = $start
    $started = $false
    try {
        if (-not $process.Start()) { throw 'The GGUF tool process failed to start.' }
        $started = $true
        $stdout = $process.StandardOutput.ReadToEndAsync()
        $stderr = $process.StandardError.ReadToEndAsync()
        $process.WaitForExit()
        $output = $stdout.GetAwaiter().GetResult()
        $diagnostics = $stderr.GetAwaiter().GetResult()
        if ($LogPath) { [IO.File]::WriteAllText($LogPath, $output + $diagnostics) }
        if ($process.ExitCode -ne 0) {
            $evidence = if ($LogPath) { " Build log: $LogPath" } else { '' }
            throw "GGUF tool exited with code $($process.ExitCode).$evidence`n$diagnostics$output"
        }
        if ($diagnostics) { Write-Verbose -Message $diagnostics.TrimEnd() }
        return $output
    } finally {
        try {
            if ($started -and -not $process.HasExited) { $process.Kill($true); $process.WaitForExit() }
        } finally { $process.Dispose() }
    }
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
