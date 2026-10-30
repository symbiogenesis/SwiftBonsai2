#Requires -Version 7.6
<#
.SYNOPSIS
Reports Windows host and toolchain facts without installing or configuring anything.
.DESCRIPTION
Run from PowerShell 7.6 or later. No model is loaded and no benchmark runs.
DataPath is optional and must be an existing absolute directory outside Git.
OutputPath is optional, must be a new absolute file outside Git, and its parent
must already exist. The JSON contains exact tool paths; review it before sharing.
Version commands run in a unique system-temp directory with the inherited environment.
Presence, executable architecture, and version do not establish backend correctness.
ToolPath optionally selects an existing checked maintenance executable for native
CPU-set and bounded Vulkan observations. Omitting it keeps inventory independent
of a Rust build. Vulkan inventory never selects or qualifies an inference backend.
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
[OutputType([Collections.IDictionary])]
param(
    [string] $DataPath,
    [string] $OutputPath,
    [string] $ToolPath,
    [switch] $PassThru
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Native.psm1')
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Capabilities.psm1')
if (-not $IsWindows) { throw 'Doctor requires Windows and PowerShell 7.6 or later.' }

$issues = [Collections.Generic.List[string]]::new()
$probeResults = @{}
$repoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))

function Get-ExternalPath {
    [CmdletBinding()]
    [OutputType([string])]

    param([string] $Path, [switch] $Directory)

    if ([string]::IsNullOrWhiteSpace($Path) -or $Path -match '%[^%]+%' -or
        -not [IO.Path]::IsPathFullyQualified($Path) -or
        $Path.Replace('/', '\').StartsWith('\\?\') -or $Path.Replace('/', '\').StartsWith('\\.\')) {
        throw 'Use a fully qualified filesystem path with all environment variables expanded.'
    }
    $fullPath = [IO.Path]::GetFullPath($Path)
    # Disallow alternate data streams, including a drive-qualified filename with a second colon.
    if ($fullPath.Substring([IO.Path]::GetPathRoot($fullPath).Length).Contains(':')) {
        throw 'Alternate data stream paths are not supported.'
    }
    $parent = if ($Directory) { $fullPath } else { [IO.Path]::GetDirectoryName($fullPath) }
    if (-not [IO.Directory]::Exists($parent)) { throw 'The external directory must already exist.' }

    # Resolve every ancestor, so a junction cannot disguise an output path in a checkout.
    $physicalParent = [IO.Path]::GetPathRoot($parent)
    $relativeParts = $parent.Substring($physicalParent.Length).Split(
        [char[]] @([IO.Path]::DirectorySeparatorChar, [IO.Path]::AltDirectorySeparatorChar),
        [StringSplitOptions]::RemoveEmptyEntries)
    foreach ($part in $relativeParts) {
        $physicalParent = Join-Path $physicalParent $part
        $item = Get-Item -LiteralPath $physicalParent -Force
        if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
            $target = $item.ResolveLinkTarget($true)
            if ($null -eq $target) { throw 'The external path contains an unresolved link.' }
            $physicalParent = $target.FullName
        }
    }
    $ancestor = [IO.DirectoryInfo]::new($physicalParent)
    while ($null -ne $ancestor) {
        if ([IO.File]::Exists((Join-Path $ancestor.FullName '.git')) -or
            [IO.Directory]::Exists((Join-Path $ancestor.FullName '.git')) -or
            $ancestor.FullName.TrimEnd('\') -eq $repoRoot.TrimEnd('\')) {
            throw 'Data and report output must be outside a Git checkout.'
        }
        $ancestor = $ancestor.Parent
    }
    if ($Directory) { return $physicalParent }
    if ([IO.File]::Exists($fullPath) -or [IO.Directory]::Exists($fullPath)) {
        throw 'OutputPath must name a new file; existing files are never overwritten.'
    }
    return Join-Path $physicalParent ([IO.Path]::GetFileName($fullPath))
}

$dataDirectory = if ($PSBoundParameters.ContainsKey('DataPath')) { Get-ExternalPath $DataPath -Directory } else { $null }
$reportPath = if ($PSBoundParameters.ContainsKey('OutputPath')) { Get-ExternalPath $OutputPath } else { $null }
$scratchRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
if (-not [IO.Path]::IsPathFullyQualified($scratchRoot) -or $scratchRoot -match '%[^%]+%') {
    throw 'The system temporary directory is not an expanded absolute path.'
}
$scratchRoot = Get-ExternalPath $scratchRoot -Directory
$scratchDirectory = Join-Path $scratchRoot ('SwiftBonsai-Doctor-' + [guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($scratchDirectory)

function Invoke-VersionCommand {
    [CmdletBinding()]
    [OutputType([pscustomobject])]

    param([string] $Path, [string[]] $Arguments)

    $result = Invoke-BonsaiNativeProcess -Executable $Path -ArgumentList $Arguments -WorkingDirectory $scratchDirectory -TimeoutSeconds 10
    return [pscustomobject] @{ ExitCode = $result.ExitCode; Text = ($result.StandardOutput + "`n" + $result.StandardError).Trim() }
}

function Get-ExecutableArchitecture {
    [CmdletBinding()]
    [OutputType([string])]

    param([string] $Path)
    $stream = $null
    $reader = $null
    try {
        $stream = [IO.File]::Open($Path, 'Open', 'Read', 'ReadWrite')
        $reader = [IO.BinaryReader]::new($stream)
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
    [CmdletBinding()]
    [OutputType([string])]

    param([string] $Name, [string[]] $Fallbacks = @())
    $command = Get-Command $Name -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($null -ne $command) { return $command.Source }
    foreach ($candidate in $Fallbacks) {
        if ([IO.File]::Exists($candidate)) { return $candidate }
    }
    return $null
}

function Get-Tool {
    [CmdletBinding()]
    [OutputType([pscustomobject])]

    param([string] $Name, [string] $Path, [string[]] $Arguments = @('--version'), [switch] $FileVersion)
    if ([string]::IsNullOrEmpty($Path)) {
        return [pscustomobject] @{ Name = $Name; Present = $false; Path = $null; Architecture = $null; Version = $null; VersionStatus = 'not-found' }
    }
    $version = $null
    $status = 'unavailable'
    try {
        if ($FileVersion) {
            $version = [Diagnostics.FileVersionInfo]::GetVersionInfo($Path).FileVersion
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

function Get-CimFact {
    [CmdletBinding()]
    [OutputType([Microsoft.Management.Infrastructure.CimInstance])]

    param([string] $ClassName, [string[]] $Properties, [string] $Filter)
    try {
        $query = @{ ClassName = $ClassName; Property = $Properties; OperationTimeoutSec = 10; ErrorAction = 'Stop' }
        if ($Filter) { $query.Filter = $Filter }
        $facts = @(Get-CimInstance @query)
        $probeResults[$ClassName] = $true
        foreach ($fact in $facts) { [Microsoft.Management.Infrastructure.CimInstance]$fact }
    }
    catch {
        $probeResults[$ClassName] = $false
        $issues.Add("CIM $ClassName unavailable; hardware facts remain unverified.")
    }
}

try {
    # Query only required properties, avoiding user, hostname, serial, and network inventories.
    $os = Get-CimFact 'Win32_OperatingSystem' @('Caption', 'Version', 'BuildNumber', 'TotalVisibleMemorySize')
    $computer = Get-CimFact 'Win32_ComputerSystem' @('Manufacturer', 'Model', 'TotalPhysicalMemory')
    $cpus = @(Get-CimFact 'Win32_Processor' @('Name', 'Manufacturer', 'NumberOfCores', 'NumberOfLogicalProcessors'))
    $gpus = @(Get-CimFact 'Win32_VideoController' @('Name', 'AdapterCompatibility', 'DriverVersion', 'DriverDate', 'Status'))
    $memoryStatus = Get-CimFact 'Win32_PerfFormattedData_PerfOS_Memory' @('AvailableBytes', 'CommitLimit', 'CommittedBytes')
    $npuDrivers = @(Get-CimFact 'Win32_PnPSignedDriver' @('DeviceName', 'Manufacturer', 'DriverVersion', 'DriverDate', 'IsSigned') -Filter "DeviceClass = 'ComputeAccelerator'")
    $isa = $null
    $isaProbeSucceeded = $false
    try {
        $isa = @{ AdvSimd = [Runtime.Intrinsics.Arm.AdvSimd]::IsSupported; DotProd = [Runtime.Intrinsics.Arm.Dp]::IsSupported }
        $isaProbeSucceeded = $true
    }
    catch { $issues.Add('Process-scoped ARM ISA probes unavailable; instruction support remains unknown.') }

    $vsRoots = [Collections.Generic.List[string]]::new()
    $programDirectories = @(${env:ProgramFiles}, ${env:ProgramFiles(x86)}, ${env:ProgramW6432}) |
        Where-Object { $_ -and [IO.Path]::IsPathFullyQualified($_) } | Select-Object -Unique
    $vswhereCandidates = @($programDirectories | ForEach-Object { Join-Path $_ 'Microsoft Visual Studio\Installer\vswhere.exe' })
    $vswhere = Find-Application 'vswhere.exe' $vswhereCandidates
    if ($vswhere) {
        try {
            $locations = Invoke-VersionCommand $vswhere @('-all', '-products', '*', '-prerelease', '-property', 'installationPath')
            if ($locations.ExitCode -eq 0) {
                foreach ($location in ($locations.Text -split '\r?\n')) {
                    if ($location -and [IO.Path]::IsPathFullyQualified($location) -and [IO.Directory]::Exists($location)) {
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
        if ([IO.Directory]::Exists($vsDirectory)) {
            foreach ($year in (Get-ChildItem -LiteralPath $vsDirectory -Directory -ErrorAction SilentlyContinue)) {
                foreach ($edition in (Get-ChildItem -LiteralPath $year.FullName -Directory -ErrorAction SilentlyContinue)) {
                    if ([IO.Directory]::Exists((Join-Path $edition.FullName 'Common7'))) { $vsRoots.Add($edition.FullName) }
                }
            }
        }
    }
    $gitFallbacks = @($programDirectories | ForEach-Object { Join-Path $_ 'Git\cmd\git.exe' })
    $cmakeFallbacks = @($programDirectories | ForEach-Object { Join-Path $_ 'CMake\bin\cmake.exe' })
    $compilerFallbacks = [Collections.Generic.List[string]]::new()
    foreach ($vsRoot in ($vsRoots | Select-Object -Unique)) {
        $gitFallbacks += Join-Path $vsRoot 'Common7\IDE\CommonExtensions\Microsoft\TeamFoundation\Team Explorer\Git\cmd\git.exe'
        $gitFallbacks += Join-Path $vsRoot 'Common7\IDE\CommonExtensions\Microsoft\TeamFoundation\Team Explorer\Git\mingw64\bin\git.exe'
        $cmakeFallbacks += Join-Path $vsRoot 'Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe'
        $msvcRoot = Join-Path $vsRoot 'VC\Tools\MSVC'
        if ([IO.Directory]::Exists($msvcRoot)) {
            foreach ($version in (Get-ChildItem -LiteralPath $msvcRoot -Directory | Sort-Object Name -Descending)) {
                # Native ARM64 host/target first; other compilers are reported honestly if found.
                foreach ($compilerLane in @('Hostarm64\arm64', 'Hostx64\arm64', 'Hostx64\x64', 'Hostx86\x86')) {
                    $compilerFallbacks.Add((Join-Path $version.FullName "bin\$compilerLane\cl.exe"))
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
    $osArchitecture = [Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
    $processArchitecture = [Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture.ToString()
    if ($osArchitecture -ne 'Arm64') { $issues.Add('The operating system is not ARM64; target-device qualification is unavailable on this host.') }
    if ($osArchitecture -ne $processArchitecture) { $issues.Add('PowerShell runs under architecture emulation; use native ARM64 PowerShell for target-device checks.') }

    $storage = [ordered] @{ Selected = ($null -ne $dataDirectory); Path = $dataDirectory; TotalBytes = $null; AvailableBytes = $null }
    if ($dataDirectory) {
        try {
            $drive = [IO.DriveInfo]::new([IO.Path]::GetPathRoot($dataDirectory))
            $storage.TotalBytes = $drive.TotalSize
            $storage.AvailableBytes = $drive.AvailableFreeSpace
        }
        catch { $issues.Add('Data directory validated, but volume capacity could not be read.') }
    }
    else { $issues.Add('No external data directory selected; storage capacity remains unverified.') }

    $capabilities = Get-BonsaiHostCapability -Facts @{
        OsArchitecture = $osArchitecture; ProcessArchitecture = $processArchitecture
        OperatingSystem = $os; Computer = $computer; Processors = $cpus; Graphics = $gpus
        Memory = $memoryStatus; NpuDrivers = $npuDrivers; Isa = $isa
        Probes = @{
            OperatingSystem = [bool]$probeResults['Win32_OperatingSystem']
            Computer = [bool]$probeResults['Win32_ComputerSystem']
            Processors = [bool]$probeResults['Win32_Processor']
            Graphics = [bool]$probeResults['Win32_VideoController']
            Memory = [bool]$probeResults['Win32_PerfFormattedData_PerfOS_Memory']
            NpuDrivers = [bool]$probeResults['Win32_PnPSignedDriver']
            Isa = $isaProbeSucceeded
        }
    }
    $nativeCpu = $null
    $nativeGpu = $null
    if ($ToolPath) {
        Import-Module (Join-Path $PSScriptRoot 'Bonsai.Tools.psm1')
        $nativeCpu = Invoke-BonsaiTool -Command host-cpu -ToolPath $ToolPath -Request @{}
        if ($nativeCpu.schema_version -ne 1 -or $nativeCpu.kind -cne 'runner-native-cpu-observations') {
            throw 'The selected maintenance tool returned an unsupported native CPU observation.'
        }
        if ($nativeCpu.status -cne 'observed') { $issues.Add('Native CPU-set observations unavailable; topology remains unknown.') }
        $nativeGpu = Invoke-BonsaiTool -Command host-gpu -ToolPath $ToolPath -Request @{}
        if ($nativeGpu.schema_version -ne 1 -or $nativeGpu.kind -cne 'runner-native-gpu-observations') {
            throw 'The selected maintenance tool returned an unsupported native GPU observation.'
        }
        if ($nativeGpu.status -cne 'observed') { $issues.Add('Native Vulkan observations unavailable; API capabilities remain unknown.') }
    }
    $report = [ordered] @{
        SchemaVersion = 1
        CollectedAtUtc = [DateTime]::UtcNow.ToString('o')
        OperatingSystem = [ordered] @{
            Description = [Runtime.InteropServices.RuntimeInformation]::OSDescription
            Caption = ($os)?.Caption
            Version = ($os)?.Version
            BuildNumber = ($os)?.BuildNumber
            Architecture = $osArchitecture
        }
        Runtime = [ordered] @{
            PowerShellVersion = $PSVersionTable.PSVersion.ToString()
            DotNet = [Runtime.InteropServices.RuntimeInformation]::FrameworkDescription
            ProcessArchitecture = $processArchitecture
            IsNativeProcess = ($processArchitecture -eq $osArchitecture)
        }
        Computer = if ($null -ne $computer) { [ordered] @{ Manufacturer = $computer.Manufacturer; Model = $computer.Model } } else { $null }
        Memory = [ordered] @{
            # WMI calls this KiB "kilobytes"; retain bytes to avoid GB/GiB admission mistakes.
            UsablePhysicalBytes = if ($null -ne ($os)?.TotalVisibleMemorySize) { [uint64] $os.TotalVisibleMemorySize * [uint64] 1024 } else { $null }
            ReportedPhysicalBytes = ($computer)?.TotalPhysicalMemory
            UsableSource = 'Win32_OperatingSystem.TotalVisibleMemorySize * 1024'
        }
        Processors = @($cpus | ForEach-Object { [ordered] @{ Name = $_.Name; Manufacturer = $_.Manufacturer; Cores = $_.NumberOfCores; LogicalProcessors = $_.NumberOfLogicalProcessors } })
        Graphics = @($gpus | ForEach-Object { [ordered] @{ Name = $_.Name; Vendor = $_.AdapterCompatibility; DriverVersion = $_.DriverVersion; DriverDate = if ($_.DriverDate) { $_.DriverDate.ToUniversalTime().ToString('o') } else { $null }; Status = $_.Status } })
        Tools = $tools
        Storage = $storage
        Capabilities = $capabilities
        NativeCpuObservations = $nativeCpu
        NativeGpuObservations = $nativeGpu
        CpuCompatibilityIdentity = Get-BonsaiCpuCompatibilityIdentity -Capabilities $capabilities
        Gaps = @($issues)
        Qualification = 'Inventory only: no engine, GPU backend, memory admission, or daily security policy has been qualified.'
    }
    if ($reportPath) {
        $json = $report | ConvertTo-Json -Depth 8
        $file = [IO.File]::Open($reportPath, 'CreateNew', 'Write', 'None')
        try {
            $writer = [IO.StreamWriter]::new($file, [Text.UTF8Encoding]::new($false))
            try { $writer.WriteLine($json) } finally { $writer.Dispose() }
        }
        finally { $file.Dispose() }
    }

    @(
        "Windows architecture: $osArchitecture; PowerShell process: $processArchitecture ($($PSVersionTable.PSVersion))"
        if ($null -ne $report.Memory.UsablePhysicalBytes) {
            "OS-usable physical memory: $($report.Memory.UsablePhysicalBytes) bytes ($([Math]::Round($report.Memory.UsablePhysicalBytes / 1GB, 2)) GiB)"
        }
        else { 'OS-usable physical memory: unavailable' }
        foreach ($cpu in $report.Processors) { "CPU: $($cpu.Name.Trim()) ($($cpu.Cores) cores, $($cpu.LogicalProcessors) logical)" }
        foreach ($gpu in $report.Graphics) { "GPU: $($gpu.Name); driver $($gpu.DriverVersion)" }
        if ($null -ne $nativeCpu -and $nativeCpu.status -ceq 'observed') {
            "Native CPU sets: $($nativeCpu.topology.logical_processors) logical; $($nativeCpu.topology.physical_cores) cores; efficiency classes $($nativeCpu.topology.efficiency_classes -join ', ')"
        }
        foreach ($tool in $tools) {
            $summary = if ($tool.Present) { "$($tool.Architecture); $($tool.Version ?? $tool.VersionStatus)" } else { 'not found' }
            "$($tool.Name): $summary"
        }
        if ($dataDirectory) { "External data directory: validated; available bytes: $($storage.AvailableBytes ?? 'unavailable')" }
        foreach ($issue in $issues) { "Gap: $issue" }
        $report.Qualification
        if ($reportPath) { 'JSON report written to the requested external file; it includes exact tool paths.' }
    ) | Write-Information -InformationAction Continue
    if ($PassThru) { Write-Output $report }
}
finally {
    # Only remove the unique directory allocated by this invocation, and only when empty.
    if ([IO.Directory]::Exists($scratchDirectory) -and
        -not [IO.Directory]::EnumerateFileSystemEntries($scratchDirectory).GetEnumerator().MoveNext()) {
        [IO.Directory]::Delete($scratchDirectory, $false)
    }
}
