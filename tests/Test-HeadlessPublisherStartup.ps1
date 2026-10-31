#requires -Version 7.6
<# .SYNOPSIS
Checks candidate identity rejection, fixed model-free arguments and aggregate admission.
#>
[CmdletBinding()]
[OutputType([string])]
param()
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot 'Bonsai.TestSupport.psm1')
Import-Module (Join-Path $PSScriptRoot '../scripts/Bonsai.Common.psm1')
$script = Join-Path $PSScriptRoot '../scripts/Invoke-HeadlessPublisherStartup.ps1'
$tokens = $null
$errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile($script, [ref]$tokens, [ref]$errors)
Assert ($errors.Count -eq 0) 'Candidate startup has a PowerShell syntax error.'
foreach ($name in @('Get-HeadlessStartupPlan', 'Get-HeadlessStartupAllowance', 'Get-HeadlessModelSmokePlan', 'Get-HeadlessModelSmokeObservation', 'Get-HeadlessModelSmokeDeadline')) {
    $definition = $ast.Find({ param($node)
        $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -ceq $name
    }, $true)
    Assert ($null -ne $definition) 'A candidate startup boundary is missing.'
    . ([scriptblock]::Create($definition.Extent.Text))
}
$scratch = Resolve-ExternalPath (Join-Path ([IO.Path]::GetTempPath()) ('Bonsai-StartupPlan-' + [guid]::NewGuid().ToString('N')))
[void][IO.Directory]::CreateDirectory($scratch)
$files = @('llama-completion.exe', 'llama-server.exe', 'llama-bench.exe', 'test-headless-argv.exe', 'ggml.dll')
$pins = @($files | ForEach-Object {
    $path = Join-Path $scratch $_
    [IO.File]::WriteAllBytes($path, [byte[]]@(1, 2, 3))
    [pscustomobject]@{ Name=$_; Bytes=3; Sha256=(Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant() }
})
try {
    $plan = Get-HeadlessStartupPlan -Bundle $scratch -PackageFiles $pins
    Assert ($plan.Artifacts.Count -eq $pins.Count) 'Supervision must verify every pinned candidate file.'
    for ($index=0; $index -lt $pins.Count; $index++) {
        Assert ($plan.Artifacts[$index].sha256 -ceq $pins[$index].Sha256 -and $plan.Artifacts[$index].bytes -eq $pins[$index].Bytes) 'Candidate identity was weakened before native verification.'
    }
    $evidence = Get-Content -LiteralPath (Join-Path $PSScriptRoot '../docs/evidence/headless-publisher-startup.json') -Raw | ConvertFrom-Json -DateKind String
    Assert ($plan.Cases.Count -eq 10) 'All ten hosted model-free probes must remain requested.'
    for ($index=0; $index -lt $plan.Cases.Count; $index++) {
        $case = $plan.Cases[$index]
        $prior = $evidence.results.probes[$index]
        Assert ($case.Name -ceq $prior.name -and
            (ConvertTo-Json -InputObject $case.Arguments -Compress) -ceq (ConvertTo-Json -InputObject $prior.arguments -Compress)) 'Owner startup arguments differ from the retained hosted fixture.'
    }
    Assert-Failure -Action { Get-HeadlessStartupPlan -Bundle $scratch -PackageFiles (@($pins) + @($pins[0])) } -Message 'Duplicate pins were accepted.'
    $pins[0].Name='../llama-completion.exe'
    Assert-Failure -Action { Get-HeadlessStartupPlan -Bundle $scratch -PackageFiles $pins } -Message 'A redirected package filename was accepted.'
    $pins[0].Name=$files[0]
    [IO.File]::WriteAllBytes((Join-Path $scratch 'unreviewed.dll'), [byte[]]@(4))
    Assert-Failure -Action { Get-HeadlessStartupPlan -Bundle $scratch -PackageFiles $pins } -Message 'An additional unpinned dependency was accepted.'
    Remove-Item -LiteralPath (Join-Path $scratch 'unreviewed.dll')
    [IO.File]::WriteAllBytes((Join-Path $scratch $files[0]), [byte[]]@(4))
    Assert-Failure -Action { Get-HeadlessStartupPlan -Bundle $scratch -PackageFiles $pins } -Message 'Changed candidate length was accepted.'
    Assert-Failure -Action { Get-HeadlessStartupPlan -Bundle $scratch -PackageFiles @($pins | Select-Object -Skip 1) } -Message 'A missing executable pin was accepted.'
    $clock = [Diagnostics.Stopwatch]::StartNew()
    Assert ((Get-HeadlessStartupAllowance -Clock $clock -BudgetSeconds 60) -le 50) 'Setup was not charged or reporting time was not reserved.'
    Assert-Failure -Action { Get-HeadlessStartupAllowance -Clock $clock -BudgetSeconds 30 } -Message 'A new probe was admitted without room for its deadline and teardown.'
    Assert ((Get-HeadlessModelSmokeDeadline -Allowance 60) -eq 24 -and (Get-HeadlessModelSmokeDeadline -Allowance 80) -eq 30) 'Model hashing and teardown were not reserved before the worker deadline.'
    Assert-Failure -Action { Get-HeadlessModelSmokeDeadline -Allowance 59 } -Message 'Model use was admitted without its measured verification reserve.'
    $lock = Read-BonsaiLock -Path (Join-Path $PSScriptRoot '../config/swift-assessment.lock.json')
    $frozen = Get-Content -LiteralPath (Join-Path $PSScriptRoot '../tools/bonsai/tests/fixtures/cpu-reports-v1.json') -Raw | ConvertFrom-Json -AsHashtable -DateKind String
    $control = @($frozen.cases | Where-Object { $_.expected.Profile -ceq 'swift-pq2-assessment' -and -not $_.expected.AdapterRequested -and $_.expected.WeightRepresentation -ceq 'PQ2_0' })[0].expected
    $cpuPlan = @{
        Policy=@{ Name='swift-pq2-assessment'; WeightRepresentation='PQ2_0'; ServiceCeilingBytes=12348030976
            RequiredAvailableBytes=16642998272; AvailableMemoryFloorBytes=4294967296 }
        AdapterLock=$null; Lock=$lock; ModelPath=(Join-Path $scratch $lock.model.filename)
        Artifact=@{ Bytes=$lock.model.bytes; Sha256=$lock.model.sha256 }; Arguments=$control.EffectiveArguments.Clone()
    }
    $cpuPlan.Arguments[[array]::IndexOf($cpuPlan.Arguments, '--model') + 1]=$cpuPlan.ModelPath
    $smoke = Get-HeadlessModelSmokePlan -CpuPlan $cpuPlan -ModelLock $lock
    Assert ($smoke.Name -ceq 'pq2-four-token-smoke' -and $smoke.Artifact.sha256 -ceq $lock.model.sha256) 'Model smoke lost the exact PQ2 identity.'
    $expected = @($cpuPlan.Arguments | Where-Object { $_ -cne '--no-perf' })
    $expected[[array]::IndexOf($expected, '--n-predict') + 1]='4'
    $expected[[array]::IndexOf($expected, '--reasoning') + 1]='off'
    $expected += '--verbose-prompt'
    Assert ((ConvertTo-Json -InputObject $smoke.Arguments -Compress) -ceq (ConvertTo-Json -InputObject $expected -Compress)) 'Smoke changed CPU controls beyond its cap, thinking and diagnostic counters.'
    Assert ($control.EffectiveArguments[[array]::IndexOf($control.EffectiveArguments, '--n-predict') + 1] -ceq '16') 'Smoke mutated its shared compiled-control arguments.'
    $cpuPlan.Policy.ServiceCeilingBytes--
    Assert-Failure -Action { Get-HeadlessModelSmokePlan -CpuPlan $cpuPlan -ModelLock $lock } -Message 'A weakened model memory policy was accepted.'
    $cpuPlan.Policy.ServiceCeilingBytes++
    $cpuPlan.Artifact.Sha256=('0' * 64)
    Assert-Failure -Action { Get-HeadlessModelSmokePlan -CpuPlan $cpuPlan -ModelLock $lock } -Message 'An unpinned model artifact was accepted.'
    $cpuPlan.Artifact.Sha256=$lock.model.sha256
    $cpuPlan.AdapterLock=@{ adapter=@{} }
    Assert-Failure -Action { Get-HeadlessModelSmokePlan -CpuPlan $cpuPlan -ModelLock $lock } -Message 'An adapter entered the no-adapter smoke.'
    $cpuPlan.AdapterLock=$null
    $cpuPlan.Arguments=@($cpuPlan.Arguments) + @('--n-predict', '4096')
    Assert-Failure -Action { Get-HeadlessModelSmokePlan -CpuPlan $cpuPlan -ModelLock $lock } -Message 'A duplicated output control was accepted.'
    [IO.File]::WriteAllText((Join-Path $scratch 'stdout.txt'), 'ready')
    $diagnostic = "main: number of tokens in prompt = 12`ncommon_perf_print:    samplers time = 0.42 ms / 16 tokens`ncommon_perf_print:        eval time = 2.00 ms / 3 runs   (0.67 ms per token, 1500.00 tokens per second)`n"
    [IO.File]::WriteAllText((Join-Path $scratch 'stderr.txt'), $diagnostic)
    $observation = Get-HeadlessModelSmokeObservation -EvidenceDirectory $scratch
    Assert ($observation.SampledOutputTokens -eq 4 -and $observation.DecodeEvaluationRuns -eq 3 -and $observation.PerformanceVerdict -ceq 'not-evaluated') 'Actual inference counters were not retained separately from process creation.'
    $prefixed = $diagnostic.Replace('main:', 'llama_completion:') -replace '(?m)^(.+)$', '0.00.585.800 I $1'
    [IO.File]::WriteAllText((Join-Path $scratch 'stderr.txt'), $prefixed)
    Assert ((Get-HeadlessModelSmokeObservation -EvidenceDirectory $scratch).SampledOutputTokens -eq 4) 'The exact candidate timestamp/logger prefix was rejected.'
    [IO.File]::WriteAllText((Join-Path $scratch 'stderr.txt'), $diagnostic.Replace('/ 3 runs', '/ 2 runs'))
    Assert-Failure -Action { Get-HeadlessModelSmokeObservation -EvidenceDirectory $scratch } -Message 'Disagreeing sampled/decode counts were accepted.'
    [IO.File]::WriteAllText((Join-Path $scratch 'stderr.txt'), 'Version and process creation alone are model-free.')
    Assert-Failure -Action { Get-HeadlessModelSmokeObservation -EvidenceDirectory $scratch } -Message 'Process startup was accepted as actual inference.'
    [IO.File]::WriteAllText((Join-Path $scratch 'stderr.txt'), ($diagnostic + $diagnostic))
    Assert-Failure -Action { Get-HeadlessModelSmokeObservation -EvidenceDirectory $scratch } -Message 'Duplicated inference counters were accepted.'
} finally {
    # Only the newly created flat fixture files are removed; no recursive delete.
    foreach ($name in $files) { Remove-Item -LiteralPath (Join-Path $scratch $name) -ErrorAction SilentlyContinue }
    Remove-Item -LiteralPath (Join-Path $scratch 'unreviewed.dll') -ErrorAction SilentlyContinue
    foreach ($name in @('stdout.txt', 'stderr.txt')) { Remove-Item -LiteralPath (Join-Path $scratch $name) -ErrorAction SilentlyContinue }
    [IO.Directory]::Delete($scratch)
}
Write-Output 'Pinned candidate, model-free argument parity and aggregate budget checks passed without executable targets or models.'
