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
        "Scope": "Batch boundary observations only; changes within samples are not monitored. Charge level and live CPU availability flags are not compatibility conditions. No energy verdict.",
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
}
