//! Scoring preserves rejection precedence and every independently required gate.

use std::collections::{HashMap, HashSet};

use serde_json::{Value, json};

use super::schema::{Evidence, Review, ReviewRecord, Sample};
use super::{
    GIB, Policy, control_identity, decode, digest, hash, number, optional_hash, require, string,
};
use crate::Result;

const ATTESTATION: &str = "I reviewed the exact captured responses without automated scoring.";

#[allow(
    clippy::struct_excessive_bools,
    reason = "Each boolean records a separate mandatory acceptance gate."
)]
struct Results {
    rejections: Vec<String>,
    pending: Vec<String>,
    prompts: Vec<Value>,
    throughput: bool,
    memory: bool,
    basis: bool,
    zero: bool,
    nonzero: bool,
    quality_failed: bool,
    quality_pending: bool,
}

impl Default for Results {
    fn default() -> Self {
        Self {
            rejections: Vec::new(),
            pending: Vec::new(),
            prompts: Vec::new(),
            throughput: true,
            memory: true,
            basis: false,
            zero: true,
            nonzero: false,
            quality_failed: false,
            quality_pending: false,
        }
    }
}

fn valid_run_url(url: &str) -> bool {
    let Some(tail) = url.strip_prefix("https://github.com/") else {
        return false;
    };
    let parts: Vec<_> = tail.split('/').collect();
    parts.len() == 5
        && !parts[0].is_empty()
        && !parts[1].is_empty()
        && !parts[0].chars().any(char::is_whitespace)
        && !parts[1].chars().any(char::is_whitespace)
        && parts[2] == "actions"
        && parts[3] == "runs"
        && !parts[4].is_empty()
        && parts[4].bytes().all(|byte| byte.is_ascii_digit())
}

fn validate_samples(evidence: &Evidence, policy: &Policy, controls_digest: &str) -> Result<()> {
    require(
        evidence.samples.len() == policy.schedule.len(),
        "Missing or extra paired samples.",
    )?;
    let mut ids = HashSet::new();
    for sample in &evidence.samples {
        string(&sample.sample_id, 128)?;
        require(ids.insert(&sample.sample_id), "Duplicate sample identity.")?;
        let scheduled = policy
            .schedule
            .iter()
            .find(|item| item.sample_id == sample.sample_id)
            .ok_or("Unscheduled sample identity.")?;
        string(&sample.prompt_id, 128)?;
        string(&sample.arm, 32)?;
        for metric in [sample.repeat, sample.order, sample.seed] {
            number(metric, 1.0, 100_000.0, true)?;
        }
        hash(&sample.controls_sha256)?;
        require(
            sample.prompt_id == scheduled.prompt_id
                && sample.repeat == f64::from(scheduled.repeat)
                && sample.arm == scheduled.arm
                && sample.order == f64::from(scheduled.order)
                && sample.seed == f64::from(scheduled.seed),
            "Sample differs from the fixed schedule.",
        )?;
        require(
            sample.controls_sha256 == controls_digest,
            "A sample changed matched controls.",
        )?;
        number(sample.decode_tokens, 16.0, 512.0, true)?;
        number(sample.decode_milliseconds, 0.000_001, 86_400_000.0, false)?;
        number(sample.peak_job_memory_bytes, 1.0, 64.0 * GIB, true)?;
        number(
            sample.minimum_available_physical_bytes,
            0.0,
            1024.0 * GIB,
            true,
        )?;
        require(
            sample.completed,
            "A paired native sample did not complete successfully.",
        )?;
        string(&sample.response_text, 1_048_576)?;
        hash(&sample.response_sha256)?;
        require(
            digest(&sample.response_text) == sample.response_sha256,
            "Response digest does not bind its exact captured text.",
        )?;
    }
    Ok(())
}

#[allow(
    clippy::manual_midpoint,
    reason = "Preserve legacy arithmetic for bounded finite observations."
)]
fn median(mut values: Vec<f64>) -> Result<f64> {
    require(
        !values.is_empty(),
        "A median needs at least one observation.",
    )?;
    values.sort_by(f64::total_cmp);
    let middle = values.len() / 2;
    Ok(if values.len().is_multiple_of(2) {
        (values[middle - 1] + values[middle]) / 2.0
    } else {
        values[middle]
    })
}

