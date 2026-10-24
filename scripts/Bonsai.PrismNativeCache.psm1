#requires -Version 7.6
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1')

function Initialize-BonsaiPrismNativeSource {
    <#
    .SYNOPSIS
    Verifies a stable native source tree against a freshly verified archive extraction.
    .DESCRIPTION
    Every source file is checked before CMake may reuse native objects. Unchanged
    files retain their timestamps and paths. A stale or modified tree fails closed.
    #>
    [CmdletBinding(SupportsShouldProcess)]
    [OutputType([string])]
    param(
        [Parameter(Mandatory)][string]$SourceDirectory,
        [Parameter(Mandatory)][string]$BuildDirectory
    )
    $source = Resolve-ExternalPath $SourceDirectory
    $build = Resolve-ExternalPath $BuildDirectory
    $separator = [IO.Path]::DirectorySeparatorChar
    $comparison = if ($IsWindows) { [StringComparison]::OrdinalIgnoreCase } else { [StringComparison]::Ordinal }
    if ($source.Equals($build, $comparison) -or $source.StartsWith($build + $separator, $comparison) -or
        $build.StartsWith($source + $separator, $comparison)) {
        throw 'Verified source and native build cache must use separate directories.'
    }
    if (-not [IO.Directory]::Exists($source)) { throw 'The freshly verified Prism source tree is missing.' }
    $cached = Join-Path $build 'source'
    $sourceEntries = @(Get-ChildItem -LiteralPath $source -Force -Recurse)
    if ($sourceEntries.Where({ $_.Attributes -band [IO.FileAttributes]::ReparsePoint }, 'First').Count) {
        throw 'Prism source trees cannot contain reparse points.'
    }
    if (-not [IO.Directory]::Exists($cached)) {
        if (Test-Path -LiteralPath $cached) { throw 'The cached Prism source path is not a directory.' }
        if ([IO.Directory]::Exists($build) -and @(Get-ChildItem -LiteralPath $build -Force).Count) {
            throw 'Native build outputs exist without their verified source tree; use a fresh native build cache.'
        }
        if (-not $PSCmdlet.ShouldProcess($cached, 'Copy freshly verified Prism sources for native build reuse')) { return }
        [void][IO.Directory]::CreateDirectory($cached)
        foreach ($entry in $sourceEntries) {
            $destination = Join-Path $cached ([IO.Path]::GetRelativePath($source, $entry.FullName))
            if ($entry.PSIsContainer) { [void][IO.Directory]::CreateDirectory($destination) }
            else {
                [void][IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($destination))
                [IO.File]::Copy($entry.FullName, $destination, $false)
                [IO.File]::SetLastWriteTimeUtc($destination, $entry.LastWriteTimeUtc)
            }
        }
    }
    $null = Resolve-ExternalPath $cached
    $cachedEntries = @(Get-ChildItem -LiteralPath $cached -Force -Recurse)
    if ($cachedEntries.Where({ $_.Attributes -band [IO.FileAttributes]::ReparsePoint }, 'First').Count) {
        throw 'Cached Prism source trees cannot contain reparse points.'
    }
    $comparer = if ($IsWindows) { [StringComparer]::OrdinalIgnoreCase } else { [StringComparer]::Ordinal }
    $files = [Collections.Generic.Dictionary[string, IO.FileInfo]]::new($comparer)
    foreach ($entry in $cachedEntries) {
        if (-not $entry.PSIsContainer) { $files.Add([IO.Path]::GetRelativePath($cached, $entry.FullName), $entry) }
    }
    $originals = @($sourceEntries | Where-Object { -not $_.PSIsContainer })
    if ($files.Count -ne $originals.Count) { throw 'Cached Prism source inventory changed; use a fresh native build cache.' }
    foreach ($entry in $originals) {
        $relative = [IO.Path]::GetRelativePath($source, $entry.FullName)
        if (-not $files.ContainsKey($relative) -or $files[$relative].Length -ne $entry.Length -or
            (Get-FileHash -LiteralPath $files[$relative].FullName -Algorithm SHA256).Hash -cne
            (Get-FileHash -LiteralPath $entry.FullName -Algorithm SHA256).Hash) {
            throw "Cached Prism source identity changed: $relative. Use a fresh native build cache."
        }
    }
    return $cached
}

