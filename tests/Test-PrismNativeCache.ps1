#requires -Version 7.6
<#
.SYNOPSIS
Checks native source reuse and rejects changed or incomplete cached source trees.
#>
[CmdletBinding()]
[OutputType([string])]
param()
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.TestSupport.psm1')
Import-Module (Join-Path $PSScriptRoot '../scripts/Bonsai.PrismNativeCache.psm1')
$scratch = Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-PrismCacheTest-' + [guid]::NewGuid().ToString('N'))
$source = Join-Path $scratch 'verified'
$cache = Join-Path $scratch 'native'
[void][IO.Directory]::CreateDirectory((Join-Path $source 'nested'))
[IO.File]::WriteAllText((Join-Path $source 'CMakeLists.txt'), 'verified build configuration')
[IO.File]::WriteAllText((Join-Path $source 'nested/kernel.cpp'), 'verified native source')
$cached = Initialize-BonsaiPrismNativeSource -SourceDirectory $source -BuildDirectory $cache
$kernel = Join-Path $cached 'nested/kernel.cpp'
$timestamp = [IO.File]::GetLastWriteTimeUtc($kernel)
[IO.File]::SetLastWriteTimeUtc((Join-Path $source 'nested/kernel.cpp'), $timestamp.AddHours(1))
Assert ((Initialize-BonsaiPrismNativeSource -SourceDirectory $source -BuildDirectory $cache) -ceq $cached) 'An intact source tree was not reusable.'
Assert ([IO.File]::GetLastWriteTimeUtc($kernel) -eq $timestamp) 'Fresh extraction timestamps invalidated native objects.'
[IO.File]::WriteAllText($kernel, 'modified native source')
Assert-Failure { Initialize-BonsaiPrismNativeSource -SourceDirectory $source -BuildDirectory $cache } 'A modified cached source was accepted.'
[IO.File]::Copy((Join-Path $source 'nested/kernel.cpp'), $kernel, $true)
$extra = Join-Path $cached 'injected.cpp'
[IO.File]::WriteAllText($extra, 'unlisted')
Assert-Failure { Initialize-BonsaiPrismNativeSource -SourceDirectory $source -BuildDirectory $cache } 'An unlisted cached source was accepted.'
[IO.File]::Delete($extra)
[IO.File]::Delete($kernel)
Assert-Failure { Initialize-BonsaiPrismNativeSource -SourceDirectory $source -BuildDirectory $cache } 'An incomplete cached tree was accepted.'
Assert-Failure { Initialize-BonsaiPrismNativeSource -SourceDirectory $source -BuildDirectory (Join-Path $source 'cache') } 'Overlapping source and cache paths were accepted.'
$orphaned = Join-Path $scratch 'orphaned-native'
[void][IO.Directory]::CreateDirectory((Join-Path $orphaned 'build-ggml'))
[IO.File]::WriteAllText((Join-Path $orphaned 'build-ggml/kernel.obj'), 'stale native output')
Assert-Failure { Initialize-BonsaiPrismNativeSource -SourceDirectory $source -BuildDirectory $orphaned } 'Native outputs without their verified source tree were accepted.'
Assert (-not [IO.Directory]::Exists((Join-Path $orphaned 'source'))) 'Orphaned native outputs were rehydrated with old source timestamps.'
Write-Output "Prism native cache source identity and timestamp checks passed. Evidence: $scratch"
