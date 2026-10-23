#requires -Version 7.6
<#
.SYNOPSIS
Kills a finite catalog of six codec source faults with tiny independent oracles.
.DESCRIPTION
Copies the dependency-free ternary crate into unique system-temp storage. The
checkout is never mutated. The six new guard tests must pass before mutation;
each mutant must compile and fail its exact guard with an assertion diagnostic.
Compilation errors, timeouts, missing tests and survivors fail the gate. Source
is restored after every mutant. Logs and a JSON receipt are retained on failure.
No dependency installation, complete existing suite or benchmark is performed.
#>
[CmdletBinding()]
[OutputType([string])]
param(
    [Parameter(Mandatory)][string]$EvidenceDirectory,
    [ValidateSet('All', 'ZeroScale', 'NonfiniteScale', 'ReservedSymbol', 'FiveTritAlias', 'FourTritTail', 'PackRounding')]
    [string]$Case = 'All',
    [ValidateRange(5, 180)][int]$BaselineCompileTimeoutSeconds = 120,
    [ValidateRange(5, 120)][int]$CompileTimeoutSeconds = 30,
    [ValidateRange(1, 30)][int]$TestTimeoutSeconds = 10
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1')
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Native.psm1')

$repoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$evidence = Resolve-ExternalPath -Path $EvidenceDirectory
if (Test-Path -LiteralPath $evidence) { throw 'Use a new evidence directory for each mutation run.' }
[void][IO.Directory]::CreateDirectory($evidence)
$work = Resolve-ExternalPath -Path (Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-Mutations-' + [guid]::NewGuid().ToString('N')))
[void][IO.Directory]::CreateDirectory($work)
$crate = Join-Path $work 'ternary'
$sourceDirectory = Join-Path $crate 'src'
$testsDirectory = Join-Path $crate 'tests'
[void][IO.Directory]::CreateDirectory($sourceDirectory)
[void][IO.Directory]::CreateDirectory($testsDirectory)
[IO.File]::Copy((Join-Path $repoRoot 'tools/ternary/Cargo.toml'), (Join-Path $crate 'Cargo.toml'))
# Retain inherited package/profile/lint policy in a minimal isolated workspace.
# Its lock is generated offline; unrelated workspace packages are never copied,
# mutated, compiled or executed by this finite catalog.
$workspaceSource = Join-Path $repoRoot 'Cargo.toml'
$workspace = [IO.File]::ReadAllText($workspaceSource)
$workspace = [regex]::Replace($workspace, '(?s)^\[workspace\].*?(?=\[workspace\.package\])', "[workspace]`nresolver = `"3`"`nmembers = [`"ternary`"]`n`n")
[IO.File]::WriteAllText((Join-Path $work 'Cargo.toml'), $workspace)
$sourcePath = Join-Path $sourceDirectory 'lib.rs'
[IO.File]::Copy((Join-Path $repoRoot 'tools/ternary/src/lib.rs'), $sourcePath)
[IO.File]::Copy((Join-Path $repoRoot 'tools/ternary/tests/mutation_guards.rs'), (Join-Path $testsDirectory 'mutation_guards.rs'))
$original = [IO.File]::ReadAllText($sourcePath)
$catalog = @(
    [pscustomobject]@{
        Name = 'ZeroScale'; Test = 'guard_zero_scale_boundary'
        Anchor = 'matches!(scale, 0 | 0x8000)'
        Replacement = 'matches!(scale, 0x8000)'
    },
    [pscustomobject]@{
        Name = 'NonfiniteScale'; Test = 'guard_nonfinite_scale_boundary'
        Anchor = 'scale & 0x7c00 == 0x7c00'
        Replacement = 'scale & 0x7fff == 0x7c00'
    },
    [pscustomobject]@{
        Name = 'ReservedSymbol'; Test = 'guard_isolated_reserved_symbol'
        Anchor = 'byte & (byte >> 1) & 0x55 != 0'
        Replacement = 'byte & (byte >> 1) & 0x55 == 0x55'
    },
    [pscustomobject]@{
        Name = 'FiveTritAlias'; Test = 'guard_five_trit_canonical_alias'
        Anchor = 'if !digits.canonical_five {'
        Replacement = 'if !digits.canonical_five && source[start + m] > 1 {'
    },
    [pscustomobject]@{
        Name = 'FourTritTail'; Test = 'guard_four_trit_implicit_zero'
        Anchor = 'if !digits.canonical_four {'
        Replacement = 'if !digits.canonical_four && !digits.canonical_five {'
    },
    [pscustomobject]@{
        Name = 'PackRounding'; Test = 'guard_pack_rounds_up'
        Anchor = 'u8::try_from((q * 256).div_ceil(243)).map_err(|_| Error::SizeOverflow)?;'
        Replacement = 'u8::try_from((q * 256) / 243).map_err(|_| Error::SizeOverflow)?;'
    }
)
$selected = @($catalog | Where-Object { $Case -eq 'All' -or $_.Name -eq $Case })
$checks = [Collections.Generic.List[object]]::new()
$outcomes = [Collections.Generic.List[object]]::new()
$receipt = [ordered]@{
    SchemaVersion = 1
    Case = $Case
    SourceSha256 = (Get-FileHash -LiteralPath $sourcePath -Algorithm SHA256).Hash.ToLowerInvariant()
    WorkspaceSha256 = (Get-FileHash -LiteralPath $workspaceSource -Algorithm SHA256).Hash.ToLowerInvariant()
    IsolatedWorkspaceSha256 = (Get-FileHash -LiteralPath (Join-Path $work 'Cargo.toml') -Algorithm SHA256).Hash.ToLowerInvariant()
    LockSha256 = $null
    WorkingDirectory = $work
    BaselineCompileTimeoutSeconds = $BaselineCompileTimeoutSeconds
    CompileTimeoutSeconds = $CompileTimeoutSeconds
    TestTimeoutSeconds = $TestTimeoutSeconds
    Checks = $checks
    Mutants = $outcomes
    Success = $false
    Error = $null
}
$cargo = (Get-Command -Name cargo -CommandType Application -ErrorAction Stop).Source
$arguments = @('+1.99.1-aarch64-pc-windows-msvc', 'test', '--locked', '--offline', '--release',
    '--manifest-path', (Join-Path $crate 'Cargo.toml'), '--target-dir', (Join-Path $work 'target'),
    '--test', 'mutation_guards')
$environment = @{ TEMP = $work; TMP = $work; CARGO_TERM_COLOR = 'never' }

function Invoke-MutationPhase {
    [CmdletBinding()]
    [OutputType([pscustomobject])]
    param(
        [Parameter(Mandatory)][string]$Name,
        [Parameter(Mandatory)][string[]]$ExtraArguments,
        [Parameter(Mandatory)][int]$TimeoutSeconds
    )
    $log = Join-Path $evidence ($Name + '.log')
    $result = Invoke-BonsaiNativeProcess -Executable $cargo -ArgumentList ($arguments + $ExtraArguments) `
        -WorkingDirectory $work -TimeoutSeconds $TimeoutSeconds -Environment $environment -LogPath $log
    $checks.Add([pscustomobject]@{ Name = $Name; ExitCode = $result.ExitCode; Log = $log })
    return $result
}

try {
    $lockLog = Join-Path $evidence 'lockfile.log'
    $lockResult = Invoke-BonsaiNativeProcess -Executable $cargo -ArgumentList @('+1.99.1-aarch64-pc-windows-msvc',
        'generate-lockfile', '--offline', '--manifest-path', (Join-Path $crate 'Cargo.toml')) `
        -WorkingDirectory $work -TimeoutSeconds $BaselineCompileTimeoutSeconds -Environment $environment -LogPath $lockLog
    $checks.Add([pscustomobject]@{ Name = 'lockfile'; ExitCode = $lockResult.ExitCode; Log = $lockLog })
    if ($lockResult.ExitCode -ne 0) { throw 'The isolated mutation workspace lock could not be generated offline.' }
    $receipt.LockSha256 = (Get-FileHash -LiteralPath (Join-Path $work 'Cargo.lock') -Algorithm SHA256).Hash.ToLowerInvariant()
    foreach ($mutant in $selected) {
        $first = $original.IndexOf($mutant.Anchor, [StringComparison]::Ordinal)
        if ($first -lt 0 -or $original.IndexOf($mutant.Anchor, $first + 1, [StringComparison]::Ordinal) -ge 0) {
            throw "Mutation source anchor must occur exactly once: $($mutant.Name)"
        }
    }
    $compile = Invoke-MutationPhase -Name 'baseline-compile' -ExtraArguments @('--no-run') -TimeoutSeconds $BaselineCompileTimeoutSeconds
    if ($compile.ExitCode -ne 0) { throw 'The unmodified mutation guard target did not compile.' }
    $baseline = Invoke-MutationPhase -Name 'baseline-tests' -ExtraArguments @('--', '--test-threads=1') -TimeoutSeconds $TestTimeoutSeconds
    if ($baseline.ExitCode -ne 0 -or $baseline.StandardOutput -notmatch 'test result: ok\. 6 passed; 0 failed; 0 ignored;') {
        throw 'All six mutation guards must pass without ignored or missing tests before mutation.'
    }
    foreach ($mutant in $selected) {
        $outcome = [pscustomobject]@{ Name = $mutant.Name; Test = $mutant.Test; Compiled = $false; Killed = $false }
        $outcomes.Add($outcome)
        try {
            [IO.File]::WriteAllText($sourcePath, $original.Replace($mutant.Anchor, $mutant.Replacement, [StringComparison]::Ordinal))
            $compile = Invoke-MutationPhase -Name ($mutant.Name + '-compile') -ExtraArguments @('--no-run') -TimeoutSeconds $CompileTimeoutSeconds
            if ($compile.ExitCode -ne 0) { throw "Mutant compilation failed; it does not count as a kill: $($mutant.Name)" }
            $outcome.Compiled = $true
            $test = Invoke-MutationPhase -Name ($mutant.Name + '-test') -ExtraArguments @($mutant.Test, '--', '--exact', '--test-threads=1') -TimeoutSeconds $TestTimeoutSeconds
            $testName = [regex]::Escape($mutant.Test)
            $output = $test.StandardOutput + $test.StandardError
            if ($test.ExitCode -ne 101 -or
                $output -notmatch "(?m)^test $testName \.\.\. FAILED\r?$" -or
                $output -notmatch 'test result: FAILED\. 0 passed; 1 failed; 0 ignored; 0 measured; 5 filtered out;' -or
                $output -notmatch '(?m)^assertion `left == right` failed\r?$') {
                throw "Mutant survived or failed without the exact guard assertion: $($mutant.Name)"
            }
            $outcome.Killed = $true
            Write-Information -MessageData "Killed $($mutant.Name) with $($mutant.Test)." -InformationAction Continue
        } finally {
            [IO.File]::WriteAllText($sourcePath, $original)
        }
    }
    $receipt.Success = $true
} catch {
    $receipt.Error = $_.Exception.Message
    throw
} finally {
    $receipt | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $evidence 'mutation-results.json') -Encoding utf8NoBOM
}
Write-Output "Killed $($outcomes.Count)/$($selected.Count) compiled mutants. Receipt: $(Join-Path $evidence 'mutation-results.json')"
