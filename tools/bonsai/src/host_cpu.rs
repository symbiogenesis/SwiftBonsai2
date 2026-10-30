//! Model-free native CPU observations for retained device tuning evidence.

use serde_json::{Value, json};

pub(super) const HOST_IDENTITY_SOURCE: &str =
    "HKLM/HARDWARE/DESCRIPTION/System/CentralProcessor/0;RtlGetVersion;IsProcessorFeaturePresent";

fn host_identity_snapshot() -> Value {
    #[cfg(windows)]
    if let Ok(host) = bonsai_windows_process::native_build_host() {
        return observed_host_identity(&host);
    }
    json!({"status": "unknown", "source": HOST_IDENTITY_SOURCE,
        "processor_name": null, "processor_identifier": null, "vendor_identifier": null,
        "windows_version": null, "processor_features": null})
}

#[cfg(windows)]
fn observed_host_identity(host: &bonsai_windows_process::NativeBuildHost) -> Value {
    json!({"status": "observed", "source": HOST_IDENTITY_SOURCE,
        "processor_name": host.processor_name, "processor_identifier": host.processor_identifier,
        "vendor_identifier": host.vendor_identifier, "windows_version": host.windows_version,
        "processor_features": host.processor_features})
}

fn isa_snapshot() -> Value {
    #[cfg(windows)]
    if let Ok(isa) = bonsai_windows_process::arm64_isa() {
        return json!({"status": "observed", "source": "IsProcessorFeaturePresent",
            "scope": "current-native-arm64-process", "advsimd": isa.advsimd,
            "dotprod": isa.dotprod, "i8mm": isa.i8mm,
            "negative_result": "unsupported-or-not-detected", "dispatch_qualified": false});
    }
    json!({"status": "unknown", "source": "IsProcessorFeaturePresent",
        "scope": "current-native-arm64-process", "advsimd": null,
        "dotprod": null, "i8mm": null,
        "negative_result": "unsupported-or-not-detected", "dispatch_qualified": false})
}

fn power_snapshot() -> Value {
    #[cfg(windows)]
    if let Ok(power) = bonsai_windows_process::power_status() {
        return json!({"status": "observed", "source": "GetSystemPowerStatus",
            "ac_online": power.ac_online, "battery_saver": power.battery_saver,
            "battery_percent": power.battery_percent});
    }
    json!({"status": "unknown", "source": "GetSystemPowerStatus",
        "ac_online": null, "battery_saver": null, "battery_percent": null})
}

fn power_policy_snapshot() -> Value {
    #[cfg(windows)]
    if let Ok(policy) = bonsai_windows_process::power_policy() {
        return json!({"status": "observed",
            "source": "PowerGetActiveScheme/PowerRegisterForEffectivePowerModeNotifications",
            "notification_version": 2, "active_scheme": policy.active_scheme,
            "effective_mode": policy.effective_mode.as_str()});
    }
    json!({"status": "unknown",
        "source": "PowerGetActiveScheme/PowerRegisterForEffectivePowerModeNotifications",
        "notification_version": 2, "active_scheme": null, "effective_mode": null})
}

#[cfg(windows)]
fn observed(sets: &[bonsai_windows_process::CpuSet]) -> Value {
    use std::collections::BTreeSet;

    let groups: BTreeSet<_> = sets.iter().map(|set| set.group).collect();
    let cores: BTreeSet<_> = sets.iter().map(|set| (set.group, set.core_index)).collect();
    let efficiency: BTreeSet<_> = sets.iter().map(|set| set.efficiency_class).collect();
    let scheduling: BTreeSet<_> = sets.iter().map(|set| set.scheduling_class).collect();
    let topology: Vec<_> = sets
        .iter()
        .map(|set| {
            json!({
                "id": set.id, "group": set.group,
                "logical_processor_index": set.logical_processor_index,
                "core_index": set.core_index, "last_level_cache_index": set.last_level_cache_index,
                "numa_node_index": set.numa_node_index, "efficiency_class": set.efficiency_class,
                "scheduling_class": set.scheduling_class
            })
        })
        .collect();
    let availability: Vec<_> = sets.iter().map(|set| json!({
        "id": set.id, "parked": set.parked, "allocated": set.allocated,
        "allocated_to_target_process": set.allocated_to_target_process, "real_time": set.real_time
    })).collect();
    json!({
        "status": "observed", "error": null,
        "topology": {"cpu_sets": topology, "processor_groups": groups,
            "logical_processors": sets.len(), "physical_cores": cores.len(),
            "efficiency_classes": efficiency, "scheduling_classes": scheduling},
        "availability": availability
    })
}

