//! Strict offline rejection screen; short prefixes cannot pass quality gates.

use std::collections::HashSet;

use serde_json::{Value, json};

use super::{REPRESENTATIONS, controls_identity, object, plan, require};
use crate::Result;
use crate::qualification::{GIB, digest, hash, number, string};

fn metric(sample: &Value, key: &str, minimum: f64, maximum: f64, integer: bool) -> Result<f64> {
    let value = sample[key]
        .as_f64()
        .ok_or("Screen metric must be numeric.")?;
    number(value, minimum, maximum, integer)?;
    Ok(value)
}

fn median(mut values: Vec<f64>) -> Result<f64> {
    require(
        values.len() == 3,
        "Screen median requires all three fixed repetitions.",
    )?;
    values.sort_by(f64::total_cmp);
    Ok(values[1])
}

fn execution(value: &Value) -> Result<()> {
    object(
        value,
        &[
            "githubActions",
            "runUrl",
            "runnerArchitecture",
            "deviceModel",
            "runnerName",
            "sourceRevision",
        ],
    )?;
    let run = value["runUrl"]
        .as_str()
        .and_then(|url| {
            url.strip_prefix("https://github.com/symbiogenesis/SwiftBonsai2Runner/actions/runs/")
        })
        .unwrap_or_default();
    let source = value["sourceRevision"].as_str().unwrap_or_default();
    require(
        value["githubActions"] == true
            && value["runnerArchitecture"] == "Arm64"
            && value["deviceModel"] == "Snapdragon X Elite"
            && !run.is_empty()
            && run.bytes().all(|byte| byte.is_ascii_digit())
            && source.len() == 40
            && source.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "Screen evidence must bind this repository's native Snapdragon CI run and source revision.",
    )?;
    string(
        value["runnerName"]
            .as_str()
            .ok_or("Screen runner name is missing.")?,
        128,
    )
}

fn sample_values(sample: &Value) -> Result<()> {
    metric(sample, "decodeTokens", 16.0, 64.0, true)?;
    metric(sample, "decodeMilliseconds", 0.000_001, 86_400_000.0, false)?;
    metric(sample, "peakJobMemoryBytes", 1.0, 64.0 * GIB, true)?;
    metric(
        sample,
        "minimumAvailablePhysicalBytes",
        0.0,
        1024.0 * GIB,
        true,
    )?;
    require(
        sample["completed"] == true,
        "A screen process did not complete successfully.",
    )?;
    let text = sample["responseText"]
        .as_str()
        .ok_or("Screen response must be exact text.")?;
    string(text, 1_048_576)?;
    require(
        sample["responseSha256"] == digest(text),
        "Screen response digest does not bind its exact text.",
    )
}

fn validate<'a>(evidence: &'a Value, screen: &Value) -> Result<&'a [Value]> {
    object(
        evidence,
        &[
            "schemaVersion",
            "runId",
            "contractSha256",
            "execution",
            "controls",
            "samples",
            "zeroScaleChecks",
        ],
    )?;
    require(
        evidence["schemaVersion"] == 1 && evidence["contractSha256"] == screen["ContractSha256"],
        "Screen schema or fixed contract changed.",
    )?;
    string(
        evidence["runId"]
            .as_str()
            .ok_or("Screen run identity is missing.")?,
        128,
    )?;
    execution(&evidence["execution"])?;
    object(&evidence["controls"], &REPRESENTATIONS)?;
    for representation in REPRESENTATIONS {
        require(
            evidence["controls"][representation]["weightRepresentation"] == representation,
            "Screen controls changed representation.",
        )?;
        controls_identity(&evidence["controls"][representation])?;
    }
    let samples = evidence["samples"]
        .as_array()
        .ok_or("Screen samples must be an array.")?;
    let schedule = screen["Schedule"]
        .as_array()
        .ok_or("Missing fixed screen schedule.")?;
    require(
        samples.len() == schedule.len(),
        "Screen requires all twelve fixed samples.",
    )?;
    let mut ids = HashSet::new();
    for sample in samples {
        let fields = object(
            sample,
            &[
                "sampleId",
                "promptId",
                "weightRepresentation",
                "arm",
                "repeat",
                "order",
                "seed",
                "adapterScale",
                "controlsSha256",
                "decodeTokens",
                "decodeMilliseconds",
                "peakJobMemoryBytes",
                "minimumAvailablePhysicalBytes",
                "completed",
                "responseText",
                "responseSha256",
            ],
        )?;
        let id = sample["sampleId"]
            .as_str()
            .ok_or("Missing screen sample ID.")?;
        require(ids.insert(id), "Duplicate screen sample ID.")?;
        let scheduled = schedule
            .iter()
            .find(|scheduled| scheduled["sampleId"] == id)
            .ok_or("Unscheduled screen sample.")?;
        for (key, expected) in scheduled
            .as_object()
            .ok_or("Invalid fixed screen sample.")?
        {
            require(
                fields.get(key) == Some(expected),
                "A screen sample changed its schedule, scale or representation.",
            )?;
        }
        let representation = sample["weightRepresentation"]
            .as_str()
            .ok_or("Missing screen representation.")?;
        require(
            sample["controlsSha256"] == controls_identity(&evidence["controls"][representation])?,
            "A screen sample changed matched execution controls.",
        )?;
        sample_values(sample)?;
    }
    Ok(samples)
}

