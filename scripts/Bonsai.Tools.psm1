#requires -Version 7.6
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1')
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Native.psm1')
Import-Module (Join-Path $PSScriptRoot 'Bonsai.BuildCache.psm1')

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
    Resolves a packaged tool or builds reviewed sources in a reusable external cache.
    .DESCRIPTION
    Unchanged build inputs and checked outputs bypass Cargo on source resolution.
    Changed inputs or ForceBuild run Cargo's ordinary freshness checks.
    Launchers can resolve once and pass ToolPath to subsequent operations.
    The output digest receipt detects corruption, not same-user tampering.
    #>
    [CmdletBinding()]
    [OutputType([string])]
    param([Parameter(Mandatory)][ValidatePattern('^[a-z-]+$')][string]$Name,
          [Parameter(Mandatory)][string]$ManifestRelativePath, [string]$ToolPath, [switch]$ForceBuild)
    if ($ToolPath) {
        if (-not [IO.Path]::IsPathFullyQualified($ToolPath) -or $ToolPath -match '%[^%]+%' -or -not [IO.File]::Exists($ToolPath)) {
            throw 'ToolPath must name an existing expanded absolute executable.'
        }
        return [IO.Path]::GetFullPath($ToolPath)
    }
    $scratch = Get-BonsaiToolScratch -Name $Name
    $manifest = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot $ManifestRelativePath))
    $repository = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
    if (-not $manifest.StartsWith($repository + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
        throw 'Tool manifests must remain inside the source repository.'
    }
    foreach ($inputName in @('Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml')) {
        if (-not [IO.File]::Exists((Join-Path $repository $inputName))) { throw "Tool build input is missing: $inputName" }
    }
    $toolchain = [IO.File]::ReadAllText((Join-Path $repository 'rust-toolchain.toml'))
    if ($toolchain -notmatch '(?m)^channel\s*=\s*"(nightly-\d{4}-\d{2}-\d{2})"\s*$') {
        throw 'Tool builds require the repository-pinned Rust toolchain.'
    }
    $channel = $Matches[1] + '-aarch64-pc-windows-msvc'
    $identity = $manifest.ToUpperInvariant() + "`n" + $channel
    $key = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($identity))).ToLowerInvariant()
    $target = Resolve-ExternalPath -Path (Join-Path $script:ToolCacheRoot $key)
    [void][IO.Directory]::CreateDirectory($target)
    $tool = Resolve-ExternalPath -Path (Join-Path $target ('release/' + $Name + '.exe'))
    $receipt = Resolve-ExternalPath -Path (Join-Path $target ($Name + '.sha256'))
    $lock = Resolve-ExternalPath -Path (Join-Path $target 'build.lock')
    try { $guard = [IO.FileStream]::new($lock, [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None) }
    catch { throw 'Another process is preparing this maintenance tool. Wait for it to finish, then retry.' }
    try {
        $fingerprint = Get-BonsaiBuildFingerprint -RepositoryRoot $repository -Identity $identity -WorkingDirectory $scratch
        $freshness = Join-Path $target ($Name + '.build.json')
        $expected = if ([IO.File]::Exists($receipt)) { [IO.File]::ReadAllText($receipt).Trim() } else { '' }
        $actual = if ([IO.File]::Exists($tool)) { (Get-FileHash -LiteralPath $tool -Algorithm SHA256).Hash } else { '' }
        $valid = $expected -match '^[a-fA-F0-9]{64}$' -and $actual -ieq $expected
        if (-not $valid -and [IO.File]::Exists($tool)) {
            # This fixed output beneath the validated external cache is disposable.
            # Removing it forces Cargo to produce the executable again.
            Remove-Item -LiteralPath $tool -Force
        }
        if (-not $ForceBuild -and $valid -and (Test-BonsaiBuildReceipt -ReceiptPath $freshness -Fingerprint $fingerprint -Executable @($tool))) {
            return $tool
        }
        [IO.File]::Delete($freshness)
        $cargo = (Get-Command -Name cargo -CommandType Application -ErrorAction Stop).Source
        $arguments = @("+$channel", 'build', '--locked', '--release', '--manifest-path', $manifest, '--target-dir', $target)
        $log = Join-Path $scratch ('cargo-build-' + [guid]::NewGuid().ToString('N') + '.log')
        $result = Invoke-BonsaiNativeProcess -Executable $cargo -ArgumentList $arguments -WorkingDirectory $scratch -Environment @{ CARGO_BUILD_FINGERPRINT = 'content'; CARGO_UNSTABLE_CHECKSUM_FRESHNESS = 'true' } -LogPath $log
        if ($result.ExitCode -ne 0) { throw "Tool build failed; inspect $log`n$($result.StandardError)" }
        if (-not [IO.File]::Exists($tool)) { throw "Tool build produced no executable; inspect $log" }
        $digest = (Get-FileHash -LiteralPath $tool -Algorithm SHA256).Hash.ToLowerInvariant()
        [IO.File]::WriteAllText($receipt, $digest)
        if ((Get-BonsaiBuildFingerprint -RepositoryRoot $repository -Identity $identity -WorkingDirectory $scratch) -ceq $fingerprint) {
            Write-BonsaiBuildReceipt -ReceiptPath $freshness -Fingerprint $fingerprint -Executable @($tool)
        }
        return $tool
    } finally { $guard.Dispose() }
}

function Invoke-BonsaiTool {
    <# .SYNOPSIS
    Runs one bounded JSON maintenance operation with a packaged or source-built tool.
    #>
    [CmdletBinding()]
    [OutputType([Collections.IDictionary])]
    param([Parameter(Mandatory)][ValidateSet('credentials','memory','cpu-check-plan','cpu-check-report','server-policy','runtime-stage','runtime-verify','completion','qualification','speech-models','inference-lock','inference-capabilities','speech-build')][string]$Command,
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
        $script:CheckedTools[$identity] = $version.Commands
    }
    if ($Command -cnotin $script:CheckedTools[$identity]) { throw "The selected maintenance tool does not support '$Command'; use a current build." }
    $lane = Join-Path $scratch ([guid]::NewGuid().ToString('N'))
    [void][IO.Directory]::CreateDirectory($lane)
    $requestPath = Join-Path $lane 'request.json'
    $payload = [ordered]@{}
    foreach ($key in $Request.Keys) { $payload[$key] = $Request[$key] }
    if ($Command -in @('completion','runtime-verify','inference-capabilities') -or
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
