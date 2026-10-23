#requires -Version 7.6
<#
.SYNOPSIS
Measures dependency candidates on native Windows 11 ARM64 GitHub Actions.
.DESCRIPTION
Uses the pinned nightly with the new trait solver, Polonius and native CPU code
generation. JSON selection requires at least nine observations per case, a 5%
equal-weight geometric mean reduction in median time per operation, and no case
over 5% slower than serde-json. Runtime results are screening evidence only.
All native processes run from external temporary storage with inherited system
environment variables. This command is CI-only and does not change production.
#>
[CmdletBinding()]
[OutputType([string])]
param(
    [Parameter(Mandatory)][string]$EvidenceDirectory,
    [string]$TargetDirectory
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1')
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Native.psm1')

if ($env:GITHUB_ACTIONS -cne 'true' -or $env:GITHUB_REF -cne 'refs/heads/main' -or
    $env:GITHUB_REPOSITORY -cne 'symbiogenesis/SwiftBonsai2' -or $env:GITHUB_EVENT_NAME -cne 'workflow_dispatch') {
    throw 'Dependency benchmarks run only by workflow_dispatch on main in symbiogenesis/SwiftBonsai2.'
}
if (-not [OperatingSystem]::IsWindows() -or
    [Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture -ne 'Arm64' -or
    [Runtime.InteropServices.RuntimeInformation]::OSArchitecture -ne 'Arm64') {
    throw 'This trial requires native ARM64 Windows and an ARM64 PowerShell process.'
}
$operatingSystem = Get-CimInstance -ClassName Win32_OperatingSystem
if ([int]$operatingSystem.BuildNumber -lt 22000 -or [int]$operatingSystem.ProductType -ne 1) {
    throw 'This trial requires Windows 11 client build 22000 or newer.'
}
$repoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$manifest = Join-Path $repoRoot 'Cargo.toml'
$configurationPath = Join-Path $repoRoot 'config/rust-nightly-trial.json'
$configuration = Get-Content -LiteralPath $configurationPath -Raw | ConvertFrom-Json -DateKind String -NoEnumerate
if ($configuration.SchemaVersion -ne 1 -or
    $configuration.NightlyToolchain -notmatch '^nightly-\d{4}-\d{2}-\d{2}-aarch64-pc-windows-msvc$') {
    throw 'Expected a date-pinned native Windows ARM64 nightly in the trial configuration.'
}
$toolchain = $configuration.NightlyToolchain
$flags = '-Znext-solver=globally -Zpolonius=next -C target-cpu=native'
$evidence = Resolve-ExternalPath -Path $EvidenceDirectory
if (Test-Path -LiteralPath $evidence) { throw 'Use a new evidence directory for each trial.' }
[void][IO.Directory]::CreateDirectory($evidence)
$tempRoot = if ($env:RUNNER_TEMP) { $env:RUNNER_TEMP } else { [IO.Path]::GetTempPath() }
$workingDirectory = Resolve-ExternalPath -Path (Join-Path $tempRoot ('SwiftBonsai-DependencyWork-' + [guid]::NewGuid().ToString('N')))
[void][IO.Directory]::CreateDirectory($workingDirectory)
$target = Resolve-ExternalPath -Path $(if ($TargetDirectory) { $TargetDirectory } else { Join-Path $workingDirectory 'target' })
if ($target -eq $evidence -or
    $target.StartsWith($evidence + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase) -or
    $evidence.StartsWith($target + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
    throw 'Cargo build storage must be separate from uploaded evidence.'
}
[void][IO.Directory]::CreateDirectory($target)
$environment = @{ RUSTFLAGS = $flags; CARGO_TARGET_DIR = $target }
$records = [Collections.Generic.List[object]]::new()
$receipt = [ordered]@{
    schema_version = 1
    revision = $env:GITHUB_SHA
    repository = $env:GITHUB_REPOSITORY
    run_id = $env:GITHUB_RUN_ID
    run_attempt = $env:GITHUB_RUN_ATTEMPT
    toolchain = $toolchain
    rustflags = $flags
    target = 'aarch64-pc-windows-msvc'
    working_directory = $workingDirectory
    target_directory = $target
    operating_system = $operatingSystem | Select-Object Caption, Version, BuildNumber, ProductType
    process_architecture = [Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture.ToString()
    os_architecture = [Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
    cpu = @(Get-CimInstance -ClassName Win32_Processor | Select-Object Name, Manufacturer, Architecture, NumberOfCores, NumberOfLogicalProcessors)
    runner_image = "$env:ImageOS $env:ImageVersion"
    source_digests = @()
    commands = $records
    success = $false
    error = $null
    runtime_automatic_promotion = $false
}

function Invoke-DependencyTool {
    [CmdletBinding()]
    [OutputType([pscustomobject])]
    param(
        [Parameter(Mandatory)][string]$Name,
        [Parameter(Mandatory)][string]$Executable,
        [Parameter(Mandatory)][AllowEmptyCollection()][string[]]$Arguments,
        [ValidateRange(1, 900)][int]$TimeoutSeconds = 300
    )
    $log = Join-Path $evidence ($Name + '.log')
    $result = Invoke-BonsaiNativeProcess -Executable $Executable -ArgumentList $Arguments `
        -WorkingDirectory $workingDirectory -Environment $environment -TimeoutSeconds $TimeoutSeconds -LogPath $log
    $records.Add([pscustomobject]@{
        name = $Name
        executable = $Executable
        arguments = $Arguments
        exit_code = $result.ExitCode
        log = $log
    })
    if ($result.ExitCode -ne 0) { throw "$Name failed with exit code $($result.ExitCode); see $log" }
    return $result
}

function Get-CaseMedian {
    [CmdletBinding()]
    [OutputType([double])]
    param([Parameter(Mandatory)][object]$Case)
    if (-not $Case.name -or [double]$Case.iterations -le 0 -or
        [double]$Case.input_bytes -le 0 -or @($Case.elapsed_ns).Count -lt 9) {
        throw 'Every eligible case needs a name, positive input/iteration counts and at least nine observations.'
    }
    $values = @($Case.elapsed_ns | ForEach-Object {
        $value = [double]$_ / [double]$Case.iterations
        if (-not [double]::IsFinite($value) -or $value -le 0) { throw 'Timing observations must be finite and positive.' }
        $value
    } | Sort-Object)
    $middle = [int][Math]::Floor($values.Count / 2)
    if ($values.Count % 2 -eq 0) { return ($values[$middle - 1] + $values[$middle]) / 2 }
    return $values[$middle]
}

function Get-JsonSelection {
    [CmdletBinding()]
    [OutputType([pscustomobject])]
    param([Parameter(Mandatory)][object[]]$Candidates)
    if ($Candidates.Count -lt 2 -or @($Candidates.engine | Select-Object -Unique).Count -ne $Candidates.Count) {
        throw 'Expected multiple distinctly named JSON candidates.'
    }
    $baseline = @($Candidates | Where-Object { $_.engine -ceq 'serde-json' })
    if ($baseline.Count -ne 1 -or -not $baseline[0].eligible -or @($baseline[0].cases).Count -eq 0) {
        throw 'The eligible serde-json baseline and its measured cases are required.'
    }
    $baselineCases = @{}
    foreach ($case in $baseline[0].cases) {
        if ($baselineCases.ContainsKey($case.name)) { throw 'Duplicate baseline case name.' }
        $baselineCases.Add($case.name, [pscustomobject]@{ case = $case; median = Get-CaseMedian -Case $case })
    }
    $scores = [Collections.Generic.List[object]]::new()
    foreach ($candidate in $Candidates) {
        $score = [ordered]@{
            engine = $candidate.engine
            eligible = [bool]$candidate.eligible
            exclusion = $candidate.exclusion
            cases = @()
            geometric_mean_time_ratio = $null
            aggregate_gain = $null
            worst_case_time_ratio = $null
            clears_thresholds = $false
        }
        if ($candidate.eligible) {
            if (@($candidate.cases).Count -ne $baselineCases.Count -or
                @($candidate.cases.name | Select-Object -Unique).Count -ne $baselineCases.Count) {
                throw "Eligible candidate $($candidate.engine) must measure each baseline case exactly once."
            }
            $caseScores = [Collections.Generic.List[object]]::new()
            $logRatios = 0.0
            $worstRatio = 0.0
            foreach ($case in $candidate.cases) {
                if (-not $baselineCases.ContainsKey($case.name)) { throw 'Candidate case differs from the baseline corpus.' }
                $reference = $baselineCases[$case.name]
                if ($case.input_bytes -ne $reference.case.input_bytes -or $case.iterations -ne $reference.case.iterations) {
                    throw 'Candidate corpus size or operation count differs from the baseline.'
                }
                $median = Get-CaseMedian -Case $case
                $ratio = $median / $reference.median
                $logRatios += [Math]::Log($ratio)
                $worstRatio = [Math]::Max($worstRatio, $ratio)
                $caseScores.Add([pscustomobject]@{
                    name = $case.name
                    input_bytes = $case.input_bytes
                    iterations = $case.iterations
                    observations = @($case.elapsed_ns).Count
                    median_ns_per_operation = $median
                    baseline_median_ns_per_operation = $reference.median
                    time_ratio = $ratio
                })
            }
            $score.cases = $caseScores.ToArray()
            $score.geometric_mean_time_ratio = [Math]::Exp($logRatios / $baselineCases.Count)
            $score.aggregate_gain = 1.0 - $score.geometric_mean_time_ratio
            $score.worst_case_time_ratio = $worstRatio
            $score.clears_thresholds = $candidate.engine -cne 'serde-json' -and
                $score.geometric_mean_time_ratio -le 0.95 -and $worstRatio -le 1.05
        }
        $scores.Add([pscustomobject]$score)
    }
    $qualifiers = @($scores | Where-Object clears_thresholds | Sort-Object geometric_mean_time_ratio, engine)
    $selected = if ($qualifiers.Count) { $qualifiers[0].engine } else { 'serde-json' }
    return [pscustomobject]@{
        schema_version = 1
        baseline = 'serde-json'
        selected = $selected
        method = 'Equal-weight geometric mean of per-case median nanoseconds per operation.'
        minimum_observations_per_case = 9
        minimum_aggregate_time_reduction = 0.05
        maximum_case_time_regression = 0.05
        candidates = $scores.ToArray()
        runtime_automatic_promotion = $false
        runtime_limitation = 'Echo I/O screening does not validate broker admission, cancellation, deadlines, HTTP behavior or inference integration.'
    }
}

try {
    foreach ($name in @('CARGO_ENCODED_RUSTFLAGS', 'RUSTDOCFLAGS', 'CARGO_ENCODED_RUSTDOCFLAGS',
        'RUSTC', 'RUSTDOC', 'RUSTC_WRAPPER', 'RUSTC_WORKSPACE_WRAPPER', 'CARGO_BUILD_RUSTFLAGS', 'CARGO_BUILD_TARGET')) {
        if ([Environment]::GetEnvironmentVariable($name)) { throw "Remove the compiler override before the controlled comparison: $name" }
    }
    foreach ($name in [Environment]::GetEnvironmentVariables().Keys) {
        if ($name -match '^CARGO_TARGET_.+_RUSTFLAGS$' -and [Environment]::GetEnvironmentVariable($name)) {
            throw "Remove the target compiler override before the controlled comparison: $name"
        }
    }
    if ($env:GITHUB_SHA -notmatch '^[a-f0-9]{40}$') { throw 'Expected the full tested Git revision.' }
    $sourcePaths = @('Cargo.toml', 'Cargo.lock', 'config/rust-nightly-trial.json',
        'src/broker/Cargo.toml', 'src/broker/src/lib.rs', 'src/broker/src/json_trial.rs', 'src/broker/src/wire.rs',
        'scripts/Invoke-DependencyTrial.ps1', 'scripts/Bonsai.Native.psm1', '.github/workflows/dependency-trial.yml')
    $sourcePaths += @(Get-ChildItem -LiteralPath (Join-Path $repoRoot 'tools/dependency-trial') -File -Recurse |
        Where-Object { $_.Extension -in @('.rs', '.toml', '.json', '.jsonl') } |
        ForEach-Object { [IO.Path]::GetRelativePath($repoRoot, $_.FullName).Replace('\', '/') })
    $receipt.source_digests = @($sourcePaths | Sort-Object -Unique | ForEach-Object {
        [pscustomobject]@{ path = $_; sha256 = (Get-FileHash -LiteralPath (Join-Path $repoRoot $_) -Algorithm SHA256).Hash.ToLowerInvariant() }
    })
    $rustup = (Get-Command -Name rustup -CommandType Application -ErrorAction Stop).Source
    $cargo = (Get-Command -Name cargo -CommandType Application -ErrorAction Stop).Source
    $rustc = (Get-Command -Name rustc -CommandType Application -ErrorAction Stop).Source
    $git = (Get-Command -Name git -CommandType Application -ErrorAction Stop).Source
    $revision = Invoke-DependencyTool -Name 'revision' -Executable $git -Arguments @('-C', $repoRoot, 'rev-parse', 'HEAD') -TimeoutSeconds 60
    if ($revision.StandardOutput.Trim() -cne $env:GITHUB_SHA) { throw 'Checked-out revision differs from the GitHub tested revision.' }
    $status = Invoke-DependencyTool -Name 'checkout-status' -Executable $git -Arguments @('-C', $repoRoot, 'status', '--porcelain') -TimeoutSeconds 60
    if ($status.StandardOutput.Trim()) { throw 'Dependency comparisons require an unchanged checkout.' }
    $null = Invoke-DependencyTool -Name 'install' -Executable $rustup `
        -Arguments @('toolchain', 'install', $toolchain, '--profile', 'minimal', '--component', 'rustfmt,clippy', '--no-self-update') -TimeoutSeconds 300
    $identity = Invoke-DependencyTool -Name 'rustc-identity' -Executable $rustc -Arguments @(('+' + $toolchain), '-vV') -TimeoutSeconds 60
    if ($identity.StandardOutput -notmatch '(?m)^host: aarch64-pc-windows-msvc\r?$' -or
        $identity.StandardOutput -notmatch '(?m)^commit-hash: [a-f0-9]{40}\r?$') {
        throw 'Expected an identified native Windows ARM64 compiler.'
    }
    $receipt['compiler_identity'] = $identity.StandardOutput.Trim()
    $null = Invoke-DependencyTool -Name 'cargo-identity' -Executable $cargo -Arguments @(('+' + $toolchain), '-V') -TimeoutSeconds 60
    $prefix = @(('+' + $toolchain))
    $null = Invoke-DependencyTool -Name 'format' -Executable $cargo `
        -Arguments ($prefix + @('fmt', '--manifest-path', $manifest, '--check', '-p', 'bonsai-dependency-trial', '-p', 'bonsai-broker'))
    $null = Invoke-DependencyTool -Name 'clippy-release' -Executable $cargo `
        -Arguments ($prefix + @('clippy', '--manifest-path', $manifest, '--locked', '--release', '--all-targets', '-p', 'bonsai-dependency-trial', '--', '-D', 'warnings'))
    $null = Invoke-DependencyTool -Name 'build-release' -Executable $cargo `
        -Arguments ($prefix + @('build', '--manifest-path', $manifest, '--locked', '--release', '-p', 'bonsai-dependency-trial'))
    $null = Invoke-DependencyTool -Name 'correctness-release' -Executable $cargo `
        -Arguments ($prefix + @('test', '--manifest-path', $manifest, '--locked', '--release', '-p', 'bonsai-dependency-trial'))
    $binary = Join-Path $target 'release/bonsai-dependency-trial.exe'
    $measurement = Invoke-DependencyTool -Name 'measure' -Executable $binary -Arguments @() -TimeoutSeconds 120
    [IO.File]::WriteAllText((Join-Path $evidence 'results.json'), $measurement.StandardOutput)
    $report = $measurement.StandardOutput | ConvertFrom-Json -DateKind String -NoEnumerate
    if ($report.schema_version -ne 1 -or @($report.runtime).Count -ne 4) { throw 'Unsupported or incomplete dependency report.' }
    $selection = Get-JsonSelection -Candidates @($report.json)
    $runtimeBaseline = @($report.runtime | Where-Object { $_.engine -ceq 'tokio' })
    if ($runtimeBaseline.Count -ne 1) { throw 'Runtime screening requires the Tokio baseline.' }
    $runtimeCases = @{}
    foreach ($case in $runtimeBaseline[0].cases) {
        if ($runtimeCases.ContainsKey($case.name)) { throw 'Duplicate runtime baseline case.' }
        $runtimeCases.Add($case.name, [pscustomobject]@{ case = $case; median = Get-CaseMedian -Case $case })
    }
    $runtimeScores = [Collections.Generic.List[object]]::new()
    if (@($report.runtime.engine | Select-Object -Unique).Count -ne 4) { throw 'Runtime candidate names must be distinct.' }
    foreach ($candidate in $report.runtime) {
        if (-not $candidate.eligible -or @($candidate.cases).Count -ne 4 -or
            @($candidate.cases.name | Select-Object -Unique).Count -ne 4) {
            throw 'Every runtime candidate must produce all four distinct screening cases.'
        }
        $caseScores = [Collections.Generic.List[object]]::new()
        $logRatios = 0.0
        foreach ($case in $candidate.cases) {
            if (-not $runtimeCases.ContainsKey($case.name)) { throw 'Runtime candidate case differs from the common workload.' }
            $reference = $runtimeCases[$case.name]
            if ($case.input_bytes -ne $reference.case.input_bytes -or $case.iterations -ne $reference.case.iterations) {
                throw 'Runtime candidates must execute identical byte and operation counts.'
            }
            $median = Get-CaseMedian -Case $case
            $ratio = $median / $reference.median
            $logRatios += [Math]::Log($ratio)
            $caseScores.Add([pscustomobject]@{
                name = $case.name
                input_bytes = $case.input_bytes
                iterations = $case.iterations
                observations = @($case.elapsed_ns).Count
                median_ns_per_operation = $median
                baseline_median_ns_per_operation = $reference.median
                time_ratio = $ratio
            })
        }
        $runtimeScores.Add([pscustomobject]@{
            engine = $candidate.engine
            geometric_mean_time_ratio = [Math]::Exp($logRatios / 4)
            cases = $caseScores.ToArray()
            automatically_promotable = $false
        })
    }
    $selection | Add-Member -NotePropertyName runtime_screening -NotePropertyValue $runtimeScores.ToArray()
    $selection | ConvertTo-Json -Depth 16 | Set-Content -LiteralPath (Join-Path $evidence 'selection.json') -Encoding utf8
    if ($env:GITHUB_STEP_SUMMARY) {
        @(
            "Revision: $env:GITHUB_SHA"
            "Compiler: $toolchain"
            "Flags: $flags"
            "JSON selection: **$($selection.selected)** (>=5% aggregate reduction; no case >5% slower; >=9 observations/case)."
            ''
            '| JSON candidate | Eligible | Aggregate time reduction | Worst case time ratio | Clears thresholds |'
            '| --- | --- | ---: | ---: | --- |'
        ) | Add-Content -LiteralPath $env:GITHUB_STEP_SUMMARY
        foreach ($score in $selection.candidates) {
            $gain = if ($null -ne $score.aggregate_gain) { '{0:P2}' -f $score.aggregate_gain } else { 'excluded' }
            $worst = if ($null -ne $score.worst_case_time_ratio) { '{0:F4}' -f $score.worst_case_time_ratio } else { '-' }
            "| $($score.engine) | $($score.eligible) | $gain | $worst | $($score.clears_thresholds) |" | Add-Content -LiteralPath $env:GITHUB_STEP_SUMMARY
        }
        @('', '| Runtime candidate | Geometric mean time ratio vs Tokio |', '| --- | ---: |') |
            Add-Content -LiteralPath $env:GITHUB_STEP_SUMMARY
        foreach ($score in $runtimeScores) {
            '| {0} | {1:F4} |' -f $score.engine, $score.geometric_mean_time_ratio |
                Add-Content -LiteralPath $env:GITHUB_STEP_SUMMARY
        }
        @('', 'Runtime results are screening evidence. Production promotion requires broker lifecycle and cancellation validation.',
            'Raw observations, medians, selection scores, source digests, CPU/OS identity and command logs are retained in the artifact.') |
            Add-Content -LiteralPath $env:GITHUB_STEP_SUMMARY
    }
    $receipt.success = $true
} catch {
    $receipt.error = $_.Exception.Message
    throw
} finally {
    $receipt | ConvertTo-Json -Depth 16 | Set-Content -LiteralPath (Join-Path $evidence 'receipt.json') -Encoding utf8
}
Write-Output "Dependency trial complete; selected JSON engine: $($selection.selected); evidence: $evidence"