fn arm_samples<'a>(evidence: &'a Evidence, prompt: &str, arm: &str) -> Vec<&'a Sample> {
    evidence
        .samples
        .iter()
        .filter(|sample| sample.prompt_id == prompt && sample.arm == arm)
        .collect()
}

fn throughput_memory(evidence: &Evidence, policy: &Policy, results: &mut Results) -> Result<()> {
    let job_memory_limit = policy.contract["gates"]["maximumPeakJobMemoryBytes"]
        .as_f64()
        .ok_or("Missing validated qualification Job-commit limit.")?;
    for prompt in &policy.prompts {
        let baseline = arm_samples(evidence, &prompt.id, "baseline");
        let adapter = arm_samples(evidence, &prompt.id, "adapter");
        let tps = |samples: &[&Sample]| {
            median(
                samples
                    .iter()
                    .map(|sample| sample.decode_tokens * 1000.0 / sample.decode_milliseconds)
                    .collect(),
            )
        };
        let memory = |samples: &[&Sample]| {
            median(
                samples
                    .iter()
                    .map(|sample| sample.peak_job_memory_bytes)
                    .collect(),
            )
        };
        let baseline_tps = tps(&baseline)?;
        let adapter_tps = tps(&adapter)?;
        let ratio = adapter_tps / baseline_tps;
        let premium = memory(&adapter)? - memory(&baseline)?;
        let passed = ratio >= 1.0;
        if !passed {
            results.throughput = false;
            results
                .rejections
                .push(format!("Throughput regression: {}.", prompt.id));
        }
        if premium > 0.75 * GIB {
            results.memory = false;
            results
                .rejections
                .push(format!("Job commit premium exceeded: {}.", prompt.id));
        }
        results.prompts.push(
            json!({"promptId": prompt.id, "baselineMedianTps": baseline_tps,
            "adapterMedianTps": adapter_tps, "tpsRatio": ratio, "tpsPassed": passed,
            "medianJobMemoryPremiumBytes": premium, "quality": []}),
        );
    }
    for sample in &evidence.samples {
        if sample.peak_job_memory_bytes > job_memory_limit
            || sample.minimum_available_physical_bytes < 4.0 * GIB
        {
            results.memory = false;
            results.rejections.push(format!(
                "Memory safety allowance exceeded: {}.",
                sample.sample_id
            ));
        }
    }
    Ok(())
}

fn basis_proof(evidence: &Evidence, results: &mut Results) -> Result<()> {
    let basis = &evidence.basis;
    require(
        matches!(basis.status.as_str(), "pending" | "verified"),
        "Unknown authoring-basis status.",
    )?;
    optional_hash(basis.evidence_sha256.as_deref())?;
    for value in [&basis.reviewer_id, &basis.note].into_iter().flatten() {
        string(value, 4096)?;
    }
    results.basis = basis.status == "verified";
    if results.basis {
        hash(
            basis
                .evidence_sha256
                .as_deref()
                .ok_or("Missing authoring-basis proof.")?,
        )?;
        string(
            basis
                .reviewer_id
                .as_deref()
                .ok_or("Missing authoring-basis reviewer.")?,
            256,
        )?;
        string(
            basis
                .note
                .as_deref()
                .ok_or("Missing authoring-basis note.")?,
            4096,
        )?;
    } else {
        results
            .pending
            .push("Grouped SSM authoring-basis/operator validation is unresolved.".into());
    }
    Ok(())
}

