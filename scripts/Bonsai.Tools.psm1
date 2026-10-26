#requires -Version 7.6
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1')
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Native.psm1')

$script:ToolScratch = @{}
$script:CheckedTools = @{}
$script:ToolCacheRoot = Join-Path $env:LOCALAPPDATA 'SwiftBonsai2/build/tools'

function Get-BonsaiToolScratch {
    <# .SYNOPSIS
    Returns the retained external diagnostics lane for one tool in this session.
    #>
    [CmdletBinding()]
    [OutputType([string])]
    param([Parameter(Mandatory)][ValidatePattern('^[a-z-]+$')][string]$Name)
    if (-not $script:ToolScratch.ContainsKey($Name)) {
        $path = Resolve-ExternalPath (Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-' + $Name + '-' + [guid]::NewGuid().ToString('N')))
        [void][IO.Directory]::CreateDirectory($path)
        $script:ToolScratch[$Name] = $path
    }
    return $script:ToolScratch[$Name]
}

function Resolve-BonsaiRustTool {
    <# .SYNOPSIS
    Bootstraps the maintenance CLI, then delegates source freshness/builds to Rust.
    #>
    [CmdletBinding()]
    [OutputType([string])]
    param([Parameter(Mandatory)][ValidatePattern('^[a-z-]+$')][string]$Name,
          [Parameter(Mandatory)][string]$ManifestRelativePath, [string]$ToolPath, [switch]$ForceBuild)
    if ($ToolPath) {
        $path = Resolve-ExternalPath -Path $ToolPath
        if (-not [IO.File]::Exists($path)) { throw 'ToolPath must name an existing expanded absolute executable.' }
        return $path
    }
    if ($Name -cne 'bonsai-tools' -or $ManifestRelativePath -cne '../tools/bonsai/Cargo.toml') { throw 'Unsupported maintenance bootstrap.' }
    $repository = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
    $pin = [IO.File]::ReadAllText((Join-Path $repository 'rust-toolchain.toml'))
    if ($pin -notmatch '(?m)^channel\s*=\s*"(nightly-\d{4}-\d{2}-\d{2})"\s*$') { throw 'Use the repository-pinned Rust toolchain.' }
    $channel = $Matches[1] + '-aarch64-pc-windows-msvc'
    $identity = (Join-Path $repository 'tools/bonsai/Cargo.toml').ToUpperInvariant() + "`n" + $channel
    $key = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($identity))).ToLowerInvariant()
    $target = Resolve-ExternalPath -Path (Join-Path $script:ToolCacheRoot $key)
    [void][IO.Directory]::CreateDirectory($target)
    $scratch = Get-BonsaiToolScratch -Name $Name
    $tool = Resolve-ExternalPath -Path (Join-Path $target 'release/bonsai-tools.exe')
    $receipt = Resolve-ExternalPath -Path (Join-Path $target 'bonsai-tools.sha256')
    try { $guard = [IO.FileStream]::new((Join-Path $target 'build.lock'), [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None) }
    catch { throw 'Another process is preparing this maintenance tool. Try again after it finishes.' }
    try {
        $expected = if ([IO.File]::Exists($receipt)) { [IO.File]::ReadAllText($receipt).Trim() } else { '' }
        $digest = if ([IO.File]::Exists($tool)) { (Get-FileHash -LiteralPath $tool -Algorithm SHA256).Hash.ToLowerInvariant() } else { '' }
        $compatible = $false
        if ($expected -cmatch '^[a-f0-9]{64}$' -and $digest -ceq $expected) {
            $probe = Invoke-BonsaiNativeProcess -Executable $tool -ArgumentList @('--version') -WorkingDirectory $scratch -TimeoutSeconds 10
            if (-not $probe.ExitCode) {
                $version = $probe.StandardOutput | ConvertFrom-Json -AsHashtable -NoEnumerate -DateKind String
                $compatible = $version.Contains('StartupProtocolVersion') -and $version.StartupProtocolVersion -eq 1 -and
                    $version.Contains('Commands') -and 'source-build' -cin $version.Commands
            }
        }
        if (-not $compatible) {
            if ([IO.File]::Exists($tool)) { [IO.File]::Delete($tool) }
            $cargo = (Get-Command cargo -CommandType Application -ErrorAction Stop).Source
            $arguments = @("+$channel", 'build', '--locked', '--release', '--manifest-path', (Join-Path $repository 'tools/bonsai/Cargo.toml'), '--target-dir', $target)
            $log = Join-Path $scratch 'bootstrap-build.log'
            $built = Invoke-BonsaiNativeProcess -Executable $cargo -ArgumentList $arguments -WorkingDirectory $scratch -Environment @{ CARGO_BUILD_FINGERPRINT = 'content'; CARGO_UNSTABLE_CHECKSUM_FRESHNESS = 'true' } -LogPath $log -TimeoutSeconds 3600
            if ($built.ExitCode) { throw "Maintenance bootstrap failed; see $log" }
            Assert-Arm64PE -Path $tool
            $digest = (Get-FileHash -LiteralPath $tool -Algorithm SHA256).Hash.ToLowerInvariant()
            [IO.File]::WriteAllText($receipt, $digest)
        }
        # Windows cannot replace an executing EXE. Immutable snapshots let Rust
        # rebuild the source CLI without locking its Cargo output at launch.
        $snapshot = Resolve-ExternalPath -Path (Join-Path -Path $target -ChildPath 'bootstrap' -AdditionalChildPath ($digest + '.exe'))
        [void][IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($snapshot))
        if (-not [IO.File]::Exists($snapshot)) { [IO.File]::Copy($tool, $snapshot, $false) }
        if ((Get-FileHash -LiteralPath $snapshot -Algorithm SHA256).Hash -ine $digest) { throw 'The maintenance bootstrap snapshot changed.' }
    } finally { $guard.Dispose() }
    $resolved = Invoke-BonsaiTool -Command source-build -ToolPath $snapshot -TimeoutSeconds 0 -Request @{
        operation = 'tool'; repositoryRoot = $repository; workingDirectory = $scratch;
        targetDirectory = $target; forceBuild = [bool]$ForceBuild
    }
    if ($resolved.ToolPath -cne $tool) { throw 'The maintenance build returned an unexpected executable path.' }
    [IO.File]::WriteAllText($receipt, (Get-FileHash -LiteralPath $tool -Algorithm SHA256).Hash.ToLowerInvariant())
    return $tool
}
function Invoke-BonsaiTool {
    <# .SYNOPSIS
    Runs one bounded JSON maintenance operation with a packaged or source-built tool.
    #>
    [CmdletBinding()]
    [OutputType([Collections.IDictionary])]
    param([Parameter(Mandatory)][ValidateSet('credentials','memory','cpu-check-plan','cpu-check-report','server-policy','runtime-stage','runtime-verify','completion','qualification','speech-models','inference-lock','inference-capabilities','speech-build','source-build','chat-launch')][string]$Command,
          [Collections.IDictionary]$Request = @{}, [string]$ToolPath,
          [ValidateRange(0,86400)][int]$TimeoutSeconds = 60)
    if (-not $ToolPath -and $env:BONSAI_TOOLS_PATH) { $ToolPath = $env:BONSAI_TOOLS_PATH }
    # The supervisor owns each sample deadline; hashing precedes that deadline
    # and paired captures can contain many ten-minute samples.
    if ($Command -eq 'completion' -and -not $PSBoundParameters.ContainsKey('TimeoutSeconds')) { $TimeoutSeconds = 0 }
    if ($Command -in @('runtime-stage','runtime-verify','inference-capabilities') -and -not $PSBoundParameters.ContainsKey('TimeoutSeconds')) { $TimeoutSeconds = 120 }
    $tool = Resolve-BonsaiRustTool -Name 'bonsai-tools' -ManifestRelativePath '../tools/bonsai/Cargo.toml' -ToolPath $ToolPath
    $scratch = Get-BonsaiToolScratch -Name 'bonsai-tools'
    $file = Get-Item -LiteralPath $tool
    $identity = $tool + ':' + $file.Length + ':' + $file.LastWriteTimeUtc.Ticks
    if (-not $script:CheckedTools.ContainsKey($identity)) {
        $probe = Invoke-BonsaiNativeProcess -Executable $tool -ArgumentList @('--version') -WorkingDirectory $scratch -TimeoutSeconds 10
        if ($probe.ExitCode -ne 0) { throw 'The maintenance tool version probe failed.' }
        $version = $probe.StandardOutput | ConvertFrom-Json -AsHashtable -NoEnumerate -DateKind String
        if ($version -isnot [Collections.IDictionary] -or $version.Name -cne 'bonsai-tools' -or $version.Version -cne '0.1.0' -or $version.ProtocolVersion -ne 5 -or
            -not $version.Contains('Commands') -or $version.Commands -isnot [array] -or
            @($version.Commands | Where-Object { $_ -isnot [string] }).Count) {
            throw 'Use a compatible bonsai-tools executable (version 0.1.0, protocol 5).'
        }
        $script:CheckedTools[$identity] = $version
    }
    $version = $script:CheckedTools[$identity]
    if ($Command -cnotin $version.Commands) { throw "The selected maintenance tool does not support '$Command'; use a current build." }
    $startup = $Command -cin @('source-build', 'chat-launch') -or
        ($Command -ceq 'speech-build' -and $Request['operation'] -cin @('models', 'environment', 'build'))
    if ($startup -and (-not $version.Contains('StartupProtocolVersion') -or $version.StartupProtocolVersion -ne 1)) {
        throw 'Use a current bonsai-tools build with startup protocol 1 for cancellable startup operations.'
    }
    $lane = Join-Path $scratch ([guid]::NewGuid().ToString('N'))
    [void][IO.Directory]::CreateDirectory($lane)
    $requestPath = Join-Path $lane 'request.json'
    $payload = [ordered]@{}
    foreach ($key in $Request.Keys) { $payload[$key] = $Request[$key] }
    if ($startup -or $Command -in @('completion','runtime-verify','inference-capabilities') -or
        ($Command -eq 'qualification' -and $Request['operation'] -in @('capture', 'screen-capture'))) {
        $currentProcess = [Diagnostics.Process]::GetCurrentProcess()
        try {
            $payload['parentProcessId'] = $PID
            $payload['parentCreationTimeFileTime'] = $currentProcess.StartTime.ToUniversalTime().ToFileTimeUtc()
        } finally { $currentProcess.Dispose() }
    }
    $json = ConvertTo-Json -InputObject $payload -Depth 64 -Compress
    [IO.File]::WriteAllText($requestPath, $json, [Text.UTF8Encoding]::new($false))
    $result = Invoke-BonsaiNativeProcess -Executable $tool -ArgumentList @($Command, '--request', $requestPath) -WorkingDirectory $lane -TimeoutSeconds $TimeoutSeconds
    if ($result.ExitCode -ne 0) { throw "Maintenance command '$Command' failed: $($result.StandardError.Trim()) Evidence: $lane" }
    if ($result.StandardError) { Write-Verbose -Message $result.StandardError.TrimEnd() }
    $value = ConvertFrom-Json -InputObject $result.StandardOutput -AsHashtable -NoEnumerate -DateKind String -Depth 64
    if ($value -isnot [Collections.IDictionary]) { throw 'The maintenance tool returned no JSON object.' }
    [IO.File]::Delete($requestPath)
    [IO.Directory]::Delete($lane, $false)
    return $value
}

Export-ModuleMember -Function Get-BonsaiToolScratch, Resolve-BonsaiRustTool, Invoke-BonsaiTool
