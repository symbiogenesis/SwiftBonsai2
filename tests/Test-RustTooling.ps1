#requires -Version 7.6
<#
.SYNOPSIS
Checks Rust policy and digest-bound maintenance bootstrap probe reuse.
.DESCRIPTION
Imports pure package/argument policy and exercises early rejection paths only.
Synthetic maintenance processes check warm bootstrap reuse and invalidation.
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
                if ($phase -eq 'doc') {
                    $formatIndex = [Array]::IndexOf($check.Arguments, '--output-format')
                    $unstableIndex = [Array]::IndexOf($check.Arguments, '-Z')
                    if ('--no-deps' -cnotin $check.Arguments -or $formatIndex -lt 0 -or
                        $check.Arguments[$formatIndex + 1] -cne 'json' -or $unstableIndex -lt 0 -or
                        $check.Arguments[$unstableIndex + 1] -cne 'unstable-options') {
                        throw 'Documentation checks must select JSON output on the pinned nightly without dependency rendering.'
                    }
                } elseif ('--output-format' -cin $check.Arguments -or 'unstable-options' -cin $check.Arguments) {
                    throw 'Documentation output flags escaped into another Rust check phase.'
                }
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
# A copied module gives the real bootstrap an external repository/cache while
# retaining the inherited Windows/profile environment. Process calls are inert.
$bootstrap = Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-ToolBootstrap-' + [guid]::NewGuid().ToString('N'))
$scripts = Join-Path $bootstrap 'scripts'
[void][IO.Directory]::CreateDirectory($scripts)
foreach ($name in @('Bonsai.Tools.psm1', 'Bonsai.Common.psm1', 'Bonsai.Native.psm1')) {
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot ('../scripts/' + $name)) -Destination $scripts
}
[IO.File]::WriteAllText((Join-Path $bootstrap 'rust-toolchain.toml'), ('[toolchain]' + "`n" + 'channel = "' + $channel.Groups[1].Value + '"' + "`n"))
$module = Import-Module (Join-Path $scripts 'Bonsai.Tools.psm1') -Force -PassThru
try {
    & $module {
        param([string]$FixtureRoot)
        $script:ToolCacheRoot = Join-Path $FixtureRoot 'cache'
        $script:ToolScratch['bonsai-tools'] = Join-Path $FixtureRoot 'evidence'
        [void][IO.Directory]::CreateDirectory($script:ToolScratch['bonsai-tools'])
        $script:ProbeRecords = [Collections.Generic.List[object]]::new()
        $script:ReplaceOutput = $false
        $script:FixtureTool = $null
        $script:VersionJson = ConvertTo-Json -Compress -InputObject @{
            Name = 'bonsai-tools'; Version = '0.1.0'; ProtocolVersion = 5
            StartupProtocolVersion = 1; BootstrapProtocolVersion = 1; ChatLaunchProtocolVersion = 1; Commands = @('source-build', 'chat-launch')
        }
        function script:Invoke-BonsaiNativeProcess {
            [CmdletBinding()]
            [OutputType([pscustomobject])]
            param([string]$Executable, [string[]]$ArgumentList, [string]$WorkingDirectory, [int]$TimeoutSeconds, [Collections.IDictionary]$Environment, [string]$LogPath)
            if (-not [IO.Path]::IsPathFullyQualified($WorkingDirectory) -or -not [IO.Directory]::Exists($WorkingDirectory)) {
                throw 'Bootstrap passed an invalid native working directory.'
            }
            $script:ProbeRecords.Add([pscustomobject]@{ Executable = $Executable; Arguments = $ArgumentList; TimeoutSeconds = $TimeoutSeconds })
            if ($ArgumentList[0] -ceq '--version') {
                return [pscustomobject]@{ ExitCode = 0; StandardOutput = $script:VersionJson; StandardError = '' }
            }
            if ($ArgumentList[0].StartsWith('+', [StringComparison]::Ordinal)) {
                if ($null -eq $Environment -or $Environment.CARGO_BUILD_FINGERPRINT -cne 'content' -or -not $LogPath) { throw 'Cold bootstrap dropped its compiler environment or evidence.' }
                [IO.File]::WriteAllBytes($script:FixtureTool, $script:FixtureBytes)
                return [pscustomobject]@{ ExitCode = 0; StandardOutput = ''; StandardError = '' }
            }
            $request = [IO.File]::ReadAllText($ArgumentList[2]) | ConvertFrom-Json -AsHashtable -NoEnumerate -DateKind String
            if ($ArgumentList[0] -ceq 'source-build') {
                $script:ProbeRecords[$script:ProbeRecords.Count - 1] | Add-Member -NotePropertyName Request -NotePropertyValue $request
                if ($script:ReplaceOutput) {
                    $stamp = [IO.File]::GetLastWriteTimeUtc($script:FixtureTool)
                    $bytes = [IO.File]::ReadAllBytes($script:FixtureTool); $bytes[511] = 1; [IO.File]::WriteAllBytes($script:FixtureTool, $bytes)
                    [IO.File]::SetLastWriteTimeUtc($script:FixtureTool, $stamp)
                }
                $digest = (Get-FileHash -LiteralPath $script:FixtureTool -Algorithm SHA256).Hash.ToLowerInvariant()
                $snapshot = Join-Path -Path $request.targetDirectory -ChildPath 'bootstrap' -AdditionalChildPath ($digest + '.exe')
                if (-not [IO.File]::Exists($snapshot)) { [IO.File]::Copy($script:FixtureTool, $snapshot) }
                $version = ConvertFrom-Json -InputObject $script:VersionJson -AsHashtable
                [IO.File]::WriteAllText((Join-Path $request.targetDirectory 'bootstrap.json'), (ConvertTo-Json -Compress -Depth 8 -InputObject @{ schemaVersion = 1; sha256 = $digest; version = $version }))
                return [pscustomobject]@{ ExitCode = 0; StandardOutput = (ConvertTo-Json -Compress -Depth 8 -InputObject @{ ToolPath = $script:FixtureTool; BootstrapPath = $snapshot; ToolSha256 = $digest; Version = $version }); StandardError = '' }
            }
            if ($ArgumentList[0] -cne 'chat-launch') { throw 'Bootstrap unexpectedly invoked a compiler or unsupported command.' }
            return [pscustomobject]@{ ExitCode = 0; StandardOutput = '{}'; StandardError = '' }
        }
        $pin = [IO.File]::ReadAllText((Join-Path $FixtureRoot 'rust-toolchain.toml'))
        if ($pin -notmatch '(?m)^channel = "(nightly-\d{4}-\d{2}-\d{2})"$') { throw 'Invalid fixture compiler pin.' }
        $identity = (Join-Path $FixtureRoot 'tools/bonsai/Cargo.toml').ToUpperInvariant() + "`n" + $Matches[1] + '-aarch64-pc-windows-msvc'
        $key = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($identity))).ToLowerInvariant().Substring(0, 24)
        $target = Join-Path $script:ToolCacheRoot $key
        [void][IO.Directory]::CreateDirectory((Join-Path $target 'release'))
        $script:FixtureTool = Join-Path $target 'release/bonsai-tools.exe'
        $script:FixtureBytes = [byte[]]::new(512)
        $script:FixtureBytes[0] = 77; $script:FixtureBytes[1] = 90
        [BitConverter]::GetBytes([int]128).CopyTo($script:FixtureBytes, 60)
        ([byte[]](80, 69, 0, 0, 100, 170)).CopyTo($script:FixtureBytes, 128)
        [IO.File]::WriteAllBytes($script:FixtureTool, $script:FixtureBytes)
        [void][IO.Directory]::CreateDirectory((Join-Path $target 'bootstrap'))
        $digest = (Get-FileHash -LiteralPath $script:FixtureTool -Algorithm SHA256).Hash.ToLowerInvariant()
        $initialSnapshot = Join-Path -Path $target -ChildPath 'bootstrap' -AdditionalChildPath ($digest + '.exe')
        [IO.File]::Copy($script:FixtureTool, $initialSnapshot)
        [IO.File]::WriteAllText((Join-Path $target 'bootstrap.json'), (ConvertTo-Json -Compress -Depth 8 -InputObject @{ schemaVersion = 1; sha256 = $digest; version = (ConvertFrom-Json -InputObject $script:VersionJson -AsHashtable) }))

        $resolved = Resolve-BonsaiRustTool -Name bonsai-tools -ManifestRelativePath '../tools/bonsai/Cargo.toml'
        $null = Invoke-BonsaiTool -Command chat-launch -ToolPath $resolved
        $probes = @($script:ProbeRecords | Where-Object { $_.Arguments[0] -ceq '--version' })
        $builds = @($script:ProbeRecords | Where-Object { $_.Arguments[0] -ceq 'source-build' })
        if ($resolved -cne $initialSnapshot -or $script:ProbeRecords.Count -ne 2 -or $probes.Count -ne 0 -or $builds.Count -ne 1 -or $builds[0].Request.forceBuild -or -not $builds[0].Request.publishBootstrap) {
            throw 'Warm bootstrap must use digest-bound Rust capabilities without an extra version process.'
        }

        # Same-size replacement with its original timestamp must still replace
        # the immutable digest identity selected for the final launch.
        $script:ReplaceOutput = $true
        $resolved = Resolve-BonsaiRustTool -Name bonsai-tools -ManifestRelativePath '../tools/bonsai/Cargo.toml' -ForceBuild
        $null = Invoke-BonsaiTool -Command chat-launch -ToolPath $resolved
        $probes = @($script:ProbeRecords | Where-Object { $_.Arguments[0] -ceq '--version' })
        $builds = @($script:ProbeRecords | Where-Object { $_.Arguments[0] -ceq 'source-build' })
        if ($probes.Count -ne 0 -or $builds.Count -ne 2 -or -not $builds[1].Request.forceBuild -or $resolved -ceq $initialSnapshot) {
            throw 'Changed tool bytes must select a new native-checked immutable image, and ForceBuild must reach Rust.'
        }

        $digest = (Get-FileHash -LiteralPath $script:FixtureTool -Algorithm SHA256).Hash.ToLowerInvariant()
        $snapshot = Join-Path -Path $target -ChildPath 'bootstrap' -AdditionalChildPath ($digest + '.exe')
        [IO.File]::WriteAllText($snapshot, 'tampered snapshot')
        $rejected = $false
        try { $null = Resolve-BonsaiRustTool -Name bonsai-tools -ManifestRelativePath '../tools/bonsai/Cargo.toml' }
        catch { $rejected = $_.Exception.Message -ceq 'The maintenance bootstrap snapshot changed.' }
        $builds = @($script:ProbeRecords | Where-Object { $_.Arguments[0] -ceq 'source-build' })
        if (-not $rejected -or $builds.Count -ne 2) { throw 'Tampered immutable bootstrap bytes reached execution.' }
        # Explicit rebuild repairs only the selected fixed cache lane, including
        # a corrupted immutable image with the same otherwise valid digest name.
        $script:ReplaceOutput = $false
        $script:FixtureBytes = [IO.File]::ReadAllBytes($script:FixtureTool)
        $resolved = Resolve-BonsaiRustTool -Name bonsai-tools -ManifestRelativePath '../tools/bonsai/Cargo.toml' -ForceBuild
        if ($resolved -cne $snapshot -or (Get-FileHash -LiteralPath $resolved -Algorithm SHA256).Hash -ine $digest) { throw 'ForceBuild did not repair the owned immutable bootstrap.' }
        [IO.File]::WriteAllText((Join-Path $target 'bootstrap.json'), '{}')
        $null = Resolve-BonsaiRustTool -Name bonsai-tools -ManifestRelativePath '../tools/bonsai/Cargo.toml' -ForceBuild
        $compilers = @($script:ProbeRecords | Where-Object { $_.Arguments[0].StartsWith('+', [StringComparison]::Ordinal) })
        if ($compilers.Count -ne 2) { throw 'ForceBuild did not repair an invalid bootstrap receipt through a cold build.' }
    } $bootstrap
} finally {
    Remove-Module -ModuleInfo $module -Force
}
Write-Output 'Rust policy and digest-bound maintenance bootstrap checks passed without starting native tools.'
