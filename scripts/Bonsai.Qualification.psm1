#requires -Version 7.6
# This scorer consumes captured evidence and external human reviews. It does not
# launch inference, execute benchmarks, invent quality scores, or authenticate a
# reviewer's identity. Hash binding detects mismatched records, not dishonesty.
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true

function Assert-QualificationShape {
    [CmdletBinding()]
    [OutputType([void])]
    param([AllowNull()][object]$Value, [string[]]$Required, [string]$Name, [string[]]$Optional = @())
    if ($Value -isnot [Collections.IDictionary]) { throw "$Name must be a JSON object." }
    $keys = @($Value.Keys)
    $allowed = $Required + $Optional
    foreach ($key in $Required) {
        if ($key -cnotin $keys) { throw "$Name is missing $key." }
    }
    foreach ($key in $keys) {
        if ($key -isnot [string] -or $key -cnotin $allowed) { throw "$Name contains an unrecognized field." }
    }
}

function Assert-QualificationString {
    [CmdletBinding()]
    [OutputType([void])]
    param([AllowNull()][object]$Value, [string]$Name, [int]$MaximumLength = 4096)
    if ($Value -isnot [string] -or [string]::IsNullOrWhiteSpace($Value) -or $Value.Length -gt $MaximumLength) {
        throw "$Name must be a nonempty bounded string."
    }
}

function Assert-QualificationHash {
    [CmdletBinding()]
    [OutputType([void])]
    param([AllowNull()][object]$Value, [string]$Name, [switch]$Nullable)
    if ($Nullable -and $null -eq $Value) { return }
    if ($Value -isnot [string] -or $Value -cnotmatch '^[a-f0-9]{64}$') { throw "$Name must be a lowercase SHA-256 digest." }
}

function Assert-QualificationNumber {
    [CmdletBinding()]
    [OutputType([void])]
    param([AllowNull()][object]$Value, [string]$Name, [double]$Minimum, [double]$Maximum, [switch]$Integer)
    if ($Value -isnot [byte] -and $Value -isnot [int16] -and $Value -isnot [int] -and $Value -isnot [long] -and
        $Value -isnot [single] -and $Value -isnot [double] -and $Value -isnot [decimal]) { throw "$Name must be numeric." }
    $number = [double]$Value
    if (-not [double]::IsFinite($number) -or $number -lt $Minimum -or $number -gt $Maximum -or
        ($Integer -and [Math]::Truncate($number) -ne $number)) { throw "$Name is outside its finite valid range." }
}

function Get-BonsaiQualificationDigest {
    <# .SYNOPSIS
    Hashes exact UTF-8 text without normalization or a byte-order mark.
    #>
    [CmdletBinding()]
    [OutputType([string])]
    param([Parameter(Mandatory)][AllowEmptyString()][string]$Text)
    return [Convert]::ToHexStringLower([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($Text)))
}

