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
    let mut changed = Vec::new();
    let mut unknown = Vec::new();
    match (
        observed_topology(&request.before),
        observed_topology(&request.after),
    ) {
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
        match (power(&request.before, field), power(&request.after, field)) {
            (Some(before), Some(after)) if before != after => changed.push(change),
            (Some(_), Some(_)) => {}
            _ => unknown.push(unavailable),
        }
    }
    match (power_policy(&request.before), power_policy(&request.after)) {
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
    Ok(json!({
        "Status": status, "Comparable": status == "observed-stable",
        "Changed": changed, "Unknown": unknown,
        "Scope": "Batch boundary observations only; changes within samples and edits to individual scheme settings are not monitored. Active scheme and effective power mode are live policy conditions. Charge level and live CPU availability flags are not compatibility conditions. No energy verdict.",
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn observation() -> Value {
        json!({
            "schema_version": 1, "kind": "runner-native-cpu-observations",
            "status": "observed", "architecture": "aarch64", "scope": "current-process",
            "source": "GetSystemCpuSetInformation", "topology": {"logical_processors": 18},
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
