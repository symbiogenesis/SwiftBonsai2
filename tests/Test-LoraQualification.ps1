#requires -Version 7.6
<# .SYNOPSIS
Focused synthetic qualification scoring regressions; no model or benchmark runs.
#>
[CmdletBinding()]
[OutputType([string])]
param([Parameter(Mandatory)][ValidateSet('Scoring', 'Rejection', 'Json', 'Compatibility', 'AppSettings')][string]$Case, [string]$ToolPath)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
Import-Module (Join-Path $PSScriptRoot '../scripts/Bonsai.Qualification.psm1')
$toolArguments = @{}
if ($ToolPath) { $toolArguments.ToolPath = $ToolPath }

Import-Module (Join-Path $PSScriptRoot 'Bonsai.TestSupport.psm1')
$golden = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'fixtures/lora-qualification-v1.json') -Raw | ConvertFrom-Json -AsHashtable -DateKind String -Depth 40

function Assert-JsonEqual {
    [CmdletBinding()]
    [OutputType([void])]
    param([object]$Actual, [object]$Expected, [string]$Message)
    $actualNode = [Text.Json.Nodes.JsonNode]::Parse(($Actual | ConvertTo-Json -Depth 40))
    $expectedNode = [Text.Json.Nodes.JsonNode]::Parse(($Expected | ConvertTo-Json -Depth 40))
    Assert ([Text.Json.Nodes.JsonNode]::DeepEquals($actualNode, $expectedNode)) $Message
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
    # The frozen accepted scenario is identical to the former generated data.
    # Clone the evidence and review so each rejection probe owns its mutations.
    $accepted = @($golden.scenarios | Where-Object name -CEQ 'accepted')
    Assert ($accepted.Count -eq 1) 'The frozen accepted qualification scenario is missing or repeated.'
    return @{
        Evidence = Copy-Fixture $accepted[0].evidence
        Review = Copy-Fixture $accepted[0].review
    }
}