function Get-BonsaiLoraQualificationPlan {
    <# .SYNOPSIS
    Returns the committed contract, its LF-normalized UTF-8 digest, and the fixed paired schedule.
    #>
    [CmdletBinding()]
    [OutputType([pscustomobject])]
    param()
    $path = Join-Path $PSScriptRoot '../config/swift-lora.qualification.json'
    $contractText = [IO.File]::ReadAllText($path)
    $contract = $contractText | ConvertFrom-Json -AsHashtable -DateKind String
    # These safety and acceptance constants cannot be relaxed by editing only JSON.
    if ($contract.schemaVersion -ne 1 -or $contract.execution.repeatCount -ne 3 -or
        $contract.execution.seed -ne 42 -or $contract.execution.minimumDecodeTokens -ne 16 -or
        $contract.execution.githubActionsRequired -ne $true -or $contract.execution.runnerArchitecture -cne 'Arm64' -or
        $contract.execution.deviceModel -cne 'Snapdragon X Elite' -or $contract.controls.maxOutputTokens -ne 512 -or
        $contract.controls.contextTokens -ne 8192 -or $contract.controls.batchTokens -ne 128 -or
        $contract.controls.microbatchTokens -ne 64 -or $contract.controls.threads -ne 4 -or $contract.controls.seed -ne 42 -or
        $contract.controls.sampler.temperature -ne 0 -or $contract.controls.sampler.topP -ne 1 -or
        $contract.controls.sampler.topK -ne 1 -or $contract.controls.sampler.minP -ne 0 -or $contract.controls.sampler.repeatPenalty -ne 1 -or
        $contract.gates.minimumPerPromptMedianTpsRatio -ne 1 -or $contract.gates.minimumQualityScore -ne 3 -or
        $contract.gates.minimumPerPromptDimensionMedianQualityDelta -ne 0 -or
        $contract.gates.rejectUnwantedAdapterRefusals -ne $true -or
        $contract.gates.maximumPeakJobMemoryBytes -ne 12GB -or
        $contract.gates.maximumPerPromptMedianJobMemoryPremiumBytes -ne 768MB -or
        $contract.gates.minimumAvailablePhysicalBytes -ne 4GB -or
        $contract.gates.basisVerificationRequired -ne $true -or
        $contract.gates.zeroScaleResponseAndLogitsIdentityRequired -ne $true -or
        $contract.gates.nonzeroLogitsApplicationRequired -ne $true -or $contract.prompts.Count -ne 4) {
        throw 'The mandatory qualification policy requires a code review before changing its fixed contract.'
    }
    $swift = Get-Content -Raw -LiteralPath (Join-Path $PSScriptRoot '../config/swift-assessment.lock.json') | ConvertFrom-Json -AsHashtable -DateKind String
    $adapter = Get-Content -Raw -LiteralPath (Join-Path $PSScriptRoot '../config/swift-lora.lock.json') | ConvertFrom-Json -AsHashtable -DateKind String
    $ptq = Get-Content -Raw -LiteralPath (Join-Path $PSScriptRoot '../config/swift.ptq1-export.json') | ConvertFrom-Json -AsHashtable -DateKind String
    $metadata = Get-Content -Raw -LiteralPath (Join-Path $PSScriptRoot '../config/swift.metadata.json') | ConvertFrom-Json -AsHashtable -DateKind String
    if ($contract.controls.runtimeCommit -cne $swift.runtime.commit -or
        $contract.controls.adapterSha256 -cne $adapter.adapter.sha256 -or
        $contract.controls.templateSha256 -cne $metadata.EmbeddedTemplates['tokenizer.chat_template'].Sha256 -or
        $contract.representations.PQ2_0 -cne $swift.model.sha256 -or
        $contract.representations['current-repacked-PTQ1_0'] -cne $ptq.OutputSha256) { throw 'Qualification pins disagree with the committed artifact identities.' }
    $schedule = [Collections.Generic.List[object]]::new()
    $promptIds = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    $index = 0
    foreach ($prompt in $contract.prompts) {
        if (-not $promptIds.Add($prompt.id) -or $prompt.category -cnotin @('general', 'creative')) { throw 'Duplicate prompt or unsupported prompt category.' }
        for ($repeat = 1; $repeat -le $contract.execution.repeatCount; $repeat++) {
            $arms = if (($repeat + $index) % 2 -eq 0) { @('adapter', 'baseline') } else { @('baseline', 'adapter') }
            foreach ($arm in $arms) {
                $schedule.Add([ordered]@{
                    sampleId = 's' + ($schedule.Count + 1).ToString('D4', [Globalization.CultureInfo]::InvariantCulture)
                    promptId = $prompt.id; repeat = $repeat; arm = $arm; order = $schedule.Count + 1; seed = $contract.execution.seed
                })
            }
        }
        $index++
    }
    return [pscustomobject]@{
        Contract = $contract
        # Git can materialize JSON as CRLF on Windows and LF on Linux. Contract
        # identity must survive that checkout conversion for external review.
        ContractSha256 = Get-BonsaiQualificationDigest -Text ($contractText.Replace("`r`n", "`n"))
        Schedule = $schedule.ToArray()
    }
}

