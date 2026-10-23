#requires -Version 7.6
<# .SYNOPSIS
Focused synthetic qualification scoring regressions; no model or benchmark runs.
#>
[CmdletBinding()]
[OutputType([string])]
param([Parameter(Mandatory)][ValidateSet('Scoring', 'Rejection', 'Json')][string]$Case)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot '../scripts/Bonsai.Qualification.psm1') -Force

function Assert {
    [CmdletBinding()]
    [OutputType([void])]
    param([bool]$Condition, [string]$Message)
    if (-not $Condition) { throw $Message }
}
function Assert-Failure {
    [CmdletBinding()]
    [OutputType([void])]
    param([scriptblock]$Action, [string]$Message)
    $failed = $false
    try { & $Action | Out-Null } catch { $failed = $true }
    Assert $failed $Message
}
function Copy-Fixture {
    [CmdletBinding()]
    [OutputType([Collections.IDictionary])]
    param([Collections.IDictionary]$Value)
    return ($Value | ConvertTo-Json -Depth 30 | ConvertFrom-Json -AsHashtable -DateKind String)
}
function New-Fixture {
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseShouldProcessForStateChangingFunctions', '', Justification = 'Constructs only an in-memory synthetic scorer fixture; no external state changes.')]
    [CmdletBinding()]
    [OutputType([hashtable])]
    param()
    $plan = Get-BonsaiLoraQualificationPlan
    $controls = Copy-Fixture $plan.Contract.controls
    $controls.weightRepresentation = 'PQ2_0'
    $controls.modelSha256 = $plan.Contract.representations.PQ2_0
    $digest = Get-BonsaiLoraControlDigest $controls
    $proof = Get-BonsaiQualificationDigest -Text 'synthetic proof, not actual qualification'
    $samples = [Collections.Generic.List[object]]::new()
    $records = [Collections.Generic.List[object]]::new()
    foreach ($item in $plan.Schedule) {
        $text = "Synthetic response $($item.promptId) $($item.repeat)."
        $hash = Get-BonsaiQualificationDigest -Text $text
        $samples.Add([ordered]@{
            sampleId = $item.sampleId; promptId = $item.promptId; repeat = $item.repeat; arm = $item.arm
            order = $item.order; seed = $item.seed; controlsSha256 = $digest
            decodeTokens = 100; decodeMilliseconds = 10000; peakJobMemoryBytes = 2GB + $(if ($item.arm -ceq 'adapter') { 560MB } else { 0 })
            minimumAvailablePhysicalBytes = 5GB; completed = $true; responseText = $text; responseSha256 = $hash
        })
        $scores = [ordered]@{ instructionFollowing = 3; correctness = 3; coherence = 3 }
        if ($item.promptId -like 'creative-*') { $scores.creativeUsefulness = 3 }
        $records.Add([ordered]@{
            sampleId = $item.sampleId; responseSha256 = $hash; reviewerId = 'synthetic-test-fixture-only'
            humanReviewed = $true; reviewedAtUtc = '2026-10-23T15:00:00Z'; answerComplete = $true; refused = $false
            scores = $scores; rationale = 'Synthetic fixture tests the scorer, never represents a real human review.'
        })
    }
    $zero = @($plan.Contract.prompts | ForEach-Object {
        $id = $_.id
        $base = @($samples | Where-Object { $_.promptId -ceq $id -and $_.arm -ceq 'baseline' -and $_.repeat -eq 1 })[0]
        [ordered]@{ promptId = $id; seed = 42; baselineResponseSha256 = $base.responseSha256; zeroScaleResponseSha256 = $base.responseSha256; baselineLogitsSha256 = $proof; zeroScaleLogitsSha256 = $proof }
    })
    return @{
        Evidence = @{
            schemaVersion = 1; runId = 'synthetic-unit-fixture'; contractSha256 = $plan.ContractSha256
            execution = @{ githubActions = $true; runUrl = 'https://github.com/example/synthetic/actions/runs/123'; runnerArchitecture = 'Arm64'; deviceModel = 'Snapdragon X Elite'; runnerName = 'synthetic' }
            controls = $controls
            basis = @{ status = 'verified'; evidenceSha256 = $proof; reviewerId = 'synthetic-fixture'; note = 'Synthetic operator proof for unit checks only.' }
            samples = $samples.ToArray(); zeroScaleChecks = $zero
            nonzeroAdapterCheck = @{ status = 'verified'; scale = 1; adapterSha256 = $controls.adapterSha256; logitsChanged = $true; evidenceSha256 = $proof }
        }
        Review = @{ schemaVersion = 1; runId = 'synthetic-unit-fixture'; contractSha256 = $plan.ContractSha256; source = 'external-human-review'; attestation = 'I reviewed the exact captured responses without automated scoring.'; reviews = $records.ToArray() }
    }
}

