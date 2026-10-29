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
    Cold-bootstraps Cargo once; Rust publishes checked immutable launch tools.
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
    $key = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($identity))).ToLowerInvariant().Substring(0, 24)
    $target = Resolve-ExternalPath -Path (Join-Path $script:ToolCacheRoot $key)
    $scratch = Get-BonsaiToolScratch -Name $Name
    $receiptPath = Resolve-ExternalPath (Join-Path $target 'bootstrap.json')
    $version = $null
    $snapshot = $null
    try {
        if ([IO.File]::Exists($receiptPath)) {
            if ((Get-Item -LiteralPath $receiptPath).Length -gt 32768) { throw 'Maintenance bootstrap receipt exceeds its byte limit.' }
            $receipt = ConvertFrom-Json -InputObject ([IO.File]::ReadAllText($receiptPath)) -AsHashtable -NoEnumerate -DateKind String
            if ($receipt -isnot [Collections.IDictionary] -or $receipt.Count -ne 3 -or
                -not $receipt.Contains('schemaVersion') -or $receipt.schemaVersion -ne 1 -or
                -not $receipt.Contains('sha256') -or $receipt.sha256 -isnot [string] -or $receipt.sha256 -cnotmatch '^[a-f0-9]{64}$' -or
                -not $receipt.Contains('version') -or $receipt.version -isnot [Collections.IDictionary]) { throw 'Invalid maintenance bootstrap receipt.' }
            $digest = $receipt.sha256
            $snapshot = Resolve-ExternalPath (Join-Path -Path $target -ChildPath 'bootstrap' -AdditionalChildPath ($digest + '.exe'))
            if (-not [IO.File]::Exists($snapshot) -or (Get-FileHash -LiteralPath $snapshot -Algorithm SHA256).Hash -ine $digest) {
                throw 'The maintenance bootstrap snapshot changed.'
            }
            # Rust published these capabilities after verifying this exact digest.
            # Arbitrary user-selected tools still receive the ordinary version probe.
            $version = $receipt.version
            if (-not $version.Contains('BootstrapProtocolVersion') -or $version.BootstrapProtocolVersion -ne 1 -or
                -not $version.Contains('Name') -or $version.Name -cne 'bonsai-tools' -or
                -not $version.Contains('Version') -or $version.Version -cne '0.1.0' -or
                -not $version.Contains('ProtocolVersion') -or $version.ProtocolVersion -ne 5 -or
                -not $version.Contains('StartupProtocolVersion') -or $version.StartupProtocolVersion -ne 1 -or
                -not $version.Contains('Commands') -or $version.Commands -isnot [array] -or
                @($version.Commands | Where-Object { $_ -isnot [string] }).Count -ne 0 -or 'source-build' -cnotin $version.Commands) {
                throw 'Use a current maintenance bootstrap receipt with bootstrap protocol 1.'
            }
        }
    } catch {
        if (-not $ForceBuild) { throw }
        $snapshot = $null
        $version = $null
    }
    if ($null -eq $snapshot) {
        # Without a trusted Rust receipt, PowerShell performs only the unavoidable
        # initial compiler bootstrap. Subsequent builds and publication are Rust.
        [void][IO.Directory]::CreateDirectory($target)
        try { $guard = [IO.FileStream]::new((Join-Path $target 'build.lock'), [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None) }
        catch { throw 'Another process is preparing this maintenance tool. Try again after it finishes.' }
        try {
            $tool = Resolve-ExternalPath (Join-Path $target 'release/bonsai-tools.exe')
            if ([IO.File]::Exists($tool)) { [IO.File]::Delete($tool) }
            $cargo = (Get-Command cargo -CommandType Application -ErrorAction Stop).Source
            $arguments = @("+$channel", 'build', '--locked', '--release', '--manifest-path', (Join-Path $repository 'tools/bonsai/Cargo.toml'), '--target-dir', $target)
            $log = Join-Path $scratch 'bootstrap-build.log'
            $built = Invoke-BonsaiNativeProcess -Executable $cargo -ArgumentList $arguments -WorkingDirectory $scratch -Environment @{ CARGO_BUILD_FINGERPRINT = 'content'; CARGO_UNSTABLE_CHECKSUM_FRESHNESS = 'true' } -LogPath $log -TimeoutSeconds 3600
            if ($built.ExitCode) { throw "Maintenance bootstrap failed; see $log" }
            Assert-Arm64PE -Path $tool
            $digest = (Get-FileHash -LiteralPath $tool -Algorithm SHA256).Hash.ToLowerInvariant()
            $snapshot = Resolve-ExternalPath (Join-Path -Path $target -ChildPath 'bootstrap' -AdditionalChildPath ($digest + '.exe'))
            [void][IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($snapshot))
            if ($ForceBuild -and [IO.File]::Exists($snapshot) -and (Get-FileHash -LiteralPath $snapshot -Algorithm SHA256).Hash -ine $digest) { [IO.File]::Delete($snapshot) }
            if (-not [IO.File]::Exists($snapshot)) { [IO.File]::Copy($tool, $snapshot, $false) }
            if ((Get-FileHash -LiteralPath $snapshot -Algorithm SHA256).Hash -ine $digest) { throw 'The maintenance bootstrap snapshot changed.' }
        } finally { $guard.Dispose() }
    }
    if ($null -ne $version) {
        $file = Get-Item -LiteralPath $snapshot
        $script:CheckedTools[$snapshot + ':' + $file.Length + ':' + $file.LastWriteTimeUtc.Ticks] = $version
    }
    $resolved = Invoke-BonsaiTool -Command source-build -ToolPath $snapshot -TimeoutSeconds 0 -Request @{
        operation = 'tool'; repositoryRoot = $repository; workingDirectory = $scratch;
        targetDirectory = $target; forceBuild = [bool]$ForceBuild; publishBootstrap = $true
    }
    if ($resolved.ToolPath -cne (Join-Path $target 'release/bonsai-tools.exe') -or
        $resolved.ToolSha256 -isnot [string] -or $resolved.ToolSha256 -cnotmatch '^[a-f0-9]{64}$') { throw 'The maintenance build returned an unexpected executable identity.' }
    $checked = Resolve-ExternalPath (Join-Path -Path $target -ChildPath 'bootstrap' -AdditionalChildPath ($resolved.ToolSha256 + '.exe'))
    if ($resolved.BootstrapPath -cne $checked) { throw 'The maintenance build returned an unexpected bootstrap path.' }
    # The native source-build operation checked the published image and performed
    # a fresh version handshake whenever the build changed its bytes.
    $file = Get-Item -LiteralPath $checked
    $script:CheckedTools[$checked + ':' + $file.Length + ':' + $file.LastWriteTimeUtc.Ticks] = $resolved.Version
    return $checked
}
function Invoke-BonsaiTool {
    <# .SYNOPSIS
    Runs one bounded JSON maintenance operation with a packaged or source-built tool.
    #>
    [CmdletBinding()]
    [OutputType([Collections.IDictionary])]
    param([Parameter(Mandatory)][ValidateSet('credentials','memory','host-cpu','cpu-evaluation','cpu-check-plan','cpu-check-report','server-policy','runtime-stage','runtime-verify','completion','qualification','runtime-promotion','speech-models','inference-lock','inference-capabilities','speech-build','speech-evaluation','source-build','chat-launch')][string]$Command,
          [Collections.IDictionary]$Request = @{}, [string]$ToolPath,
          [ValidateRange(0,86400)][int]$TimeoutSeconds = 60)
    if (-not $ToolPath -and $env:BONSAI_TOOLS_PATH) { $ToolPath = $env:BONSAI_TOOLS_PATH }
    # The supervisor owns each sample deadline; hashing precedes that deadline
    # and paired captures can contain many ten-minute samples.
    if ($Command -eq 'completion' -and -not $PSBoundParameters.ContainsKey('TimeoutSeconds')) { $TimeoutSeconds = 0 }
    if ($Command -eq 'speech-evaluation' -and $Request['operation'] -eq 'session' -and -not $PSBoundParameters.ContainsKey('TimeoutSeconds')) { $TimeoutSeconds = 0 }
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
    if ($Command -ceq 'chat-launch' -and (-not $version.Contains('ChatLaunchProtocolVersion') -or $version.ChatLaunchProtocolVersion -ne 1)) {
        throw 'Use a current bonsai-tools build with native chat admission protocol 1.'
    }
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
        ($Command -eq 'speech-evaluation' -and $Request['operation'] -eq 'session') -or
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
