//! Small representation decision screen; it never qualifies or promotes a profile.

mod capture;
mod score;
#[cfg(test)]
mod tests;

use std::path::Path;

use serde_json::{Map, Value, json};

use super::{decode, digest, policy, read_json, require, take};
use crate::Result;

const REPRESENTATIONS: [&str; 2] = ["PQ2_0", "current-repacked-PTQ1_0"];
const PROMPT_ID: &str = "creative-scene";
const SCALE: f64 = 1.3;

fn plan() -> Result<Value> {
    let policy = policy()?;
    let prompt = policy.contract["prompts"]
        .as_array()
        .and_then(|prompts| prompts.iter().find(|prompt| prompt["id"] == PROMPT_ID))
        .ok_or("The fixed creative decision prompt is missing.")?;
    let mut controls = policy.contract["controls"].clone();
    controls["maxOutputTokens"] = json!(64);
    let contract = json!({
        "schemaVersion": 1, "purpose": "reject-or-continue representation screen; never qualification or promotion",
        "standardContractSha256": policy.contract_sha256, "prompt": prompt,
        "repeatCount": 3, "adapterScale": SCALE, "controls": controls,
        "representations": policy.contract["representations"],
        "minimumDecodeTokens": 16, "minimumPerArmMedianTpsRatio": 1,
        "minimumQualityScore": 3,
        "maximumPeakJobMemoryBytes": policy.contract["gates"]["maximumPeakJobMemoryBytes"],
        "minimumAvailablePhysicalBytes": 4 * 1_073_741_824_u64,
        "maximumMedianJobMemoryPremiumBytes": 805_306_368_u64,
        "scope": "One fixed creative prefix, 64 output tokens per process, 12 timed fresh-process samples and two loaded-at-zero controls. Prefixes cannot establish answer completeness, quality, first-token latency, thermal behavior, long-context correctness or broad capability."
    });
    let bytes = serde_json::to_string(&contract).map_err(|error| error.to_string())?;
    let mut schedule = Vec::with_capacity(12);
    for repeat in 1..=3 {
        let mut arms = [
            (0, "baseline"),
            (1, "baseline"),
            (0, "adapter"),
            (1, "adapter"),
        ];
        if repeat == 2 {
            arms.reverse();
        }
        for (representation, arm) in arms {
            let order = schedule.len() + 1;
            schedule.push(
                json!({"sampleId": format!("screen-{order:04}"), "promptId": PROMPT_ID,
                "weightRepresentation": REPRESENTATIONS[representation], "arm": arm,
                "repeat": repeat, "order": order, "seed": 42,
                "adapterScale": if arm == "adapter" { SCALE } else { 0.0 }}),
            );
        }
    }
    Ok(
        json!({"Contract": contract, "ContractSha256": digest(&bytes), "Schedule": schedule,
        "TimedSamples": 12, "ZeroScaleSamples": 2, "DefaultMayChange": false,
        "StandardProfileMayEnable": false}),
    )
}

fn controls_identity(value: &Value) -> Result<String> {
    let mut standard = value.clone();
    require(
        standard["maxOutputTokens"] == 64,
        "The decision screen requires its explicit 64-token output cap.",
    )?;
    standard["maxOutputTokens"] = json!(512);
    let controls = decode(standard)?;
    super::control_identity(&controls, &policy()?)?;
    let bytes = serde_json::to_string(value).map_err(|error| error.to_string())?;
    Ok(digest(&bytes))
}

fn object<'a>(value: &'a Value, keys: &[&str]) -> Result<&'a Map<String, Value>> {
    let fields = value.as_object().ok_or("Screen fields must be objects.")?;
    require(
        fields.len() == keys.len() && keys.iter().all(|key| fields.contains_key(*key)),
        "Screen fields are missing or unrecognized.",
    )?;
    Ok(fields)
}

pub(super) fn run(operation: &str, mut request: Map<String, Value>) -> Result<Value> {
    let result = match operation {
        "screen-plan" => plan()?,
        "screen-capture" => return capture::run(request),
        "screen-score" => {
            let evidence: Value = take(&mut request, "evidence")?;
            let review: Option<Value> = take(&mut request, "review")?;
            score::score(&evidence, review.as_ref())?
        }
        "screen-file-score" => {
            let evidence: String = take(&mut request, "evidencePath")?;
            let review: Option<String> = take(&mut request, "reviewPath")?;
            score::score(
                &read_json(Path::new(&evidence))?,
                review
                    .as_deref()
                    .map(|path| read_json(Path::new(path)))
                    .transpose()?
                    .as_ref(),
            )?
        }
        _ => return Err("Unknown representation screen operation.".into()),
    };
    require(
        request.is_empty(),
        "Unrecognized representation screen request field.",
    )?;
    Ok(result)
}