function Initialize-BonsaiPrismNativeAdapter {
    <#
    .SYNOPSIS
    Keeps verified adapter build inputs at stable paths and timestamps.
    .DESCRIPTION
    Fresh checkouts must not invalidate a cached native bridge's translation
    unit and headers. Copy only CMake/native inputs; then compare the complete
    cached inventory and every file's bytes with the current checkout. Modified,
    missing or injected cached files fail closed and require a fresh cache.
    #>
    [CmdletBinding(SupportsShouldProcess)]
    [OutputType([string])]
    param(
        [Parameter(Mandatory)][string]$AdapterDirectory,
        [Parameter(Mandatory)][string]$BuildDirectory
    )
    if (-not [IO.Path]::IsPathFullyQualified($AdapterDirectory) -or $AdapterDirectory -match '%[^%]+%') {
        throw 'AdapterDirectory must be an expanded absolute directory.'
    }
    $source = [IO.Path]::GetFullPath($AdapterDirectory)
    $build = Resolve-ExternalPath $BuildDirectory
    $comparison = if ($IsWindows) { [StringComparison]::OrdinalIgnoreCase } else { [StringComparison]::Ordinal }
    $separator = [IO.Path]::DirectorySeparatorChar
    if ($source.Equals($build, $comparison) -or $source.StartsWith($build + $separator, $comparison) -or
        $build.StartsWith($source.TrimEnd($separator) + $separator, $comparison)) {
        throw 'Adapter sources and native build cache must use separate directories.'
    }
    if (-not [IO.Directory]::Exists($source)) { throw 'The current Prism adapter source tree is missing.' }
    $cursor = $source
    while ($cursor) {
        if ((Get-Item -LiteralPath $cursor -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) {
            throw 'Prism adapter source paths cannot contain reparse points.'
        }
        $cursor = [IO.Path]::GetDirectoryName($cursor)
    }
    $sourceEntries = @(Get-ChildItem -LiteralPath $source -Force -Recurse)
    if ($sourceEntries.Where({ $_.Attributes -band [IO.FileAttributes]::ReparsePoint }, 'First').Count) {
        throw 'Prism adapter source trees cannot contain reparse points.'
    }
    $originals = @($sourceEntries | Where-Object {
        -not $_.PSIsContainer -and ($_.Name -ceq 'CMakeLists.txt' -or $_.Extension -cin @('.c', '.cpp', '.h', '.hpp', '.inc', '.cmake'))
    })
    if (-not [IO.File]::Exists((Join-Path $source 'CMakeLists.txt')) -or $originals.Count -eq 0) {
        throw 'The Prism adapter has no native CMake inputs.'
    }
    $cached = Join-Path $build 'adapter'
    $null = Resolve-ExternalPath $cached
    if (-not [IO.Directory]::Exists($cached)) {
        if (Test-Path -LiteralPath $cached) { throw 'The cached Prism adapter path is not a directory.' }
        if (-not $PSCmdlet.ShouldProcess($cached, 'Copy current Prism adapter inputs for native build reuse')) { return }
        [void][IO.Directory]::CreateDirectory($cached)
        foreach ($entry in $originals) {
            $destination = Join-Path $cached ([IO.Path]::GetRelativePath($source, $entry.FullName))
            [void][IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($destination))
            [IO.File]::Copy($entry.FullName, $destination, $false)
            [IO.File]::SetLastWriteTimeUtc($destination, $entry.LastWriteTimeUtc)
        }
    }
    $cachedEntries = @(Get-ChildItem -LiteralPath $cached -Force -Recurse)
    if ($cachedEntries.Where({ $_.Attributes -band [IO.FileAttributes]::ReparsePoint }, 'First').Count) {
        throw 'Cached Prism adapter trees cannot contain reparse points.'
    }
    $comparer = if ($IsWindows) { [StringComparer]::OrdinalIgnoreCase } else { [StringComparer]::Ordinal }
    $files = [Collections.Generic.Dictionary[string, IO.FileInfo]]::new($comparer)
    foreach ($entry in $cachedEntries) {
        if (-not $entry.PSIsContainer) { $files.Add([IO.Path]::GetRelativePath($cached, $entry.FullName), $entry) }
    }
    if ($files.Count -ne $originals.Count) { throw 'Cached Prism adapter inventory changed; use a fresh native build cache.' }
    foreach ($entry in $originals) {
        $relative = [IO.Path]::GetRelativePath($source, $entry.FullName)
        if (-not $files.ContainsKey($relative) -or $files[$relative].Length -ne $entry.Length -or
            (Get-FileHash -LiteralPath $files[$relative].FullName -Algorithm SHA256).Hash -cne
            (Get-FileHash -LiteralPath $entry.FullName -Algorithm SHA256).Hash) {
            throw "Cached Prism adapter identity changed: $relative. Use a fresh native build cache."
        }
    }
    return $cached
}

Export-ModuleMember -Function Initialize-BonsaiPrismNativeSource, Initialize-BonsaiPrismNativeAdapter
