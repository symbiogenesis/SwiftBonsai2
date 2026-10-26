//! Fail-closed aggregation of the selected GitHub Actions jobs.

use std::collections::BTreeSet;

use serde_json::Value;

/// Return each failed selected job, rejecting incomplete or malformed results.
///
/// Only a successful job or a deliberately unselected skipped job can pass.
#[must_use]
pub fn failures(needs: &Value, jobs: &Value) -> Vec<String> {
    if needs["changes"]["result"] != "success" {
        return vec!["CI impact selection did not succeed".into()];
    }
    checked_failures(needs, jobs)
        .unwrap_or_else(|| vec!["Missing or malformed CI selection/job result".into()])
}

fn checked_failures(needs: &Value, jobs: &Value) -> Option<Vec<String>> {
    let needs = needs.as_object()?;
    let jobs = jobs.as_object()?;
    let actual: BTreeSet<_> = needs.keys().map(String::as_str).collect();
    let expected: BTreeSet<_> = jobs.keys().map(String::as_str).chain(["changes"]).collect();
    if actual != expected {
        return None;
    }
    let plan: Value =
        serde_json::from_str(needs.get("changes")?["outputs"]["plan"].as_str()?).ok()?;
    let selected = plan.get("groups")?.as_object()?;
    let mut errors = Vec::new();
    for (job, group) in jobs {
        let result = needs.get(job)?.get("result")?.as_str()?;
        let expected = selected.get(group.as_str()?)?.as_bool()?;
        if result != "success" && (expected || result != "skipped") {
            let expected = if expected { "True" } else { "False" };
            errors.push(format!("{job}: selected={expected}, result={result}"));
        }
    }
    Some(errors)
}
