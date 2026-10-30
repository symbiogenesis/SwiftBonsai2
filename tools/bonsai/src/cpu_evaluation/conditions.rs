//! Conservative comparison eligibility from observed batch boundary conditions.

use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    operation: String,
    before: Value,
    after: Value,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "PascalCase", deny_unknown_fields)]
struct SampleBoundaries {
    #[serde(default)]
    before: Value,
    #[serde(default)]
    after: Value,
    // Retained receipts may contain this derived value. Always recompute it.
    #[serde(default, rename = "Comparison")]
    _comparison: Option<serde::de::IgnoredAny>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SampleRequest {
    operation: String,
    baseline: Option<SampleBoundaries>,
    candidate: Option<SampleBoundaries>,
}

fn observed_topology(report: &Value) -> Option<&Value> {
    (report["schema_version"] == 1
        && report["kind"] == "runner-native-cpu-observations"
        && report["status"] == "observed"
        && report["architecture"] == "aarch64"
        && report["scope"] == "current-process"
        && report["source"] == "GetSystemCpuSetInformation"
        && report["topology"]["logical_processors"]
            .as_u64()
            .is_some_and(|count| (1..=4096).contains(&count)))
    .then_some(&report["topology"])
}

fn observed_host_identity(report: &Value) -> Option<&Value> {
    let identity = &report["host_identity"];
    if report["architecture"] != "aarch64"
        || identity["status"] != "observed"
        || identity["source"] != crate::host_cpu::HOST_IDENTITY_SOURCE
    {
        return None;
    }
    for field in [
        "processor_name",
        "processor_identifier",
        "vendor_identifier",
    ] {
        let text = identity[field].as_str()?;
        if text.is_empty()
            || text.len() > 2048
            || text.trim().is_empty()
            || text.trim().eq_ignore_ascii_case("unknown")
            || text.chars().any(char::is_control)
        {
            return None;
        }
    }
    let version = identity["windows_version"].as_array()?;
    if version.len() != 3
        || version.iter().any(|part| {
            part.as_u64()
                .is_none_or(|part| u32::try_from(part).is_err())
        })
        || version[0].as_u64()? == 0
        || version[2].as_u64()? == 0
    {
        return None;
    }
    let features = identity["processor_features"].as_array()?;
    if features.len() > 256
        || features
            .iter()
            .any(|feature| feature.as_u64().is_none_or(|feature| feature >= 256))
        || features
            .windows(2)
            .any(|pair| pair[0].as_u64() >= pair[1].as_u64())
    {
        return None;
    }
    Some(identity)
}

fn power(report: &Value, field: &str) -> Option<bool> {
    (report["power_snapshot"]["status"] == "observed"
        && report["power_snapshot"]["source"] == "GetSystemPowerStatus")
        .then(|| report["power_snapshot"][field].as_bool())
        .flatten()
}

fn power_policy(report: &Value) -> Option<(&str, &str)> {
    let policy = &report["power_policy_snapshot"];
    if policy["status"] != "observed"
        || policy["source"]
            != "PowerGetActiveScheme/PowerRegisterForEffectivePowerModeNotifications"
        || policy["notification_version"] != 2
    {
        return None;
    }
    let scheme = policy["active_scheme"].as_str()?;
    let mode = policy["effective_mode"].as_str()?;
    let valid_scheme = scheme.len() == 36
        && scheme.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
            }
        })
        && scheme != "00000000-0000-0000-0000-000000000000";
    let valid_mode = matches!(
        mode,
        "battery-saver"
            | "better-battery"
            | "balanced"
            | "high-performance"
            | "max-performance"
            | "game-mode"
            | "mixed-reality"
    );
    (valid_scheme && valid_mode).then_some((scheme, mode))
}

pub(super) fn compare(request: Value) -> crate::Result<Value> {
    let request: Request = serde_json::from_value(request).map_err(|error| error.to_string())?;
    if request.operation != "compare-conditions" {
        return Err("Unsupported CPU condition comparison operation.".into());
    }
    Ok(compare_observations(&request.before, &request.after))
}

