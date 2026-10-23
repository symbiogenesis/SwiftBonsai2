#Requires -Version 7.4
<#
.SYNOPSIS
Reports Windows host and toolchain facts without installing or configuring anything.
.DESCRIPTION
Run from PowerShell 7.4 or later. No model is loaded and no benchmark runs.
DataPath is optional and must be an existing absolute directory outside Git.
OutputPath is optional, must be a new absolute file outside Git, and its parent
must already exist. The JSON contains exact tool paths; review it before sharing.
Version commands run in a unique system-temp directory with the inherited environment.
Presence, executable architecture, and version do not establish backend correctness.
.EXAMPLE
./scripts/Doctor.ps1
.EXAMPLE
./scripts/Doctor.ps1 -DataPath D:\BonsaiData -OutputPath D:\Reports\doctor.json
.LINK
https://learn.microsoft.com/en-us/dotnet/api/system.runtime.interopservices.runtimeinformation.osarchitecture
.LINK
https://learn.microsoft.com/en-us/windows/win32/cimwin32prov/win32-operatingsystem
.LINK
https://learn.microsoft.com/en-us/windows/win32/cimwin32prov/win32-computersystem
.LINK
https://learn.microsoft.com/en-us/windows/win32/cimwin32prov/win32-videocontroller
#>
[CmdletBinding()]
param(
    [string] $DataPath,
    [string] $OutputPath,
    [switch] $PassThru
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if (-not $IsWindows) { throw 'Doctor requires Windows and PowerShell 7.4 or later.' }

$issues = [System.Collections.Generic.List[string]]::new()
$repoRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))