fn zero_proofs(evidence: &Evidence, policy: &Policy, results: &mut Results) -> Result<()> {
    require(
        evidence.zero_scale_checks.len() == policy.prompts.len(),
        "Every fixed prompt requires one zero-scale control.",
    )?;
    let mut ids = HashSet::new();
    for check in &evidence.zero_scale_checks {
        number(check.seed, 42.0, 42.0, true)?;
        require(
            ids.insert(&check.prompt_id)
                && policy
                    .prompts
                    .iter()
                    .any(|prompt| prompt.id == check.prompt_id),
            "Invalid or duplicate zero-scale control.",
        )?;
        hash(&check.baseline_response_sha256)?;
        hash(&check.zero_scale_response_sha256)?;
        optional_hash(check.baseline_logits_sha256.as_deref())?;
        optional_hash(check.zero_scale_logits_sha256.as_deref())?;
        let baseline = evidence
            .samples
            .iter()
            .find(|sample| {
                sample.prompt_id == check.prompt_id
                    && sample.repeat == 1.0
                    && sample.arm == "baseline"
            })
            .ok_or("Missing zero-scale baseline.")?;
        require(
            check.baseline_response_sha256 == baseline.response_sha256,
            "Zero-scale control references a different baseline capture.",
        )?;
        if check.baseline_response_sha256 != check.zero_scale_response_sha256 {
            results.zero = false;
            results.rejections.push(format!(
                "Zero-scale response identity failed: {}.",
                check.prompt_id
            ));
        }
        match (
            &check.baseline_logits_sha256,
            &check.zero_scale_logits_sha256,
        ) {
            (Some(left), Some(right)) if left == right => {}
            (Some(_), Some(_)) => {
                results.zero = false;
                results.rejections.push(format!(
                    "Zero-scale logits identity failed: {}.",
                    check.prompt_id
                ));
            }
            _ => {
                results.zero = false;
                results.pending.push(format!(
                    "Zero-scale native logits identity is unavailable: {}.",
                    check.prompt_id
                ));
            }
        }
    }
    Ok(())
}

fn nonzero_proof(evidence: &Evidence, policy: &Policy, results: &mut Results) -> Result<()> {
    let nonzero = &evidence.nonzero_adapter_check;
    require(
        nonzero.scale == policy.adapter_scale(),
        "Nonzero proof uses a different qualification adapter scale.",
    )?;
    hash(&nonzero.adapter_sha256)?;
    require(
        matches!(nonzero.status.as_str(), "pending" | "verified")
            && policy.contract["controls"]["adapterSha256"] == nonzero.adapter_sha256,
        "Invalid nonzero adapter control.",
    )?;
    optional_hash(nonzero.evidence_sha256.as_deref())?;
    results.nonzero = nonzero.status == "verified"
        && nonzero.logits_changed == Some(true)
        && nonzero.evidence_sha256.is_some();
    if nonzero.status == "verified" && nonzero.logits_changed == Some(false) {
        results
            .rejections
            .push("Nonzero adapter control did not change native logits.".into());
    } else if !results.nonzero {
        results
            .pending
            .push("Nonzero adapter application lacks independent native logits evidence.".into());
    }
    Ok(())
}

fn dimensions(category: &str) -> &'static [&'static str] {
    if category == "creative" {
        &[
            "instructionFollowing",
            "correctness",
            "coherence",
            "creativeUsefulness",
        ]
    } else {
        &["instructionFollowing", "correctness", "coherence"]
    }
}

fn valid_timestamp(text: &str) -> bool {
    let bytes = text.as_bytes();
    if bytes.len() != 20
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
        || bytes[19] != b'Z'
    {
        return false;
    }
    let part = |start, end| {
        text.get(start..end)
            .and_then(|word| word.parse::<u32>().ok())
    };
    let (Some(year), Some(month), Some(day), Some(hour), Some(minute), Some(second)) = (
        part(0, 4),
        part(5, 7),
        part(8, 10),
        part(11, 13),
        part(14, 16),
        part(17, 19),
    ) else {
        return false;
    };
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => 0,
    };
    year > 0
        && year <= 9999
        && day > 0
        && day <= days
        && hour <= 23
        && minute <= 59
        && second <= 59
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| [4, 7, 10, 13, 16, 19].contains(&index) || byte.is_ascii_digit())
}