fn compare_observations(before: &Value, after: &Value) -> Value {
    let mut changed = Vec::new();
    let mut unknown = Vec::new();
    match (
        observed_host_identity(before),
        observed_host_identity(after),
    ) {
        (Some(before), Some(after)) if before != after => changed.push("host-identity-changed"),
        (Some(_), Some(_)) => {}
        _ => unknown.push("host-identity-unavailable"),
    }
    match (observed_topology(before), observed_topology(after)) {
        (Some(before), Some(after)) if before != after => changed.push("topology-changed"),
        (Some(_), Some(_)) => {}
        _ => unknown.push("topology-unavailable"),
    }
    for (field, change, unavailable) in [
        (
            "ac_online",
            "power-source-changed",
            "power-source-unavailable",
        ),
        (
            "battery_saver",
            "battery-saver-changed",
            "battery-saver-unavailable",
        ),
    ] {
        match (power(before, field), power(after, field)) {
            (Some(before), Some(after)) if before != after => changed.push(change),
            (Some(_), Some(_)) => {}
            _ => unknown.push(unavailable),
        }
    }
    match (power_policy(before), power_policy(after)) {
        (Some(before), Some(after)) if before != after => changed.push("power-policy-changed"),
        (Some(_), Some(_)) => {}
        _ => unknown.push("power-policy-unavailable"),
    }
    let status = if !changed.is_empty() {
        "changed"
    } else if !unknown.is_empty() {
        "unknown"
    } else {
        "observed-stable"
    };
    json!({
        "Status": status, "Comparable": status == "observed-stable",
        "Changed": changed, "Unknown": unknown,
        "Scope": "Batch boundary observations only; CPU registry identity, Windows kernel version and reported feature set must match independently of live power conditions. Changes within samples and edits to individual scheme settings are not monitored. Charge level and live CPU availability flags are not compatibility conditions. No energy verdict.",
    })
}

