#requires -Version 7.6
<#
.SYNOPSIS
Checks Rust batching, feature isolation, production toolchains and suite rejection.
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

$phases = @('fmt', 'clippy', 'doc', 'test', 'build')
foreach ($group in @('All', 'Runtime', 'Broker')) {
    $packages = @(Get-BonsaiRustPackage -Group $group)
    foreach ($checkProfile in @('Foundation', 'Compatibility', 'Native')) {
        $plan = @(Get-BonsaiRustCheckPlan -Package $packages -Phase ($phases + 'fmt') -CheckProfile $checkProfile -Offline)
        $expectedCount = if ($group -ne 'Broker' -and $checkProfile -ne 'Native') { 9 } else { 5 }
        if ($plan.Count -ne $expectedCount) { throw "Unexpected Cargo process count for $group/$checkProfile." }
        foreach ($phase in $phases) {
            $checks = @($plan | Where-Object Phase -CEQ $phase)
            $covered = @($checks.Packages | Sort-Object)
            if (($covered -join ',') -cne (($packages.Name | Sort-Object) -join ',')) {
                throw "Batching omitted or duplicated package coverage for $group/$checkProfile/$phase."
            }
            foreach ($check in $checks) {
                $selected = [Collections.Generic.List[string]]::new()
                for ($index = 0; $index -lt $check.Arguments.Count; $index++) {
                    if ($check.Arguments[$index] -ceq '--package') { $selected.Add($check.Arguments[$index + 1]) }
                }
                if (($selected -join ',') -cne ($check.Packages -join ',') -or '--workspace' -cin $check.Arguments -or '--all' -cin $check.Arguments) {
                    throw 'A Cargo batch widened its explicit selected-package boundary.'
                }
                if ($phase -eq 'fmt') {
                    if ($checks.Count -ne 1 -or '--check' -cnotin $check.Arguments -or '--all-features' -cin $check.Arguments) {
                        throw 'Formatting must check all selected packages in one feature-free invocation.'
                    }
                    continue
                }
                foreach ($required in @('--locked', '--release', '--offline')) {
                    if ($required -cnotin $check.Arguments) { throw "A Cargo batch dropped $required." }
                }
                if ($phase -eq 'clippy' -and '--all-targets' -cnotin $check.Arguments) { throw 'Batching omitted a Clippy target.' }
                if ($phase -eq 'test' -and '--no-fail-fast' -cnotin $check.Arguments) {
                    throw 'One failed test binary must not suppress later package suites in a Cargo batch.'
                }
                if ('bonsai-prism' -cin $check.Packages -and $checkProfile -ne 'Native') {
                    if ($check.Packages.Count -ne 1 -or '--all-features' -cin $check.Arguments) {
                        throw 'Restricted Prism feature checks must remain isolated from all-feature packages.'
                    }
                } elseif ('--all-features' -cnotin $check.Arguments) { throw 'An all-feature package lost its feature audit.' }
            }
        }
    }
}
$selection = @(Get-BonsaiRustPackage -Name bonsai-ternary,bonsai-prism)
$plan = @(Get-BonsaiRustCheckPlan -Package $selection -Phase fmt,test -CheckProfile Compatibility)
if ($plan.Count -ne 3 -or @($plan.Packages | Sort-Object -Unique).Count -ne 2) {
    throw 'A partial package selection expanded to the full workspace.'
}
$rejected = $false
try { $null = Get-BonsaiRustArgument -Package $selection -Phase test -CheckProfile Compatibility }
catch { $rejected = $_.Exception.Message -ceq 'Only all-feature packages can share a Cargo compile command.' }
if (-not $rejected) { throw 'A direct Cargo argument request mixed restricted and all-feature packages.' }

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
Write-Output 'Rust batching, feature isolation, production-toolchain and local-suite policy checks passed without starting native tools.'