function Get-BonsaiLoraControlDigest {
    <# .SYNOPSIS
    Validates fixed controls and returns a stable digest for every paired sample.
    #>
    [CmdletBinding()]
    [OutputType([string])]
    param([Parameter(Mandatory)][Collections.IDictionary]$Controls)
    $plan = Get-BonsaiLoraQualificationPlan
    $expected = $plan.Contract.controls
    Assert-QualificationShape -Value $Controls -Required (@('modelSha256', 'weightRepresentation') + @($expected.Keys)) -Name 'controls'
    Assert-QualificationHash -Value $Controls.modelSha256 -Name 'controls.modelSha256'
    if ($Controls.weightRepresentation -isnot [string] -or $Controls.weightRepresentation -cnotin @($plan.Contract.representations.Keys) -or
        $Controls.modelSha256 -cne $plan.Contract.representations[$Controls.weightRepresentation]) { throw 'Unpinned model or weight representation.' }
    $canonical = [ordered]@{ modelSha256 = $Controls.modelSha256; weightRepresentation = $Controls.weightRepresentation }
    foreach ($key in @($expected.Keys | Sort-Object -CaseSensitive)) {
        if ($key -ceq 'sampler') {
            Assert-QualificationShape -Value $Controls.sampler -Required @($expected.sampler.Keys) -Name 'controls.sampler'
            $sampler = [ordered]@{}
            foreach ($samplerKey in @($expected.sampler.Keys | Sort-Object -CaseSensitive)) {
                Assert-QualificationNumber -Value $Controls.sampler[$samplerKey] -Name "controls.sampler.$samplerKey" -Minimum 0 -Maximum 100
                if ($Controls.sampler[$samplerKey] -ne $expected.sampler[$samplerKey]) { throw 'Sampler does not match the fixed contract.' }
                $sampler[$samplerKey] = [double]$Controls.sampler[$samplerKey]
            }
            $canonical[$key] = $sampler
        } elseif ($expected[$key] -is [string]) {
            if ($Controls[$key] -isnot [string] -or $Controls[$key] -cne $expected[$key]) { throw "Unpinned control: $key." }
            $canonical[$key] = $Controls[$key]
        } else {
            Assert-QualificationNumber -Value $Controls[$key] -Name "controls.$key" -Minimum 0 -Maximum 100000 -Integer
            if ($Controls[$key] -ne $expected[$key]) { throw "Unmatched control: $key." }
            $canonical[$key] = [long]$Controls[$key]
        }
    }
    return Get-BonsaiQualificationDigest -Text ($canonical | ConvertTo-Json -Depth 8 -Compress)
}

function Get-QualificationMedian {
    [CmdletBinding()]
    [OutputType([double])]
    param([double[]]$Values)
    if ($Values.Count -eq 0) { throw 'A median needs at least one observation.' }
    $sorted = [double[]]$Values.Clone()
    [Array]::Sort($sorted)
    $middle = [int][Math]::Floor($sorted.Count / 2)
    if ($sorted.Count % 2) { return $sorted[$middle] }
    return ($sorted[$middle - 1] + $sorted[$middle]) / 2
}

function Get-QualificationDimension {
    [CmdletBinding()]
    [OutputType([string])]
    param([string]$Category)
    $dimensions = @('instructionFollowing', 'correctness', 'coherence')
    if ($Category -ceq 'creative') { $dimensions += 'creativeUsefulness' }
    return $dimensions
}

