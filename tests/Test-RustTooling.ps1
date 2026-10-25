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
$toolchainSource = Get-Content -LiteralPath (Join-Path $PSScriptRoot '../rust-toolchain.toml') -Raw
$channel = [regex]::Match($toolchainSource, '(?m)^channel = "(nightly-\d{4}-\d{2}-\d{2})"\r?$')
if (-not $channel.Success) { throw 'Rust tooling requires the pinned nightly from rust-toolchain.toml.' }
$productionToolchain = $channel.Groups[1].Value + '-aarch64-pc-windows-msvc'
# Exercise optional native feature policy without registering external packages
# as local source. This fixture never invokes Cargo or resolves a manifest.
$restricted = @{
    Name = 'policy-restricted'
    Features = @()
    CompatibilityCompileFeatures = @('native-loader')
    NativeFeatures = @('*')
}
foreach ($phase in @('clippy', 'doc', 'build')) {
    $arguments = @(Get-BonsaiRustArgument -Package $restricted -Phase $phase -CheckProfile Compatibility)
    if ('native-loader' -cnotin $arguments -or '--all-features' -cin $arguments) {
        throw "Compatibility $phase must compile the loader without native-test linking."
    }
}
$arguments = @(Get-BonsaiRustArgument -Package $restricted -Phase test -CheckProfile Compatibility)
if ('--features' -cin $arguments -or '--all-features' -cin $arguments) {
    throw 'Compatibility correctness must exclude optional native-test linking.'
}
$arguments = @(Get-BonsaiRustArgument -Package $restricted -Phase clippy -CheckProfile Native)
if ('--all-features' -cnotin $arguments) { throw 'The configured native lane omitted its full feature audit.' }
$arguments = @(Get-BonsaiRustArgument -Package $restricted -Phase fmt -CheckProfile Native)
if ($arguments[0] -cne ('+' + $productionToolchain)) {
    throw 'Default Rust commands must select the configured production nightly.'
}
if ('--package' -cnotin $arguments -or 'policy-restricted' -cnotin $arguments -or '--all' -cin $arguments) {
    throw 'Formatting expanded beyond the selected package.'
}

$phases = @('fmt', 'clippy', 'doc', 'test', 'build')
foreach ($group in @('All', 'Broker')) {
    $packages = @(Get-BonsaiRustPackage -Group $group)
    foreach ($checkProfile in @('Foundation', 'Compatibility', 'Native')) {
        $plan = @(Get-BonsaiRustCheckPlan -Package $packages -Phase ($phases + 'fmt') -CheckProfile $checkProfile -Offline)
        $expectedCount = 5
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
                if ('--all-features' -cnotin $check.Arguments) { throw 'An all-feature package lost its feature audit.' }
            }
        }
    }
}
$selection = @(Get-BonsaiRustPackage -Name bonsai-windows-process,bonsai-windows-credentials)
$plan = @(Get-BonsaiRustCheckPlan -Package $selection -Phase fmt,test -CheckProfile Compatibility)
if ($plan.Count -ne 2 -or @($plan.Packages | Sort-Object -Unique).Count -ne 2) {
    throw 'A partial package selection expanded to the full workspace.'
}
$mixed = @($selection[0], $restricted)
$plan = @(Get-BonsaiRustCheckPlan -Package $mixed -Phase fmt,test -CheckProfile Compatibility)
if ($plan.Count -ne 3 -or @($plan.Packages | Sort-Object -Unique).Count -ne 2) {
    throw 'Restricted feature policy did not isolate its correctness command.'
}
$rejected = $false
try { $null = Get-BonsaiRustArgument -Package $mixed -Phase test -CheckProfile Compatibility }
catch { $rejected = $_.Exception.Message -ceq 'Only all-feature packages can share a Cargo compile command.' }
if (-not $rejected) { throw 'A direct Cargo argument request mixed restricted and all-feature packages.' }

$scratch = Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-RustPolicy-' + [guid]::NewGuid().ToString('N'))
$previousActions = $env:GITHUB_ACTIONS
try {
    $env:GITHUB_ACTIONS = 'false'
    foreach ($selection in @(@{ Phase = @('test') }, @{ FullTests = $true })) {
        $rejected = $false
        try {
            $null = Invoke-BonsaiRustCheck -Package bonsai-tools -TargetDirectory (Join-Path $scratch 'target') `
                -EvidenceDirectory (Join-Path $scratch 'evidence') @selection
        } catch {
            $rejected = $_.Exception.Message -ceq 'Complete Rust suites run only in GitHub Actions on main.'
        }
        if (-not $rejected) { throw 'A local complete-suite selection bypassed its CI guard.' }
    }
    if ([IO.Directory]::Exists($scratch)) { throw 'A rejected suite or package selection created scratch or build outputs.' }
} finally {
    $env:GITHUB_ACTIONS = $previousActions
}
Write-Output 'Rust batching, feature isolation, production-toolchain and local-suite policy checks passed without starting native tools.'
