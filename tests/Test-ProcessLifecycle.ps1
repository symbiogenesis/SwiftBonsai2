#requires -Version 7.6
<#
.SYNOPSIS
Runs one focused synthetic Rust Windows process ownership or resource-limit check.
.DESCRIPTION
Uses the existing audited Windows process crate and its test-only native probe.
No model, inference, benchmark, full suite or C# process implementation is involved.
#>
[CmdletBinding()]
[OutputType([string])]
param([Parameter(Mandatory)][ValidateSet('Lifetime', 'Limits', 'LaunchContract')][string]$Case)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
if (-not $IsWindows) { throw 'Windows Job lifecycle checks require Windows 10 or later.' }
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
Import-Module (Join-Path $repo 'scripts/Bonsai.Native.psm1')
$scratch = [IO.Path]::GetFullPath((Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-ProcessTest-' + [guid]::NewGuid().ToString('N'))))
[void][IO.Directory]::CreateDirectory($scratch)
$test = switch ($Case) {
    'Lifetime' { 'stop_and_drop_kill_descendants_and_observe_memory' }
    'Limits' { 'committed_memory_limit_denies_allocations' }
    'LaunchContract' { 'argv_environment_and_native_cwd' }
}
$cargo = (Get-Command cargo -CommandType Application -ErrorAction Stop).Source
$arguments = @('+1.99.1-aarch64-pc-windows-msvc', 'test', '--locked', '--offline', '--release',
    '--manifest-path', (Join-Path $repo 'src/windows-process/Cargo.toml'), '--target-dir', (Join-Path $scratch 'target'),
    '--features', 'test-probe', '--test', 'ownership', $test, '--', '--exact')
$result = Invoke-BonsaiNativeProcess -Executable $cargo -ArgumentList $arguments -WorkingDirectory $scratch -TimeoutSeconds 300 -LogPath (Join-Path $scratch 'test.log')
if ($result.ExitCode -ne 0) { throw "Focused Rust Windows Job $Case check failed. Evidence: $scratch" }
Write-Output "Focused Rust Windows Job $Case checks passed. Evidence: $scratch"
