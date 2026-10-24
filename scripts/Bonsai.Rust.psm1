#requires -Version 7.6
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1')
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Native.psm1')

function Get-BonsaiRustPackage {
    <#
    .SYNOPSIS
    Selects registered Rust packages without invoking any native tools.
    #>
    [CmdletBinding()]
    [OutputType([Collections.IDictionary])]
    param(
        [ValidateSet('All', 'Runtime', 'Broker')][string]$Group = 'All',
        [string[]]$Name = @()
    )
    $registry = Import-PowerShellDataFile (Join-Path $PSScriptRoot '../config/rust-packages.psd1')
    $packages = @($registry.Packages | Where-Object { $Group -eq 'All' -or $_.Group -eq $Group })
    foreach ($selected in $Name) {
        if ($selected -cnotin $packages.Name) { throw "Unknown Rust package in group ${Group}: $selected" }
    }
    $packages | Where-Object { $Name.Count -eq 0 -or $_.Name -cin $Name }
}

function Get-BonsaiRustArgument {
    <#
    .SYNOPSIS
    Builds one package's literal Cargo command with explicit feature selection.
    #>
    [CmdletBinding()]
    [OutputType([string], [array])]
    param(
        [Parameter(Mandatory)][Collections.IDictionary]$Package,
        [string]$Toolchain = 'nightly-2026-10-22-aarch64-pc-windows-msvc',
        [Parameter(Mandatory)][ValidateSet('fmt', 'clippy', 'doc', 'test', 'build')][string]$Phase,
        [ValidateSet('Foundation', 'Compatibility', 'Native')][string]$CheckProfile = 'Foundation',
        [switch]$Offline
    )
    $manifest = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../Cargo.toml'))
    $arguments = @(('+' + $Toolchain), $Phase, '--manifest-path', $manifest, '--package', $Package.Name)
    if ($Phase -eq 'fmt') { return $arguments + '--check' }
    $arguments += @('--locked', '--release')
    if ($Offline) { $arguments += '--offline' }
    $features = @($Package.Features)
    if ($CheckProfile -eq 'Native' -and $Package.Contains('NativeFeatures')) { $features = @($Package.NativeFeatures) }
    elseif ($CheckProfile -eq 'Compatibility' -and $Phase -ne 'test' -and $Package.Contains('CompatibilityCompileFeatures')) {
        $features = @($Package.CompatibilityCompileFeatures)
    }
    if ('*' -in $features) { $arguments += '--all-features' }
    elseif ($features.Count) { $arguments += @('--features', ($features -join ',')) }
    if ($Phase -eq 'clippy') { $arguments += @('--all-targets', '--', '-D', 'warnings') }
    elseif ($Phase -eq 'doc') { $arguments += '--no-deps' }
    return $arguments
}