function Test-BonsaiLoraQualification {
    <#
    .SYNOPSIS
    Scores a matched CI capture and separately supplied external human review.
    .DESCRIPTION
    Malformed or mismatched evidence throws. Valid evidence with known regressions
    is rejected. Missing correctness proof, reviews, or completed answers remains
    pending. Acceptance requires every gate; this never enables a serving profile.
    #>
    [CmdletBinding()]
    [OutputType([pscustomobject])]
    param(
        [Parameter(Mandatory)][Collections.IDictionary]$Evidence,
        [AllowNull()][Collections.IDictionary]$Review
    )
    $plan = Get-BonsaiLoraQualificationPlan
    $contract = $plan.Contract
    Assert-QualificationShape -Value $Evidence -Required @('schemaVersion', 'runId', 'contractSha256', 'execution', 'controls', 'basis', 'samples', 'zeroScaleChecks', 'nonzeroAdapterCheck') -Name 'evidence'
    Assert-QualificationNumber -Value $Evidence.schemaVersion -Name 'schemaVersion' -Minimum 1 -Maximum 1 -Integer
    Assert-QualificationHash -Value $Evidence.contractSha256 -Name 'contractSha256'
    if ($Evidence.schemaVersion -ne 1 -or $Evidence.contractSha256 -cne $plan.ContractSha256) { throw 'Evidence uses a different contract or schema.' }
    Assert-QualificationString -Value $Evidence.runId -Name 'runId' -MaximumLength 128
    Assert-QualificationShape -Value $Evidence.execution -Required @('githubActions', 'runUrl', 'runnerArchitecture', 'deviceModel', 'runnerName') -Name 'execution'
    if ($Evidence.execution.githubActions -isnot [bool] -or -not $Evidence.execution.githubActions -or
        $Evidence.execution.runnerArchitecture -isnot [string] -or $Evidence.execution.deviceModel -isnot [string] -or
        $Evidence.execution.runnerArchitecture -cne $contract.execution.runnerArchitecture -or
        $Evidence.execution.deviceModel -cne $contract.execution.deviceModel -or
        $Evidence.execution.runUrl -isnot [string] -or
        $Evidence.execution.runUrl -cnotmatch '^https://github\.com/[^/\s]+/[^/\s]+/actions/runs/[0-9]+$') { throw 'Qualification requires a recorded CI run on the pinned Snapdragon ARM64 device.' }
    Assert-QualificationString -Value $Evidence.execution.runnerName -Name 'runnerName' -MaximumLength 128
    $controlDigest = Get-BonsaiLoraControlDigest -Controls $Evidence.controls
    if ($Evidence.samples -isnot [array] -or $Evidence.samples.Count -ne $plan.Schedule.Count) { throw 'Missing or extra paired samples.' }
    $samples = @{}
    foreach ($sample in $Evidence.samples) {
        Assert-QualificationShape -Value $sample -Required @('sampleId', 'promptId', 'repeat', 'arm', 'order', 'seed', 'controlsSha256', 'decodeTokens', 'decodeMilliseconds', 'peakJobMemoryBytes', 'minimumAvailablePhysicalBytes', 'completed', 'responseText', 'responseSha256') -Name 'sample' -Optional @('diagnostics')
        if ($sample.Contains('diagnostics') -and $sample.diagnostics -isnot [Collections.IDictionary]) { throw 'Optional sample diagnostics must be an object.' }
        Assert-QualificationString -Value $sample.sampleId -Name 'sampleId' -MaximumLength 128
        if ($samples.ContainsKey($sample.sampleId)) { throw 'Duplicate sample identity.' }
        $scheduled = @($plan.Schedule | Where-Object { $_.sampleId -ceq $sample.sampleId })
        if ($scheduled.Count -ne 1) { throw 'Unscheduled sample identity.' }
        Assert-QualificationString -Value $sample.promptId -Name 'sample.promptId' -MaximumLength 128
        Assert-QualificationString -Value $sample.arm -Name 'sample.arm' -MaximumLength 32
        foreach ($numberName in @('repeat', 'order', 'seed')) { Assert-QualificationNumber -Value $sample[$numberName] -Name $numberName -Minimum 1 -Maximum 100000 -Integer }
        Assert-QualificationHash -Value $sample.controlsSha256 -Name 'sample.controlsSha256'
        foreach ($key in @('promptId', 'repeat', 'arm', 'order', 'seed')) {
            if ($sample[$key] -cne $scheduled[0][$key]) { throw "Sample differs from the fixed schedule: $key." }
        }
        if ($sample.controlsSha256 -cne $controlDigest) { throw 'A sample changed matched controls.' }
        Assert-QualificationNumber -Value $sample.decodeTokens -Name 'decodeTokens' -Minimum $contract.execution.minimumDecodeTokens -Maximum $contract.controls.maxOutputTokens -Integer
        Assert-QualificationNumber -Value $sample.decodeMilliseconds -Name 'decodeMilliseconds' -Minimum 0.000001 -Maximum 86400000
        Assert-QualificationNumber -Value $sample.peakJobMemoryBytes -Name 'peakJobMemoryBytes' -Minimum 1 -Maximum 64GB -Integer
        Assert-QualificationNumber -Value $sample.minimumAvailablePhysicalBytes -Name 'minimumAvailablePhysicalBytes' -Minimum 0 -Maximum 1TB -Integer
        if ($sample.completed -isnot [bool] -or -not $sample.completed) { throw 'A paired native sample did not complete successfully.' }
        Assert-QualificationString -Value $sample.responseText -Name 'responseText' -MaximumLength 1048576
        Assert-QualificationHash -Value $sample.responseSha256 -Name 'responseSha256'
        if ((Get-BonsaiQualificationDigest -Text $sample.responseText) -cne $sample.responseSha256) { throw 'A response hash does not bind its exact captured text.' }
        $samples[$sample.sampleId] = $sample
    }

    $rejections = [Collections.Generic.List[string]]::new()
    $pending = [Collections.Generic.List[string]]::new()
    $promptResults = [Collections.Generic.List[object]]::new()
    $throughputPassed = $true
    $memoryPassed = $true
    foreach ($prompt in $contract.prompts) {
        $baseline = @($Evidence.samples | Where-Object { $_.promptId -ceq $prompt.id -and $_.arm -ceq 'baseline' })
        $adapter = @($Evidence.samples | Where-Object { $_.promptId -ceq $prompt.id -and $_.arm -ceq 'adapter' })
        $baseTps = Get-QualificationMedian -Values @($baseline | ForEach-Object { [double]$_.decodeTokens * 1000 / $_.decodeMilliseconds })
        $adapterTps = Get-QualificationMedian -Values @($adapter | ForEach-Object { [double]$_.decodeTokens * 1000 / $_.decodeMilliseconds })
        $ratio = $adapterTps / $baseTps
        $premium = (Get-QualificationMedian -Values @($adapter.peakJobMemoryBytes)) - (Get-QualificationMedian -Values @($baseline.peakJobMemoryBytes))
        $tpsOk = $ratio -ge $contract.gates.minimumPerPromptMedianTpsRatio
        if (-not $tpsOk) { $throughputPassed = $false; $rejections.Add("Throughput regression: $($prompt.id).") }
        if ($premium -gt $contract.gates.maximumPerPromptMedianJobMemoryPremiumBytes) {
            $memoryPassed = $false; $rejections.Add("Job commit premium exceeded: $($prompt.id).")
        }
        $promptResults.Add([ordered]@{ promptId = $prompt.id; baselineMedianTps = $baseTps; adapterMedianTps = $adapterTps; tpsRatio = $ratio; tpsPassed = $tpsOk; medianJobMemoryPremiumBytes = $premium; quality = @() })
    }
    foreach ($sample in $Evidence.samples) {
        if ($sample.peakJobMemoryBytes -gt $contract.gates.maximumPeakJobMemoryBytes -or
            $sample.minimumAvailablePhysicalBytes -lt $contract.gates.minimumAvailablePhysicalBytes) {
            $memoryPassed = $false; $rejections.Add("Memory safety allowance exceeded: $($sample.sampleId).")
        }
    }

    Assert-QualificationShape -Value $Evidence.basis -Required @('status', 'evidenceSha256', 'reviewerId', 'note') -Name 'basis'
    if ($Evidence.basis.status -isnot [string] -or $Evidence.basis.status -cnotin @('pending', 'verified')) { throw 'Unknown authoring-basis status.' }
    Assert-QualificationHash -Value $Evidence.basis.evidenceSha256 -Name 'basis.evidenceSha256' -Nullable
    foreach ($name in @('reviewerId', 'note')) { if ($null -ne $Evidence.basis[$name]) { Assert-QualificationString -Value $Evidence.basis[$name] -Name ('basis.' + $name) } }
    $basisPassed = $Evidence.basis.status -ceq 'verified'
    if ($basisPassed) {
        Assert-QualificationHash -Value $Evidence.basis.evidenceSha256 -Name 'basis.evidenceSha256'
        Assert-QualificationString -Value $Evidence.basis.reviewerId -Name 'basis.reviewerId' -MaximumLength 256
        Assert-QualificationString -Value $Evidence.basis.note -Name 'basis.note'
    } else { $pending.Add('Grouped SSM authoring-basis/operator validation is unresolved.') }

    if ($Evidence.zeroScaleChecks -isnot [array] -or $Evidence.zeroScaleChecks.Count -ne $contract.prompts.Count) { throw 'Every fixed prompt requires one zero-scale control.' }
    $zeroPassed = $true
    $zeroIds = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    foreach ($check in $Evidence.zeroScaleChecks) {
        Assert-QualificationShape -Value $check -Required @('promptId', 'seed', 'baselineResponseSha256', 'zeroScaleResponseSha256', 'baselineLogitsSha256', 'zeroScaleLogitsSha256') -Name 'zeroScaleCheck'
        Assert-QualificationNumber -Value $check.seed -Name 'zeroScaleCheck.seed' -Minimum 42 -Maximum 42 -Integer
        if ($check.promptId -isnot [string] -or -not $zeroIds.Add($check.promptId) -or
            $check.promptId -cnotin @($contract.prompts.id) -or $check.seed -ne $contract.execution.seed) { throw 'Invalid or duplicate zero-scale control.' }
        foreach ($name in @('baselineResponseSha256', 'zeroScaleResponseSha256')) { Assert-QualificationHash -Value $check[$name] -Name $name }
        foreach ($name in @('baselineLogitsSha256', 'zeroScaleLogitsSha256')) { Assert-QualificationHash -Value $check[$name] -Name $name -Nullable }
        $baseline = @($Evidence.samples | Where-Object { $_.promptId -ceq $check.promptId -and $_.repeat -eq 1 -and $_.arm -ceq 'baseline' })[0]
        if ($check.baselineResponseSha256 -cne $baseline.responseSha256) { throw 'Zero-scale control references a different baseline capture.' }
        if ($check.baselineResponseSha256 -cne $check.zeroScaleResponseSha256) { $zeroPassed = $false; $rejections.Add("Zero-scale response identity failed: $($check.promptId).") }
        if ($null -eq $check.baselineLogitsSha256 -or $null -eq $check.zeroScaleLogitsSha256) {
            $zeroPassed = $false; $pending.Add("Zero-scale native logits identity is unavailable: $($check.promptId).")
        } elseif ($check.baselineLogitsSha256 -cne $check.zeroScaleLogitsSha256) {
            $zeroPassed = $false; $rejections.Add("Zero-scale logits identity failed: $($check.promptId).")
        }
    }
    Assert-QualificationShape -Value $Evidence.nonzeroAdapterCheck -Required @('status', 'scale', 'adapterSha256', 'logitsChanged', 'evidenceSha256') -Name 'nonzeroAdapterCheck'
    $nonzero = $Evidence.nonzeroAdapterCheck
    Assert-QualificationNumber -Value $nonzero.scale -Name 'nonzeroAdapterCheck.scale' -Minimum 1 -Maximum 1 -Integer
    Assert-QualificationHash -Value $nonzero.adapterSha256 -Name 'nonzeroAdapterCheck.adapterSha256'
    if ($nonzero.status -isnot [string] -or $nonzero.status -cnotin @('pending', 'verified') -or $nonzero.scale -ne 1 -or
        $nonzero.adapterSha256 -cne $contract.controls.adapterSha256 -or
        ($null -ne $nonzero.logitsChanged -and $nonzero.logitsChanged -isnot [bool])) { throw 'Invalid nonzero adapter control.' }
    Assert-QualificationHash -Value $nonzero.evidenceSha256 -Name 'nonzeroAdapterCheck.evidenceSha256' -Nullable
    $nonzeroPassed = $nonzero.status -ceq 'verified' -and $nonzero.logitsChanged -eq $true -and $null -ne $nonzero.evidenceSha256
    if ($nonzero.status -ceq 'verified' -and $nonzero.logitsChanged -eq $false) { $rejections.Add('Nonzero adapter control did not change native logits.') }
    elseif (-not $nonzeroPassed) { $pending.Add('Nonzero adapter application lacks independent native logits evidence.') }

    $reviews = @{}
    if ($null -ne $Review) {
        Assert-QualificationShape -Value $Review -Required @('schemaVersion', 'runId', 'contractSha256', 'source', 'attestation', 'reviews') -Name 'review'
        Assert-QualificationNumber -Value $Review.schemaVersion -Name 'review.schemaVersion' -Minimum 1 -Maximum 1 -Integer
        Assert-QualificationHash -Value $Review.contractSha256 -Name 'review.contractSha256'
        Assert-QualificationString -Value $Review.runId -Name 'review.runId' -MaximumLength 128
        Assert-QualificationString -Value $Review.source -Name 'review.source'
        Assert-QualificationString -Value $Review.attestation -Name 'review.attestation'
        if ($Review.schemaVersion -ne 1 -or $Review.runId -cne $Evidence.runId -or $Review.contractSha256 -cne $plan.ContractSha256 -or
            $Review.source -cne 'external-human-review' -or $Review.attestation -cne 'I reviewed the exact captured responses without automated scoring.' -or
            $Review.reviews -isnot [array]) { throw 'Quality review is not an external human attestation bound to this run.' }
        foreach ($record in $Review.reviews) {
            Assert-QualificationShape -Value $record -Required @('sampleId', 'responseSha256', 'reviewerId', 'humanReviewed', 'reviewedAtUtc', 'answerComplete', 'refused', 'scores', 'rationale') -Name 'review record'
            Assert-QualificationString -Value $record.sampleId -Name 'review.sampleId' -MaximumLength 128
            if ($reviews.ContainsKey($record.sampleId) -or -not $samples.ContainsKey($record.sampleId)) { throw 'Duplicate or unknown quality review sample.' }
            Assert-QualificationHash -Value $record.responseSha256 -Name 'review.responseSha256'
            $sample = $samples[$record.sampleId]
            if ($record.responseSha256 -cne $sample.responseSha256 -or $record.humanReviewed -isnot [bool] -or -not $record.humanReviewed -or
                $record.answerComplete -isnot [bool] -or $record.refused -isnot [bool]) { throw 'Quality review is unbound or lacks explicit human review/completion/refusal declarations.' }
            Assert-QualificationString -Value $record.reviewerId -Name 'review.reviewerId' -MaximumLength 256
            Assert-QualificationString -Value $record.rationale -Name 'review.rationale' -MaximumLength 8192
            if ($record.reviewedAtUtc -isnot [string] -or $record.reviewedAtUtc -cnotmatch '^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$') { throw 'Review timestamp must be an explicit UTC second.' }
            $date = [datetime]::MinValue
            if (-not [datetime]::TryParseExact($record.reviewedAtUtc, 'yyyy-MM-ddTHH:mm:ssZ', [Globalization.CultureInfo]::InvariantCulture, [Globalization.DateTimeStyles]::AssumeUniversal, [ref]$date)) { throw 'Invalid human review timestamp.' }
            $prompt = @($contract.prompts | Where-Object { $_.id -ceq $sample.promptId })[0]
            $dimensions = @(Get-QualificationDimension -Category $prompt.category)
            Assert-QualificationShape -Value $record.scores -Required $dimensions -Name 'review.scores'
            foreach ($dimension in $dimensions) { Assert-QualificationNumber -Value $record.scores[$dimension] -Name "review.scores.$dimension" -Minimum 0 -Maximum 4 -Integer }
            $reviews[$record.sampleId] = $record
        }
    }
    $qualityPassed = $true
    $qualityPending = $false
    $qualityFailed = $false
    foreach ($result in $promptResults) {
        $prompt = @($contract.prompts | Where-Object { $_.id -ceq $result.promptId })[0]
        $promptSamples = @($Evidence.samples | Where-Object { $_.promptId -ceq $prompt.id })
        $result.baselineReviewedRefusals = @($promptSamples | Where-Object { $_.arm -ceq 'baseline' -and $reviews.ContainsKey($_.sampleId) -and $reviews[$_.sampleId].refused }).Count
        $result.adapterReviewedRefusals = @($promptSamples | Where-Object { $_.arm -ceq 'adapter' -and $reviews.ContainsKey($_.sampleId) -and $reviews[$_.sampleId].refused }).Count
        if ($result.adapterReviewedRefusals -gt 0) {
            $qualityPassed = $false; $qualityFailed = $true
            $rejections.Add("Unwanted adapter refusal on a fixed benign prompt: $($prompt.id).")
        }
        $missing = @($promptSamples | Where-Object { -not $reviews.ContainsKey($_.sampleId) -or -not $reviews[$_.sampleId].answerComplete })
        if ($missing.Count) { $qualityPassed = $false; $qualityPending = $true; $pending.Add("Complete external human quality review is missing: $($prompt.id)."); continue }
        $dimensions = @(Get-QualificationDimension -Category $prompt.category)
        $dimensionResults = [Collections.Generic.List[object]]::new()
        foreach ($dimension in $dimensions) {
            $baseScores = @($promptSamples | Where-Object { $_.arm -ceq 'baseline' } | ForEach-Object { [double]$reviews[$_.sampleId].scores[$dimension] })
            $adapterScores = @($promptSamples | Where-Object { $_.arm -ceq 'adapter' } | ForEach-Object { [double]$reviews[$_.sampleId].scores[$dimension] })
            $baseMedian = Get-QualificationMedian -Values $baseScores
            $adapterMedian = Get-QualificationMedian -Values $adapterScores
            $ok = $adapterMedian -ge $contract.gates.minimumQualityScore -and
                ($adapterMedian - $baseMedian) -ge $contract.gates.minimumPerPromptDimensionMedianQualityDelta
            if (-not $ok) { $qualityPassed = $false; $qualityFailed = $true; $rejections.Add("Quality regression or minimum failed: $($prompt.id)/$dimension.") }
            $dimensionResults.Add([ordered]@{ dimension = $dimension; baselineMedian = $baseMedian; adapterMedian = $adapterMedian; passed = $ok })
        }
        $result.quality = $dimensionResults.ToArray()
    }
    $status = if ($rejections.Count) { 'rejected' } elseif ($pending.Count) { 'pending' } else { 'accepted' }
    return [pscustomobject]@{
        SchemaVersion = 1; RunId = $Evidence.runId; ContractSha256 = $plan.ContractSha256; ControlsSha256 = $controlDigest
        Status = $status; StandardProfileMayEnable = $status -ceq 'accepted'
        Gates = [ordered]@{
            Throughput = $(if ($throughputPassed) { 'passed' } else { 'failed' })
            Quality = $(if ($qualityFailed) { 'failed' } elseif ($qualityPending) { 'pending' } elseif ($qualityPassed) { 'passed' } else { 'failed' })
            MemoryAllowance = $(if ($memoryPassed) { 'passed' } else { 'failed' })
            AuthoringBasis = $(if ($basisPassed) { 'passed' } else { 'pending' })
            ZeroScaleIdentity = $(if ($zeroPassed) { 'passed' } else { 'not-passed' })
            NonzeroApplication = $(if ($nonzeroPassed) { 'passed' } else { 'not-passed' })
        }
        Prompts = $promptResults.ToArray(); Rejections = $rejections.ToArray(); Pending = $pending.ToArray()
        Scope = 'Matched pinned representation only. Job commit is not total RAM. Human review identity is recorded, not authenticated. No serving configuration was changed.'
    }
}

Microsoft.PowerShell.Core\Export-ModuleMember -Function Get-BonsaiQualificationDigest, Get-BonsaiLoraQualificationPlan, Get-BonsaiLoraControlDigest, Test-BonsaiLoraQualification





