//! Screen regressions use synthetic counters and never execute a model or benchmark.

use serde_json::{Value, json};

use super::{REPRESENTATIONS, controls_identity, plan, plan_version, score};
use crate::Result;
use crate::qualification::digest;

pub(super) fn condition_boundaries() -> Value {
    let observed = json!({
        "schema_version": 1, "kind": "runner-native-cpu-observations",
        "status": "observed", "architecture": "aarch64", "scope": "current-process",
        "source": "GetSystemCpuSetInformation", "topology": {"logical_processors": 12},
        "host_identity": {"status": "observed", "source": crate::host_cpu::HOST_IDENTITY_SOURCE,
            "processor_name": "Qualcomm Snapdragon X Elite",
            "processor_identifier": "ARMv8 (64-bit) Family 8 Model 0 Revision 1",
            "vendor_identifier": "Qualcomm Technologies Inc", "windows_version": [10, 0, 26300],
            "processor_features": [19, 43]},
        "power_snapshot": {"status": "observed", "source": "GetSystemPowerStatus",
            "ac_online": true, "battery_saver": false},
        "power_policy_snapshot": {"status": "observed",
            "source": "PowerGetActiveScheme/PowerRegisterForEffectivePowerModeNotifications",
            "notification_version": 2,
            "active_scheme": "381b4222-f694-41f0-9685-ff5bb260df2e", "effective_mode": "balanced"},
    });
    json!({"Before": observed, "After": observed})
}

fn evidence() -> Result<Value> {
    let screen = plan()?;
    let mut controls = json!({});
    for representation in REPRESENTATIONS {
        let mut control = screen["Contract"]["controls"].clone();
        control["modelSha256"] = screen["Contract"]["representations"][representation].clone();
        control["weightRepresentation"] = json!(representation);
        controls[representation] = control;
    }
    let mut samples = Vec::new();
    for scheduled in screen["Schedule"]
        .as_array()
        .ok_or("Missing synthetic schedule.")?
    {
        let mut sample = scheduled.clone();
        let representation = sample["weightRepresentation"]
            .as_str()
            .ok_or("Missing synthetic representation.")?;
        let identity = controls_identity(&controls[representation])?;
        let text = format!("Synthetic bounded prefix for {representation}.");
        for (key, value) in [
            ("controlsSha256", json!(identity)),
            ("decodeTokens", json!(63)),
            ("decodeMilliseconds", json!(1000)),
            ("peakJobMemoryBytes", json!(1_073_741_824)),
            ("minimumAvailablePhysicalBytes", json!(8_589_934_592_u64)),
            ("completed", json!(true)),
            ("responseSha256", json!(digest(&text))),
            ("responseText", json!(text)),
        ] {
            sample[key] = value;
        }
        samples.push(sample);
    }
    let zero: Vec<_> = REPRESENTATIONS
        .iter()
        .map(|representation| {
            let text = format!("Synthetic bounded prefix for {representation}.");
            json!({"weightRepresentation": representation, "baselineResponseSha256": digest(&text),
            "zeroScaleResponseSha256": digest(&text)})
        })
        .collect();
    let conditions: Vec<_> = samples
        .iter()
        .map(|sample| sample["sampleId"].clone())
        .chain(REPRESENTATIONS.map(|representation| json!(format!("zero-{representation}"))))
        .map(|id| json!({"sampleId": id, "observations": condition_boundaries()}))
        .collect();
    Ok(
        json!({"schemaVersion": 2, "runId": "synthetic-screen", "contractSha256": screen["ContractSha256"],
        "execution": {"githubActions": true, "runUrl": "https://github.com/symbiogenesis/SwiftBonsai2Runner/actions/runs/1",
            "runnerArchitecture": "Arm64", "deviceModel": "Snapdragon X Elite", "runnerName": "synthetic-runner",
            "sourceRevision": "7335eb9cf009989d4a9a11ed98878b28ad15dddd"},
        "controls": controls, "samples": samples, "zeroScaleChecks": zero, "cpuConditions": conditions}),
    )
}

#[test]
fn fixed_prefix_screen_never_promotes_or_claims_quality() -> Result<()> {
    let screen = plan()?;
    assert_eq!(screen["TimedSamples"], 12);
    assert_eq!(screen["ZeroScaleSamples"], 2);
    assert_eq!(screen["Contract"]["controls"]["maxOutputTokens"], 64);
    assert_eq!(screen["Contract"]["adapterScale"], 1.3);
    let report = score::score(&evidence()?, None)?;
    assert_eq!(report["Status"], "continue");
    assert_eq!(report["DefaultMayChange"], false);
    assert_eq!(report["StandardProfileMayEnable"], false);
    assert_eq!(report["Gates"]["Quality"], "pending");
    assert_eq!(report["Gates"]["NativeLogits"], "pending");
    Ok(())
}

#[test]
fn x2_screen_requires_matching_observation_and_retains_device_scope() -> Result<()> {
    let mut evidence = evidence()?;
    evidence["execution"]["deviceModel"] = json!("Snapdragon X2 Elite Extreme");
    assert!(score::score(&evidence, None).is_err());
    evidence["execution"]["processorName"] = json!("Snapdragon(R) X2 Elite Extreme - X2E-94-100");
    let report = score::score(&evidence, None)?;
    assert_eq!(report["Status"], "continue");
    assert_eq!(report["Execution"], evidence["execution"]);
    evidence["execution"]["deviceModel"] = json!("Snapdragon X Elite");
    assert!(score::score(&evidence, None).is_err());
    evidence["execution"]["deviceModel"] = json!("Snapdragon X2 Elite Extreme");
    evidence["execution"]["processorName"] = Value::Null;
    assert!(score::score(&evidence, None).is_err());
    Ok(())
}