if ($Case -ceq 'AppSettings') {
    $standard = Get-BonsaiLoraQualificationPlan @toolArguments
    $app = Get-BonsaiLoraQualificationPlan @toolArguments -AppSettings
    Assert ($app.Contract.adapterScale -eq 1.3 -and $app.Contract.controls.maxOutputTokens -eq 512 -and
        $app.ContractSha256 -cne $standard.ContractSha256) 'App scale lacks its separate full-response contract.'
    $scratch = Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-AppQualification-' + [guid]::NewGuid().ToString('N'))
    [void][IO.Directory]::CreateDirectory($scratch)
    $plan = & (Join-Path $PSScriptRoot '../scripts/Invoke-LoraQualification.ps1') @toolArguments -DataDirectory $scratch -UseRepackedPtq1 -AppSettings -PlanOnly
    Assert ($plan.AdapterScale -eq 1.3 -and $plan.QualificationProfile -ceq 'app-scale-1.3' -and
        $plan.ContractSha256 -ceq $app.ContractSha256 -and -not $plan.ModelProcessStarted) 'AppSettings did not reach PlanOnly.'
    $fixture = New-Fixture
    $fixture.Evidence.contractSha256 = $app.ContractSha256
    $fixture.Evidence.nonzeroAdapterCheck.scale = 1.3
    $fixture.Evidence.controls.adapterScale = 1.3
    $digest = Get-BonsaiLoraControlDigest @toolArguments -Controls $fixture.Evidence.controls -AppSettings
    foreach ($sample in $fixture.Evidence.samples) { $sample.controlsSha256 = $digest }
    $fixture.Review.contractSha256 = $app.ContractSha256
    $result = Test-BonsaiLoraQualification @toolArguments -Evidence $fixture.Evidence -Review $fixture.Review -AppSettings
    Assert ($result.Status -ceq 'accepted' -and $result.AppSettingGatesPassed -and
        -not $result.StandardProfileMayEnable) 'The separate app gate bypassed its scope or failed matched evidence.'
    $evidencePath = Join-Path $scratch 'evidence.json'
    $reviewPath = Join-Path $scratch 'review.json'
    [IO.File]::WriteAllText($evidencePath, ($fixture.Evidence | ConvertTo-Json -Depth 30))
    [IO.File]::WriteAllText($reviewPath, ($fixture.Review | ConvertTo-Json -Depth 30))
    $command = Join-Path $PSScriptRoot '../scripts/Get-LoraQualification.ps1'
    $result = & $command @toolArguments -EvidencePath $evidencePath -ReviewPath $reviewPath -AppSettings
    Assert ($result.AppSettingGatesPassed -and $result.AdapterScale -eq 1.3) 'AppSettings did not reach file scoring.'
    Assert-Failure { & $command @toolArguments -EvidencePath $evidencePath -ReviewPath $reviewPath } 'Scale-one scoring accepted app evidence.'
} elseif ($Case -ceq 'Compatibility') {
    Assert-JsonEqual -Actual (Get-BonsaiLoraQualificationPlan @toolArguments) -Expected $golden.plan -Message 'The packaged plan differs from the frozen PowerShell contract.'
    foreach ($vector in $golden.controlVectors) {
        Assert ((Get-BonsaiLoraControlDigest @toolArguments $vector.controls) -ceq $vector.sha256) 'A legacy canonical control identity changed.'
    }
    foreach ($vector in $golden.textVectors) {
        Assert ((Get-BonsaiQualificationDigest -Text $vector.text) -ceq $vector.sha256) 'Exact Unicode/newline text hashing changed.'
    }
    foreach ($scenario in $golden.scenarios) {
        $result = Test-BonsaiLoraQualification @toolArguments -Evidence $scenario.evidence -Review $scenario.review
        Assert-JsonEqual -Actual $result -Expected $scenario.expected -Message "Frozen $($scenario.name) scoring changed."
    }
} elseif ($Case -ceq 'Json') {
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
    $result = & $command @toolArguments -EvidencePath $paths.Evidence -ReviewPath $paths.Review
    Assert ($result.Status -ceq 'accepted') 'JSON object evidence and timestamp-preserving review did not pass.'
    foreach ($name in @('Evidence', 'Review')) {
        foreach ($root in @("[$($json[$name])]", "[[$($json[$name])]]", '[]', 'null', 'true', '"text"')) {
            [IO.File]::WriteAllText($paths[$name], $root)
            Assert-Failure { & $command @toolArguments -EvidencePath $paths.Evidence -ReviewPath $paths.Review } "A non-object $name root was accepted."
        }
        [IO.File]::WriteAllText($paths[$name], $json[$name])
    }
} elseif ($Case -ceq 'Scoring') {
    $fixture = New-Fixture
    $result = Test-BonsaiLoraQualification @toolArguments -Evidence $fixture.Evidence -Review $fixture.Review
    Assert ($result.Status -ceq 'accepted' -and $result.StandardProfileMayEnable -and $result.Gates.Throughput -ceq 'passed' -and $result.Gates.Quality -ceq 'passed') 'Matched synthetic evidence did not pass.'
    Assert ($result.Prompts.Count -eq 4 -and $result.Prompts[0].medianJobMemoryPremiumBytes -eq 560MB) 'A modest bounded adapter RAM premium was rejected or omitted.'
    $reordered = [ordered]@{}
    foreach ($key in @($fixture.Evidence.controls.Keys | Sort-Object -Descending)) { $reordered[$key] = $fixture.Evidence.controls[$key] }
    Assert ((Get-BonsaiLoraControlDigest @toolArguments $reordered) -ceq (Get-BonsaiLoraControlDigest @toolArguments $fixture.Evidence.controls)) 'Control digests depend on JSON property order.'
    $plan = Get-BonsaiLoraQualificationPlan @toolArguments
    Assert ($plan.Schedule.Count -eq 24 -and $plan.Schedule[0].arm -ceq 'baseline' -and $plan.Schedule[2].arm -ceq 'adapter') 'The paired alternating schedule changed.'
    Assert ((Get-BonsaiQualificationDigest -Text "exact`n") -cne (Get-BonsaiQualificationDigest -Text 'exact')) 'Response hashing normalized captured newlines.'

    $pending = Copy-Fixture $fixture
    $pending.Evidence.basis = @{ status = 'pending'; evidenceSha256 = $null; reviewerId = $null; note = $null }
    $pending.Evidence.nonzeroAdapterCheck.status = 'pending'
    $pending.Evidence.nonzeroAdapterCheck.logitsChanged = $null
    $pending.Evidence.nonzeroAdapterCheck.evidenceSha256 = $null
    foreach ($check in $pending.Evidence.zeroScaleChecks) { $check.baselineLogitsSha256 = $null; $check.zeroScaleLogitsSha256 = $null }
    $result = Test-BonsaiLoraQualification @toolArguments -Evidence $pending.Evidence
    Assert ($result.Status -ceq 'pending' -and -not $result.StandardProfileMayEnable -and $result.Gates.Quality -ceq 'pending' -and $result.Gates.Throughput -ceq 'passed') 'Missing native correctness evidence or quality review was accepted.'
    $result = Test-BonsaiLoraQualification @toolArguments -Evidence $fixture.Evidence
    Assert ($result.Status -ceq 'pending') 'Absent human quality reviews were accepted.'
    $incomplete = Copy-Fixture $fixture
    $incomplete.Review.reviews[0].answerComplete = $false
    $result = Test-BonsaiLoraQualification @toolArguments -Evidence $incomplete.Evidence -Review $incomplete.Review
    Assert ($result.Status -ceq 'pending' -and $result.Gates.Quality -ceq 'pending') 'A truncated answer was treated as qualified output.'

    $refusal = Copy-Fixture $fixture
    $refusal.Review.reviews[1].refused = $true
    $result = Test-BonsaiLoraQualification @toolArguments -Evidence $refusal.Evidence -Review $refusal.Review
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
    $result = Test-BonsaiLoraQualification @toolArguments -Evidence $regression.Evidence -Review $regression.Review
    Assert ($result.Status -ceq 'rejected' -and $result.Gates.Throughput -ceq 'failed' -and $result.Gates.Quality -ceq 'passed') 'Better quality or faster other prompts masked a throughput regression.'

    $regression = Copy-Fixture $fixture
    foreach ($sample in $regression.Evidence.samples) { if ($sample.arm -ceq 'adapter') { $sample.decodeMilliseconds = 5000 } }
    foreach ($record in $regression.Review.reviews) {
        $sample = @($regression.Evidence.samples | Where-Object { $_.sampleId -ceq $record.sampleId })[0]
        if ($sample.arm -ceq 'adapter' -and $sample.promptId -ceq 'creative-scene') { $record.scores.creativeUsefulness = 2 }
    }
    $result = Test-BonsaiLoraQualification @toolArguments -Evidence $regression.Evidence -Review $regression.Review
    Assert ($result.Status -ceq 'rejected' -and $result.Gates.Throughput -ceq 'passed' -and $result.Gates.Quality -ceq 'failed') 'Better throughput masked weaker creative output.'

    $regression = Copy-Fixture $fixture
    foreach ($sample in $regression.Evidence.samples) { if ($sample.arm -ceq 'adapter') { $sample.peakJobMemoryBytes = 3GB } }
    $result = Test-BonsaiLoraQualification @toolArguments -Evidence $regression.Evidence -Review $regression.Review
    Assert ($result.Status -ceq 'rejected' -and $result.Gates.MemoryAllowance -ceq 'failed') 'Excessive adapter memory premium was accepted.'
    $regression = Copy-Fixture $fixture
    $regression.Evidence.samples[0].minimumAvailablePhysicalBytes = 4GB - 1
    $result = Test-BonsaiLoraQualification @toolArguments -Evidence $regression.Evidence -Review $regression.Review
    Assert ($result.Status -ceq 'rejected') 'Physical memory reserve violation was accepted.'
    $regression = Copy-Fixture $fixture
    $regression.Evidence.zeroScaleChecks[0].zeroScaleLogitsSha256 = Get-BonsaiQualificationDigest -Text 'different logits'
    $result = Test-BonsaiLoraQualification @toolArguments -Evidence $regression.Evidence -Review $regression.Review
    Assert ($result.Status -ceq 'rejected') 'Zero-scale numerical mismatch was accepted.'
    $regression = Copy-Fixture $fixture
    $regression.Evidence.nonzeroAdapterCheck.logitsChanged = $false
    $result = Test-BonsaiLoraQualification @toolArguments -Evidence $regression.Evidence -Review $regression.Review
    Assert ($result.Status -ceq 'rejected') 'An unapplied nonzero adapter was accepted.'
} else {
    $fixture = New-Fixture
    foreach ($bad in @([double]::NaN, [double]::PositiveInfinity, 0, -1, '10000', $true)) {
        $invalid = Copy-Fixture $fixture
        $invalid.Evidence.samples[0].decodeMilliseconds = $bad
        Assert-Failure { Test-BonsaiLoraQualification @toolArguments -Evidence $invalid.Evidence -Review $invalid.Review } 'Invalid timing metric was accepted.'
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
        Assert-Failure { Test-BonsaiLoraQualification @toolArguments -Evidence $invalid.Evidence -Review $invalid.Review } 'Malformed, mismatched, or forged-shape evidence was accepted.'
    }
}
"Focused LoRA qualification $Case checks passed. Synthetic fixtures are not model qualification."


