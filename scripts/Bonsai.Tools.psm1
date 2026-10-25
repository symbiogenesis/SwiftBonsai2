#requires -Version 7.6
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1')
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Native.psm1')

$script:ToolScratch = @{}
$script:CheckedTools = @{}

function Get-BonsaiToolScratch {
    <# .SYNOPSIS
    Returns the retained external incremental build lane for one tool in this session.
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
    Resolves a packaged tool or builds reviewed sources in an external incremental lane.
    #>
    [CmdletBinding()]
    [OutputType([string])]
    param([Parameter(Mandatory)][ValidatePattern('^[a-z-]+$')][string]$Name,
          [Parameter(Mandatory)][string]$ManifestRelativePath, [string]$ToolPath)
    if ($ToolPath) {
        if (-not [IO.Path]::IsPathFullyQualified($ToolPath) -or $ToolPath -match '%[^%]+%' -or -not [IO.File]::Exists($ToolPath)) {
            throw 'ToolPath must name an existing expanded absolute executable.'
        }
        return [IO.Path]::GetFullPath($ToolPath)
    }
    $scratch = Get-BonsaiToolScratch -Name $Name
    $cargo = (Get-Command -Name cargo -CommandType Application -ErrorAction Stop).Source
    $manifest = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot $ManifestRelativePath))
    $target = Join-Path $scratch 'target'
    $arguments = @('+nightly-2026-10-22-aarch64-pc-windows-msvc', 'build', '--locked', '--release', '--manifest-path', $manifest, '--target-dir', $target)
    $log = Join-Path $scratch ('cargo-build-' + [guid]::NewGuid().ToString('N') + '.log')
    $result = Invoke-BonsaiNativeProcess -Executable $cargo -ArgumentList $arguments -WorkingDirectory $scratch -LogPath $log
    if ($result.ExitCode -ne 0) { throw "Tool build failed; inspect $log`n$($result.StandardError)" }
    $tool = Join-Path $target ('release/' + $Name + '.exe')
    if (-not [IO.File]::Exists($tool)) { throw "Tool build produced no executable; inspect $log" }
    return $tool
}

function Invoke-BonsaiTool {
    <# .SYNOPSIS
    Runs one bounded JSON maintenance operation with a packaged or source-built tool.
    #>
    [CmdletBinding()]
    [OutputType([Collections.IDictionary])]
    param([Parameter(Mandatory)][ValidateSet('credentials','memory','cpu-check-plan','server-policy','runtime-verify','completion','qualification')][string]$Command,
          [Collections.IDictionary]$Request = @{}, [string]$ToolPath,
          [ValidateRange(0,86400)][int]$TimeoutSeconds = 60)
    if (-not $ToolPath -and $env:BONSAI_TOOLS_PATH) { $ToolPath = $env:BONSAI_TOOLS_PATH }
    # The supervisor owns each sample deadline; hashing precedes that deadline
    # and paired captures can contain many ten-minute samples.
    if ($Command -eq 'completion' -and -not $PSBoundParameters.ContainsKey('TimeoutSeconds')) { $TimeoutSeconds = 0 }
    if ($Command -eq 'runtime-verify' -and -not $PSBoundParameters.ContainsKey('TimeoutSeconds')) { $TimeoutSeconds = 120 }
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
    if ($Command -in @('completion','runtime-verify')) {
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