#[test]
fn screen_job_commit_gate_matches_the_enforced_process_boundary() -> Result<()> {
    let limit = bonsai_cpu_policy::Profile::SwiftPq2Assessment.memory_limit_bytes();
    assert_eq!(limit, 12_348_030_976);
    assert_eq!(plan()?["Contract"]["maximumPeakJobMemoryBytes"], limit);
    for representation in REPRESENTATIONS {
        for (observed, expected) in [(limit, "continue"), (limit + 1, "rejected")] {
            let mut evidence = evidence()?;
            let sample = evidence["samples"]
                .as_array_mut()
                .and_then(|samples| {
                    samples.iter_mut().find(|sample| {
                        sample["weightRepresentation"] == representation
                            && sample["arm"] == "baseline"
                    })
                })
                .ok_or("Missing synthetic baseline sample.")?;
            sample["peakJobMemoryBytes"] = json!(observed);
            let report = score::score(&evidence, None)?;
            assert_eq!(report["Status"], expected);
            assert_eq!(report["DefaultMayChange"], false);
        }
    }
    Ok(())
}

#[test]
fn prefix_screen_rejects_regressions_and_mismatched_evidence() -> Result<()> {
    let base = evidence()?;
    for pointer in [
        "/controls/PQ2_0/maxOutputTokens",
        "/samples/0/adapterScale",
        "/samples/0/order",
        "/samples/0/decodeTokens",
        "/samples/0/controlsSha256",
        "/samples/0/responseSha256",
        "/execution/runnerName",
        "/execution/sourceRevision",
    ] {
        let mut changed = base.clone();
        *changed
            .pointer_mut(pointer)
            .ok_or("Missing synthetic mutation target.")? = Value::Null;
        assert!(
            score::score(&changed, None).is_err(),
            "Accepted changed control {pointer}"
        );
    }
    let mut incomplete = base.clone();
    incomplete["samples"]
        .as_array_mut()
        .ok_or("Missing synthetic samples.")?
        .pop();
    assert!(score::score(&incomplete, None).is_err());
    let mut slow = base.clone();
    for sample in slow["samples"]
        .as_array_mut()
        .ok_or("Missing synthetic samples.")?
    {
        if sample["weightRepresentation"] == REPRESENTATIONS[1] {
            sample["decodeMilliseconds"] = json!(2000);
        }
    }
    assert_eq!(score::score(&slow, None)?["Status"], "rejected");
    let mut zero = base.clone();
    zero["zeroScaleChecks"][0]["zeroScaleResponseSha256"] = json!(digest("changed"));
    assert_eq!(score::score(&zero, None)?["Status"], "rejected");
    let mut resource = base;
    resource["samples"][0]["minimumAvailablePhysicalBytes"] = json!(0);
    assert_eq!(score::score(&resource, None)?["Status"], "rejected");
    Ok(())
}

#[test]
fn screen_conditions_invalidate_timing_without_rejecting_the_candidate() -> Result<()> {
    assert_eq!(
        plan_version(1)?["ContractSha256"],
        "ed8ab507e5a866c3ed707a5ae727f33126e45c55fd265500dc959f054cb08171"
    );
    let mut slow = evidence()?;
    for sample in slow["samples"].as_array_mut().ok_or("Missing samples.")? {
        if sample["weightRepresentation"] == REPRESENTATIONS[1] {
            sample["decodeMilliseconds"] = json!(2000);
        }
    }
    assert_eq!(score::score(&slow, None)?["Status"], "rejected");
    for pointer in [
        "/cpuConditions/1/observations/Before/power_snapshot/ac_online",
        "/cpuConditions/1/observations/After/power_policy_snapshot/effective_mode",
        "/cpuConditions/13/observations/After/topology/logical_processors",
    ] {
        let mut changed = slow.clone();
        *changed.pointer_mut(pointer).ok_or("Missing condition.")? = Value::Null;
        let report = score::score(&changed, None)?;
        assert_eq!(report["Status"], "inconclusive", "{pointer}");
        assert_eq!(report["CpuConditions"]["Comparable"], false);
        assert_eq!(report["Comparisons"][0]["throughputPassed"], Value::Null);
        assert_eq!(report["Comparisons"][0]["ptq1ToPq2TpsRatio"], Value::Null);
        assert_eq!(report["Rejections"], json!([]));
    }
    // Individually stable processes can still disagree between boundaries.
    let mut changed = slow.clone();
    for boundary in ["Before", "After"] {
        changed["cpuConditions"][1]["observations"][boundary]["power_snapshot"]["ac_online"] =
            json!(false);
    }
    let report = score::score(&changed, None)?;
    assert_eq!(report["Status"], "inconclusive");
    assert_eq!(
        report["CpuConditions"]["Details"][1]["comparison"]["BetweenSamples"]["Comparable"],
        false
    );
    // Retained derived comparisons cannot override raw observations.
    changed["cpuConditions"][1]["observations"]["Comparison"] = json!({"Comparable": true});
    assert_eq!(score::score(&changed, None)?["Status"], "inconclusive");

    let mut legacy = slow;
    legacy["schemaVersion"] = json!(1);
    legacy["contractSha256"] = plan_version(1)?["ContractSha256"].clone();
    legacy
        .as_object_mut()
        .ok_or("Missing evidence.")?
        .remove("cpuConditions");
    let report = score::score(&legacy, None)?;
    assert_eq!(report["Status"], "inconclusive");
    assert_eq!(report["CpuConditions"]["Status"], "legacy-unobserved");
    assert_eq!(report["Decision"], "rerun-with-stable-cpu-conditions");
    Ok(())
}
