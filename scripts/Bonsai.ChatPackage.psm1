#requires -Version 7.6
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1')

function Get-ChatPackageFileName {
    <# .SYNOPSIS
    Returns the fixed portable package layout; manifests cannot choose paths.
    #>
    [CmdletBinding()]
    [OutputType([string[]])]
    param()
    return [string[]]@('bin/bonsai-chat.exe', 'bin/bonsai-broker.exe', 'bin/bonsai-asr.exe',
        'bin/bonsai-tts.exe', 'bin/bonsai-tools.exe', 'LICENSE', 'NOTICE', 'THIRD-PARTY-NOTICES.txt',
        'scripts/Start-Chat.ps1', 'scripts/Start-InstalledChat.ps1', 'scripts/Install-ChatPackage.ps1', 'scripts/Bonsai.ChatPackage.psm1',
        'scripts/Bonsai.Common.psm1', 'scripts/Bonsai.Native.psm1', 'scripts/Bonsai.Tools.psm1',
        'scripts/Bonsai.NativeSpeech.psm1', 'scripts/Get-NativeSpeechModels.ps1',
        'scripts/New-DevelopmentKeys.ps1')
}

function Read-ChatPackageJson {
    <# .SYNOPSIS
    Reads bounded package metadata without accepting duplicate JSON properties.
    #>
    [CmdletBinding()]
    [OutputType([Collections.IDictionary])]
    param([Parameter(Mandatory)][string]$Path, [Parameter(Mandatory)][string[]]$Field)
    $path = Resolve-ExternalPath $Path
    $stream = [IO.File]::Open($path, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
    try {
        if ($stream.Length -gt 32768) { throw 'Package metadata exceeds its byte limit.' }
        $reader = [IO.StreamReader]::new($stream, [Text.UTF8Encoding]::new($false, $true))
        try { $text = $reader.ReadToEnd() } finally { $reader.Dispose() }
    } finally { $stream.Dispose() }
    $options = [Text.Json.JsonDocumentOptions]::new()
    $options.AllowDuplicateProperties = $false
    $document = [Text.Json.JsonDocument]::Parse($text, $options)
    $document.Dispose()
    $value = ConvertFrom-Json -InputObject $text -AsHashtable -NoEnumerate -DateKind String
    if ($value -isnot [Collections.IDictionary] -or $value.Count -ne $Field.Count) { throw 'Invalid package metadata fields.' }
    foreach ($name in $Field) { if ($name -cnotin $value.Keys) { throw 'Invalid package metadata fields.' } }
    return $value
}

function Get-CheckedChatPackage {
    <# .SYNOPSIS
    Verifies a fixed-layout package against an independently supplied manifest hash.
    .DESCRIPTION
    Digests detect changed bytes, not publisher authenticity or same-user isolation.
    The caller must review the source revision, binaries and supplied notices.
    #>
    [CmdletBinding()]
    [OutputType([pscustomobject])]
    param([Parameter(Mandatory)][string]$PackageDirectory,
          [Parameter(Mandatory)][ValidatePattern('^[a-f0-9]{64}$')][string]$ManifestSha256)
    # PowerShell pattern attributes accept uppercase hex too. Canonicalize
    # before deriving a directory or pointer identity so it remains readable.
    $ManifestSha256 = $ManifestSha256.ToLowerInvariant()
    $directory = Resolve-ExternalPath $PackageDirectory
    $manifestPath = Resolve-ExternalPath (Join-Path $directory 'package.json')
    $info = Get-Item -LiteralPath $manifestPath
    if ($info.Length -gt 32768) { throw 'Package manifest exceeds its byte limit.' }
    Test-LockedArtifact -Path $manifestPath -Bytes $info.Length -Sha256 $ManifestSha256
    $manifest = Read-ChatPackageJson -Path $manifestPath -Field @('schemaVersion', 'kind', 'version', 'sourceRevision', 'target', 'files')
    if ($manifest.schemaVersion -isnot [long] -or $manifest.schemaVersion -ne 1 -or
        $manifest.kind -isnot [string] -or $manifest.kind -cne 'bonsai-chat-package' -or
        $manifest.target -isnot [string] -or $manifest.target -cne 'aarch64-pc-windows-msvc' -or
        $manifest.version -isnot [string] -or $manifest.version -cnotmatch '^[0-9]+\.[0-9]+\.[0-9]+$' -or $manifest.version.Length -gt 32 -or
        $manifest.sourceRevision -isnot [string] -or $manifest.sourceRevision -cnotmatch '^[a-f0-9]{40}$' -or
        $manifest.files -isnot [array]) { throw 'Unsupported chat package identity.' }
    $names = @(Get-ChatPackageFileName)
    if ($manifest.files.Count -ne $names.Count) { throw 'Package file inventory differs from the fixed layout.' }
    $seen = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    foreach ($file in $manifest.files) {
        if ($file -isnot [Collections.IDictionary] -or $file.Count -ne 3 -or
            'name' -cnotin $file.Keys -or 'bytes' -cnotin $file.Keys -or 'sha256' -cnotin $file.Keys -or
            $file.name -isnot [string] -or $file.name -cnotin $names -or -not $seen.Add($file.name) -or
            $file.bytes -isnot [long] -or $file.bytes -le 0 -or
            $file.sha256 -isnot [string] -or $file.sha256 -cnotmatch '^[a-f0-9]{64}$') { throw 'Invalid package file identity.' }
        $limit = if ($file.name.StartsWith('bin/', [StringComparison]::Ordinal)) { 512MB } else { 16MB }
        if ($file.bytes -gt $limit) { throw 'Package file exceeds its byte limit.' }
        $path = Resolve-ExternalPath (Join-Path $directory $file.name)
        Test-LockedArtifact -Path $path -Bytes $file.bytes -Sha256 $file.sha256
        if ($file.name.StartsWith('bin/', [StringComparison]::Ordinal)) { Assert-Arm64PE -Path $path }
    }
    Assert-ChatPackageInventory -Directory $directory
    return [pscustomobject]@{ Directory = $directory; ManifestSha256 = $ManifestSha256; Manifest = $manifest
        Id = $manifest.version + '-' + $manifest.sourceRevision + '-' + $ManifestSha256 }
}

function Assert-ChatPackageInventory {
    <# .SYNOPSIS
    Rejects package additions and links before Windows can load packaged code.
    #>
    [CmdletBinding()]
    [OutputType([void])]
    param([Parameter(Mandatory)][string]$Directory)
    $names = @(Get-ChatPackageFileName)
    # Extra DLLs and loader redirection files must reject before tool execution.
    $fileCount = 0
    foreach ($container in @($directory, (Join-Path $directory 'bin'), (Join-Path $directory 'scripts'))) {
        [void](Resolve-ExternalPath $container)
        foreach ($entry in (Get-ChildItem -LiteralPath $container -Force)) {
            # Check each directory before opening it; never recursively traverse
            # package-selected directories or reparse points.
            [void](Resolve-ExternalPath $entry.FullName)
            $relative = [IO.Path]::GetRelativePath($directory, $entry.FullName).Replace('\', '/')
            if ($entry.PSIsContainer) {
                if ($relative -cnotin @('bin', 'scripts')) { throw 'Unexpected directory in chat package.' }
                continue
            }
            if ($relative -cne 'package.json' -and $relative -cnotin $names) { throw 'Unexpected files in chat package.' }
            $fileCount++
        }
    }
    if ($fileCount -ne $names.Count + 1) { throw 'Unexpected files in chat package.' }
}

function New-ChatPackage {
    <# .SYNOPSIS
    Assembles reviewed binaries, launch code and caller-reviewed third-party notices.
    #>
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseShouldProcessForStateChangingFunctions', '', Justification = 'Explicit creation of a new package; existing paths are never replaced.')]
    [CmdletBinding()]
    [OutputType([pscustomobject])]
    param([Parameter(Mandatory)][string]$OutputDirectory,
          [Parameter(Mandatory)][ValidatePattern('^[0-9]+\.[0-9]+\.[0-9]+$')][string]$Version,
          [Parameter(Mandatory)][ValidatePattern('^[a-f0-9]{40}$')][string]$SourceRevision,
          [Parameter(Mandatory)][Collections.IDictionary]$Binary,
          [Parameter(Mandatory)][string]$ThirdPartyNoticePath)
    $output = Resolve-ExternalPath $OutputDirectory
    if (Test-Path -LiteralPath $output) { throw 'Package output already exists.' }
    $names = @(Get-ChatPackageFileName)
    $sources = @{}
    foreach ($name in $names) {
        if ($name.StartsWith('bin/', [StringComparison]::Ordinal)) {
            $key = [IO.Path]::GetFileNameWithoutExtension($name)
            if (-not $Binary.Contains($key)) { throw 'All five package executables are required.' }
            $sources[$name] = Resolve-ExternalPath $Binary[$key]
            Assert-Arm64PE -Path $sources[$name]
        } elseif ($name -ceq 'THIRD-PARTY-NOTICES.txt') {
            $sources[$name] = Resolve-ExternalPath $ThirdPartyNoticePath
        } else { $sources[$name] = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot ('../' + $name))) }
        if (-not [IO.File]::Exists($sources[$name])) { throw "Package input is missing: $name" }
        $length = (Get-Item -LiteralPath $sources[$name]).Length
        $limit = if ($name.StartsWith('bin/', [StringComparison]::Ordinal)) { 512MB } else { 16MB }
        if ($length -le 0 -or $length -gt $limit) { throw "Package input exceeds its byte bounds: $name" }
    }
    $stage = Resolve-ExternalPath ($output + '.stage-' + [guid]::NewGuid().ToString('N'))
    [void][IO.Directory]::CreateDirectory($stage)
    $files = [Collections.Generic.List[object]]::new()
    foreach ($name in $names) {
        $destination = Resolve-ExternalPath (Join-Path $stage $name)
        [void][IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($destination))
        [IO.File]::Copy($sources[$name], $destination, $false)
        $info = Get-Item -LiteralPath $destination
        $files.Add([ordered]@{ name = $name; bytes = $info.Length; sha256 = (Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash.ToLowerInvariant() })
    }
    $manifest = [ordered]@{ schemaVersion = 1; kind = 'bonsai-chat-package'; version = $Version
        sourceRevision = $SourceRevision; target = 'aarch64-pc-windows-msvc'; files = $files.ToArray() }
    $path = Join-Path $stage 'package.json'
    [IO.File]::WriteAllText($path, (ConvertTo-Json -InputObject $manifest -Depth 8), [Text.UTF8Encoding]::new($false))
    $digest = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()
    [void](Get-CheckedChatPackage -PackageDirectory $stage -ManifestSha256 $digest)
    [IO.Directory]::Move($stage, $output)
    return Get-CheckedChatPackage -PackageDirectory $output -ManifestSha256 $digest
}

function Read-ChatPackageSelection {
    <# .SYNOPSIS
    Reads a checked installation pointer without authorizing its asserted digest.
    #>
    [CmdletBinding()]
    [OutputType([Collections.IDictionary])]
    param([Parameter(Mandatory)][string]$InstallationDirectory)
    $selection = Read-ChatPackageJson -Path (Join-Path $InstallationDirectory 'active.json') -Field @('schemaVersion', 'active', 'previous')
    if ($selection.schemaVersion -isnot [long] -or $selection.schemaVersion -ne 1) { throw 'Unsupported package selection.' }
    foreach ($item in @($selection.active, $selection.previous)) {
        if ($null -eq $item) { continue }
        if ($item -isnot [Collections.IDictionary] -or $item.Count -ne 2 -or
            'id' -cnotin $item.Keys -or 'manifestSha256' -cnotin $item.Keys -or
            $item.id -isnot [string] -or $item.id -cnotmatch '^[0-9]+\.[0-9]+\.[0-9]+-[a-f0-9]{40}-[a-f0-9]{64}$' -or
            $item.id.Length -gt 138 -or $item.manifestSha256 -isnot [string] -or
            $item.manifestSha256 -cnotmatch '^[a-f0-9]{64}$' -or -not $item.id.EndsWith('-' + $item.manifestSha256, [StringComparison]::Ordinal)) {
            throw 'Invalid installed package selection.'
        }
    }
    if ($null -eq $selection.active) { throw 'An active package is required.' }
    return $selection
}

function Get-InstalledChatPackage {
    <# .SYNOPSIS
    Verifies the complete active package before returning its launch paths.
    #>
    [CmdletBinding()]
    [OutputType([pscustomobject])]
    param([Parameter(Mandatory)][string]$InstallationDirectory, [switch]$Native)
    $root = Resolve-ExternalPath $InstallationDirectory
    $selection = Read-ChatPackageSelection -InstallationDirectory $root
    if ($Native) {
        $bootstrap = Get-ChatPackageBootstrap -InstallationDirectory $root
        $package = Invoke-ChatPackageInspection -ToolPath $bootstrap.ToolPath -InstallationDirectory $root
        if ($package.Directory -cne $bootstrap.Directory -or $package.Id -cne $bootstrap.Id -or
            $package.ManifestSha256 -cne $bootstrap.ManifestSha256) { throw 'The verified installed package selection changed during launch.' }
        return [pscustomobject]$package
    }
    $package = Get-CheckedChatPackage -PackageDirectory (Join-Path -Path $root -ChildPath 'versions' -AdditionalChildPath $selection.active.id) -ManifestSha256 $selection.active.manifestSha256
    if ($package.Id -cne $selection.active.id) { throw 'Selected package identity disagrees with its manifest.' }
    return $package
}

function Get-ChatPackageBootstrap {
    <# .SYNOPSIS
    Checks the selected maintenance tool and its loader inventory before execution.
    #>
    [CmdletBinding()]
    [OutputType([pscustomobject])]
    param([Parameter(Mandatory)][string]$InstallationDirectory)
    $root = Resolve-ExternalPath $InstallationDirectory
    $selection = Read-ChatPackageSelection -InstallationDirectory $root
    # Bootstrap only the selected maintenance executable in PowerShell. Its
    # independently selected manifest digest must be checked before any
    # package executable runs; Rust verifies the complete package afterwards.
    $directory = Resolve-ExternalPath (Join-Path -Path $root -ChildPath 'versions' -AdditionalChildPath $selection.active.id)
    $manifestPath = Resolve-ExternalPath (Join-Path $directory 'package.json')
    $info = Get-Item -LiteralPath $manifestPath
    if ($info.Length -gt 32768) { throw 'Package manifest exceeds its byte limit.' }
    Test-LockedArtifact -Path $manifestPath -Bytes $info.Length -Sha256 $selection.active.manifestSha256
    $manifest = Read-ChatPackageJson -Path $manifestPath -Field @('schemaVersion', 'kind', 'version', 'sourceRevision', 'target', 'files')
    if ($manifest.schemaVersion -isnot [long] -or $manifest.schemaVersion -ne 1 -or
        $manifest.kind -cne 'bonsai-chat-package' -or $manifest.target -cne 'aarch64-pc-windows-msvc' -or
        $manifest.version -isnot [string] -or $manifest.version -cnotmatch '^[0-9]+\.[0-9]+\.[0-9]+$' -or $manifest.version.Length -gt 32 -or
        $manifest.sourceRevision -isnot [string] -or $manifest.sourceRevision -cnotmatch '^[a-f0-9]{40}$' -or
        $manifest.files -isnot [array] -or
        ($manifest.version + '-' + $manifest.sourceRevision + '-' + $selection.active.manifestSha256) -cne $selection.active.id) {
        throw 'Unsupported installed chat package identity.'
    }
    Assert-ChatPackageInventory -Directory $directory
    $tools = @($manifest.files | Where-Object { $_ -is [Collections.IDictionary] -and $_.Contains('name') -and $_.name -ceq 'bin/bonsai-tools.exe' })
    if ($tools.Count -ne 1 -or $tools[0].Count -ne 3 -or 'bytes' -cnotin $tools[0].Keys -or 'sha256' -cnotin $tools[0].Keys -or
        $tools[0].bytes -isnot [long] -or $tools[0].bytes -le 0 -or $tools[0].bytes -gt 512MB -or
        $tools[0].sha256 -isnot [string] -or $tools[0].sha256 -cnotmatch '^[a-f0-9]{64}$') { throw 'Invalid packaged maintenance executable identity.' }
    $tool = Resolve-ExternalPath (Join-Path $directory 'bin/bonsai-tools.exe')
    Test-LockedArtifact -Path $tool -Bytes $tools[0].bytes -Sha256 $tools[0].sha256
    Assert-Arm64PE -Path $tool
    return [pscustomobject]@{ Directory = $directory; Id = $selection.active.id
        ManifestSha256 = $selection.active.manifestSha256; ToolPath = $tool; InstallationDirectory = $root }
}

function Invoke-ChatPackageInspection {
    <# .SYNOPSIS
    Invokes an already verified package tool before loading other package modules.
    #>
    [CmdletBinding()]
    [OutputType([Collections.IDictionary])]
    param([Parameter(Mandatory)][string]$ToolPath, [Parameter(Mandatory)][string]$InstallationDirectory)
    return Invoke-ChatPackageCommand -ToolPath $ToolPath -Request @{ operation = 'installed'; installationDirectory = $InstallationDirectory } -TimeoutSeconds 120
}

function Start-InstalledChatPackage {
    <# .SYNOPSIS
    Opens a verified installed app through one native verification and launch command.
    #>
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseShouldProcessForStateChangingFunctions', '', Justification = 'Explicit synchronous app launch; native Rust owns admission and process lifetime.')]
    [CmdletBinding()]
    [OutputType([Collections.IDictionary], [hashtable])]
    param([Parameter(Mandatory)][string]$InstallationDirectory,
          [Parameter(Mandatory)][Collections.IDictionary]$Options)
    # Return before package hashing for an existing instance. Rust repeats and
    # owns admission across the actual UI lifetime; this probe holds no lease.
    $instanceDirectory = Resolve-ExternalPath -Path (Join-Path $env:LOCALAPPDATA 'SwiftBonsai2')
    [void][IO.Directory]::CreateDirectory($instanceDirectory)
    $instancePath = Resolve-ExternalPath -Path (Join-Path $instanceDirectory 'chat.lock')
    try {
        $probe = [IO.FileStream]::new($instancePath, [IO.FileMode]::OpenOrCreate,
            [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
        $probe.Dispose()
    } catch [IO.IOException] {
        if (($_.Exception.HResult -band 0xffff) -notin @(32, 33)) { throw }
        return @{ Status = 'already-running'; Message = 'Swift Bonsai 2 is already starting or running. No second instance was opened.' }
    }
    $bootstrap = Get-ChatPackageBootstrap -InstallationDirectory $InstallationDirectory
    $process = [Diagnostics.Process]::GetCurrentProcess()
    try {
        $request = @{ operation = 'launch'; installationDirectory = $bootstrap.InstallationDirectory
            expectedDirectory = $bootstrap.Directory; expectedId = $bootstrap.Id
            expectedManifestSha256 = $bootstrap.ManifestSha256; options = $Options
            parentProcessId = $PID; parentCreationTimeFileTime = $process.StartTime.ToUniversalTime().ToFileTimeUtc() }
    } finally { $process.Dispose() }
    return Invoke-ChatPackageCommand -ToolPath $bootstrap.ToolPath -Request $request -TimeoutSeconds 0
}

function Invoke-ChatPackageCommand {
    <# .SYNOPSIS
    Invokes the trusted native package command with external process storage.
    #>
    [CmdletBinding()]
    [OutputType([Collections.IDictionary])]
    param([Parameter(Mandatory)][string]$ToolPath,
          [Parameter(Mandatory)][Collections.IDictionary]$Request,
          [Parameter(Mandatory)][ValidateRange(0,86400)][int]$TimeoutSeconds)
    $scratch = Resolve-ExternalPath (Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-Package-' + [guid]::NewGuid().ToString('N')))
    [void][IO.Directory]::CreateDirectory($scratch)
    $requestPath = Join-Path $scratch 'request.json'
    [IO.File]::WriteAllText($requestPath, (ConvertTo-Json -Compress -Depth 16 -InputObject $Request), [Text.UTF8Encoding]::new($false))
    $start = [Diagnostics.ProcessStartInfo]::new($ToolPath)
    $start.WorkingDirectory = $scratch
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    foreach ($argument in @('chat-package', '--request', $requestPath)) { $start.ArgumentList.Add($argument) }
    $process = [Diagnostics.Process]::new()
    $process.StartInfo = $start
    $started = $false
    try {
        if (-not $process.Start()) { throw 'Packaged maintenance inspection did not start.' }
        $started = $true
        $stdout = $process.StandardOutput.ReadToEndAsync()
        $stderr = $process.StandardError.ReadToEndAsync()
        if ($TimeoutSeconds -eq 0) { $process.WaitForExit() }
        elseif (-not $process.WaitForExit($TimeoutSeconds * 1000)) {
            $process.Kill($true)
            [void]$process.WaitForExit(10000)
            throw "Packaged maintenance inspection timed out. Evidence: $scratch"
        }
        if (-not $stdout.Wait(10000) -or -not $stderr.Wait(10000)) { throw "Packaged maintenance output timed out. Evidence: $scratch" }
        if ($process.ExitCode) { throw "Installed package verification failed: $($stderr.GetAwaiter().GetResult().Trim()) Evidence: $scratch" }
        $result = ConvertFrom-Json -InputObject $stdout.GetAwaiter().GetResult() -AsHashtable -NoEnumerate -DateKind String -Depth 16
        if ($result -isnot [Collections.IDictionary]) { throw 'The package verifier returned no JSON object.' }
        [IO.File]::Delete($requestPath)
        [IO.Directory]::Delete($scratch, $false)
        return $result
    } finally {
        try {
            if ($started -and -not $process.HasExited) {
                $process.Kill($true)
                [void]$process.WaitForExit(10000)
            }
        } finally { $process.Dispose() }
    }
}

function Set-ChatPackage {
    <# .SYNOPSIS
    Installs and atomically activates a checked package, or rolls back explicitly.
    .DESCRIPTION
    A single-writer lock serializes install/update/rollback. Each version is copied
    to unique staging and verified before publication. Older versions are retained.
    Failure before the atomic selection write preserves the previous selection.
    No package code executes, and model files or recovery data are never changed.
    #>
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseShouldProcessForStateChangingFunctions', '', Justification = 'Explicit install/activation command; immutable versions and rollback are retained.')]
    [CmdletBinding(DefaultParameterSetName = 'Install')]
    [OutputType([pscustomobject])]
    param([Parameter(Mandatory)][string]$InstallationDirectory,
          [Parameter(Mandatory, ParameterSetName = 'Install')][string]$PackageDirectory,
          [Parameter(Mandatory, ParameterSetName = 'Install')][ValidatePattern('^[a-f0-9]{64}$')][string]$ManifestSha256,
          [Parameter(Mandatory, ParameterSetName = 'Rollback')][switch]$Rollback)
    $root = Resolve-ExternalPath $InstallationDirectory
    [void][IO.Directory]::CreateDirectory($root)
    $lockPath = Resolve-ExternalPath (Join-Path $root 'package.lock')
    try { $guard = [IO.File]::Open($lockPath, [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None) }
    catch { throw 'Another package operation owns this installation.' }
    try {
        $pointer = Resolve-ExternalPath (Join-Path $root 'active.json')
        $old = if ([IO.File]::Exists($pointer)) { Read-ChatPackageSelection -InstallationDirectory $root } else { $null }
        if ($Rollback) {
            if ($null -eq $old -or $null -eq $old.previous) { throw 'No previous package is available for rollback.' }
            $package = Get-CheckedChatPackage -PackageDirectory (Join-Path -Path $root -ChildPath 'versions' -AdditionalChildPath $old.previous.id) -ManifestSha256 $old.previous.manifestSha256
            if ($package.Id -cne $old.previous.id) { throw 'Previous package identity disagrees with its manifest.' }
        } else {
            $source = Get-CheckedChatPackage -PackageDirectory $PackageDirectory -ManifestSha256 $ManifestSha256
            $versions = Resolve-ExternalPath (Join-Path $root 'versions')
            [void][IO.Directory]::CreateDirectory($versions)
            $destination = Resolve-ExternalPath (Join-Path $versions $source.Id)
            if (-not [IO.Directory]::Exists($destination)) {
                $stage = Resolve-ExternalPath (Join-Path $versions ('stage-' + [guid]::NewGuid().ToString('N')))
                [void][IO.Directory]::CreateDirectory($stage)
                foreach ($name in (@(Get-ChatPackageFileName) + @('package.json'))) {
                    $path = Resolve-ExternalPath (Join-Path $stage $name)
                    [void][IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($path))
                    [IO.File]::Copy((Resolve-ExternalPath (Join-Path $source.Directory $name)), $path, $false)
                }
                [void](Get-CheckedChatPackage -PackageDirectory $stage -ManifestSha256 $ManifestSha256)
                [IO.Directory]::Move($stage, $destination)
            }
            $package = Get-CheckedChatPackage -PackageDirectory $destination -ManifestSha256 $ManifestSha256
        }
        if ($null -ne $old -and $old.active.id -ceq $package.Id) { return $package }
        $selection = [ordered]@{ schemaVersion = 1; active = @{ id = $package.Id; manifestSha256 = $package.ManifestSha256 }
            previous = $(if ($null -ne $old) { $old.active } else { $null }) }
        $temporary = Resolve-ExternalPath (Join-Path $root ('selection-' + [guid]::NewGuid().ToString('N') + '.json'))
        $stream = [IO.File]::Open($temporary, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
        try {
            $bytes = [Text.UTF8Encoding]::new($false).GetBytes((ConvertTo-Json -InputObject $selection -Depth 8))
            $stream.Write($bytes); $stream.Flush($true)
        } finally { $stream.Dispose() }
        if ([IO.File]::Exists($pointer)) { [IO.File]::Replace($temporary, $pointer, [NullString]::Value) }
        else { [IO.File]::Move($temporary, $pointer) }
        return $package
    } finally { $guard.Dispose() }
}

Export-ModuleMember -Function New-ChatPackage, Get-CheckedChatPackage, Get-InstalledChatPackage, Set-ChatPackage, Start-InstalledChatPackage
