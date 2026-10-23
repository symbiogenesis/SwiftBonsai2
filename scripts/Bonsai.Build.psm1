#requires -Version 7.6
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true

function Resolve-BuildParallelism {
    <#
    .SYNOPSIS
    Chooses a bounded compiler budget from explicit or host CPU/memory limits.
    #>
    [CmdletBinding()]
    [OutputType([int])]
    param([int]$Requested, [int]$ProcessorCount, [long]$AvailableBytes)
    if ($Requested -lt 0 -or $Requested -gt 12 -or $ProcessorCount -lt 1 -or $AvailableBytes -lt -1) { throw 'Invalid build parallelism input.' }
    if ($Requested -gt 0) { return $Requested }
    # A starting budget, not a measured optimum: retain two CPU threads and 2 GiB
    # for interactive work, then allow at most one compiler per remaining GiB.
    $cpuBudget = [Math]::Min(8, [Math]::Max(1, $ProcessorCount - 2))
    if ($AvailableBytes -eq -1) { return [Math]::Min(2, $cpuBudget) }
    $memoryBudget = [Math]::Max(1.0, [Math]::Floor(($AvailableBytes - 2GB) / 1GB))
    [int][Math]::Min([double]$cpuBudget, $memoryBudget)
}

function Get-EngineBuildArgument {
    <#
    .SYNOPSIS
    Builds literal CMake arguments with shared MSBuild compiler limits.
    #>
    [CmdletBinding()]
    [OutputType([string], [array])]
    param([string]$BuildDirectory, [int]$Jobs)
    if ($Jobs -lt 1 -or $Jobs -gt 12) { throw 'Build jobs must be between 1 and 12.' }
    # CMake's --parallel limits MSBuild project workers. The pinned upstream
    # enables MultiToolTask, whose compiler limit otherwise defaults to all CPUs.
    # Share the same limit across projects instead of multiplying worker counts.
    @('--build', $BuildDirectory, '--config', 'Release', '--target', 'llama-server', '--parallel', [string]$Jobs,
        '--', '/p:UseMultiToolTask=true', '/p:EnforceProcessCountAcrossBuilds=true', "/p:CL_MPCount=$Jobs", "/p:MultiProcMaxCount=$Jobs")
}

Export-ModuleMember -Function Resolve-BuildParallelism, Get-EngineBuildArgument
