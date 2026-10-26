#requires -Version 7.6
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1')
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Native.psm1')

function Clear-BonsaiWorkspaceBuild {
    <# .SYNOPSIS
    Invalidates workspace outputs on a cache miss while retaining registry builds.
    #>
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseShouldProcessForStateChangingFunctions', '', Justification = 'Invalidates only the owned external Cargo cache for an explicitly requested build.')]
    [CmdletBinding()]
    [OutputType([void])]
    param([Parameter(Mandatory)][string]$RepositoryRoot, [Parameter(Mandatory)][string]$CargoPath,
          [Parameter(Mandatory)][string]$Channel, [Parameter(Mandatory)][string]$TargetDirectory,
          [Parameter(Mandatory)][string]$WorkingDirectory, [string]$BuildTarget)
    $target = Resolve-ExternalPath -Path $TargetDirectory
    $working = Resolve-ExternalPath -Path $WorkingDirectory
    $repository = [IO.Path]::GetFullPath($RepositoryRoot)
    $packages = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    foreach ($root in @('src', 'tools')) {
        foreach ($manifest in [IO.Directory]::EnumerateFiles((Join-Path $repository $root), 'Cargo.toml', [IO.SearchOption]::AllDirectories)) {
            $content = [IO.File]::ReadAllText($manifest)
            if ($content -match '(?ms)^\[package\]\s*\r?\n(?:(?!^\[).)*?^name\s*=\s*"([a-z0-9-]+)"') { [void]$packages.Add($Matches[1]) }
        }
    }
    if (-not $packages.Count) { throw 'No local workspace packages were found for build invalidation.' }
    $arguments = @("+$Channel", 'clean', '--release', '--manifest-path', (Join-Path $repository 'Cargo.toml'), '--target-dir', $target)
    if ($BuildTarget) { $arguments += @('--target', $BuildTarget) }
    foreach ($package in ($packages | Sort-Object)) { $arguments += @('--package', $package) }
    $log = Join-Path $working ('cargo-invalidate-' + [guid]::NewGuid().ToString('N') + '.log')
    $result = Invoke-BonsaiNativeProcess -Executable $CargoPath -ArgumentList $arguments -WorkingDirectory $working -LogPath $log -TimeoutSeconds 120
    if ($result.ExitCode) { throw "Workspace build invalidation failed; see $log" }
}

function Add-BonsaiFingerprintPart {
    [CmdletBinding()]
    [OutputType([void])]
    param([Parameter(Mandatory)][Security.Cryptography.IncrementalHash]$Hash,
          [Parameter(Mandatory)][AllowEmptyString()][string]$Value)
    $bytes = [Text.Encoding]::UTF8.GetBytes($Value)
    $Hash.AppendData([BitConverter]::GetBytes([long]$bytes.Length))
    $Hash.AppendData($bytes)
}

