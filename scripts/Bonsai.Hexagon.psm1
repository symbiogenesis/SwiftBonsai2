#requires -Version 7.6
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true

function Get-BonsaiHexagonLocalPath {
    <# .SYNOPSIS
    Accepts only expanded local paths without reparse points or alternate streams.
    #>
    [CmdletBinding()]
    [OutputType([string])]
    param([Parameter(Mandatory)][string]$Path, [switch]$NewFile)
    if (-not [IO.Path]::IsPathFullyQualified($Path) -or $Path -match '%[^%]+%' -or
        $Path.Replace('/', '\').StartsWith('\\')) { throw 'Use an expanded absolute local filesystem path.' }
    $full = [IO.Path]::GetFullPath($Path)
    if ($full.Substring([IO.Path]::GetPathRoot($full).Length).Contains(':')) { throw 'Alternate data stream paths are unsupported.' }
    $parent = if ($NewFile) { [IO.Path]::GetDirectoryName($full) } else { $full }
    $ancestor = [IO.DirectoryInfo]::new($parent)
    while ($null -ne $ancestor) {
        if (-not $ancestor.Exists -or ($ancestor.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
            throw 'Every selected directory must exist and must not be a reparse point.'
        }
        if ($NewFile -and ([IO.File]::Exists((Join-Path $ancestor.FullName '.git')) -or
            [IO.Directory]::Exists((Join-Path $ancestor.FullName '.git')))) { throw 'Receipt output must be outside Git.' }
        $ancestor = $ancestor.Parent
    }
    if ($NewFile -and (Test-Path -LiteralPath $full)) { throw 'Receipt output must be a new file.' }
    return $full
}

function Get-BonsaiHexagonPackage {
    <# .SYNOPSIS
    Hashes bounded top-level package files without loading code or establishing catalog membership.
    #>
    [CmdletBinding()]
    [OutputType([Collections.Specialized.OrderedDictionary])]
    param([string]$PackagePath)
    if (-not $PackagePath) { return [ordered]@{ Status = 'not-selected'; Files = @(); TotalBytes = 0 } }
    $root = Get-BonsaiHexagonLocalPath -Path $PackagePath
    $files = [Collections.Generic.List[object]]::new()
    $entries = 0
    [long]$total = 0
    foreach ($path in [IO.Directory]::EnumerateFileSystemEntries($root)) {
        $entries++
        if ($entries -gt 128) { throw 'Package inventory exceeds 128 top-level entries; select the runtime directory.' }
        $attributes = [IO.File]::GetAttributes($path)
        if ($attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Package inventory contains a reparse point.' }
        if ($attributes -band [IO.FileAttributes]::Directory) { continue }
        $name = [IO.Path]::GetFileName($path)
        $extension = [IO.Path]::GetExtension($path).ToLowerInvariant()
        if ($extension -notin @('.dll', '.exe', '.so', '.cat') -and $name -notmatch '^(LICENSE|NOTICE)(\.[A-Za-z0-9_-]+)?$') { continue }
        if ($files.Count -ge 64) { throw 'Package inventory exceeds 64 selected files.' }
        $stream = [IO.File]::Open($path, 'Open', 'Read', 'Read')
        try {
            $length = $stream.Length
            if ($length -gt 128MB -or $total + $length -gt 256MB) { throw 'Package hashing exceeds 128 MiB per file or 256 MiB aggregate.' }
            $digest = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($stream)).ToLowerInvariant()
            if ($stream.Length -ne $length) { throw 'Package file changed during hashing.' }
            $total += $length
            $signature = $null
            if ($extension -in @('.dll', '.exe', '.cat')) {
                $observation = Get-AuthenticodeSignature -LiteralPath $path
                $signature = [ordered]@{
                    Status = $observation.Status.ToString()
                    SignerThumbprint = if ($null -ne $observation.SignerCertificate) { $observation.SignerCertificate.Thumbprint } else { $null }
                    Scope = if ($extension -eq '.cat') { 'catalog-signature-only; member binding unverified' } else { 'file-signature-observation-only' }
                }
            }
            $files.Add([ordered]@{ Name = $name; Bytes = $length; Sha256 = $digest; Signature = $signature })
        } finally { $stream.Dispose() }
    }
    return [ordered]@{ Status = 'inventoried'; Files = @($files); TotalBytes = $total }
}

function Get-BonsaiHexagonInventory {
    <# .SYNOPSIS
    Collects inventory inside the preflight's disposable, deadline-bound PowerShell process.
    #>
    [CmdletBinding()]
    [OutputType([Collections.Specialized.OrderedDictionary])]
    param([string]$PackagePath)
    $processors = @(Get-CimInstance -ClassName Win32_Processor -OperationTimeoutSec 5 -Property Name,Manufacturer,Architecture | Select-Object -First 4 Name,Manufacturer,Architecture)
    $drivers = @(Get-CimInstance -ClassName Win32_PnPSignedDriver -Filter "DeviceClass = 'ComputeAccelerator'" -OperationTimeoutSec 5 -Property DeviceName,Manufacturer,DriverVersion,DriverDate,IsSigned,InfName,DeviceID |
        Select-Object -First 16 DeviceName,Manufacturer,DriverVersion,@{ Name = 'DriverDate'; Expression = { if ($null -ne $_.DriverDate) { $_.DriverDate.ToUniversalTime().ToString('o') } else { $null } } },IsSigned,InfName,DeviceID)
    return [ordered]@{
        OsArchitecture = [Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
        ProcessArchitecture = [Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture.ToString()
        Processors = $processors
        Drivers = $drivers
        Package = Get-BonsaiHexagonPackage -PackagePath $PackagePath
    }
}

function New-BonsaiHexagonReceipt {
    <# .SYNOPSIS
    Keeps inventory, deployment verification and actual execution as separate evidence gates.
    #>
    [CmdletBinding()]
    [OutputType([Collections.Specialized.OrderedDictionary])]
    param([AllowNull()][Collections.IDictionary]$Inventory, [Parameter(Mandatory)][string]$CollectionStatus,
        [Parameter(Mandatory)][int]$MaximumSeconds, [Parameter(Mandatory)][long]$ElapsedMilliseconds)
    $blockers = [Collections.Generic.List[string]]::new()
    if ($null -eq $Inventory) { $blockers.Add('Inventory unavailable; device and deployment identity remain unknown.') }
    else {
        if ($Inventory.OsArchitecture -cne 'Arm64' -or $Inventory.ProcessArchitecture -cne 'Arm64') { $blockers.Add('Native Windows ARM64 execution has not been observed.') }
        if (@($Inventory.Drivers).Count -eq 0) { $blockers.Add('No ComputeAccelerator driver was observed.') }
        if ($Inventory.Package.Status -cne 'inventoried') { $blockers.Add('No explicit runtime package directory was selected.') }
        elseif (@($Inventory.Package.Files).Count -eq 0) { $blockers.Add('The selected directory contained no eligible runtime, DSP, catalog or notice files.') }
    }
    $blockers.Add('Exact source/license/package ABI and DSP catalog membership/installed trust require independent verification.')
    $blockers.Add('Device mapping and supported-type dispatch have not executed; DMA64 and effective VTCM remain unknown.')
    $blockers.Add('Swift packed arithmetic, full state, lifecycle, resource and paired benefit gates remain pending.')
    return [ordered]@{
        SchemaVersion = 1; Kind = 'runner-hexagon-preflight'; CollectedAtUtc = [DateTime]::UtcNow.ToString('o')
        CollectionStatus = $CollectionStatus; MaximumSeconds = $MaximumSeconds; ElapsedMilliseconds = $ElapsedMilliseconds
        Inventory = $Inventory; Execution = 'not-attempted'; Qualification = 'pending'; ProfileEligible = $false
        TrustChanged = $false; DriverInstalled = $false; ModelLoaded = $false
        Blockers = @($blockers)
    }
}

Export-ModuleMember -Function Get-BonsaiHexagonLocalPath, Get-BonsaiHexagonPackage, Get-BonsaiHexagonInventory, New-BonsaiHexagonReceipt
