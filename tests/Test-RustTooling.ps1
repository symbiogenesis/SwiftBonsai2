#requires -Version 7.6
<#
.SYNOPSIS
Checks Rust feature isolation and rejection of local complete-suite execution.
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
    if ([IO.Directory]::Exists($scratch)) { throw 'A rejected suite selection created scratch or build outputs.' }
} finally {
    $env:GITHUB_ACTIONS = $previousActions
}
Write-Output 'Rust feature isolation and local-suite rejection checks passed without starting native tools.'
