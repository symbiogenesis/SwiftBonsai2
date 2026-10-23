#requires -Version 7.6
<#
.SYNOPSIS
Compares pinned stable and nightly Rust without changing the default toolchain.
.DESCRIPTION
Compatibility runs formatting, Release Clippy and documentation. FullTests adds
complete correctness suites only in main-branch GitHub Actions. Probe compiles
small language fixtures using stable and four modes of the same nightly binary.
Logs, compiler identities and receipts remain in external evidence directories.
This command runs no benchmarks and makes no inference-performance claim.
#>
[CmdletBinding()]
[OutputType([string])]
param(
    [Parameter(Mandatory)][ValidateSet('Compatibility', 'Probe')][string]$Mode,
    [string]$Toolchain,
    [string]$TargetDirectory,
    [Parameter(Mandatory)][string]$EvidenceDirectory,
    [ValidateSet('Polonius', 'NextSolver', 'All')][string]$ProbeCase = 'All',
    [switch]$FullTests
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1')
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Native.psm1')

$repoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$configuration = Get-Content -LiteralPath (Join-Path $repoRoot 'config/rust-nightly-trial.json') -Raw |
    ConvertFrom-Json -DateKind String -NoEnumerate
if ($configuration.SchemaVersion -ne 1 -or
    $configuration.StableToolchain -cne '1.99.1-aarch64-pc-windows-msvc' -or
    $configuration.NightlyToolchain -notmatch '^nightly-\d{4}-\d{2}-\d{2}-aarch64-pc-windows-msvc$') {
    throw 'The trial requires the stable baseline and a date-pinned Windows ARM64 nightly.'
}
$evidence = Resolve-ExternalPath -Path $EvidenceDirectory
if (Test-Path -LiteralPath $evidence) { throw 'Use a new evidence directory for each trial invocation.' }
[void][IO.Directory]::CreateDirectory($evidence)
# Test scratch may contain synthetic credentials or large fixtures. Keep it out
# of the uploaded evidence tree, which contains command logs and receipts only.
$workingDirectory = Resolve-ExternalPath -Path (Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-RustTrialWork-' + [guid]::NewGuid().ToString('N')))
[void][IO.Directory]::CreateDirectory($workingDirectory)
$records = [Collections.Generic.List[object]]::new()
$identities = [Collections.Generic.List[object]]::new()
$receipt = [ordered]@{
    SchemaVersion = 1
    Mode = $Mode
    ProbeCase = $ProbeCase
    FullTests = [bool]$FullTests
    Revision = $env:GITHUB_SHA
    StableToolchain = $configuration.StableToolchain
    NightlyToolchain = $configuration.NightlyToolchain
    WorkingDirectory = $workingDirectory
    Identities = $identities
    Checks = $records
    Success = $false
    Error = $null
    PerformanceMeasured = $false
}

function Invoke-TrialTool {
    [CmdletBinding()]
    [OutputType([pscustomobject])]
    param(
        [Parameter(Mandatory)][string]$Name,
        [Parameter(Mandatory)][string]$Executable,
        [Parameter(Mandatory)][string[]]$Arguments,
        [Collections.IDictionary]$Environment,
        [ValidateRange(1, 3600)][int]$TimeoutSeconds = 900
    )
    $logPath = Join-Path $evidence ($Name + '.log')
    $result = Invoke-BonsaiNativeProcess -Executable $Executable -ArgumentList $Arguments `
        -WorkingDirectory $workingDirectory -TimeoutSeconds $TimeoutSeconds `
        -Environment $Environment -LogPath $logPath
    $records.Add([pscustomobject]@{
        Name = $Name
        Executable = $Executable
        Arguments = $Arguments
        ExitCode = $result.ExitCode
        Log = $logPath
        Passed = $result.ExitCode -eq 0
    })
    return $result
}

function Get-TrialCompilerIdentity {
    [CmdletBinding()]
    [OutputType([void])]
    param([Parameter(Mandatory)][string]$SelectedToolchain)
    $result = Invoke-TrialTool -Name ($SelectedToolchain + '-identity') -Executable $rustc `
        -Arguments @(('+' + $SelectedToolchain), '-vV') -TimeoutSeconds 60
    if ($result.ExitCode -ne 0 -or
        $result.StandardOutput -notmatch '(?m)^host: aarch64-pc-windows-msvc\r?$' -or
        $result.StandardOutput -notmatch '(?m)^commit-hash: [a-f0-9]{40}\r?$') {
        throw "Expected an identified native ARM64 compiler: $SelectedToolchain"
    }
    $identities.Add([pscustomobject]@{
        Toolchain = $SelectedToolchain
        Version = $result.StandardOutput.Trim()
    })
}

try {
    if ($FullTests -and ($env:GITHUB_ACTIONS -cne 'true' -or $env:GITHUB_REF -cne 'refs/heads/main')) {
        throw 'Complete suites run only in GitHub Actions on main.'
    }
    if ($Mode -eq 'Probe' -and $ProbeCase -eq 'All' -and -not $FullTests) {
        throw 'The complete probe selection requires hosted -FullTests; select one ProbeCase locally.'
    }
    foreach ($name in @('RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS', 'RUSTDOCFLAGS', 'CARGO_ENCODED_RUSTDOCFLAGS',
        'RUSTC', 'RUSTDOC', 'RUSTC_WRAPPER', 'RUSTC_WORKSPACE_WRAPPER', 'CARGO_BUILD_RUSTFLAGS', 'CARGO_BUILD_TARGET')) {
        if ([Environment]::GetEnvironmentVariable($name)) { throw "Remove the compiler override before this controlled comparison: $name" }
    }
    foreach ($name in [Environment]::GetEnvironmentVariables().Keys) {
        if ($name -match '^CARGO_TARGET_.+_RUSTFLAGS$' -and [Environment]::GetEnvironmentVariable($name)) {
            throw "Remove the target compiler override before this controlled comparison: $name"
        }
    }
    $rustc = (Get-Command -Name rustc -CommandType Application -ErrorAction Stop).Source
    $cargo = (Get-Command -Name cargo -CommandType Application -ErrorAction Stop).Source
    if ($Mode -eq 'Compatibility') {
        if ($Toolchain -cne $configuration.StableToolchain -and $Toolchain -cne $configuration.NightlyToolchain) {
            throw 'Compatibility must select a toolchain from the trial configuration.'
        }
        Get-TrialCompilerIdentity -SelectedToolchain $Toolchain
        $target = Resolve-ExternalPath -Path $TargetDirectory
        if ($target.StartsWith($evidence + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase) -or $target -eq $evidence -or
            $evidence.StartsWith($target + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
            throw 'Cargo targets must be separate from uploaded evidence.'
        }
        [void][IO.Directory]::CreateDirectory($target)
        $environment = @{ CARGO_TARGET_DIR = $target; TEMP = $workingDirectory; TMP = $workingDirectory }
        $packages = @('tools/bitcos', 'tools/gguf', 'tools/prism', 'src/windows-process',
            'src/windows-credentials', 'src/broker', 'src/prism-runtime', 'tools/rust-trial')
        $phases = @('fmt', 'clippy', 'doc')
        if ($FullTests) { $phases += 'test' }
        foreach ($package in $packages) {
            $manifest = Join-Path $repoRoot ($package + '/Cargo.toml')
            foreach ($phase in $phases) {
                $arguments = @(('+' + $Toolchain), $phase, '--manifest-path', $manifest)
                if ($phase -eq 'fmt') { $arguments += '--check' }
                else {
                    $arguments += @('--locked', '--release')
                    # The pinned native engine's linked regressions remain in Foundation.
                    if ($package -eq 'tools/prism') {
                        if ($phase -ne 'test') { $arguments += @('--features', 'engine-loader') }
                    } else { $arguments += '--all-features' }
                    if ($phase -eq 'clippy') { $arguments += @('--all-targets', '--', '-D', 'warnings') }
                    elseif ($phase -eq 'doc') { $arguments += '--no-deps' }
                }
                $name = $package.Replace('/', '-') + '-' + $phase
                $result = Invoke-TrialTool -Name $name -Executable $cargo -Arguments $arguments -Environment $environment
                Write-Information -MessageData "$name exit $($result.ExitCode)" -InformationAction Continue
            }
        }
        if ($records.Where({ -not $_.Passed }).Count) { throw 'Compatibility checks failed; inspect the recorded command logs.' }
    } else {
        Get-TrialCompilerIdentity -SelectedToolchain $configuration.StableToolchain
        Get-TrialCompilerIdentity -SelectedToolchain $configuration.NightlyToolchain
        $modes = @(
            @{ Name = 'stable'; Toolchain = $configuration.StableToolchain; Flags = @(); Trait = $false; Polonius = $false },
            @{ Name = 'nightly-old'; Toolchain = $configuration.NightlyToolchain; Flags = @('-Znext-solver=coherence', '-Zpolonius=off'); Trait = $false; Polonius = $false },
            @{ Name = 'nightly-trait'; Toolchain = $configuration.NightlyToolchain; Flags = @('-Znext-solver=globally', '-Zpolonius=off'); Trait = $true; Polonius = $false },
            @{ Name = 'nightly-polonius'; Toolchain = $configuration.NightlyToolchain; Flags = @('-Znext-solver=coherence', '-Zpolonius=next'); Trait = $false; Polonius = $true },
            @{ Name = 'nightly-both'; Toolchain = $configuration.NightlyToolchain; Flags = @('-Znext-solver=globally', '-Zpolonius=next'); Trait = $true; Polonius = $true }
        )
        $fixtures = @(
            @{ Name = 'stable_control'; Kind = 'Control'; Error = $null },
            @{ Name = 'invalid_alias'; Kind = 'Invalid'; Error = 'E0499' }
        )
        if ($ProbeCase -in @('Polonius', 'All')) { $fixtures += @{ Name = 'polonius'; Kind = 'Polonius'; Error = 'E0499' } }
        if ($ProbeCase -in @('NextSolver', 'All')) { $fixtures += @{ Name = 'next_solver'; Kind = 'Trait'; Error = 'E0277' } }
        foreach ($compilerMode in $modes) {
            foreach ($fixture in $fixtures) {
                $name = $compilerMode.Name + '-' + $fixture.Name
                $source = Join-Path $repoRoot ('tools/rust-trial/fixtures/' + $fixture.Name + '.rs')
                $arguments = @(('+' + $compilerMode.Toolchain), $source, '--edition=2024', '--crate-type=lib',
                    '--emit=metadata', '--error-format=json', '-Dwarnings', '-o', (Join-Path $evidence ($name + '.rmeta')))
                $arguments += $compilerMode.Flags
                $expectedAcceptance = switch ($fixture.Kind) {
                    'Control' { $true }
                    'Invalid' { $false }
                    'Polonius' { $compilerMode.Polonius }
                    'Trait' { $compilerMode.Trait }
                }
                $result = Invoke-TrialTool -Name $name -Executable $rustc -Arguments $arguments -TimeoutSeconds 120
                $codedErrors = [Collections.Generic.List[string]]::new()
                $ice = $result.StandardError -match 'internal compiler error|compiler unexpectedly panicked'
                foreach ($line in ($result.StandardError -split "`n")) {
                    if ($line.StartsWith('{')) {
                        $diagnostic = $line | ConvertFrom-Json -DateKind String
                        if ($diagnostic.level -eq 'error' -and $null -ne $diagnostic.code) {
                            $codedErrors.Add($diagnostic.code.code)
                        }
                    }
                }
                $passed = if ($expectedAcceptance) { $result.ExitCode -eq 0 -and $codedErrors.Count -eq 0 -and -not $ice }
                    else { $result.ExitCode -eq 1 -and $codedErrors.Count -gt 0 -and -not $ice -and
                        @($codedErrors | Where-Object { $_ -cne $fixture.Error }).Count -eq 0 }
                $record = $records[$records.Count - 1]
                $record.Passed = $passed
                $record | Add-Member -NotePropertyMembers @{
                    ExpectedAcceptance = $expectedAcceptance
                    ExpectedError = $fixture.Error
                    CodedErrors = @($codedErrors)
                    SourceSha256 = (Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash.ToLowerInvariant()
                }
                Write-Information -MessageData "$name expected acceptance=$expectedAcceptance passed=$passed" -InformationAction Continue
            }
        }
        if ($records.Where({ -not $_.Passed }).Count) { throw 'Compiler behavior differed from the controlled probe expectations.' }
    }
    $receipt.Success = $true
} catch {
    $receipt.Error = $_.Exception.Message
    throw
} finally {
    $receipt | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath (Join-Path $evidence 'receipt.json') -Encoding utf8
}
Write-Output "Rust $Mode trial passed; evidence: $evidence"
