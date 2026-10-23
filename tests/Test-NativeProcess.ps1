#requires -Version 7.6
<#
.SYNOPSIS
Runs one focused synthetic native-tool launch and output-capture check.
.DESCRIPTION
Creates disposable PowerShell probes in a unique external temp directory. These
checks exercise process mechanics without models, builds, or benchmarks.
#>
[CmdletBinding()]
[OutputType([string])]
param([Parameter(Mandatory)][ValidateSet('Capture', 'Failure', 'Environment', 'Timeout', 'Drain')][string]$Case)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true

Import-Module -Name (Join-Path $PSScriptRoot '../scripts/Bonsai.Native.psm1') -Force
$executable = [Environment]::ProcessPath
$scratch = [IO.Path]::GetFullPath((Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-NativeProcessTest-' + [guid]::NewGuid().ToString('N'))))
if (-not [IO.Path]::IsPathFullyQualified($scratch) -or $scratch -match '%[^%]+%') { throw 'The native test lane must be expanded and absolute.' }
$workingDirectory = Join-Path $scratch 'working directory & literal'
[void][IO.Directory]::CreateDirectory($workingDirectory)
$probe = Join-Path $scratch 'native probe.ps1'
[IO.File]::WriteAllText($probe, @'
#requires -Version 7.6
[CmdletBinding()]
[OutputType([void])]
param(
    [Parameter(Mandatory)][string]$Mode,
    [Parameter(ValueFromRemainingArguments)][AllowEmptyCollection()][AllowEmptyString()][string[]]$Values = @()
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
switch ($Mode) {
    'Arguments' {
        [Console]::Out.Write((ConvertTo-Json -InputObject @($Values) -Compress))
        [Console]::Error.Write(" `tstderr evidence`r`n ")
    }
    'Volume' {
        [Console]::Out.Write(" `tstdout prefix`r`n")
        [Console]::Error.Write(" `tstderr prefix`r`n")
        for ($index = 0; $index -lt 64; $index++) {
            [Console]::Out.Write([string]::new([char]'o', 8192))
            [Console]::Error.Write([string]::new([char]'e', 8192))
        }
        [Console]::Out.Write("`r`n stdout suffix`t ")
        [Console]::Error.Write("`r`n stderr suffix`t ")
    }
    'Failure' {
        [Console]::Out.Write(" failure output`t`r`n ")
        [Console]::Error.Write(" failure error`t`r`n ")
        exit 37
    }
    'Environment' {
        $systemVariables = [ordered]@{}
        foreach ($name in @('SystemDrive', 'SystemRoot', 'windir', 'ProgramData', 'LOCALAPPDATA', 'APPDATA', 'USERPROFILE')) {
            $systemVariables[$name] = [Environment]::GetEnvironmentVariable($name)
        }
        [Console]::Out.Write((ConvertTo-Json -InputObject ([ordered]@{
            WorkingDirectory = [Environment]::CurrentDirectory
            Overlay = [Environment]::GetEnvironmentVariable($Values[0])
            SystemVariables = $systemVariables
        }) -Compress))
    }
    'Sleep' {
        [IO.File]::WriteAllText($Values[0], [string]$PID)
        Start-Sleep -Seconds 300
    }
    { $_ -in @('Tree', 'Drain') } {
        $start = [Diagnostics.ProcessStartInfo]::new([Environment]::ProcessPath)
        $start.WorkingDirectory = [Environment]::CurrentDirectory
        $start.UseShellExecute = $false
        $start.CreateNoWindow = $true
        foreach ($argument in @('-NoLogo', '-NoProfile', '-NonInteractive', '-File', $PSCommandPath, '-Mode', 'Sleep', ($Values[0] + '.child'))) { $start.ArgumentList.Add($argument) }
        $child = [Diagnostics.Process]::Start($start)
        try {
            [IO.File]::WriteAllText($Values[0], (ConvertTo-Json -InputObject @($PID, $child.Id) -Compress))
            [Console]::Out.Write('before-timeout stdout')
            [Console]::Out.Flush()
            [Console]::Error.Write('before-timeout stderr')
            [Console]::Error.Flush()
            if ($Mode -eq 'Tree') { Start-Sleep -Seconds 300 }
        } finally { $child.Dispose() }
    }
    default { throw "Unknown native probe mode: $Mode" }
}
'@)

Import-Module (Join-Path $PSScriptRoot 'Bonsai.TestSupport.psm1')


function Invoke-Probe {
    [CmdletBinding()]
    [OutputType([pscustomobject])]
    param(
        [string]$Mode,
        [AllowEmptyCollection()][AllowEmptyString()][string[]]$Arguments = @(),
        [int]$TimeoutSeconds = 30,
        [Collections.IDictionary]$Environment,
        [string]$LogPath
    )
    $parameters = @{
        Executable = $executable
        ArgumentList = @('-NoLogo', '-NoProfile', '-NonInteractive', '-File', $probe, '-Mode', $Mode) + $Arguments
        WorkingDirectory = $workingDirectory
        TimeoutSeconds = $TimeoutSeconds
    }
    if ($Environment) { $parameters.Environment = $Environment }
    if ($LogPath) { $parameters.LogPath = $LogPath }
    return Invoke-BonsaiNativeProcess @parameters
}
function Stop-TestProcess {
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseShouldProcessForStateChangingFunctions', '', Justification = 'Only processes identified by this synthetic probe are cleaned up.')]
    [CmdletBinding()]
    [OutputType([void])]
    param([int]$ProcessId)
    $process = $null
    try {
        try { $process = [Diagnostics.Process]::GetProcessById($ProcessId) } catch [ArgumentException] { return }
        if (-not $process.HasExited) { $process.Kill($true); $process.WaitForExit() }
    } finally { if ($null -ne $process) { $process.Dispose() } }
}

switch ($Case) {
    'Capture' {
        $arguments = @('', 'space and & shell | symbols', 'a"b', 'C:\trailing slash\', 'slash\\"quote', '$(literal); < > %PATH%', "unicode-$([char]0x03bb)")
        $result = Invoke-Probe -Mode Arguments -Arguments $arguments
        Assert ($result.ExitCode -eq 0) 'The literal argument probe failed.'
        $actual = @($result.StandardOutput | ConvertFrom-Json -DateKind String)
        Assert ($actual.Count -eq $arguments.Count) 'The literal native argument count changed.'
        for ($index = 0; $index -lt $arguments.Count; $index++) { Assert ($actual[$index] -ceq $arguments[$index]) "Native argument $index changed." }
        Assert ($result.StandardError -ceq " `tstderr evidence`r`n ") 'Standard error whitespace changed or was merged into standard output.'
        $result = Invoke-Probe -Mode Volume
        Assert ($result.ExitCode -eq 0) 'The simultaneous output probe failed.'
        Assert ($result.StandardOutput -ceq (" `tstdout prefix`r`n" + [string]::new([char]'o', 524288) + "`r`n stdout suffix`t ")) 'Large standard output was truncated, mixed, or normalized.'
        Assert ($result.StandardError -ceq (" `tstderr prefix`r`n" + [string]::new([char]'e', 524288) + "`r`n stderr suffix`t ")) 'Large standard error was truncated, mixed, or normalized.'
        # pwsh with no arguments is interactive; use a benign native executable
        # to exercise the empty argument collection without an input dependency.
        $noArgumentExecutable = if ($IsWindows) { Join-Path $env:SystemRoot 'System32/hostname.exe' } else { '/bin/pwd' }
        $result = Invoke-BonsaiNativeProcess -Executable $noArgumentExecutable -ArgumentList @() -WorkingDirectory $workingDirectory -TimeoutSeconds 30
        Assert ($result.ExitCode -eq 0 -and $result.StandardOutput.Length -gt 0) 'An empty native argument collection was rejected.'
    }
    'Failure' {
        $log = Join-Path $scratch 'nonzero.log'
        $result = Invoke-Probe -Mode Failure -LogPath $log
        Assert ($result.ExitCode -eq 37) 'The helper threw or changed a nonzero exit code.'
        Assert ($result.StandardOutput -ceq " failure output`t`r`n " -and $result.StandardError -ceq " failure error`t`r`n ") 'Nonzero process output was lost.'
        Assert ([IO.File]::ReadAllText($log) -ceq ($result.StandardOutput + $result.StandardError)) 'The combined nonzero process log changed.'
        foreach ($directory in @('.', '%TEMP%', (Join-Path $scratch 'missing'), $probe)) {
            $null = Assert-Failure { Invoke-BonsaiNativeProcess -Executable $executable -ArgumentList @('-NoProfile', '-Command', 'exit 0') -WorkingDirectory $directory } "Invalid working directory was accepted: $directory"
        }
        $null = Assert-Failure { Invoke-BonsaiNativeProcess -Executable (Join-Path $scratch 'missing.exe') -ArgumentList @() -WorkingDirectory $workingDirectory } 'A missing executable was accepted.'
        $null = Assert-Failure { Invoke-Probe -Mode Failure -LogPath 'relative.log' } 'A relative native log path was accepted.'
    }
    'Environment' {
        $variableName = 'SWIFTBONSAI_NATIVE_TEST_' + [guid]::NewGuid().ToString('N')
        $original = [Environment]::GetEnvironmentVariable($variableName)
        try {
            [Environment]::SetEnvironmentVariable($variableName, 'parent value')
            $result = Invoke-Probe -Mode Environment -Arguments @($variableName) -Environment @{ $variableName = 'child value & literal' }
            Assert ($result.ExitCode -eq 0) 'The child environment probe failed.'
            $actual = $result.StandardOutput | ConvertFrom-Json -DateKind String
            Assert ($actual.WorkingDirectory -ceq $workingDirectory) 'The child native working directory was not the explicit external temp directory.'
            Assert ($actual.Overlay -ceq 'child value & literal') 'The per-child environment overlay was lost.'
            Assert ([Environment]::GetEnvironmentVariable($variableName) -ceq 'parent value') 'The helper mutated the parent environment.'
            foreach ($name in @('SystemDrive', 'SystemRoot', 'windir', 'ProgramData', 'LOCALAPPDATA', 'APPDATA', 'USERPROFILE')) {
                Assert ($actual.SystemVariables.PSObject.Properties[$name].Value -ceq [Environment]::GetEnvironmentVariable($name)) "The inherited system/profile variable $name changed."
            }
        } finally { [Environment]::SetEnvironmentVariable($variableName, $original) }
    }
    { $_ -in @('Timeout', 'Drain') } {
        $marker = Join-Path $scratch 'process-tree.json'
        $log = Join-Path $scratch 'timeout.log'
        $mode = if ($Case -eq 'Timeout') { 'Tree' } else { 'Drain' }
        $processIds = @()
        try {
            $message = Assert-Failure -PassThru { Invoke-Probe -Mode $mode -Arguments @($marker) -TimeoutSeconds 10 -LogPath $log } 'The long-running process or inherited output pipe was not bounded.'
            Assert ([IO.File]::Exists($marker)) 'The synthetic process did not publish its ownership marker.'
            $processIds = @([IO.File]::ReadAllText($marker) | ConvertFrom-Json -DateKind String)
            Assert ($processIds.Count -eq 2) 'The synthetic process tree marker was incomplete.'
            if ($Case -eq 'Timeout') {
                Assert ($message -match 'exceeded|timed out|timeout') 'The long-running process failed for a reason other than timeout.'
                Assert ([IO.File]::Exists($log)) 'Timeout output was not retained.'
                $output = [IO.File]::ReadAllText($log)
                Assert ($output.Contains('before-timeout stdout') -and $output.Contains('before-timeout stderr')) 'The timeout log omitted pre-timeout output.'
                foreach ($processId in $processIds) {
                    $process = $null
                    try {
                        try { $process = [Diagnostics.Process]::GetProcessById([int]$processId) } catch [ArgumentException] { continue }
                        Assert ($process.WaitForExit(10000)) "Native timeout left process $processId running."
                    } finally { if ($null -ne $process) { $process.Dispose() } }
                }
            } else {
                # Ordinary tools are not Windows Job-owned. A root that already
                # exited cannot provide descendant ownership; this case verifies
                # bounded pipe drainage and explicitly cleans up its own child.
                Assert ($message -match 'output drain timed out') 'An inherited pipe failed for a reason other than bounded output drainage.'
            }
        } finally {
            if (-not $processIds.Count -and [IO.File]::Exists($marker)) { $processIds = @([IO.File]::ReadAllText($marker) | ConvertFrom-Json -DateKind String) }
            foreach ($processId in $processIds) { Stop-TestProcess -ProcessId ([int]$processId) }
        }
    }
}
Write-Output "Focused native process $Case checks passed. Evidence: $scratch"