function Get-ExternalPath {
    param([string] $Path, [switch] $Directory)

    if ([string]::IsNullOrWhiteSpace($Path) -or $Path -match '%[^%]+%' -or
        -not [System.IO.Path]::IsPathFullyQualified($Path) -or
        $Path.StartsWith('\\?\') -or $Path.StartsWith('\\.\')) {
        throw 'Use a fully qualified filesystem path with all environment variables expanded.'
    }
    $fullPath = [System.IO.Path]::GetFullPath($Path)
    # Disallow alternate data streams, including a drive-qualified filename with a second colon.
    if ($fullPath.Substring([System.IO.Path]::GetPathRoot($fullPath).Length).Contains(':')) {
        throw 'Alternate data stream paths are not supported.'
    }
    $parent = if ($Directory) { $fullPath } else { [System.IO.Path]::GetDirectoryName($fullPath) }
    if (-not [System.IO.Directory]::Exists($parent)) { throw 'The external directory must already exist.' }

    # Resolve every ancestor, so a junction cannot disguise an output path in a checkout.
    $physicalParent = [System.IO.Path]::GetPathRoot($parent)
    $relativeParts = $parent.Substring($physicalParent.Length).Split(
        [char[]] @([System.IO.Path]::DirectorySeparatorChar, [System.IO.Path]::AltDirectorySeparatorChar),
        [System.StringSplitOptions]::RemoveEmptyEntries)
    foreach ($part in $relativeParts) {
        $physicalParent = Join-Path $physicalParent $part
        $item = Get-Item -LiteralPath $physicalParent -Force
        if (($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
            $target = $item.ResolveLinkTarget($true)
            if ($null -eq $target) { throw 'The external path contains an unresolved link.' }
            $physicalParent = $target.FullName
        }
    }
    $ancestor = [System.IO.DirectoryInfo]::new($physicalParent)
    while ($null -ne $ancestor) {
        if ([System.IO.File]::Exists((Join-Path $ancestor.FullName '.git')) -or
            [System.IO.Directory]::Exists((Join-Path $ancestor.FullName '.git')) -or
            $ancestor.FullName.TrimEnd('\') -eq $repoRoot.TrimEnd('\')) {
            throw 'Data and report output must be outside a Git checkout.'
        }
        $ancestor = $ancestor.Parent
    }
    if ($Directory) { return $physicalParent }
    if ([System.IO.File]::Exists($fullPath) -or [System.IO.Directory]::Exists($fullPath)) {
        throw 'OutputPath must name a new file; existing files are never overwritten.'
    }
    return Join-Path $physicalParent ([System.IO.Path]::GetFileName($fullPath))
}

$dataDirectory = if ($PSBoundParameters.ContainsKey('DataPath')) { Get-ExternalPath $DataPath -Directory } else { $null }
$reportPath = if ($PSBoundParameters.ContainsKey('OutputPath')) { Get-ExternalPath $OutputPath } else { $null }
$scratchRoot = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
if (-not [System.IO.Path]::IsPathFullyQualified($scratchRoot) -or $scratchRoot -match '%[^%]+%') {
    throw 'The system temporary directory is not an expanded absolute path.'
}
$scratchRoot = Get-ExternalPath $scratchRoot -Directory
$scratchDirectory = Join-Path $scratchRoot ('SwiftBonsai-Doctor-' + [guid]::NewGuid().ToString('N'))
[System.IO.Directory]::CreateDirectory($scratchDirectory) | Out-Null

function Invoke-VersionCommand {
    param([string] $Path, [string[]] $Arguments)

    $start = [System.Diagnostics.ProcessStartInfo]::new()
    $start.FileName = $Path
    $start.WorkingDirectory = $scratchDirectory
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    foreach ($argument in $Arguments) { $start.ArgumentList.Add($argument) }
    $process = [System.Diagnostics.Process]::new()
    $process.StartInfo = $start
    try {
        if (-not $process.Start()) { throw 'Version process could not start.' }
        $stdout = $process.StandardOutput.ReadToEndAsync()
        $stderr = $process.StandardError.ReadToEndAsync()
        if (-not $process.WaitForExit(10000)) {
            $process.Kill($true)
            $process.WaitForExit()
            throw 'Version process exceeded ten seconds.'
        }
        $output = $stdout.GetAwaiter().GetResult() + "`n" + $stderr.GetAwaiter().GetResult()
        return [pscustomobject] @{ ExitCode = $process.ExitCode; Text = $output.Trim() }
    }
    finally { $process.Dispose() }
}

function Get-ExecutableArchitecture {
    param([string] $Path)
    $stream = $null
    $reader = $null
    try {
        $stream = [System.IO.File]::Open($Path, 'Open', 'Read', 'ReadWrite')
        $reader = [System.IO.BinaryReader]::new($stream)
        if ($stream.Length -lt 64 -or $reader.ReadUInt16() -ne 0x5A4D) { return 'unknown' }
        $stream.Position = 0x3C
        $peOffset = $reader.ReadInt32()
        if ($peOffset -lt 0 -or $peOffset + 6 -gt $stream.Length) { return 'unknown' }
        $stream.Position = $peOffset
        if ($reader.ReadUInt32() -ne 0x4550) { return 'unknown' }
        switch ($reader.ReadUInt16()) {
            0xAA64 { return 'Arm64' }
            0xA641 { return 'Arm64EC' }
            0xA64E { return 'Arm64X' }
            0x8664 { return 'X64' }
            0x014C { return 'X86' }
            default { return 'unknown' }
        }
    }
    catch { return 'unknown' }
    finally {
        if ($null -ne $reader) { $reader.Dispose() }
        elseif ($null -ne $stream) { $stream.Dispose() }
    }
}

function Find-Application {
    param([string] $Name, [string[]] $Fallbacks = @())
    $command = Get-Command $Name -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($null -ne $command) { return $command.Source }
    foreach ($candidate in $Fallbacks) {
        if ([System.IO.File]::Exists($candidate)) { return $candidate }
    }
    return $null
}

function Get-Tool {
    param([string] $Name, [string] $Path, [string[]] $Arguments = @('--version'), [switch] $FileVersion)
    if ([string]::IsNullOrEmpty($Path)) {
        return [pscustomobject] @{ Name = $Name; Present = $false; Path = $null; Architecture = $null; Version = $null; VersionStatus = 'not-found' }
    }
    $version = $null
    $status = 'unavailable'
    try {
        if ($FileVersion) {
            $version = [System.Diagnostics.FileVersionInfo]::GetVersionInfo($Path).FileVersion
            $status = if ($version) { 'file-metadata' } else { 'unavailable' }
        }
        else {
            $result = Invoke-VersionCommand $Path $Arguments
            if ($result.ExitCode -eq 0 -and $result.Text) {
                $version = ($result.Text -split '\r?\n')[0]
                $status = 'reported'
            }
            else { $status = 'command-failed' }
        }
    }
    catch { $status = 'command-failed' }
    return [pscustomobject] @{
        Name = $Name; Present = $true; Path = $Path
        Architecture = Get-ExecutableArchitecture $Path; Version = $version; VersionStatus = $status
    }
}

function Get-CimFacts {
    param([string] $ClassName, [string[]] $Properties)
    try { Get-CimInstance -ClassName $ClassName -Property $Properties -ErrorAction Stop }
    catch { $issues.Add("CIM $ClassName unavailable; hardware facts remain unverified.") }
}

try {
    # Query only required properties, avoiding user, hostname, serial, and network inventories.
    $os = Get-CimFacts 'Win32_OperatingSystem' @('Caption', 'Version', 'BuildNumber', 'TotalVisibleMemorySize')
    $computer = Get-CimFacts 'Win32_ComputerSystem' @('Manufacturer', 'Model', 'TotalPhysicalMemory')
    $cpus = @(Get-CimFacts 'Win32_Processor' @('Name', 'Manufacturer', 'NumberOfCores', 'NumberOfLogicalProcessors'))
    $gpus = @(Get-CimFacts 'Win32_VideoController' @('Name', 'AdapterCompatibility', 'DriverVersion', 'DriverDate', 'Status'))

    $vsRoots = [System.Collections.Generic.List[string]]::new()
    $programDirectories = @(${env:ProgramFiles}, ${env:ProgramFiles(x86)}, ${env:ProgramW6432}) |
        Where-Object { $_ -and [System.IO.Path]::IsPathFullyQualified($_) } | Select-Object -Unique
    $vswhereCandidates = @($programDirectories | ForEach-Object { Join-Path $_ 'Microsoft Visual Studio\Installer\vswhere.exe' })
    $vswhere = Find-Application 'vswhere.exe' $vswhereCandidates
    if ($vswhere) {
        try {
            $locations = Invoke-VersionCommand $vswhere @('-all', '-products', '*', '-prerelease', '-property', 'installationPath')
            if ($locations.ExitCode -eq 0) {
                foreach ($location in ($locations.Text -split '\r?\n')) {
                    if ($location -and [System.IO.Path]::IsPathFullyQualified($location) -and [System.IO.Directory]::Exists($location)) {
                        $vsRoots.Add($location)
                    }
                }
            }
        }
        catch { $issues.Add('Visual Studio discovery failed; only PATH and standard installation locations were checked.') }
    }
    # Fallback for hosts where the installer utility is absent or blocked; no recursive disk search.
    foreach ($programDirectory in $programDirectories) {
        $vsDirectory = Join-Path $programDirectory 'Microsoft Visual Studio'
        if ([System.IO.Directory]::Exists($vsDirectory)) {
            foreach ($year in (Get-ChildItem -LiteralPath $vsDirectory -Directory -ErrorAction SilentlyContinue)) {
                foreach ($edition in (Get-ChildItem -LiteralPath $year.FullName -Directory -ErrorAction SilentlyContinue)) {
                    if ([System.IO.Directory]::Exists((Join-Path $edition.FullName 'Common7'))) { $vsRoots.Add($edition.FullName) }
                }
            }
        }
    }
    $gitFallbacks = @($programDirectories | ForEach-Object { Join-Path $_ 'Git\cmd\git.exe' })
    $cmakeFallbacks = @($programDirectories | ForEach-Object { Join-Path $_ 'CMake\bin\cmake.exe' })
    $compilerFallbacks = @()
    foreach ($vsRoot in ($vsRoots | Select-Object -Unique)) {
        $gitFallbacks += Join-Path $vsRoot 'Common7\IDE\CommonExtensions\Microsoft\TeamFoundation\Team Explorer\Git\cmd\git.exe'
        $gitFallbacks += Join-Path $vsRoot 'Common7\IDE\CommonExtensions\Microsoft\TeamFoundation\Team Explorer\Git\mingw64\bin\git.exe'
        $cmakeFallbacks += Join-Path $vsRoot 'Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe'
        $msvcRoot = Join-Path $vsRoot 'VC\Tools\MSVC'
        if ([System.IO.Directory]::Exists($msvcRoot)) {
            foreach ($version in (Get-ChildItem -LiteralPath $msvcRoot -Directory | Sort-Object Name -Descending)) {
                # Native ARM64 host/target first; other compilers are reported honestly if found.
                foreach ($compilerLane in @('Hostarm64\arm64', 'Hostx64\arm64', 'Hostx64\x64', 'Hostx86\x86')) {
                    $compilerFallbacks += Join-Path $version.FullName "bin\$compilerLane\cl.exe"
                }
            }
        }
    }
    $tools = @(
        Get-Tool 'PowerShell' (Join-Path $PSHOME 'pwsh.exe') -FileVersion
        Get-Tool 'Git' (Find-Application 'git.exe' $gitFallbacks)
        Get-Tool 'CMake' (Find-Application 'cmake.exe' $cmakeFallbacks)
        Get-Tool 'MSVC' (Find-Application 'cl.exe' $compilerFallbacks) -FileVersion
        Get-Tool 'Clang' (Find-Application 'clang.exe')
        Get-Tool 'Rust' (Find-Application 'rustc.exe')
        Get-Tool 'Cargo' (Find-Application 'cargo.exe')
    )
    foreach ($tool in $tools) {
        if (-not $tool.Present) { $issues.Add("$($tool.Name) not found; no dependency was installed.") }
        elseif ($tool.VersionStatus -in @('unavailable', 'command-failed')) { $issues.Add("$($tool.Name) found but its version could not be read.") }
    }
    $osArchitecture = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
    $processArchitecture = [System.Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture.ToString()
    if ($osArchitecture -ne 'Arm64') { $issues.Add('The operating system is not ARM64; target-device qualification is unavailable on this host.') }
    if ($osArchitecture -ne $processArchitecture) { $issues.Add('PowerShell runs under architecture emulation; use native ARM64 PowerShell for target-device checks.') }

    $storage = [ordered] @{ Selected = ($null -ne $dataDirectory); Path = $dataDirectory; TotalBytes = $null; AvailableBytes = $null }
    if ($dataDirectory) {
        try {
            $drive = [System.IO.DriveInfo]::new([System.IO.Path]::GetPathRoot($dataDirectory))
            $storage.TotalBytes = $drive.TotalSize
            $storage.AvailableBytes = $drive.AvailableFreeSpace
        }
        catch { $issues.Add('Data directory validated, but volume capacity could not be read.') }
    }
    else { $issues.Add('No external data directory selected; storage capacity remains unverified.') }

    $report = [ordered] @{
        SchemaVersion = 1
        CollectedAtUtc = [DateTime]::UtcNow.ToString('o')
        OperatingSystem = [ordered] @{
            Description = [System.Runtime.InteropServices.RuntimeInformation]::OSDescription
            Caption = if ($null -ne $os) { $os.Caption } else { $null }
            Version = if ($null -ne $os) { $os.Version } else { $null }
            BuildNumber = if ($null -ne $os) { $os.BuildNumber } else { $null }
            Architecture = $osArchitecture
        }
        Runtime = [ordered] @{
            PowerShellVersion = $PSVersionTable.PSVersion.ToString()
            DotNet = [System.Runtime.InteropServices.RuntimeInformation]::FrameworkDescription
            ProcessArchitecture = $processArchitecture
            IsNativeProcess = ($processArchitecture -eq $osArchitecture)
        }
        Computer = if ($null -ne $computer) { [ordered] @{ Manufacturer = $computer.Manufacturer; Model = $computer.Model } } else { $null }
        Memory = [ordered] @{
            # WMI calls this KiB "kilobytes"; retain bytes to avoid GB/GiB admission mistakes.
            UsablePhysicalBytes = if ($null -ne $os -and $null -ne $os.TotalVisibleMemorySize) { [uint64] $os.TotalVisibleMemorySize * [uint64] 1024 } else { $null }
            ReportedPhysicalBytes = if ($null -ne $computer) { $computer.TotalPhysicalMemory } else { $null }
            UsableSource = 'Win32_OperatingSystem.TotalVisibleMemorySize * 1024'
        }
        Processors = @($cpus | ForEach-Object { [ordered] @{ Name = $_.Name; Manufacturer = $_.Manufacturer; Cores = $_.NumberOfCores; LogicalProcessors = $_.NumberOfLogicalProcessors } })
        Graphics = @($gpus | ForEach-Object { [ordered] @{ Name = $_.Name; Vendor = $_.AdapterCompatibility; DriverVersion = $_.DriverVersion; DriverDate = if ($_.DriverDate) { $_.DriverDate.ToUniversalTime().ToString('o') } else { $null }; Status = $_.Status } })
        Tools = $tools
        Storage = $storage
        Gaps = @($issues)
        Qualification = 'Inventory only: no engine, GPU backend, memory admission, or daily security policy has been qualified.'
    }
    if ($reportPath) {
        $json = $report | ConvertTo-Json -Depth 8
        $file = [System.IO.File]::Open($reportPath, 'CreateNew', 'Write', 'None')
        try {
            $writer = [System.IO.StreamWriter]::new($file, [System.Text.UTF8Encoding]::new($false))
            try { $writer.WriteLine($json) } finally { $writer.Dispose() }
        }
        finally { $file.Dispose() }
    }

    Write-Host "Windows architecture: $osArchitecture; PowerShell process: $processArchitecture ($($PSVersionTable.PSVersion))"
    if ($null -ne $report.Memory.UsablePhysicalBytes) {
        Write-Host "OS-usable physical memory: $($report.Memory.UsablePhysicalBytes) bytes ($([Math]::Round($report.Memory.UsablePhysicalBytes / 1GB, 2)) GiB)"
    }
    else { Write-Host 'OS-usable physical memory: unavailable' }
    foreach ($cpu in $report.Processors) { Write-Host "CPU: $($cpu.Name.Trim()) ($($cpu.Cores) cores, $($cpu.LogicalProcessors) logical)" }
    foreach ($gpu in $report.Graphics) { Write-Host "GPU: $($gpu.Name); driver $($gpu.DriverVersion)" }
    foreach ($tool in $tools) {
        $summary = if ($tool.Present) { "$($tool.Architecture); $($tool.Version ?? $tool.VersionStatus)" } else { 'not found' }
        Write-Host "$($tool.Name): $summary"
    }
    if ($dataDirectory) { Write-Host "External data directory: validated; available bytes: $($storage.AvailableBytes ?? 'unavailable')" }
    foreach ($issue in $issues) { Write-Host "Gap: $issue" }
    Write-Host $report.Qualification
    if ($reportPath) { Write-Host 'JSON report written to the requested external file; it includes exact tool paths.' }
    if ($PassThru) { Write-Output $report }
}
finally {
    # Only remove the unique directory allocated by this invocation, and only when empty.
    if ([System.IO.Directory]::Exists($scratchDirectory) -and
        -not [System.IO.Directory]::EnumerateFileSystemEntries($scratchDirectory).GetEnumerator().MoveNext()) {
        [System.IO.Directory]::Delete($scratchDirectory, $false)
    }
}