fn validate_reviews<'a>(
    review: Option<&'a Review>,
    evidence: &Evidence,
    policy: &Policy,
) -> Result<HashMap<String, &'a ReviewRecord>> {
    let mut records = HashMap::new();
    let Some(review) = review else {
        return Ok(records);
    };
    number(review.schema_version, 1.0, 1.0, true)?;
    hash(&review.contract_sha256)?;
    string(&review.run_id, 128)?;
    string(&review.source, 4096)?;
    string(&review.attestation, 4096)?;
    require(
        review.run_id == evidence.run_id
            && review.contract_sha256 == policy.contract_sha256
            && review.source == "external-human-review"
            && review.attestation == ATTESTATION,
        "Quality review is not an external human attestation bound to this run.",
    )?;
    for record in &review.reviews {
        string(&record.sample_id, 128)?;
        let id = record.sample_id.to_ascii_lowercase();
        let sample = evidence
            .samples
            .iter()
            .find(|sample| sample.sample_id == id)
            .ok_or("Unknown quality review sample.")?;
        require(
            records.insert(id, record).is_none(),
            "Duplicate quality review sample.",
        )?;
        hash(&record.response_sha256)?;
        require(
            record.response_sha256 == sample.response_sha256 && record.human_reviewed,
            "Quality review is unbound or lacks explicit human review declarations.",
        )?;
        string(&record.reviewer_id, 256)?;
        string(&record.rationale, 8192)?;
        require(
            valid_timestamp(&record.reviewed_at_utc),
            "Review timestamp must be a valid explicit UTC second.",
        )?;
        let prompt = policy
            .prompts
            .iter()
            .find(|prompt| prompt.id == sample.prompt_id)
            .ok_or("Unknown reviewed prompt.")?;
        let expected = dimensions(&prompt.category);
        require(
            record.scores.len() == expected.len()
                && record
                    .scores
                    .keys()
                    .all(|key| expected.contains(&key.as_str())),
            "Unknown or missing quality score dimension.",
        )?;
        for value in record.scores.values() {
            number(
                value.as_f64().ok_or("Quality score must be numeric.")?,
                0.0,
                4.0,
                true,
            )?;
        }
    }
    Ok(records)
}

fn quality(
    evidence: &Evidence,
    policy: &Policy,
    reviews: &HashMap<String, &ReviewRecord>,
    results: &mut Results,
) -> Result<()> {
    for (index, prompt) in policy.prompts.iter().enumerate() {
        let samples: Vec<_> = evidence
            .samples
            .iter()
            .filter(|sample| sample.prompt_id == prompt.id)
            .collect();
        let refusals = |arm| {
            samples
                .iter()
                .filter(|sample| {
                    sample.arm == arm
                        && reviews
                            .get(&sample.sample_id)
                            .is_some_and(|review| review.refused)
                })
                .count()
        };
        let adapter_refusals = refusals("adapter");
        let result = &mut results.prompts[index];
        result["baselineReviewedRefusals"] = json!(refusals("baseline"));
        result["adapterReviewedRefusals"] = json!(adapter_refusals);
        if adapter_refusals > 0 {
            results.quality_failed = true;
            results.rejections.push(format!(
                "Unwanted adapter refusal on a fixed benign prompt: {}.",
                prompt.id
            ));
        }
        if samples.iter().any(|sample| {
            !reviews
                .get(&sample.sample_id)
                .is_some_and(|review| review.answer_complete)
        }) {
            results.quality_pending = true;
            results.pending.push(format!(
                "Complete external human quality review is missing: {}.",
                prompt.id
            ));
            continue;
        }
        let mut dimension_results = Vec::new();
        for dimension in dimensions(&prompt.category) {
            let scores = |arm: &str| -> Result<f64> {
                samples
                    .iter()
                    .filter(|sample| sample.arm == arm)
                    .map(|sample| {
                        reviews
                            .get(&sample.sample_id)
                            .and_then(|review| review.scores.get(*dimension))
                            .and_then(Value::as_f64)
                            .ok_or_else(|| "Missing validated quality score.".into())
                    })
                    .collect::<Result<Vec<_>>>()
                    .and_then(median)
            };
            let baseline = scores("baseline")?;
            let adapter = scores("adapter")?;
            let passed = adapter >= 3.0 && adapter - baseline >= 0.0;
            if !passed {
                results.quality_failed = true;
                results.rejections.push(format!(
                    "Quality regression or minimum failed: {}/{dimension}.",
                    prompt.id
                ));
            }
            dimension_results.push(json!({"dimension": dimension, "baselineMedian": baseline, "adapterMedian": adapter, "passed": passed}));
        }
        result["quality"] = json!(dimension_results);
    }
    Ok(())
}

