#requires -Version 7.6
<#
.SYNOPSIS
Checks Rust feature isolation, production toolchains and local-suite rejection.
.DESCRIPTION
Imports pure package/argument policy and exercises early rejection paths only.
No compiler, test suite, benchmark, model or native tool is started.
#>
[CmdletBinding()]
[OutputType([string])]
param()
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot '../scripts/Bonsai.Rust.psm1') -Force
$configuration = Get-Content -LiteralPath (Join-Path $PSScriptRoot '../config/rust-nightly-trial.json') -Raw |
    ConvertFrom-Json -DateKind String -NoEnumerate

$prism = Get-BonsaiRustPackage -Name bonsai-prism
foreach ($phase in @('clippy', 'doc', 'build')) {
    $arguments = @(Get-BonsaiRustArgument -Package $prism -Phase $phase -CheckProfile Compatibility)
    if ('engine-loader' -cnotin $arguments -or '--all-features' -cin $arguments) {
        throw "Compatibility $phase must compile the loader without native-test linking."
    }
}
$arguments = @(Get-BonsaiRustArgument -Package $prism -Phase test -CheckProfile Compatibility)
if ('--features' -cin $arguments -or '--all-features' -cin $arguments) {
    throw 'Compatibility correctness must exclude optional native-test linking.'
}
$arguments = @(Get-BonsaiRustArgument -Package $prism -Phase clippy -CheckProfile Native)
if ('--all-features' -cnotin $arguments) { throw 'The configured native lane omitted its full feature audit.' }
$arguments = @(Get-BonsaiRustArgument -Package $prism -Phase fmt -CheckProfile Native)
if ($arguments[0] -cne ('+' + $configuration.NightlyToolchain)) {
    throw 'Default Rust commands must select the configured production nightly.'
}
if ('--package' -cnotin $arguments -or 'bonsai-prism' -cnotin $arguments -or '--all' -cin $arguments) {
    throw 'Formatting expanded beyond the selected package.'
}

$scratch = Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-RustPolicy-' + [guid]::NewGuid().ToString('N'))
$previousActions = $env:GITHUB_ACTIONS
try {
    $env:GITHUB_ACTIONS = 'false'
    foreach ($selection in @(@{ Phase = @('test') }, @{ FullTests = $true })) {
        $rejected = $false
        try {
            $null = Invoke-BonsaiRustCheck -Package bonsai-ternary -TargetDirectory (Join-Path $scratch 'target') `
                -EvidenceDirectory (Join-Path $scratch 'evidence') @selection
        } catch {
            $rejected = $_.Exception.Message -ceq 'Complete Rust suites run only in GitHub Actions on main.'
        }
        if (-not $rejected) { throw 'A local complete-suite selection bypassed its CI guard.' }
    }
    $rejected = $false
    try {
        $null = & (Join-Path $PSScriptRoot 'Test-RustNightly.ps1') -Mode Compatibility `
            -Toolchain $configuration.StableToolchain -TargetDirectory (Join-Path $scratch 'target') `
            -EvidenceDirectory (Join-Path $scratch 'evidence')
    } catch {
        $rejected = $_.Exception.Message -ceq 'Compatibility requires the pinned production nightly; stable is an isolated probe control.'
    }
    if (-not $rejected) { throw 'Workspace compatibility accepted the stable probe compiler.' }
    foreach ($selection in @(
        @{ Arguments = @{ Mode = 'Compatibility'; PackageGroup = 'Runtime'; Package = @('bonsai-broker') }; Error = 'Unknown Rust package in group Runtime: bonsai-broker' }
        @{ Arguments = @{ Mode = 'Compatibility'; PackageGroup = 'Broker'; Package = @('bonsai-missing') }; Error = 'Unknown Rust package in group Broker: bonsai-missing' }
        @{ Arguments = @{ Mode = 'Probe'; ProbeCase = 'Polonius'; Package = @('bonsai-rust-trial') }; Error = 'PackageGroup and Package apply only to Compatibility.' }
        @{ Arguments = @{ Mode = 'Probe'; ProbeCase = 'Polonius'; PackageGroup = 'Runtime' }; Error = 'PackageGroup and Package apply only to Compatibility.' }
        @{ Arguments = @{ Mode = 'Compatibility'; PackageGroup = 'Runtime'; Package = @('bonsai-prism-runtime'); FullTests = $true }; Error = 'Complete suites run only in GitHub Actions on main.' }
    )) {
        $rejected = $false
        try {
            $arguments = $selection.Arguments
            $null = & (Join-Path $PSScriptRoot 'Test-RustNightly.ps1') @arguments `
                -TargetDirectory (Join-Path $scratch 'target') -EvidenceDirectory (Join-Path $scratch 'evidence')
        } catch {
            $rejected = $_.Exception.Message -ceq $selection.Error
        }
        if (-not $rejected) { throw 'Nightly package selection bypassed its group, probe or local-suite policy.' }
    }
    $rejected = $false
    try {
        $null = & (Join-Path $PSScriptRoot 'Test-RustNightly.ps1') -Mode Compatibility -Package @() `
            -TargetDirectory (Join-Path $scratch 'target') -EvidenceDirectory (Join-Path $scratch 'evidence')
    } catch {
        $rejected = $_.FullyQualifiedErrorId -like 'ParameterArgumentValidationError*'
    }
    if (-not $rejected) { throw 'An empty explicit nightly package selection expanded to a complete group.' }
    if ([IO.Directory]::Exists($scratch)) { throw 'A rejected suite or package selection created scratch or build outputs.' }
} finally {
    $env:GITHUB_ACTIONS = $previousActions
}
Write-Output 'Rust feature isolation, production-toolchain and local-suite policy checks passed without starting native tools.'
