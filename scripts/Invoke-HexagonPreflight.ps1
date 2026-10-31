#requires -Version 7.6
<#
.SYNOPSIS
Collects bounded Windows Hexagon device and explicit runtime-package inventory.
.DESCRIPTION
Inventory only: never loads a model, DSP or inference DLL, installs a driver,
changes trust or boot policy, or enables an accelerator profile. PackagePath
selects an existing local runtime directory; at most 128 top-level entries and
64 eligible files are inspected, with 128 MiB per file and 256 MiB total hashing.
Catalog signatures do not prove DSP membership or installed deployment trust.
MaximumSeconds is the remaining allowance for this invocation, including worker
startup and cleanup, within the owner's single five-minute aggregate budget.
OutputPath optionally selects a new receipt outside Git. Failure and timeout
receipts retain unknown evidence and leave every execution gate pending.
#>
[CmdletBinding()]
[OutputType([Collections.Specialized.OrderedDictionary])]
param(
    [string]$PackagePath,
    [string]$OutputPath,
    [ValidateRange(5,300)][int]$MaximumSeconds = 45,
    [switch]$PassThru
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
$timer = [Diagnostics.Stopwatch]::StartNew()
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Hexagon.psm1')
if (-not $IsWindows) { throw 'Hexagon preflight requires Windows.' }
if ($PackagePath) { $PackagePath = Get-BonsaiHexagonLocalPath -Path $PackagePath }
if ($OutputPath) { $OutputPath = Get-BonsaiHexagonLocalPath -Path $OutputPath -NewFile }
$scratch = Get-BonsaiHexagonLocalPath -Path ([IO.Path]::GetTempPath())
$module = Join-Path $PSScriptRoot 'Bonsai.Hexagon.psm1'
# Literal string quoting plus EncodedCommand passes paths as data without a shell.
$command = "`$ErrorActionPreference = 'Stop'; Import-Module '" + $module.Replace("'", "''") +
    "'; Get-BonsaiHexagonInventory -PackagePath '" + $PackagePath.Replace("'", "''") + "' | ConvertTo-Json -Depth 8 -Compress"
$start = [Diagnostics.ProcessStartInfo]::new((Join-Path $PSHOME 'pwsh.exe'))
$start.WorkingDirectory = $scratch
$start.UseShellExecute = $false
$start.CreateNoWindow = $true
$start.RedirectStandardOutput = $true
$start.RedirectStandardError = $true
foreach ($argument in @('-NoLogo', '-NoProfile', '-NonInteractive', '-EncodedCommand', [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($command)))) { $start.ArgumentList.Add($argument) }
$process = [Diagnostics.Process]::new()
$process.StartInfo = $start
$inventory = $null
$status = 'failed'
$started = $false
try {
    if (-not $process.Start()) { throw 'Hexagon inventory worker failed to start.' }
    $started = $true
    $stdout = $process.StandardOutput.ReadToEndAsync()
    $stderr = $process.StandardError.ReadToEndAsync()
    # Reserve two seconds for process termination, pipe closure and receipt output.
    $remaining = [Math]::Max(1, ($MaximumSeconds * 1000) - $timer.ElapsedMilliseconds - 2000)
    if (-not $process.WaitForExit([int]$remaining)) { $status = 'deadline-expired' }
    elseif ($process.ExitCode -eq 0 -and $stdout.Wait(250) -and $stderr.Wait(250)) {
        $json = $stdout.GetAwaiter().GetResult()
        if ($json.Length -le 262144 -and $stderr.GetAwaiter().GetResult().Length -eq 0) {
            $inventory = ConvertFrom-Json -InputObject $json -AsHashtable -NoEnumerate -DateKind String
            $status = 'collected'
        }
    }
} catch {
    # Do not publish raw provider errors or turn an incomplete inventory into facts.
    $inventory = $null
    $status = 'collection-failed'
} finally {
    try {
        if ($started -and -not $process.HasExited) {
            $process.Kill($true)
            if (-not $process.WaitForExit(1000)) { throw 'Hexagon inventory worker termination is unverified.' }
        }
    } finally { $process.Dispose() }
}
$report = New-BonsaiHexagonReceipt -Inventory $inventory -CollectionStatus $status -MaximumSeconds $MaximumSeconds -ElapsedMilliseconds $timer.ElapsedMilliseconds
if ($OutputPath) {
    $file = [IO.File]::Open($OutputPath, 'CreateNew', 'Write', 'None')
    try {
        $bytes = [Text.UTF8Encoding]::new($false).GetBytes(($report | ConvertTo-Json -Depth 10) + [Environment]::NewLine)
        $file.Write($bytes)
    } finally { $file.Dispose() }
}
Write-Information -MessageData "Hexagon inventory: $status; execution and profile qualification pending." -InformationAction Continue
if ($PassThru) { Write-Output $report }