fn zero_controls(evidence: &Value, samples: &[Value], rejections: &mut Vec<String>) -> Result<()> {
    let checks = evidence["zeroScaleChecks"]
        .as_array()
        .ok_or("Screen zero controls must be an array.")?;
    require(
        checks.len() == 2,
        "Screen requires one loaded-at-zero control per representation.",
    )?;
    let mut ids = HashSet::new();
    for check in checks {
        object(
            check,
            &[
                "weightRepresentation",
                "baselineResponseSha256",
                "zeroScaleResponseSha256",
            ],
        )?;
        let representation = check["weightRepresentation"]
            .as_str()
            .ok_or("Missing zero-control representation.")?;
        require(
            REPRESENTATIONS.contains(&representation) && ids.insert(representation),
            "Unknown or duplicate screen zero control.",
        )?;
        let baseline = samples
            .iter()
            .find(|sample| {
                sample["weightRepresentation"] == representation
                    && sample["arm"] == "baseline"
                    && sample["repeat"] == 1
            })
            .ok_or("Missing zero-control screen baseline.")?;
        require(
            check["baselineResponseSha256"] == baseline["responseSha256"],
            "Zero-control baseline digest changed.",
        )?;
        hash(
            check["zeroScaleResponseSha256"]
                .as_str()
                .ok_or("Missing zero-control response digest.")?,
        )?;
        if check["baselineResponseSha256"] != check["zeroScaleResponseSha256"] {
            rejections.push(format!(
                "Loaded-at-zero response identity failed: {representation}."
            ));
        }
    }
    Ok(())
}

fn arm(samples: &[Value], representation: &str, arm: &str, key: &str) -> Result<f64> {
    median(
        samples
            .iter()
            .filter(|sample| {
                sample["weightRepresentation"] == representation && sample["arm"] == arm
            })
            .map(|sample| {
                if key == "tps" {
                    Ok(metric(sample, "decodeTokens", 16.0, 64.0, true)? * 1000.0
                        / metric(sample, "decodeMilliseconds", 0.000_001, 86_400_000.0, false)?)
                } else {
                    metric(sample, key, 1.0, 64.0 * GIB, true)
                }
            })
            .collect::<Result<Vec<_>>>()?,
    )
}

fn comparisons(samples: &[Value], rejections: &mut Vec<String>) -> Result<Vec<Value>> {
    let mut comparisons = Vec::with_capacity(2);
    for name in ["baseline", "adapter"] {
        let pq2 = arm(samples, REPRESENTATIONS[0], name, "tps")?;
        let ptq1 = arm(samples, REPRESENTATIONS[1], name, "tps")?;
        let ratio = ptq1 / pq2;
        if ratio < 1.0 {
            rejections.push(format!("PTQ1 screen throughput is below PQ2: {name}."));
        }
        comparisons.push(json!({"arm": name, "pq2MedianTps": pq2, "ptq1MedianTps": ptq1,
            "ptq1ToPq2TpsRatio": ratio, "throughputPassed": ratio >= 1.0,
            "medianJobMemoryDeltaBytes": arm(samples, REPRESENTATIONS[1], name, "peakJobMemoryBytes")?
                - arm(samples, REPRESENTATIONS[0], name, "peakJobMemoryBytes")?}));
    }
    for representation in REPRESENTATIONS {
        let premium = arm(samples, representation, "adapter", "peakJobMemoryBytes")?
            - arm(samples, representation, "baseline", "peakJobMemoryBytes")?;
        if premium > 0.75 * GIB {
            rejections.push(format!(
                "Screen LoRA Job-commit premium exceeds 768 MiB: {representation}."
            ));
        }
    }
    Ok(comparisons)
}

pub(super) fn score(evidence: &Value, review: Option<&Value>) -> Result<Value> {
    require(
        review.is_none(),
        "A 64-token prefix screen cannot consume full-answer qualification reviews.",
    )?;
    let screen = plan()?;
    let samples = validate(evidence, &screen)?;
    let job_memory_limit = screen["Contract"]["maximumPeakJobMemoryBytes"]
        .as_f64()
        .ok_or("Missing validated screen Job-commit limit.")?;
    let mut rejections = Vec::new();
    for sample in samples {
        if metric(sample, "peakJobMemoryBytes", 1.0, 64.0 * GIB, true)? > job_memory_limit
            || metric(
                sample,
                "minimumAvailablePhysicalBytes",
                0.0,
                1024.0 * GIB,
                true,
            )? < 4.0 * GIB
        {
            rejections.push(format!(
                "Screen resource allowance exceeded: {}.",
                sample["sampleId"]
            ));
        }
    }
    zero_controls(evidence, samples, &mut rejections)?;
    let comparisons = comparisons(samples, &mut rejections)?;
    Ok(
        json!({"SchemaVersion": 1, "RunId": evidence["runId"], "ContractSha256": screen["ContractSha256"],
        "Status": if rejections.is_empty() { "continue" } else { "rejected" },
        "Decision": if rejections.is_empty() { "continue-correctness-and-review" } else { "stop-candidate" },
        "DefaultMayChange": false, "StandardProfileMayEnable": false,
        "Comparisons": comparisons, "Rejections": rejections,
        "Gates": {"Quality": "pending", "AuthoringBasis": "pending", "NativeLogits": "pending", "FullQualification": "not-run"},
        "SerializedSavingBytes": 1_259_520_000_u64,
        "Scope": "Reject-or-continue diagnostic from 64-token prefixes at LoRA scale 1.3. Job commit excludes mapped weight residency and is not total RAM. Complete quality, logits/basis correctness, long-context, first-token and sustained-device behavior remain unqualified. No serving configuration was changed."}),
    )
}