pub(super) fn compare_samples(request: Value) -> crate::Result<Value> {
    let request: SampleRequest =
        serde_json::from_value(request).map_err(|error| error.to_string())?;
    if request.operation != "compare-sample-conditions" {
        return Err("Unsupported CPU sample condition comparison operation.".into());
    }
    let baseline = request.baseline.unwrap_or_default();
    let candidate = request.candidate.unwrap_or_default();
    let details = [
        (
            "Baseline",
            compare_observations(&baseline.before, &baseline.after),
        ),
        (
            "Candidate",
            compare_observations(&candidate.before, &candidate.after),
        ),
        (
            "BetweenSamples",
            compare_observations(&baseline.after, &candidate.before),
        ),
    ];
    let mut changed = Vec::new();
    let mut unknown = Vec::new();
    for (name, comparison) in &details {
        for (field, reasons) in [("Changed", &mut changed), ("Unknown", &mut unknown)] {
            for reason in comparison[field]
                .as_array()
                .ok_or("Missing CPU condition comparison reasons.")?
            {
                reasons.push(format!(
                    "{name}:{}",
                    reason.as_str().ok_or("Invalid CPU condition reason.")?
                ));
            }
        }
    }
    let status = if !changed.is_empty() {
        "changed"
    } else if !unknown.is_empty() {
        "unknown"
    } else {
        "observed-stable"
    };
    let mut result = json!({
        "Status": status, "Comparable": status == "observed-stable",
        "Changed": changed, "Unknown": unknown,
        "Scope": "All four sample boundary observations must agree; transitions within a sample remain unobserved. Retained derived comparisons are never trusted. No energy verdict.",
    });
    for (name, comparison) in details {
        result[name] = comparison;
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn observation() -> Value {
        json!({
            "schema_version": 1, "kind": "runner-native-cpu-observations",
            "status": "observed", "architecture": "aarch64", "scope": "current-process",
            "source": "GetSystemCpuSetInformation", "topology": {"logical_processors": 18},
            "host_identity": {"status": "observed", "source": crate::host_cpu::HOST_IDENTITY_SOURCE,
                "processor_name": "Snapdragon X2 Elite Extreme - X2E94100",
                "processor_identifier": "ARMv8 (64-bit) Family 8 Model 0 Revision 1",
                "vendor_identifier": "Qualcomm Technologies Inc", "windows_version": [10, 0, 26300],
                "processor_features": [19, 43, 66]},
            "availability": [{"id": 1, "parked": false}],
            "power_snapshot": {"status": "observed", "source": "GetSystemPowerStatus",
                "ac_online": true, "battery_saver": false, "battery_percent": 80},
            "power_policy_snapshot": {"status": "observed",
                "source": "PowerGetActiveScheme/PowerRegisterForEffectivePowerModeNotifications",
                "notification_version": 2,
                "active_scheme": "381b4222-f694-41f0-9685-ff5bb260df2e", "effective_mode": "balanced"},
        })
    }

    fn result(after: &Value) -> crate::Result<Value> {
        super::super::run(
            json!({"operation": "compare-conditions", "before": observation(), "after": after}),
        )
    }

    #[test]
    fn sample_conditions_require_all_four_observed_boundaries() -> crate::Result<()> {
        let stable = json!({
            "operation": "compare-sample-conditions",
            "baseline": {"Before": observation(), "After": observation()},
            "candidate": {"Before": observation(), "After": observation()},
        });
        assert_eq!(super::super::run(stable.clone())?["Comparable"], true);

        // Batch start/end agree even though both samples span a policy change.
        let mut excursion = stable.clone();
        for (sample, boundary) in [("baseline", "After"), ("candidate", "Before")] {
            excursion[sample][boundary]["power_policy_snapshot"]["effective_mode"] =
                json!("max-performance");
            excursion[sample]["Comparison"] = json!({"Comparable": true});
        }
        assert_eq!(
            result(&excursion["candidate"]["After"])?["Comparable"],
            true
        );
        let compared = super::super::run(excursion)?;
        assert_eq!(compared["Status"], "changed");
        assert_eq!(compared["Comparable"], false);
        assert_eq!(compared["Baseline"]["Comparable"], false);
        assert_eq!(compared["Candidate"]["Comparable"], false);
        assert_eq!(compared["BetweenSamples"]["Comparable"], true);

        // Both individual samples are stable, but use different power modes.
        let mut between = stable.clone();
        for boundary in ["Before", "After"] {
            between["candidate"][boundary]["power_policy_snapshot"]["effective_mode"] =
                json!("max-performance");
        }
        let compared = super::super::run(between)?;
        assert_eq!(compared["Comparable"], false);
        assert_eq!(compared["Baseline"]["Comparable"], true);
        assert_eq!(compared["Candidate"]["Comparable"], true);
        assert_eq!(
            compared["Changed"],
            json!(["BetweenSamples:power-policy-changed"])
        );

        for incomplete in [
            json!({"operation": "compare-sample-conditions"}),
            json!({"operation": "compare-sample-conditions", "baseline": null,
                "candidate": {"Comparison": {"Comparable": true}}}),
            {
                let mut input = stable.clone();
                input["baseline"] = json!({"Before": observation()});
                input
            },
        ] {
            let compared = super::super::run(incomplete)?;
            assert_eq!(compared["Status"], "unknown");
            assert_eq!(compared["Comparable"], false);
            assert!(
                compared["Unknown"]
                    .as_array()
                    .is_some_and(|reasons| !reasons.is_empty())
            );
        }

        let mut live_changes = stable.clone();
        live_changes["candidate"]["After"]["power_snapshot"]["battery_percent"] = json!(79);
        live_changes["baseline"]["After"]["availability"][0]["parked"] = json!(true);
        assert_eq!(super::super::run(live_changes)?["Comparable"], true);

        for input in [
            {
                let mut input = stable.clone();
                input["allowChanged"] = json!(true);
                input
            },
            {
                let mut input = stable;
                input["baseline"]["allowChanged"] = json!(true);
                input
            },
        ] {
            assert!(super::super::run(input).is_err());
        }
        Ok(())
    }

    #[test]
    fn condition_comparison_rejects_known_drift_and_unknowns_without_treating_charge_as_energy()
    -> crate::Result<()> {
        let mut after = observation();
        after["power_snapshot"]["battery_percent"] = json!(79);
        after["availability"][0]["parked"] = json!(true);
        after["isa"] = json!({"status": "observed"});
        assert_eq!(result(&after)?["Comparable"], true);
        for (section, field, value, reason) in [
            (
                "topology",
                "logical_processors",
                json!(12),
                "topology-changed",
            ),
            (
                "power_snapshot",
                "ac_online",
                json!(false),
                "power-source-changed",
            ),
            (
                "power_snapshot",
                "battery_saver",
                json!(true),
                "battery-saver-changed",
            ),
        ] {
            let mut after = observation();
            after[section][field] = value;
            let compared = result(&after)?;
            assert_eq!(compared["Status"], "changed");
            assert_eq!(compared["Comparable"], false);
            assert_eq!(compared["Changed"], json!([reason]));
        }
        for after in [
            json!(null),
            json!({"status": "unknown"}),
            {
                let mut after = observation();
                after["topology"] = json!({});
                after
            },
            {
                let mut after = observation();
                after["power_snapshot"]["ac_online"] = Value::Null;
                after
            },
        ] {
            let compared = result(&after)?;
            assert_eq!(compared["Status"], "unknown");
            assert_eq!(compared["Comparable"], false);
        }
        let mut input = json!({"operation": "compare-conditions", "before": observation(), "after": observation()});
        input["allowChanged"] = json!(true);
        assert!(compare(input).is_err());
        Ok(())
    }

    #[test]
    fn condition_comparison_binds_exact_native_host_even_when_core_counts_match()
    -> crate::Result<()> {
        for (field, changed) in [
            (
                "processor_name",
                json!("Snapdragon X2 Elite Extreme - X2E96100"),
            ),
            (
                "processor_identifier",
                json!("ARMv8 (64-bit) Family 8 Model 0 Revision 2"),
            ),
            ("vendor_identifier", json!("Different Vendor")),
            ("windows_version", json!([10, 0, 26301])),
            ("processor_features", json!([19, 43])),
        ] {
            let mut after = observation();
            after["host_identity"][field] = changed;
            let compared = result(&after)?;
            assert_eq!(compared["Comparable"], false);
            assert_eq!(compared["Changed"], json!(["host-identity-changed"]));
        }
        for (field, malformed) in [
            ("status", json!("unknown")),
            ("source", json!("CIM")),
            ("processor_name", Value::Null),
            ("processor_name", json!("unknown")),
            ("processor_name", json!(" ")),
            ("processor_name", json!("X2\n")),
            ("processor_name", json!("x".repeat(2049))),
            ("windows_version", json!([10, 0])),
            ("windows_version", json!([10, 0, 0])),
            ("windows_version", json!([10, 0, 4_294_967_296_u64])),
            ("processor_features", Value::Null),
            ("processor_features", json!([19, 19])),
            ("processor_features", json!([66, 19])),
            ("processor_features", json!([256])),
            ("processor_features", json!(["19"])),
            ("processor_features", json!(vec![19; 257])),
        ] {
            let mut after = observation();
            after["host_identity"][field] = malformed;
            let compared = result(&after)?;
            assert_eq!(compared["Comparable"], false, "accepted {field}");
            assert_eq!(compared["Unknown"], json!(["host-identity-unavailable"]));
        }
        let mut legacy = observation();
        legacy
            .as_object_mut()
            .ok_or("Missing observation object.")?
            .remove("host_identity");
        assert_eq!(result(&legacy)?["Comparable"], false);
        let mut pair = json!({
            "operation": "compare-sample-conditions",
            "baseline": {"Before": observation(), "After": observation()},
            "candidate": {"Before": observation(), "After": observation()},
        });
        pair["candidate"]["Before"]["host_identity"]["processor_features"] = json!([19, 43]);
        pair["candidate"]["Comparison"] = json!({"Comparable": true});
        let compared = super::super::run(pair)?;
        assert_eq!(compared["Comparable"], false);
        assert_eq!(compared["Candidate"]["Comparable"], false);
        assert_eq!(compared["BetweenSamples"]["Comparable"], false);
        Ok(())
    }

    #[test]
    fn condition_comparison_rejects_power_mode_drift_on_unchanged_ac_and_missing_policy()
    -> crate::Result<()> {
        for (field, value) in [
            ("effective_mode", json!("max-performance")),
            (
                "active_scheme",
                json!("8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c"),
            ),
        ] {
            let mut after = observation();
            after["power_policy_snapshot"][field] = value;
            let compared = result(&after)?;
            assert_eq!(compared["Status"], "changed");
            assert_eq!(compared["Comparable"], false);
            assert_eq!(compared["Changed"], json!(["power-policy-changed"]));
        }
        for policy in [
            Value::Null,
            json!({"status": "unknown"}),
            {
                let mut policy = observation()["power_policy_snapshot"].clone();
                policy["effective_mode"] = json!("future-mode");
                policy
            },
            {
                let mut policy = observation()["power_policy_snapshot"].clone();
                policy["active_scheme"] = json!("unknown");
                policy
            },
            {
                let mut policy = observation()["power_policy_snapshot"].clone();
                policy["active_scheme"] = json!("00000000-0000-0000-0000-000000000000");
                policy
            },
            {
                let mut policy = observation()["power_policy_snapshot"].clone();
                policy["notification_version"] = json!(1);
                policy
            },
        ] {
            let mut after = observation();
            after["power_policy_snapshot"] = policy;
            let compared = result(&after)?;
            assert_eq!(compared["Status"], "unknown");
            assert_eq!(compared["Comparable"], false);
            assert_eq!(compared["Unknown"], json!(["power-policy-unavailable"]));
        }
        let mut legacy = observation();
        legacy
            .as_object_mut()
            .ok_or("Missing observation object.")?
            .remove("power_policy_snapshot");
        assert_eq!(result(&legacy)?["Comparable"], false);
        assert_eq!(
            super::super::run(json!({
                "operation": "compare-conditions", "before": legacy, "after": observation(),
            }))?["Comparable"],
            false
        );
        Ok(())
    }
}
