//! Eligibility of timing evidence; changed or missing conditions never reject weights.

use serde_json::{Value, json};

use super::{REPRESENTATIONS, object, require};
use crate::Result;

pub(super) fn evaluate(evidence: &Value, screen: &Value) -> Result<Value> {
    if evidence["schemaVersion"] == 1 {
        return Ok(json!({"Comparable": false, "Status": "legacy-unobserved",
            "Details": [], "Scope": "Schema-1 captures retain measurements but lack required CPU condition observations. Timing cannot accept or reject PTQ1; rerun with schema 2."}));
    }
    let observations = evidence["cpuConditions"]
        .as_array()
        .ok_or("Screen CPU conditions must be an array.")?;
    let mut ids: Vec<_> = screen["Schedule"]
        .as_array()
        .ok_or("Missing screen schedule.")?
        .iter()
        .map(|sample| sample["sampleId"].as_str().map(str::to_owned))
        .collect::<Option<_>>()
        .ok_or("Missing scheduled screen identity.")?;
    ids.extend(REPRESENTATIONS.map(|representation| format!("zero-{representation}")));
    require(
        observations.len() == ids.len(),
        "Screen requires CPU observations for all fourteen processes.",
    )?;
    let mut comparable = true;
    let mut details = Vec::with_capacity(ids.len());
    let mut previous = None;
    for (observation, id) in observations.iter().zip(ids) {
        object(observation, &["sampleId", "observations"])?;
        require(
            observation["sampleId"] == id,
            "Screen CPU condition order or sample identity changed.",
        )?;
        let current = &observation["observations"];
        // The shared comparator validates the native observation grammar and
        // recomputes all derived comparisons, including between processes.
        let comparison = crate::cpu_evaluation::run(json!({
            "operation": "compare-sample-conditions",
            "baseline": previous.unwrap_or(current), "candidate": current,
        }))?;
        comparable &= comparison["Comparable"] == true;
        details.push(json!({"sampleId": id, "comparison": comparison}));
        previous = Some(current);
    }
    Ok(json!({"Comparable": comparable,
        "Status": if comparable { "observed-stable" } else { "ineligible" },
        "Details": details, "Scope": screen["Contract"]["cpuConditions"]["scope"]}))
}