function Invoke-BonsaiRustCheck {
    <#
    .SYNOPSIS
    Runs selected Rust phases with external targets and a retained check receipt.
    #>
    [CmdletBinding()]
    [OutputType([Collections.IDictionary], [Collections.Specialized.OrderedDictionary])]
    param(
        [string[]]$Package = @(),
        [ValidateSet('All', 'Runtime', 'Broker')][string]$Group = 'All',
        [string]$Toolchain = 'nightly-2026-10-22-aarch64-pc-windows-msvc',
        [Parameter(Mandatory)][string]$TargetDirectory,
        [Parameter(Mandatory)][string]$EvidenceDirectory,
        [ValidateSet('Foundation', 'Compatibility', 'Native')][string]$CheckProfile = 'Foundation',
        [ValidateSet('fmt', 'clippy', 'doc', 'test', 'build')][string[]]$Phase = @('fmt', 'clippy', 'doc'),
        [ValidateRange(1, 3600)][int]$TimeoutSeconds = 900,
        [switch]$FullTests,
        [switch]$Build,
        [switch]$Offline
    )
    $phases = [Collections.Generic.List[string]]::new()
    foreach ($item in $Phase) { if ($item -cnotin $phases) { $phases.Add($item) } }
    if ($FullTests -and 'test' -cnotin $phases) { $phases.Add('test') }
    if ($Build -and 'build' -cnotin $phases) { $phases.Add('build') }
    if ('test' -in $phases -and ($env:GITHUB_ACTIONS -cne 'true' -or $env:GITHUB_REF -cne 'refs/heads/main')) {
        throw 'Complete Rust suites run only in GitHub Actions on main.'
    }
    $packages = @(Get-BonsaiRustPackage -Group $Group -Name $Package)
    if (-not $packages.Count -or -not $phases.Count) { throw 'Select at least one registered package and one phase.' }
    $target = Resolve-ExternalPath -Path $TargetDirectory
    $evidence = Resolve-ExternalPath -Path $EvidenceDirectory
    if ($target -eq $evidence -or $target.StartsWith($evidence + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase) -or
        $evidence.StartsWith($target + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
        throw 'Cargo targets must be separate from uploaded evidence.'
    }
    if (Test-Path -LiteralPath $evidence) { throw 'Use a new Rust check evidence directory.' }
    [void][IO.Directory]::CreateDirectory($target)
    [void][IO.Directory]::CreateDirectory($evidence)
    # Synthetic test scratch can contain credentials and large files; upload
    # command evidence separately and preserve every inherited system variable.
    $work = Resolve-ExternalPath -Path (Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-RustCheck-' + [guid]::NewGuid().ToString('N')))
    [void][IO.Directory]::CreateDirectory($work)
    $environment = @{ CARGO_TARGET_DIR = $target; TEMP = $work; TMP = $work }
    $cargo = (Get-Command -Name cargo -CommandType Application -ErrorAction Stop).Source
    $rustc = (Get-Command -Name rustc -CommandType Application -ErrorAction Stop).Source
    $records = [Collections.Generic.List[object]]::new()
    $receipt = [ordered]@{
        SchemaVersion = 1; Revision = $env:GITHUB_SHA; Toolchain = $Toolchain; CheckProfile = $CheckProfile
        FingerprintMode = if ($env:CARGO_BUILD_FINGERPRINT) { $env:CARGO_BUILD_FINGERPRINT } else { 'mtime' }
        Packages = @($packages.Name); TargetDirectory = $target; WorkingDirectory = $work
        CompilerIdentity = $null; Checks = $records; Success = $false; Error = $null
        PerformanceMeasured = $false
        WorkspaceSha256 = (Get-FileHash -LiteralPath (Join-Path $PSScriptRoot '../Cargo.toml') -Algorithm SHA256).Hash.ToLowerInvariant()
        LockSha256 = (Get-FileHash -LiteralPath (Join-Path $PSScriptRoot '../Cargo.lock') -Algorithm SHA256).Hash.ToLowerInvariant()
        RegistrySha256 = (Get-FileHash -LiteralPath (Join-Path $PSScriptRoot '../config/rust-packages.psd1') -Algorithm SHA256).Hash.ToLowerInvariant()
    }
    try {
        $identity = Invoke-BonsaiNativeProcess -Executable $rustc -ArgumentList @(('+' + $Toolchain), '-vV') `
            -WorkingDirectory $work -TimeoutSeconds 60 -LogPath (Join-Path $evidence 'rustc.log')
        if ($identity.ExitCode -ne 0) { throw 'Rust compiler identification failed.' }
        $receipt.CompilerIdentity = $identity.StandardOutput.Trim()
        foreach ($selected in $packages) {
            foreach ($step in $phases) {
                $arguments = @(Get-BonsaiRustArgument -Package $selected -Toolchain $Toolchain -Phase $step -CheckProfile $CheckProfile -Offline:$Offline)
                $name = $selected.Name + '-' + $step
                $log = Join-Path $evidence ($name + '.log')
                $record = [pscustomobject]@{
                    Name = $name; Executable = $cargo; Arguments = $arguments; ExitCode = $null
                    Log = $log; Passed = $false
                }
                $records.Add($record)
                $result = Invoke-BonsaiNativeProcess -Executable $cargo -ArgumentList $arguments `
                    -WorkingDirectory $work -TimeoutSeconds $TimeoutSeconds -Environment $environment -LogPath $log
                $record.ExitCode = $result.ExitCode
                $record.Passed = $result.ExitCode -eq 0
                Write-Information -MessageData "$name exit $($result.ExitCode)" -InformationAction Continue
            }
        }
        if ($records.Where({ -not $_.Passed }).Count) { throw 'Rust checks failed; inspect the retained command logs.' }
        $receipt.Success = $true
    } catch {
        $receipt.Error = $_.Exception.Message
        throw
    } finally {
        $receipt | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $evidence 'rust-checks.json') -Encoding utf8NoBOM
    }
    return $receipt
}

Export-ModuleMember -Function Get-BonsaiRustPackage, Get-BonsaiRustArgument, Invoke-BonsaiRustCheck