function Get-BonsaiBuildFingerprint {
    <#
    .SYNOPSIS
    Hashes source and build settings for conservative unchanged-build reuse.
    .DESCRIPTION
    Reads file contents rather than trusting timestamps. The whole checkout is
    included, except Git, agent, dependency and documented output directories.
    Cargo configuration and build environment changes invalidate reuse. External
    path dependencies and source/tool overrides deliberately disable reuse.
    #>
    [CmdletBinding()]
    [OutputType([string])]
    param([Parameter(Mandatory)][string]$RepositoryRoot, [string]$Identity = '', [string]$WorkingDirectory)
    if (-not [IO.Path]::IsPathFullyQualified($RepositoryRoot) -or $RepositoryRoot -match '%[^%]+%') {
        throw 'RepositoryRoot must be an expanded absolute path.'
    }
    $repository = [IO.Path]::GetFullPath($RepositoryRoot).TrimEnd([IO.Path]::DirectorySeparatorChar)
    $root = [IO.DirectoryInfo]::new($repository)
    if (-not $root.Exists -or $root.Attributes -band [IO.FileAttributes]::ReparsePoint) {
        throw 'Build inputs require an existing repository without reparse points.'
    }
    $files = [Collections.Generic.List[string]]::new()
    $directories = [Collections.Generic.Stack[IO.DirectoryInfo]]::new()
    $directories.Push($root)
    while ($directories.Count) {
        $directory = $directories.Pop()
        foreach ($entry in $directory.EnumerateFileSystemInfos()) {
            if ($entry.Name -in @('.git', '.agents', '.codex', 'target', 'node_modules') -or
                ($directory.FullName -ceq $repository -and $entry.Name -in @('bin', 'obj', 'artifacts')) -or
                ($directory.FullName -ieq (Join-Path $repository 'tools/validation') -and $entry.Name -in @('bin', 'obj'))) { continue }
            if ($entry.Attributes -band [IO.FileAttributes]::ReparsePoint) {
                throw "Reparse-point build inputs are unsupported: $($entry.FullName)"
            }
            if ($entry -is [IO.DirectoryInfo]) { $directories.Push($entry) }
            else { $files.Add($entry.FullName) }
        }
    }
    $files.Sort([StringComparer]::Ordinal)
    $hash = [Security.Cryptography.IncrementalHash]::CreateHash([Security.Cryptography.HashAlgorithmName]::SHA256)
    $unsupported = $false
    try {
        Add-BonsaiFingerprintPart -Hash $hash -Value 'bonsai-build-inputs-v1'
        Add-BonsaiFingerprintPart -Hash $hash -Value $repository.ToUpperInvariant()
        Add-BonsaiFingerprintPart -Hash $hash -Value $Identity
        foreach ($file in $files) {
            Add-BonsaiFingerprintPart -Hash $hash -Value ([IO.Path]::GetRelativePath($repository, $file))
            $stream = [IO.FileStream]::new($file, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
            try { $hash.AppendData([Security.Cryptography.SHA256]::HashData($stream)) }
            finally { $stream.Dispose() }
            if ([IO.Path]::GetFileName($file) -ceq 'Cargo.toml') {
                # Cargo target paths and local dependency paths must remain in
                # the tree whose bytes are represented by this fingerprint.
                foreach ($match in [regex]::Matches([IO.File]::ReadAllText($file), '(?m)\bpath\s*=\s*["'']([^"'']+)["'']')) {
                    $dependencyPath = $match.Groups[1].Value
                    $dependency = if ([IO.Path]::IsPathFullyQualified($dependencyPath)) { [IO.Path]::GetFullPath($dependencyPath) }
                        else { [IO.Path]::GetFullPath((Join-Path ([IO.Path]::GetDirectoryName($file)) $dependencyPath)) }
                    if (-not $dependency.StartsWith($repository + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
                        $unsupported = $true
                    }
                }
            }
        }
        # Include compiler, Cargo and native build settings. Terminal/session
        # IDs change on every new shell and are not compiler inputs.
        $environment = [Environment]::GetEnvironmentVariables()
        $names = [string[]]@($environment.Keys | Where-Object {
            $_ -match '^(CARGO.*|RUST.*|BONSAI.*|CMAKE.*|CC(_.*)?|CXX(_.*)?|CFLAGS(_.*)?|CXXFLAGS(_.*)?|CL|_CL_|AR(_.*)?|RC|INCLUDE|LIB|LIBPATH|PATH|PATHEXT|SYSTEMROOT|SYSTEMDRIVE|WINDIR|PROGRAMDATA|PROGRAMFILES.*|USERPROFILE|LOCALAPPDATA|APPDATA|VSCMD.*|VSINSTALLDIR|VCINSTALLDIR|VCTOOLS.*|WINDOWSSDK.*|UNIVERSALCRT.*|UCRTVERSION|TRANSCRIBE.*|HOST.*|TARGET.*|BINDGEN.*|NUM_JOBS|OPT_LEVEL|PROFILE|DEBUG|SDKROOT|PKG_CONFIG.*)$'
        })
        [Array]::Sort($names, [StringComparer]::Ordinal)
        foreach ($name in $names) {
            Add-BonsaiFingerprintPart -Hash $hash -Value $name
            Add-BonsaiFingerprintPart -Hash $hash -Value ([string]$environment[$name])
        }
        $configs = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
        if ($WorkingDirectory) {
            if (-not [IO.Path]::IsPathFullyQualified($WorkingDirectory) -or $WorkingDirectory -match '%[^%]+%') {
                throw 'WorkingDirectory must be an expanded absolute path.'
            }
            $cursor = [IO.Path]::GetFullPath($WorkingDirectory)
            while ($cursor) {
                foreach ($name in @('config', 'config.toml')) {
                    $candidate = Join-Path -Path $cursor -ChildPath '.cargo' -AdditionalChildPath $name
                    # A new random launch directory is not an input; only an
                    # existing Cargo configuration under it affects the build.
                    if ([IO.File]::Exists($candidate)) { [void]$configs.Add($candidate) }
                }
                $cursor = [IO.Path]::GetDirectoryName($cursor)
            }
        }
        $cursor = $repository
        while ($cursor) {
            foreach ($name in @('config', 'config.toml')) { [void]$configs.Add((Join-Path -Path $cursor -ChildPath '.cargo' -AdditionalChildPath $name)) }
            $cursor = [IO.Path]::GetDirectoryName($cursor)
        }
        $cargoHome = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path ([Environment]::GetFolderPath('UserProfile')) '.cargo' }
        if (-not [IO.Path]::IsPathFullyQualified($cargoHome) -or $cargoHome -match '%[^%]+%') { $unsupported = $true }
        else {
            foreach ($name in @('config', 'config.toml')) { [void]$configs.Add((Join-Path $cargoHome $name)) }
        }
        $configPaths = [string[]]@($configs)
        [Array]::Sort($configPaths, [StringComparer]::Ordinal)
        foreach ($config in $configPaths) {
            Add-BonsaiFingerprintPart -Hash $hash -Value $config
            if (-not [IO.File]::Exists($config)) {
                Add-BonsaiFingerprintPart -Hash $hash -Value 'missing'
                continue
            }
            if ([IO.File]::GetAttributes($config) -band [IO.FileAttributes]::ReparsePoint) { $unsupported = $true }
            $content = [IO.File]::ReadAllText($config)
            Add-BonsaiFingerprintPart -Hash $hash -Value $content
            if ($content -match '(?m)\b(paths|path|directory|local-registry|replace-with|rustc|rustc-wrapper|rustc-workspace-wrapper|linker)\s*=') {
                $unsupported = $true
            }
        }
        foreach ($name in @('RUSTC', 'RUSTC_WRAPPER', 'RUSTC_WORKSPACE_WRAPPER', 'CARGO_BUILD_RUSTC', 'CARGO_BUILD_RUSTC_WRAPPER', 'CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER')) {
            if ($environment.Contains($name) -and $environment[$name]) { $unsupported = $true }
        }
        if ($unsupported) {
            # The ordinary Cargo build remains the authority for unrepresented
            # external inputs; no receipt can match this per-call nonce.
            Add-BonsaiFingerprintPart -Hash $hash -Value ([guid]::NewGuid().ToString('N'))
        }
        return [Convert]::ToHexString($hash.GetHashAndReset()).ToLowerInvariant()
    } finally { $hash.Dispose() }
}

function Test-BonsaiBuildReceipt {
    <# .SYNOPSIS
    Accepts only the exact source fingerprint and unchanged executable bytes.
    #>
    [CmdletBinding()]
    [OutputType([bool])]
    param([Parameter(Mandatory)][string]$ReceiptPath, [Parameter(Mandatory)][string]$Fingerprint,
          [Parameter(Mandatory)][string[]]$Executable)
    try {
        $receipt = Resolve-ExternalPath -Path $ReceiptPath
        if ($Fingerprint -cnotmatch '^[a-f0-9]{64}$' -or -not [IO.File]::Exists($receipt) -or
            [IO.FileInfo]::new($receipt).Length -gt 65536) { return $false }
        $record = [IO.File]::ReadAllText($receipt) | ConvertFrom-Json -AsHashtable -NoEnumerate -DateKind String
        if ($record -isnot [Collections.IDictionary] -or -not $record.Contains('Version') -or $record.Version -ne 1 -or
            -not $record.Contains('Fingerprint') -or $record.Fingerprint -cne $Fingerprint -or
            -not $record.Contains('Outputs') -or $record.Outputs -isnot [array] -or $record.Outputs.Count -ne $Executable.Count) { return $false }
        for ($index = 0; $index -lt $Executable.Count; $index++) {
            $path = Resolve-ExternalPath -Path $Executable[$index]
            $output = $record.Outputs[$index]
            if ($output -isnot [Collections.IDictionary] -or -not $output.Contains('Path') -or
                $output.Path -isnot [string] -or -not [string]::Equals($path, $output.Path, [StringComparison]::OrdinalIgnoreCase) -or
                -not $output.Contains('Sha256') -or $output.Sha256 -isnot [string] -or $output.Sha256 -cnotmatch '^[a-f0-9]{64}$' -or
                -not [IO.File]::Exists($path)) { return $false }
            $stream = [IO.FileStream]::new($path, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
            try { $actual = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($stream)).ToLowerInvariant() }
            finally { $stream.Dispose() }
            if ($actual -cne $output.Sha256) { return $false }
        }
        return $true
    } catch { return $false }
}

function Write-BonsaiBuildReceipt {
    <# .SYNOPSIS
    Atomically records a successful build's inputs and exact executable digests.
    #>
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseShouldProcessForStateChangingFunctions', '', Justification = 'Private cache receipt for an explicitly requested successful build.')]
    [CmdletBinding()]
    [OutputType([void])]
    param([Parameter(Mandatory)][string]$ReceiptPath,
          [Parameter(Mandatory)][ValidatePattern('^[a-f0-9]{64}$')][string]$Fingerprint,
          [Parameter(Mandatory)][string[]]$Executable)
    $receipt = Resolve-ExternalPath -Path $ReceiptPath
    $outputs = @(
        foreach ($file in $Executable) {
            $path = Resolve-ExternalPath -Path $file
            $stream = [IO.FileStream]::new($path, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
            try { $digest = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($stream)).ToLowerInvariant() }
            finally { $stream.Dispose() }
            @{ Path = $path; Sha256 = $digest }
        }
    )
    $record = @{ Version = 1; Fingerprint = $Fingerprint; Outputs = $outputs }
    $temporary = Resolve-ExternalPath -Path ($receipt + '.' + [guid]::NewGuid().ToString('N') + '.tmp')
    try {
        [IO.File]::WriteAllText($temporary, (ConvertTo-Json -InputObject $record -Depth 4 -Compress), [Text.UTF8Encoding]::new($false))
        [IO.File]::Move($temporary, $receipt, $true)
    } finally { if ([IO.File]::Exists($temporary)) { [IO.File]::Delete($temporary) } }
}

Export-ModuleMember -Function Get-BonsaiBuildFingerprint, Test-BonsaiBuildReceipt, Write-BonsaiBuildReceipt, Clear-BonsaiWorkspaceBuild
