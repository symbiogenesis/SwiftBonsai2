//! Screen regressions use synthetic counters and never execute a model or benchmark.

use serde_json::{Value, json};

use super::{REPRESENTATIONS, controls_identity, plan, score};
use crate::Result;
use crate::qualification::digest;

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
    Ok(
        json!({"schemaVersion": 1, "runId": "synthetic-screen", "contractSha256": screen["ContractSha256"],
        "execution": {"githubActions": true, "runUrl": "https://github.com/symbiogenesis/SwiftBonsai2Runner/actions/runs/1",
            "runnerArchitecture": "Arm64", "deviceModel": "Snapdragon X Elite", "runnerName": "synthetic-runner",
            "sourceRevision": "7335eb9cf009989d4a9a11ed98878b28ad15dddd"},
        "controls": controls, "samples": samples, "zeroScaleChecks": zero}),
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