pub(super) fn score(evidence: Value, review: Option<Value>, policy: &Policy) -> Result<Value> {
    let evidence: Evidence = decode(evidence)?;
    let review: Option<Review> = review.map(decode).transpose()?;
    number(evidence.schema_version, 1.0, 1.0, true)?;
    hash(&evidence.contract_sha256)?;
    require(
        evidence.contract_sha256 == policy.contract_sha256,
        "Evidence uses a different contract or schema.",
    )?;
    string(&evidence.run_id, 128)?;
    let execution = &evidence.execution;
    require(
        execution.github_actions
            && execution.runner_architecture == "Arm64"
            && bonsai_cpu_policy::hardware::SnapdragonFamily::from_evidence(
                &execution.device_model,
                execution.processor_name.as_deref(),
            )
            .is_some()
            && valid_run_url(&execution.run_url),
        "Qualification requires a recorded CI run with matching Snapdragon X/X2 ARM64 identity.",
    )?;
    string(&execution.runner_name, 128)?;
    let (controls_digest, _) = control_identity(&evidence.controls, policy)?;
    validate_samples(&evidence, policy, &controls_digest)?;
    let mut results = Results::default();
    throughput_memory(&evidence, policy, &mut results)?;
    basis_proof(&evidence, &mut results)?;
    zero_proofs(&evidence, policy, &mut results)?;
    nonzero_proof(&evidence, policy, &mut results)?;
    let reviews = validate_reviews(review.as_ref(), &evidence, policy)?;
    quality(&evidence, policy, &reviews, &mut results)?;
    let status = if !results.rejections.is_empty() {
        "rejected"
    } else if !results.pending.is_empty() {
        "pending"
    } else {
        "accepted"
    };
    let pass = |flag| if flag { "passed" } else { "failed" };
    let mut report = json!({"SchemaVersion": 1, "RunId": evidence.run_id, "ContractSha256": policy.contract_sha256, "ControlsSha256": controls_digest,
        "Status": status, "StandardProfileMayEnable": status == "accepted", "Gates": {
            "Throughput": pass(results.throughput),
            "Quality": if results.quality_failed { "failed" } else if results.quality_pending { "pending" } else { "passed" },
            "MemoryAllowance": pass(results.memory), "AuthoringBasis": if results.basis { "passed" } else { "pending" },
            "ZeroScaleIdentity": if results.zero { "passed" } else { "not-passed" },
            "NonzeroApplication": if results.nonzero { "passed" } else { "not-passed" }},
        "Prompts": results.prompts, "Rejections": results.rejections, "Pending": results.pending,
        "Scope": "Matched pinned representation only. Job commit is not total RAM. Human review identity is recorded, not authenticated. No serving configuration was changed."});
    if policy.profile == super::QualificationProfile::AppScale {
        report["StandardProfileMayEnable"] = json!(false);
        report["AppSettingGatesPassed"] = json!(status == "accepted");
        report["QualificationProfile"] = json!("app-scale-1.3");
        report["AdapterScale"] = json!(policy.adapter_scale());
        report["Scope"] = json!(
            "Matched pinned representation at app adapter scale 1.3 with fixed greedy controls and 512-token cap. Requires complete human-reviewed answers. Other app settings, broad creative quality, sustained comfort and daily service remain separate. No serving configuration was changed."
        );
    }
    // Preserve frozen legacy X1 receipts while binding new observations in
    // reports so a device-specific result cannot lose its family identity.
    if let Some(processor_name) = &execution.processor_name {
        report["Execution"] = json!({"deviceModel": execution.device_model,
            "processorName": processor_name, "runnerArchitecture": execution.runner_architecture,
            "runUrl": execution.run_url, "runnerName": execution.runner_name});
    }
    Ok(report)
}
