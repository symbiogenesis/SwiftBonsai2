#requires -Version 7.6
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1')
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Native.psm1')
Import-Module (Join-Path $PSScriptRoot 'Bonsai.BuildCache.psm1')

function Invoke-ChatBuild {
    <#
    .SYNOPSIS
    Checks and builds selected chat executables in a reusable external Cargo cache.
    .DESCRIPTION
    Unchanged source/build inputs and checked outputs bypass Cargo entirely.
    Changed inputs or ForceBuild run Cargo's pinned dependency/compiler checks.
    Cache outputs must also match their previous SHA-256 receipt before reuse;
    missing or changed outputs are rebuilt. The receipt is a corruption check,
    not a boundary against another process running as the same Windows user.
    #>
    [CmdletBinding()]
    [OutputType([pscustomobject])]
    param([Parameter(Mandatory)][string]$DataDirectory, [Parameter(Mandatory)][string]$WorkingDirectory,
          [Parameter(Mandatory)][string]$RepositoryRoot,
          [Parameter(Mandatory)][ValidateSet('bonsai-chat', 'bonsai-broker')][string[]]$Package,
          [switch]$ForceBuild)
    $working = Resolve-ExternalPath -Path $WorkingDirectory
    $data = Resolve-ExternalPath -Path $DataDirectory
    if (-not [IO.Path]::IsPathFullyQualified($RepositoryRoot) -or $RepositoryRoot -match '%[^%]+%') {
        throw 'RepositoryRoot must be an expanded absolute path.'
    }
    $repository = [IO.Path]::GetFullPath($RepositoryRoot)
    foreach ($name in @('Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml')) {
        if (-not [IO.File]::Exists((Join-Path $repository $name))) { throw "Chat build input is missing: $name" }
    }
    $toolchain = Get-Content -LiteralPath (Join-Path $repository 'rust-toolchain.toml') -Raw
    if ($toolchain -notmatch '(?m)^channel\s*=\s*"(nightly-\d{4}-\d{2}-\d{2})"\s*$') {
        throw 'Chat builds require the repository-pinned Rust toolchain.'
    }
    $channel = $Matches[1] + '-aarch64-pc-windows-msvc'
    # Separate checkouts/toolchains cannot overwrite each other's launch outputs.
    $identity = $repository.ToUpperInvariant() + "`n" + $channel
    $key = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($identity))).ToLowerInvariant()
    $cache = Resolve-ExternalPath -Path (Join-Path -Path $data -ChildPath 'build/chat' -AdditionalChildPath $key)
    [void][IO.Directory]::CreateDirectory($cache)
    [void][IO.Directory]::CreateDirectory($working)
    $lock = Resolve-ExternalPath -Path (Join-Path $cache 'build.lock')
    try { $guard = [IO.FileStream]::new($lock, [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None) }
    catch { throw 'Another chat launch is preparing this checkout. Wait for it to finish, then launch again.' }
    try {
        $result = [ordered]@{ ChatPath = $null; BrokerPath = $null; TargetDirectory = $cache }
        $packages = @($Package | Sort-Object -Unique)
        $fingerprint = Get-BonsaiBuildFingerprint -RepositoryRoot $repository -Identity $channel -WorkingDirectory $working
        $pending = @()
        foreach ($name in $packages) {
            $path = Resolve-ExternalPath -Path (Join-Path -Path $cache -ChildPath 'aarch64-pc-windows-msvc/release' -AdditionalChildPath ($name + '.exe'))
            $receipt = Resolve-ExternalPath -Path (Join-Path $cache ($name + '.sha256'))
            if ([IO.File]::Exists($path)) {
                $expected = if ([IO.File]::Exists($receipt)) { [IO.File]::ReadAllText($receipt).Trim() } else { '' }
                $actual = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash
                if ($expected -notmatch '^[a-fA-F0-9]{64}$' -or $actual -ine $expected) {
                    # Only these fixed executables beneath the validated cache
                    # are disposable. Cargo must produce them again before launch.
                    Remove-Item -LiteralPath $path -Force
                }
            }
            $freshness = Join-Path $cache ($name + '.build.json')
            if ($ForceBuild -or -not (Test-BonsaiBuildReceipt -ReceiptPath $freshness -Fingerprint $fingerprint -Executable @($path))) {
                [IO.File]::Delete($freshness)
                $pending += $name
            }
        }
        if ($pending.Count) {
            # Local dependencies can also have preserved timestamps. Invalidate
            # the workspace together, then recreate every selected executable.
            $pending = $packages
            foreach ($name in $pending) { [IO.File]::Delete((Join-Path $cache ($name + '.build.json'))) }
            $cargo = Get-Command -Name cargo -CommandType Application -ErrorAction SilentlyContinue
            if (-not $cargo) { throw "Install Rust and the $channel toolchain, or supply -ChatPath and -BrokerPath. See docs/development-guide.md." }
            Clear-BonsaiWorkspaceBuild -RepositoryRoot $repository -CargoPath $cargo.Source -Channel $channel -TargetDirectory $cache -WorkingDirectory $working -BuildTarget 'aarch64-pc-windows-msvc'
            $arguments = @("+$channel", 'build', '--locked', '--release', '--manifest-path', (Join-Path $repository 'Cargo.toml'),
                '--target', 'aarch64-pc-windows-msvc', '--target-dir', $cache)
            foreach ($name in $pending) { $arguments += @('--package', $name) }
            $log = Join-Path $working 'chat-build.log'
            Write-Information -MessageData 'Checking the native chat build. Unchanged compiler outputs are reused.' -InformationAction Continue
            $build = Invoke-BonsaiNativeProcess -Executable $cargo.Source -ArgumentList $arguments -WorkingDirectory $working -TimeoutSeconds 3600 -LogPath $log
            if ($build.ExitCode -ne 0) { throw "Chat build failed. Inspect $log. Install the pinned Rust toolchain and native ARM64 Visual Studio C++ tools if required." }
        }
        $settled = if ($pending.Count) { Get-BonsaiBuildFingerprint -RepositoryRoot $repository -Identity $channel -WorkingDirectory $working } else { $fingerprint }
        foreach ($name in $packages) {
            $path = Resolve-ExternalPath -Path (Join-Path -Path $cache -ChildPath 'aarch64-pc-windows-msvc/release' -AdditionalChildPath ($name + '.exe'))
            Assert-Arm64PE -Path $path
            $receipt = Resolve-ExternalPath -Path (Join-Path $cache ($name + '.sha256'))
            [IO.File]::WriteAllText($receipt, (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant())
            if ($name -cin $pending -and $settled -ceq $fingerprint) {
                Write-BonsaiBuildReceipt -ReceiptPath (Join-Path $cache ($name + '.build.json')) -Fingerprint $fingerprint -Executable @($path)
            }
            if ($name -ceq 'bonsai-chat') { $result.ChatPath = $path } else { $result.BrokerPath = $path }
        }
        return [pscustomobject]$result
    } finally { $guard.Dispose() }
}

Export-ModuleMember -Function Invoke-ChatBuild