/// Reports bounded Windows CPU-set observations without changing scheduling.
///
/// Availability flags are a live snapshot. CPU-set IDs and class numbers are
/// observations, not a persisted affinity policy, SKU identity or ISA verdict.
/// A failed probe returns explicit unknowns; it never synthesizes a topology
/// from the process's available parallelism or a marketing specification.
///
/// # Errors
/// Rejects every request except an empty JSON object.
pub fn run(request: &Value) -> crate::Result<Value> {
    if !request.as_object().is_some_and(serde_json::Map::is_empty) {
        return Err("Native CPU inventory requires an empty request object.".into());
    }
    #[cfg(windows)]
    let observation = bonsai_windows_process::cpu_sets().map(|sets| observed(&sets));
    #[cfg(not(windows))]
    let observation: std::io::Result<Value> =
        Err(std::io::Error::from(std::io::ErrorKind::Unsupported));
    let mut report = observation.unwrap_or_else(|_| {
        json!({
            "status": "unknown", "topology": null, "availability": null,
            "error": "native-cpu-query-unavailable"
        })
    });
    report["schema_version"] = json!(1);
    report["kind"] = json!("runner-native-cpu-observations");
    report["architecture"] = json!(std::env::consts::ARCH);
    report["scope"] = json!("current-process");
    report["source"] = json!("GetSystemCpuSetInformation");
    report["qualification"] = json!("unqualified");
    report["host_identity"] = host_identity_snapshot();
    report["power_snapshot"] = power_snapshot();
    report["power_policy_snapshot"] = power_policy_snapshot();
    report["isa"] = isa_snapshot();
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::run;
    use serde_json::json;

    #[test]
    fn inventory_rejects_injected_topology_and_execution_controls() {
        for request in [
            json!(null),
            json!([]),
            json!({"threads": 18}),
            json!({"topology": {}}),
        ] {
            assert!(run(&request).is_err());
        }
    }

    #[cfg(windows)]
    #[test]
    fn host_identity_retains_native_cpu_kernel_and_feature_facts_separately() {
        let host = bonsai_windows_process::NativeBuildHost {
            processor_name: "Snapdragon X2 Elite Extreme - X2E94100".into(),
            processor_identifier: "ARMv8 (64-bit) Family 8 Model 0 Revision 1".into(),
            vendor_identifier: "Qualcomm Technologies Inc".into(),
            windows_version: [10, 0, 26300],
            processor_features: vec![19, 43, 66],
        };
        let identity = super::observed_host_identity(&host);
        assert_eq!(identity["status"], "observed");
        assert_eq!(identity["processor_name"], host.processor_name);
        assert_eq!(identity["processor_identifier"], host.processor_identifier);
        assert_eq!(identity["vendor_identifier"], host.vendor_identifier);
        assert_eq!(identity["windows_version"], json!([10, 0, 26300]));
        assert_eq!(identity["processor_features"], json!([19, 43, 66]));
        for live_field in [
            "ac_online",
            "battery_saver",
            "battery_percent",
            "availability",
        ] {
            assert!(identity.get(live_field).is_none());
        }
    }

    #[cfg(windows)]
    #[test]
    fn observation_separates_group_relative_topology_from_live_availability() {
        use bonsai_windows_process::CpuSet;

        let first = CpuSet {
            id: 1,
            group: 0,
            logical_processor_index: 0,
            core_index: 0,
            last_level_cache_index: 0,
            numa_node_index: 0,
            efficiency_class: 1,
            scheduling_class: 2,
            parked: false,
            allocated: false,
            allocated_to_target_process: false,
            real_time: false,
        };
        let second = CpuSet {
            id: 2,
            group: 1,
            ..first
        };
        let third = CpuSet {
            id: 3,
            logical_processor_index: 1,
            parked: true,
            ..first
        };
        let report = super::observed(&[first, second, third]);
        assert_eq!(report["topology"]["logical_processors"], 3);
        assert_eq!(report["topology"]["physical_cores"], 2);
        assert_eq!(report["topology"]["processor_groups"], json!([0, 1]));
        assert_eq!(report["availability"][2]["parked"], true);
        assert!(report["topology"]["cpu_sets"][2].get("parked").is_none());
        assert_eq!(report["topology"]["efficiency_classes"], json!([1]));
    }
}
