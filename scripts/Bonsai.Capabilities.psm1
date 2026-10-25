#requires -Version 7.6
# Host inventory projection only. Runtime kernel eligibility belongs to Prism.
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true

function Get-CapabilityProperty {
    [CmdletBinding()]
    [OutputType([object])]
    param([AllowNull()][object]$Value, [Parameter(Mandatory)][string]$Name)
    if ($null -eq $Value) { return $null }
    if ($Value -is [Collections.IDictionary]) { return $Value[$Name] }
    $property = $Value.PSObject.Properties[$Name]
    if ($null -eq $property) { return $null }
    return $property.Value
}

function Get-CapabilityText {
    [CmdletBinding()]
    [OutputType([string])]
    param([AllowNull()][object]$Value)
    if ($null -eq $Value) { return $null }
    if ($Value -isnot [string] -or $Value.Length -gt 512 -or $Value -match '[\x00-\x1f\x7f]') {
        throw 'Invalid capability text fact.'
    }
    if ([string]::IsNullOrWhiteSpace($Value)) { return $null }
    return $Value.Trim()
}

function Get-CapabilityUInt64 {
    [CmdletBinding()]
    [OutputType([uint64])]
    param([AllowNull()][object]$Value)
    if ($null -eq $Value) { return $null }
    if ($Value -isnot [byte] -and $Value -isnot [sbyte] -and
        $Value -isnot [int16] -and $Value -isnot [uint16] -and
        $Value -isnot [int32] -and $Value -isnot [uint32] -and
        $Value -isnot [int64] -and $Value -isnot [uint64]) {
        throw 'Capability counts and bytes must be unsigned integer facts.'
    }
    if ($Value -lt 0) { throw 'Capability counts and bytes must be nonnegative.' }
    return [uint64]$Value
}

function Get-CapabilityFlag {
    [CmdletBinding()]
    [OutputType([bool])]
    param([AllowNull()][object]$Value)
    if ($null -eq $Value) { return $null }
    if ($Value -isnot [bool]) { throw 'Capability flags must be boolean facts.' }
    return $Value
}

function Get-CapabilityDate {
    [CmdletBinding()]
    [OutputType([string])]
    param([AllowNull()][object]$Value)
    if ($null -eq $Value) { return $null }
    if ($Value -isnot [datetime] -and $Value -isnot [datetimeoffset]) { throw 'Invalid capability date fact.' }
    return $Value.ToUniversalTime().ToString('o')
}

function Get-CapabilityStatus {
    [CmdletBinding()]
    [OutputType([string])]
    param([bool]$Succeeded, [bool]$Incomplete)
    if (-not $Succeeded) { return 'unknown' }
    if ($Incomplete) { return 'partial' }
    return 'observed'
}

