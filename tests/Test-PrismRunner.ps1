#requires -Version 7.6
<#
.SYNOPSIS
Checks one focused Prism scheduler regression using isolated synthetic adapters.
#>
[CmdletBinding()]
[OutputType([string])]
param([Parameter(Mandatory)][ValidateSet('Scheduling', 'ParallelTargets', 'Identity', 'ProcessFailure')][string]$Case)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.TestSupport.psm1')
Import-Module (Join-Path $PSScriptRoot '../scripts/Bonsai.PrismRunner.psm1')
$scratch = Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-PrismRunnerTest-' + [guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($scratch)
$probe = Join-Path $scratch 'adapter.ps1'
$commit = 'a' * 40
# File rendezvous proves adapters overlap without using elapsed-time assertions.
# Each adapter's Rust command also checks that its C++ control has completed.
[IO.File]::WriteAllText($probe, @'
param([string]$Id, [string]$Target, [string]$Output, [string]$Directory, [string]$Commit, [string]$Mode)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if ($env:BONSAI_RUNNER_SENTINEL -cne 'inherited') { throw 'The child environment was lost.' }
if ($Mode -ceq 'ProcessFailure') { [Console]::Error.WriteLine('synthetic adapter failure detail'); exit 17 }
if ($Mode -ceq 'ParallelTargets') {
    $active = Join-Path $Directory "$Id-$Target-active"
    $gatePath = Join-Path $Directory 'concurrency.lock'
    $gate = $null
    $deadline = [DateTime]::UtcNow.AddSeconds(10)
    while ($null -eq $gate) {
        try { $gate = [IO.File]::Open($gatePath, [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None) }
        catch [IO.IOException] {
            if ([DateTime]::UtcNow -ge $deadline) { throw 'Concurrency evidence lock timed out.' }
            Start-Sleep -Milliseconds 10
        }
    }
    try {
        [IO.File]::WriteAllText($active, '')
        $count = @([IO.Directory]::GetFiles($Directory, '*-active')).Count
        if ($count -gt 2) { throw 'The global native process concurrency bound was exceeded.' }
        if ($count -eq 2) { [IO.File]::WriteAllText((Join-Path $Directory 'overlap-observed'), '') }
    } finally { $gate.Dispose() }
    try {
        [IO.File]::WriteAllText((Join-Path $Directory "$Id-$Target-started"), '')
        while (-not [IO.File]::Exists((Join-Path $Directory "$Id-cpp-started")) -or
            -not [IO.File]::Exists((Join-Path $Directory "$Id-rust-started"))) {
            if ([DateTime]::UtcNow -ge $deadline) { throw 'A single adapter target pair did not overlap.' }
            Start-Sleep -Milliseconds 10
        }
        if ($Target -ceq 'rust') {
            [IO.File]::WriteAllText((Join-Path $Directory "$Id-rust-complete"), '')
        } else {
            while (-not [IO.File]::Exists((Join-Path $Directory "$Id-rust-complete"))) {
                if ([DateTime]::UtcNow -ge $deadline) { throw 'The reversed target completion rendezvous timed out.' }
                Start-Sleep -Milliseconds 10
            }
        }
    } finally { [IO.File]::Delete($active) }
}
if ($Mode -ceq 'Scheduling') {
    if ($Target -ceq 'cpp') {
        [IO.File]::WriteAllText((Join-Path $Directory "$Id-started"), '')
        $deadline = [DateTime]::UtcNow.AddSeconds(10)
        while (@([IO.Directory]::GetFiles($Directory, '*-started')).Count -lt 2) {
            if ([DateTime]::UtcNow -ge $deadline) { throw 'Independent adapters did not overlap.' }
            Start-Sleep -Milliseconds 10
        }
        [IO.File]::WriteAllText((Join-Path $Directory "$Id-cpp-complete"), '')
    } elseif (-not [IO.File]::Exists((Join-Path $Directory "$Id-cpp-complete"))) {
        throw 'A Rust command overtook its C++ control.'
    }
}
$reportedId = if ($Mode -ceq 'Identity') { 'tests/test-unexpected.cpp' } else { "tests/test-$Id.cpp" }
$report = @{ SchemaVersion = 1; UpstreamCommit = $Commit; Target = $Target; Cases = @(@{ Id = $reportedId; Status = 'pass'; Detail = 'synthetic adapter' }) }
[IO.File]::WriteAllText($Output, ($report | ConvertTo-Json -Depth 5))
'@)
$work = @(
    foreach ($ordinal in 0..1) {
        $commands = @(
            foreach ($target in @('cpp', 'rust')) {
                $output = Join-Path $scratch "$ordinal-$target.json"
                [pscustomobject]@{
                    Target = $target; OutputPath = $output; LogPath = Join-Path $scratch "$ordinal-$target.log"
                    Arguments = @('-NoLogo', '-NoProfile', '-File', $probe, '-Id', [string]$ordinal, '-Target', $target,
                        '-Output', $output, '-Directory', $scratch, '-Commit', $commit, '-Mode', $Case)
                }
            }
        )
        [pscustomobject]@{ Ordinal = $ordinal; Id = "tests/test-$ordinal.cpp"; TimeoutSeconds = 20 + $ordinal; Commands = $commands }
    }
)
$parameters = @{
    Cases = $work; Executable = (Get-Command pwsh -CommandType Application).Source
    EvidenceDirectory = $scratch; UpstreamCommit = $commit; Parallel = 2
    Environment = @{ BONSAI_RUNNER_SENTINEL = 'inherited' }
}
if ($Case -ceq 'ParallelTargets') {
    $parameters.ParallelTargets = $true
    $parameters.Cases = @($work[0])
    $single = @(Invoke-BonsaiPrismCaseBatch @parameters)
    Assert ($single.Count -eq 1 -and $single[0].Reference.Id -ceq $work[0].Id -and $single[0].Rust.Id -ceq $work[0].Id) 'A single adapter did not return both overlapping target results.'
    Assert ([IO.File]::Exists((Join-Path $scratch 'overlap-observed'))) 'The single adapter targets did not run at the same time.'
    $parameters.Cases = $work
    $results = @(Invoke-BonsaiPrismCaseBatch @parameters)
    Assert ($results.Count -eq 2 -and $results[0].Ordinal -eq 0 -and $results[1].Ordinal -eq 1) 'Overlapping targets changed the source order.'
    foreach ($result in $results) {
        Assert (($result.Commands.Target -join ',') -ceq 'cpp,rust') 'Target completion order changed the retained command order.'
        Assert ($result.Reference.Id -ceq $result.Id -and $result.Rust.Id -ceq $result.Id) 'Target overlap changed a source identity.'
    }
    Assert (@([IO.Directory]::GetFiles($scratch, '*-active')).Count -eq 0) 'A synthetic target remained active after the bounded queue completed.'
    $parameters.Cases = @($work[0])
    foreach ($failure in @('Identity', 'ProcessFailure')) {
        foreach ($command in $work[0].Commands) { $command.Arguments[-1] = $failure }
        $diagnostic = Assert-Failure { Invoke-BonsaiPrismCaseBatch @parameters } -PassThru
        $expected = if ($failure -ceq 'Identity') { 'unexpected case identity' } else { 'exited 17' }
        Assert ($diagnostic.Contains($expected, [StringComparison]::Ordinal)) 'Target overlap lost a command failure diagnostic.'
        if ($failure -ceq 'ProcessFailure') {
            Assert ($diagnostic.Contains('synthetic adapter failure detail', [StringComparison]::Ordinal)) 'Target overlap lost the command stderr.'
        }
    }
} elseif ($Case -ceq 'Scheduling') {
    $results = @(Invoke-BonsaiPrismCaseBatch @parameters)
    Assert ($results.Count -eq 2 -and $results[0].Ordinal -eq 0 -and $results[1].Ordinal -eq 1) 'Concurrent completion must preserve original adapter order.'
    foreach ($ordinal in 0..1) {
        Assert ($results[$ordinal].Reference.Id -ceq "tests/test-$ordinal.cpp" -and $results[$ordinal].Rust.Id -ceq "tests/test-$ordinal.cpp") 'Both independent control identities must survive scheduling.'
    }
} else {
    $diagnostic = Assert-Failure { Invoke-BonsaiPrismCaseBatch @parameters } -PassThru
    $expected = if ($Case -ceq 'Identity') { 'unexpected case identity' } else { 'exited 17' }
    Assert ($diagnostic.Contains($expected, [StringComparison]::Ordinal)) "The scheduler did not retain its $Case failure diagnostic."
    if ($Case -ceq 'ProcessFailure') {
        Assert ($diagnostic.Contains('synthetic adapter failure detail', [StringComparison]::Ordinal)) 'The scheduler must surface the adapter stderr without downloading a large evidence archive.'
    }
}
return "Prism runner $Case passed; evidence: $scratch"
