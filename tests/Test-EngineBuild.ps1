#requires -Version 7.6
[CmdletBinding()]
[OutputType([string])]
param([Parameter(Mandatory)][ValidateSet('Parallelism')][string]$Case)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true

if ($Case -ne 'Parallelism') { throw 'Unsupported focused engine build case.' }

# Import only the pure scheduling helpers. This focused check never fetches
# engine sources, starts a native build, or changes the host's build environment.
Import-Module (Join-Path $PSScriptRoot '../scripts/Bonsai.Build.psm1') -Force
Import-Module (Join-Path $PSScriptRoot 'Bonsai.TestSupport.psm1')



foreach ($scenario in @(
    @{ Cpus = 12; Bytes = 32GB; Jobs = 8 },
    @{ Cpus = [int]::MaxValue; Bytes = [long]::MaxValue; Jobs = 8 },
    @{ Cpus = 4; Bytes = 32GB; Jobs = 2 },
    @{ Cpus = 1; Bytes = 32GB; Jobs = 1 },
    @{ Cpus = 12; Bytes = 5GB; Jobs = 3 },
    @{ Cpus = 12; Bytes = (5GB - 1); Jobs = 2 },
    @{ Cpus = 12; Bytes = 0; Jobs = 1 },
    @{ Cpus = 12; Bytes = 2GB; Jobs = 1 },
    @{ Cpus = 12; Bytes = -1; Jobs = 2 },
    @{ Cpus = 1; Bytes = -1; Jobs = 1 }
)) {
    $actual = Resolve-BuildParallelism 0 $scenario.Cpus $scenario.Bytes
    Assert ($actual -eq $scenario.Jobs) "Automatic scheduling budget differs for CPUs=$($scenario.Cpus), bytes=$($scenario.Bytes)."
}
Assert ((Resolve-BuildParallelism 12 1 0) -eq 12) 'An explicit supported concurrency override was ignored.'
Assert-Failure { Resolve-BuildParallelism -1 12 32GB } 'A negative job request was accepted.'
Assert-Failure { Resolve-BuildParallelism 13 12 32GB } 'An unbounded job request was accepted.'
Assert-Failure { Resolve-BuildParallelism 0 0 32GB } 'An invalid processor count was accepted.'
Assert-Failure { Resolve-BuildParallelism 0 12 -2 } 'An invalid available-memory count was accepted.'

# Spaces and shell metacharacters must stay in one argument; native invocation
# uses ProcessStartInfo.ArgumentList, not a concatenated command line.
$build = 'C:\external data & fixtures\build'
$arguments = @(Get-EngineBuildArgument $build 3)
Assert ($arguments.Count -eq 13 -and $arguments[1] -ceq $build) 'The build path was split or extra native arguments were added.'
Assert ($arguments[2] -ceq '--config' -and $arguments[3] -ceq 'Release' -and
    $arguments[4] -ceq '--target' -and $arguments[5] -ceq 'llama-server') 'The focused Release server target changed.'
Assert ($arguments[6] -ceq '--parallel' -and $arguments[7] -ceq '3' -and $arguments[8] -ceq '--') 'Project concurrency was not passed through CMake.'
foreach ($property in @('/p:UseMultiToolTask=true', '/p:EnforceProcessCountAcrossBuilds=true', '/p:CL_MPCount=3', '/p:MultiProcMaxCount=3')) {
    Assert ($arguments[9..12] -ccontains $property) "Shared compiler scheduling omitted $property."
}
Assert-Failure { Get-EngineBuildArgument $build 0 } 'An unresolved automatic budget reached the build command.'
Assert-Failure { Get-EngineBuildArgument $build 13 } 'An unsupported build concurrency reached the build command.'
Write-Output 'Engine build parallelism checks passed.'
