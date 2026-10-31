#requires -Version 7.6
<# .SYNOPSIS
Checks the exact archived hosted publisher candidate on X2 without loading models.
.DESCRIPTION
Requires an already extracted candidate and prebuilt maintenance tool. Fixed
version/help and seven argument-only probes use the required headless supervisor.
All setup, identity verification, probes and teardown share one bounded budget;
this is model-free startup evidence, not runtime admission or a thread benchmark.
#>
[CmdletBinding()]
[OutputType([pscustomobject])]
param(
    [Parameter(Mandatory)][string]$CandidateDirectory,
    [Parameter(Mandatory)][string]$ToolPath,
    [Parameter(Mandatory)][string]$OutputDirectory,
    [ValidateRange(45,180)][int]$WorkBudgetSeconds = 90
)
$workClock = [Diagnostics.Stopwatch]::StartNew()
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
if (-not $IsWindows -or [Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture -ne 'Arm64' -or
    [Runtime.InteropServices.RuntimeInformation]::OSArchitecture -ne 'Arm64') {
    throw 'Candidate startup requires native Windows ARM64 PowerShell.'
}
foreach ($name in @('ACTIONS_RUNTIME_TOKEN', 'ACTIONS_ID_TOKEN_REQUEST_TOKEN', 'GITHUB_TOKEN', 'GH_TOKEN', 'ACTIONS_RUNNER_INPUT_TOKEN')) {
    if (Test-Path -LiteralPath ('Env:' + $name)) { throw 'Run the model-free candidate probe without inherited CI credentials.' }
}
if (@(Get-ChildItem Env: | Where-Object Name -Match '^(LLAMA_|LLAMACPP_|GGML_|AIP_|MTMD_|HF_TOKEN$|HF_ENDPOINT$|MODEL_ENDPOINT$)').Count) {
    throw 'Inherited model/backend overrides are unsupported for candidate startup.'
}
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Common.psm1')
Import-Module (Join-Path $PSScriptRoot 'Bonsai.Tools.psm1')

function Get-HeadlessStartupAllowance {
    <# .SYNOPSIS
    Reserves reporting time after the native aggregate verification/teardown budget.
    #>
    [CmdletBinding()]
    [OutputType([int])]
    param([Diagnostics.Stopwatch]$Clock, [int]$BudgetSeconds)
    $remaining = [int][Math]::Floor($BudgetSeconds - $Clock.Elapsed.TotalSeconds) - 10
    if ($remaining -lt 24) { throw 'Candidate startup budget is exhausted; remaining probes stay pending.' }
    return $remaining
}

function Get-HeadlessStartupPlan {
    <# .SYNOPSIS
    Allows only fixed model-free probes of the complete pinned hosted bundle.
    #>
    [CmdletBinding()]
    [OutputType([hashtable])]
    param([string]$Bundle, [object[]]$PackageFiles)
    if ($PackageFiles.Count -lt 4 -or $PackageFiles.Count -gt 32) { throw 'Invalid candidate package inventory.' }
    $names = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
    $artifacts = foreach ($file in $PackageFiles) {
        if ($file.Name -cnotmatch '^[a-zA-Z0-9_.-]+$' -or -not $names.Add($file.Name) -or
            $file.Bytes -le 0 -or $file.Sha256 -cnotmatch '^[a-f0-9]{64}$') { throw 'Invalid candidate package identity.' }
        $path = Resolve-ExternalPath (Join-Path $Bundle $file.Name)
        $item = Get-Item -LiteralPath $path -Force
        if ($item.PSIsContainer -or $item.Length -ne $file.Bytes) { throw 'A pinned candidate file is missing or has changed length.' }
        @{ path=$path; bytes=$file.Bytes; sha256=$file.Sha256 }
    }
    $actual = @(Get-ChildItem -LiteralPath $Bundle -Force)
    if ($actual.Count -ne $names.Count -or @($actual | Where-Object { $_.PSIsContainer -or -not $names.Contains($_.Name) }).Count) {
        throw 'Candidate bundle contains unpinned entries.'
    }
    foreach ($required in @('llama-completion.exe', 'llama-server.exe', 'llama-bench.exe', 'test-headless-argv.exe')) {
        if (-not $names.Contains($required)) { throw 'A required model-free candidate executable is not pinned.' }
    }
    $cases = @(
        @{ Name='completion-version'; Executable='llama-completion.exe'; Arguments=@('--version') },
        @{ Name='server-version'; Executable='llama-server.exe'; Arguments=@('--version') },
        @{ Name='bench-help'; Executable='llama-bench.exe'; Arguments=@('--help') }
    )
    $vectors = @('', '日本語 🪴 café', 'C:\notes dir\ "quoted" tail\\', "line1`tline2`nline3", '*.gguf', '  leading and trailing  ', 'slashes\\\"quote')
    for ($index=0; $index -lt $vectors.Count; $index++) {
        $cases += @{ Name=('argv-' + $index); Executable='test-headless-argv.exe'
            Arguments=@('--model', '模型/test.gguf', '--seed', [string]$index, '--prompt', $vectors[$index]) }
    }
    return @{ Artifacts=@($artifacts); Cases=$cases }
}

$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$pinPath = Join-Path $repo 'docs/evidence/headless-publisher-startup.json'
$pin = Get-Content -LiteralPath $pinPath -Raw | ConvertFrom-Json -DateKind String
$source = Get-Content -LiteralPath (Join-Path $repo 'docs/runtime-candidates/headless-ucrt-argv.json') -Raw | ConvertFrom-Json -DateKind String
if ($pin.schemaVersion -ne 1 -or $pin.kind -cne 'publisher-headless-hosted-model-free-startup' -or
    $pin.results.allProbesPassed -cne $true -or $pin.execution.hosted -cne $true -or
    $pin.source.base.commit -cne $source.base.commit -or $pin.source.patch.sha256 -cne $source.patch.sha256 -or
    $pin.results.packageFiles.Count -ne 14) { throw 'The reviewed hosted candidate evidence is incomplete or disagrees with the source candidate.' }
$candidate = Resolve-ExternalPath $CandidateDirectory
$tool = Resolve-ExternalPath $ToolPath
$output = Resolve-ExternalPath $OutputDirectory
$bundle = Resolve-ExternalPath (Join-Path $candidate 'bundle')
if ($output.Equals($candidate, [StringComparison]::OrdinalIgnoreCase) -or
    $output.StartsWith($candidate.TrimEnd('\') + '\', [StringComparison]::OrdinalIgnoreCase)) {
    throw 'Startup evidence must be separate from the archived candidate.'
}
$plan = Get-HeadlessStartupPlan -Bundle $bundle -PackageFiles $pin.results.packageFiles
if (-not [IO.File]::Exists($tool)) { throw 'Provide an existing prebuilt maintenance tool; owner-device builds are disabled.' }
if (Test-Path -LiteralPath $output) { throw 'Use a fresh external startup evidence directory.' }
[void][IO.Directory]::CreateDirectory($output)
$receiptPath = Join-Path $output 'startup.json'
$receipt = [ordered]@{
    SchemaVersion=1; Status='preparing'; Scope='x2-hosted-candidate-model-free-startup-only'
    HostedRunId=$pin.execution.runId; HostedArtifactSha256=$pin.artifact.sha256
    HostedEvidenceSha256=(Get-FileHash -LiteralPath $pinPath -Algorithm SHA256).Hash.ToLowerInvariant()
    Source=$pin.source; PackageFiles=$pin.results.packageFiles
    SupervisorSha256=$null
    WorkerPolicy='ordinary-required-headless'; WorkBudgetSeconds=$WorkBudgetSeconds
    StartedAtUtc=[DateTime]::UtcNow.ToString('o'); Probes=@(); ModelLoaded=$false
    ProductionRuntimePinChanged=$false; NumericalOrPerformanceQualified=$false
}
$toolGuard = $null
try {
    # Keep the observed supervisor immutable, including its cached version handshake.
    $toolGuard = [IO.FileStream]::new($tool, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
    $receipt.SupervisorSha256=[Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($toolGuard)).ToLowerInvariant()
    $receipt | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $receiptPath -Encoding utf8NoBOM
    $allowance = Get-HeadlessStartupAllowance -Clock $workClock -BudgetSeconds $WorkBudgetSeconds
    $native = Invoke-BonsaiTool -Command host-cpu -ToolPath $tool -TimeoutSeconds ([Math]::Min(10, $allowance))
    $receipt['HostIdentity']=$native
    if ($native.status -cne 'observed' -or $native.architecture -cne 'aarch64' -or
        $native.host_identity.processor_name -cnotmatch '^Snapdragon.* X2') {
        throw 'This owner-device probe requires an observed native Snapdragon X2 identity.'
    }
    # A wildcard sentinel makes unintended CRT wildcard expansion observable.
    [IO.File]::WriteAllText((Join-Path $output 'literal-wildcard-fixture.gguf'), 'Synthetic argv sentinel; not a model.')
    $receipt.Status='probing'
    foreach ($case in $plan.Cases) {
        $lane = Join-Path $output $case.Name
        [void][IO.Directory]::CreateDirectory($lane)
        $receipt['PendingProbe']=$case.Name
        $receipt | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $receiptPath -Encoding utf8NoBOM
        $allowance = Get-HeadlessStartupAllowance -Clock $workClock -BudgetSeconds $WorkBudgetSeconds
        $result = Invoke-BonsaiTool -Command completion -ToolPath $tool -TimeoutSeconds $allowance -Request @{
            schemaVersion=1; executable=(Join-Path $bundle $case.Executable); arguments=$case.Arguments
            workingDirectory=$output; evidenceDirectory=$lane; artifacts=$plan.Artifacts
            memoryLimitBytes=536870912; requiredAvailableBytes=536870912; physicalReserveBytes=0
            timeoutSeconds=10; totalTimeoutSeconds=($allowance - 1)
            stdoutLimitBytes=65536; stderrLimitBytes=65536; requireNonemptyStdout=$false; ciOnly=$false
        }
        $receipt.Probes += @{ Name=$case.Name; Arguments=$case.Arguments; Result=$result; ModelLoaded=$false }
    }
    $receipt.Remove('PendingProbe')
    $receipt.Status='passed'
} catch {
    $receipt.Status='failed-or-pending'
    throw
} finally {
    try {
        $receipt['ElapsedMilliseconds']=$workClock.ElapsedMilliseconds
        $receipt | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $receiptPath -Encoding utf8NoBOM
    } finally {
        if ($null -ne $toolGuard) { $toolGuard.Dispose() }
    }
}
[pscustomobject]@{ Status=$receipt.Status; EvidenceDirectory=$output; ModelLoaded=$false; RuntimePinChanged=$false }