if ($Case -ceq 'Json') {
    Import-Module (Join-Path $PSScriptRoot '../scripts/Bonsai.Common.psm1') -Force
    $scratch = Resolve-ExternalPath (Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-QualificationJson-' + [guid]::NewGuid().ToString('N')))
    [void][IO.Directory]::CreateDirectory($scratch)
    $fixture = New-Fixture
    $paths = @{}
    $json = @{}
    foreach ($name in @('Evidence', 'Review')) {
        $paths[$name] = Join-Path $scratch ($name + '.json')
        $json[$name] = $fixture[$name] | ConvertTo-Json -Depth 30
        [IO.File]::WriteAllText($paths[$name], $json[$name])
    }
    $command = Join-Path $PSScriptRoot '../scripts/Get-LoraQualification.ps1'
    $result = & $command -EvidencePath $paths.Evidence -ReviewPath $paths.Review
    Assert ($result.Status -ceq 'accepted') 'JSON object evidence and timestamp-preserving review did not pass.'
    foreach ($name in @('Evidence', 'Review')) {
        foreach ($root in @("[$($json[$name])]", "[[$($json[$name])]]", '[]', 'null', 'true', '"text"')) {
            [IO.File]::WriteAllText($paths[$name], $root)
            Assert-Failure { & $command -EvidencePath $paths.Evidence -ReviewPath $paths.Review } "A non-object $name root was accepted."
        }
        [IO.File]::WriteAllText($paths[$name], $json[$name])
    }
} elseif ($Case -ceq 'Scoring') {
    $fixture = New-Fixture
    $result = Test-BonsaiLoraQualification -Evidence $fixture.Evidence -Review $fixture.Review
    Assert ($result.Status -ceq 'accepted' -and $result.StandardProfileMayEnable -and $result.Gates.Throughput -ceq 'passed' -and $result.Gates.Quality -ceq 'passed') 'Matched synthetic evidence did not pass.'
    Assert ($result.Prompts.Count -eq 4 -and $result.Prompts[0].medianJobMemoryPremiumBytes -eq 560MB) 'A modest bounded adapter RAM premium was rejected or omitted.'
    $reordered = [ordered]@{}
    foreach ($key in @($fixture.Evidence.controls.Keys | Sort-Object -Descending)) { $reordered[$key] = $fixture.Evidence.controls[$key] }
    Assert ((Get-BonsaiLoraControlDigest $reordered) -ceq (Get-BonsaiLoraControlDigest $fixture.Evidence.controls)) 'Control digests depend on JSON property order.'
    $plan = Get-BonsaiLoraQualificationPlan
    Assert ($plan.Schedule.Count -eq 24 -and $plan.Schedule[0].arm -ceq 'baseline' -and $plan.Schedule[2].arm -ceq 'adapter') 'The paired alternating schedule changed.'
    Assert ((Get-BonsaiQualificationDigest -Text "exact`n") -cne (Get-BonsaiQualificationDigest -Text 'exact')) 'Response hashing normalized captured newlines.'

    $pending = Copy-Fixture $fixture
    $pending.Evidence.basis = @{ status = 'pending'; evidenceSha256 = $null; reviewerId = $null; note = $null }
    $pending.Evidence.nonzeroAdapterCheck.status = 'pending'
    $pending.Evidence.nonzeroAdapterCheck.logitsChanged = $null
    $pending.Evidence.nonzeroAdapterCheck.evidenceSha256 = $null
    foreach ($check in $pending.Evidence.zeroScaleChecks) { $check.baselineLogitsSha256 = $null; $check.zeroScaleLogitsSha256 = $null }
    $result = Test-BonsaiLoraQualification -Evidence $pending.Evidence
    Assert ($result.Status -ceq 'pending' -and -not $result.StandardProfileMayEnable -and $result.Gates.Quality -ceq 'pending' -and $result.Gates.Throughput -ceq 'passed') 'Missing native correctness evidence or quality review was accepted.'
    $result = Test-BonsaiLoraQualification -Evidence $fixture.Evidence
    Assert ($result.Status -ceq 'pending') 'Absent human quality reviews were accepted.'
    $incomplete = Copy-Fixture $fixture
    $incomplete.Review.reviews[0].answerComplete = $false
    $result = Test-BonsaiLoraQualification -Evidence $incomplete.Evidence -Review $incomplete.Review
    Assert ($result.Status -ceq 'pending' -and $result.Gates.Quality -ceq 'pending') 'A truncated answer was treated as qualified output.'

    $refusal = Copy-Fixture $fixture
    $refusal.Review.reviews[1].refused = $true
    $result = Test-BonsaiLoraQualification -Evidence $refusal.Evidence -Review $refusal.Review
    Assert ($result.Status -ceq 'rejected' -and $result.Gates.Quality -ceq 'failed' -and
        $result.Prompts[0].adapterReviewedRefusals -eq 1) 'An explicitly reviewed unwanted refusal passed on high numeric scores.'

    $regression = Copy-Fixture $fixture
    foreach ($sample in $regression.Evidence.samples) {
        if ($sample.arm -ceq 'adapter') { $sample.decodeMilliseconds = if ($sample.promptId -ceq 'general-logic') { 11000 } else { 1000 } }
    }
    foreach ($record in $regression.Review.reviews) {
        if (@($regression.Evidence.samples | Where-Object { $_.sampleId -ceq $record.sampleId })[0].arm -ceq 'adapter') {
            foreach ($dimension in @($record.scores.Keys)) { $record.scores[$dimension] = 4 }
        }
    }
    $result = Test-BonsaiLoraQualification -Evidence $regression.Evidence -Review $regression.Review
    Assert ($result.Status -ceq 'rejected' -and $result.Gates.Throughput -ceq 'failed' -and $result.Gates.Quality -ceq 'passed') 'Better quality or faster other prompts masked a throughput regression.'

    $regression = Copy-Fixture $fixture
    foreach ($sample in $regression.Evidence.samples) { if ($sample.arm -ceq 'adapter') { $sample.decodeMilliseconds = 5000 } }
    foreach ($record in $regression.Review.reviews) {
        $sample = @($regression.Evidence.samples | Where-Object { $_.sampleId -ceq $record.sampleId })[0]
        if ($sample.arm -ceq 'adapter' -and $sample.promptId -ceq 'creative-scene') { $record.scores.creativeUsefulness = 2 }
    }
    $result = Test-BonsaiLoraQualification -Evidence $regression.Evidence -Review $regression.Review
    Assert ($result.Status -ceq 'rejected' -and $result.Gates.Throughput -ceq 'passed' -and $result.Gates.Quality -ceq 'failed') 'Better throughput masked weaker creative output.'

    $regression = Copy-Fixture $fixture
    foreach ($sample in $regression.Evidence.samples) { if ($sample.arm -ceq 'adapter') { $sample.peakJobMemoryBytes = 3GB } }
    $result = Test-BonsaiLoraQualification -Evidence $regression.Evidence -Review $regression.Review
    Assert ($result.Status -ceq 'rejected' -and $result.Gates.MemoryAllowance -ceq 'failed') 'Excessive adapter memory premium was accepted.'
    $regression = Copy-Fixture $fixture
    $regression.Evidence.samples[0].minimumAvailablePhysicalBytes = 4GB - 1
    $result = Test-BonsaiLoraQualification -Evidence $regression.Evidence -Review $regression.Review
    Assert ($result.Status -ceq 'rejected') 'Physical memory reserve violation was accepted.'
    $regression = Copy-Fixture $fixture
    $regression.Evidence.zeroScaleChecks[0].zeroScaleLogitsSha256 = Get-BonsaiQualificationDigest -Text 'different logits'
    $result = Test-BonsaiLoraQualification -Evidence $regression.Evidence -Review $regression.Review
    Assert ($result.Status -ceq 'rejected') 'Zero-scale numerical mismatch was accepted.'
    $regression = Copy-Fixture $fixture
    $regression.Evidence.nonzeroAdapterCheck.logitsChanged = $false
    $result = Test-BonsaiLoraQualification -Evidence $regression.Evidence -Review $regression.Review
    Assert ($result.Status -ceq 'rejected') 'An unapplied nonzero adapter was accepted.'
} else {
    $fixture = New-Fixture
    foreach ($bad in @([double]::NaN, [double]::PositiveInfinity, 0, -1, '10000', $true)) {
        $invalid = Copy-Fixture $fixture
        $invalid.Evidence.samples[0].decodeMilliseconds = $bad
        Assert-Failure { Test-BonsaiLoraQualification -Evidence $invalid.Evidence -Review $invalid.Review } 'Invalid timing metric was accepted.'
    }
    $mutations = @(
        { param([Collections.IDictionary]$item) $item.Evidence.samples[0].decodeTokens = 15 },
        { param([Collections.IDictionary]$item) $item.Evidence.samples[0].decodeTokens = 16.5 },
        { param([Collections.IDictionary]$item) $item.Evidence.samples[0].decodeTokens = 513 },
        { param([Collections.IDictionary]$item) $item.Evidence.samples[1] = $item.Evidence.samples[0] },
        { param([Collections.IDictionary]$item) $item.Evidence.samples = @($item.Evidence.samples | Select-Object -Skip 1) },
        { param([Collections.IDictionary]$item) $item.Evidence.samples[0].responseText += 'edited' },
        { param([Collections.IDictionary]$item) $item.Evidence.samples[0].controlsSha256 = '0' * 64 },
        { param([Collections.IDictionary]$item) $item.Evidence.samples[0].arm = 'adapter' },
        { param([Collections.IDictionary]$item) $item.Evidence.samples[0].seed = 43 },
        { param([Collections.IDictionary]$item) $item.Evidence.samples[0].seed = $true },
        { param([Collections.IDictionary]$item) $item.Evidence.samples[0].repeat = '1' },
        { param([Collections.IDictionary]$item) $item.Evidence.schemaVersion = $true },
        { param([Collections.IDictionary]$item) $item.Evidence.samples[0].completed = $false },
        { param([Collections.IDictionary]$item) $item.Evidence.controls.modelSha256 = '0' * 64 },
        { param([Collections.IDictionary]$item) $item.Evidence.controls.runtimeCommit = '0' * 40 },
        { param([Collections.IDictionary]$item) $item.Evidence.controls.adapterSha256 = '0' * 64 },
        { param([Collections.IDictionary]$item) $item.Evidence.controls.templateSha256 = '0' * 64 },
        { param([Collections.IDictionary]$item) $item.Evidence.controls.sampler.temperature = 0.7 },
        { param([Collections.IDictionary]$item) $item.Evidence.controls.extraArgument = '--unreviewed' },
        { param([Collections.IDictionary]$item) $item.Evidence.execution.githubActions = $false },
        { param([Collections.IDictionary]$item) $item.Evidence.execution.deviceModel = 'Cobalt100' },
        { param([Collections.IDictionary]$item) $item.Evidence.contractSha256 = '0' * 64 },
        { param([Collections.IDictionary]$item) $item.Evidence.zeroScaleChecks[1] = $item.Evidence.zeroScaleChecks[0] },
        { param([Collections.IDictionary]$item) $item.Review.reviews[1] = $item.Review.reviews[0] },
        { param([Collections.IDictionary]$item) $item.Review.reviews[0].responseSha256 = '0' * 64 },
        { param([Collections.IDictionary]$item) $item.Review.runId = 'another-run' },
        { param([Collections.IDictionary]$item) $item.Review.source = 'automatic-model-grader' },
        { param([Collections.IDictionary]$item) $item.Review.reviews[0].humanReviewed = $false },
        { param([Collections.IDictionary]$item) $item.Review.reviews[0].scores.correctness = 5 },
        { param([Collections.IDictionary]$item) $item.Review.reviews[0].scores.correctness = [double]::NaN },
        { param([Collections.IDictionary]$item) $item.Review.reviews[0].reviewedAtUtc = '2026-02-30T12:00:00Z' }
    )
    foreach ($mutation in $mutations) {
        $invalid = Copy-Fixture $fixture
        & $mutation $invalid
        Assert-Failure { Test-BonsaiLoraQualification -Evidence $invalid.Evidence -Review $invalid.Review } 'Malformed, mismatched, or forged-shape evidence was accepted.'
    }
}
"Focused LoRA qualification $Case checks passed. Synthetic fixtures are not model qualification."