function Get-BonsaiHostCapability {
    <#
    .SYNOPSIS
    Projects bounded observed host facts without inferring execution eligibility.
    .DESCRIPTION
    Missing or failed probes remain null/unknown. An empty successful device
    inventory differs from a failed probe. ISA facts describe this process only;
    they cannot authorize a runtime built for a different architecture.
    #>
    [CmdletBinding()]
    [OutputType([Collections.Specialized.OrderedDictionary])]
    param([Parameter(Mandatory)][Collections.IDictionary]$Facts)

    $osArchitecture = Get-CapabilityText $Facts['OsArchitecture']
    $processArchitecture = Get-CapabilityText $Facts['ProcessArchitecture']
    $osArchitecture = if ($osArchitecture -in @('Arm64', 'X64', 'X86', 'Arm')) { $osArchitecture.ToLowerInvariant() } else { $null }
    $processArchitecture = if ($processArchitecture -in @('Arm64', 'X64', 'X86', 'Arm')) { $processArchitecture.ToLowerInvariant() } else { $null }
    $probes = $Facts['Probes']
    if ($null -eq $probes) { $probes = @{} }
    if ($probes -isnot [Collections.IDictionary]) { throw 'Invalid capability probe results.' }
    foreach ($entry in $probes.GetEnumerator()) { $null = Get-CapabilityFlag $entry.Value }
    $os = if ($probes['OperatingSystem']) { $Facts['OperatingSystem'] } else { $null }
    $computer = if ($probes['Computer']) { $Facts['Computer'] } else { $null }
    $memory = if ($probes['Memory']) { $Facts['Memory'] } else { $null }
    $features = if ($probes['Isa']) { $Facts['Isa'] } else { $null }
    $version = Get-CapabilityText (Get-CapabilityProperty $os 'Version')
    $buildNumber = Get-CapabilityText (Get-CapabilityProperty $os 'BuildNumber')
    $native = if ($null -ne $osArchitecture -and $null -ne $processArchitecture) { $osArchitecture -ceq $processArchitecture } else { $null }

    $processors = @(
        if ($probes['Processors']) {
            if (@($Facts['Processors']).Count -gt 64) { throw 'Too many processor inventory entries.' }
            foreach ($cpu in $Facts['Processors']) {
                if ($null -eq $cpu) { throw 'Invalid processor inventory entry.' }
                [ordered]@{
                    name = Get-CapabilityText (Get-CapabilityProperty $cpu 'Name')
                    manufacturer = Get-CapabilityText (Get-CapabilityProperty $cpu 'Manufacturer')
                    physical_cores = Get-CapabilityUInt64 (Get-CapabilityProperty $cpu 'NumberOfCores')
                    logical_processors = Get-CapabilityUInt64 (Get-CapabilityProperty $cpu 'NumberOfLogicalProcessors')
                }
            }
        }
    )
    $graphics = @(
        if ($probes['Graphics']) {
            if (@($Facts['Graphics']).Count -gt 64) { throw 'Too many graphics inventory entries.' }
            foreach ($gpu in $Facts['Graphics']) {
                if ($null -eq $gpu) { throw 'Invalid graphics inventory entry.' }
                [ordered]@{
                    name = Get-CapabilityText (Get-CapabilityProperty $gpu 'Name')
                    vendor = Get-CapabilityText (Get-CapabilityProperty $gpu 'AdapterCompatibility')
                    driver_version = Get-CapabilityText (Get-CapabilityProperty $gpu 'DriverVersion')
                    driver_date = Get-CapabilityDate (Get-CapabilityProperty $gpu 'DriverDate')
                }
            }
        }
    )
    $npus = @(
        if ($probes['NpuDrivers']) {
            if (@($Facts['NpuDrivers']).Count -gt 64) { throw 'Too many compute accelerator inventory entries.' }
            foreach ($npu in $Facts['NpuDrivers']) {
                if ($null -eq $npu) { throw 'Invalid compute accelerator inventory entry.' }
                [ordered]@{
                    name = Get-CapabilityText (Get-CapabilityProperty $npu 'DeviceName')
                    vendor = Get-CapabilityText (Get-CapabilityProperty $npu 'Manufacturer')
                    driver_version = Get-CapabilityText (Get-CapabilityProperty $npu 'DriverVersion')
                    driver_date = Get-CapabilityDate (Get-CapabilityProperty $npu 'DriverDate')
                    driver_signed = Get-CapabilityFlag (Get-CapabilityProperty $npu 'IsSigned')
                }
            }
        }
    )
    $installed = Get-CapabilityUInt64 (Get-CapabilityProperty $computer 'TotalPhysicalMemory')
    $usableKiB = Get-CapabilityUInt64 (Get-CapabilityProperty $os 'TotalVisibleMemorySize')
    if ($null -ne $usableKiB -and $usableKiB -gt ([uint64]::MaxValue -shr 10)) { throw 'Capability memory conversion exceeds its byte bound.' }
    $usable = if ($null -ne $usableKiB) { [uint64]($usableKiB -shl 10) } else { $null }
    $available = Get-CapabilityUInt64 (Get-CapabilityProperty $memory 'AvailableBytes')
    $commitLimit = Get-CapabilityUInt64 (Get-CapabilityProperty $memory 'CommitLimit')
    $committed = Get-CapabilityUInt64 (Get-CapabilityProperty $memory 'CommittedBytes')
    if ($null -ne $commitLimit -and $null -ne $committed -and $committed -gt $commitLimit) {
        throw 'Capability committed memory exceeds its observed limit.'
    }
    $advsimd = Get-CapabilityFlag (Get-CapabilityProperty $features 'AdvSimd')
    $dotprod = Get-CapabilityFlag (Get-CapabilityProperty $features 'DotProd')
    # An emulated process cannot establish ISA availability in a native runtime.
    # Retain the observation's process architecture so consumers must match scope.
    return [ordered]@{
        schema_version = 1
        kind = 'runner-host-capabilities'
        operating_system = [ordered]@{
            architecture = $osArchitecture; version = $version; build_number = $buildNumber
            source = 'RuntimeInformation.OSArchitecture;Win32_OperatingSystem'
            status = Get-CapabilityStatus ($null -ne $osArchitecture) ($null -eq $version -or $null -eq $buildNumber)
        }
        process = [ordered]@{
            architecture = $processArchitecture; is_native = $native
            source = 'RuntimeInformation.ProcessArchitecture'
            status = Get-CapabilityStatus ($null -ne $processArchitecture) ($null -eq $native)
        }
        cpu = [ordered]@{
            identities = $processors; source = 'Win32_Processor'
            status = Get-CapabilityStatus ([bool]$probes['Processors']) ($processors.Count -eq 0 -or @($processors | Where-Object { $null -eq $_.name -or $null -eq $_.physical_cores -or $null -eq $_.logical_processors }).Count -gt 0)
            topology = [ordered]@{ processor_groups = $null; efficiency_classes = $null; status = 'unknown' }
            isa = [ordered]@{
                architecture = $processArchitecture; scope = 'current-process'; advsimd = $advsimd; dotprod = $dotprod
                source = '.NET System.Runtime.Intrinsics.Arm.AdvSimd/Dp.IsSupported'
                status = Get-CapabilityStatus ([bool]$probes['Isa']) ($null -eq $advsimd -or $null -eq $dotprod -or $null -eq $processArchitecture)
            }
        }
        memory = [ordered]@{
            installed_physical_bytes = $installed; usable_physical_bytes = $usable
            available_physical_bytes = $available; commit_limit_bytes = $commitLimit; committed_bytes = $committed
            source = 'Win32_ComputerSystem;Win32_OperatingSystem;Win32_PerfFormattedData_PerfOS_Memory'
            status = Get-CapabilityStatus ([bool]($probes['Computer'] -or $probes['OperatingSystem'] -or $probes['Memory'])) ($null -eq $installed -or $null -eq $usable -or $null -eq $available -or $null -eq $commitLimit -or $null -eq $committed)
        }
        gpu = [ordered]@{
            devices = $graphics; source = 'Win32_VideoController'
            status = Get-CapabilityStatus ([bool]$probes['Graphics']) (@($graphics | Where-Object { $null -eq $_.name -or $null -eq $_.driver_version }).Count -gt 0)
            api_features = $null; device_budget_bytes = $null
        }
        npu = [ordered]@{
            devices = $npus; source = 'Win32_PnPSignedDriver:DeviceClass=ComputeAccelerator'
            status = Get-CapabilityStatus ([bool]$probes['NpuDrivers']) (@($npus | Where-Object { $null -eq $_.name -or $null -eq $_.driver_version -or $null -eq $_.driver_signed }).Count -gt 0)
            architecture = $null; firmware_version = $null; backend_readiness = $null
        }
    }
}

Export-ModuleMember -Function Get-BonsaiHostCapability
