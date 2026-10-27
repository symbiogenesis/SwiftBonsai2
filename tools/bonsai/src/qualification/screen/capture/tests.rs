//! Synthetic supervision fixtures; no native inference or timing is executed.

use std::cell::Cell;
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use super::{Request, capture_with};
use crate::Result;
use crate::qualification::{capture::save, digest, read_json};

fn admission(plan: &Value) -> Value {
    let fixed = &plan["Policy"];
    json!({"Status": "admission-passed", "ModelRevision": plan["Lock"]["model"]["revision"],
        "RuntimeCommit": plan["Lock"]["runtime"]["commit"], "ModelPath": plan["ModelPath"],
        "ArtifactSha256": plan["Artifact"]["Sha256"], "EstimatedWeightBytes": plan["Artifact"]["Bytes"],
        "RequiredAvailableBytes": fixed["RequiredAvailableBytes"], "ServiceCeilingBytes": fixed["ServiceCeilingBytes"],
        "AvailableMemoryFloorBytes": fixed["AvailableMemoryFloorBytes"], "AvailablePhysicalBytesAtAdmission": 20_u64 * 1024 * 1024 * 1024})
}

fn fixture() -> Result<(Request, Value)> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "Bonsai-ScreenFixture-{}-{stamp}",
        std::process::id()
    ));
    let data = root.join("data");
    let evidence = root.join("evidence");
    fs::create_dir_all(&data).map_err(|error| error.to_string())?;
    fs::create_dir(&evidence).map_err(|error| error.to_string())?;
    let make_plan = |repacked| {
        bonsai_cpu_policy::run(json!({"operation": "plan", "dataDirectory": data,
        "profile": "swift-pq2-assessment", "useAbliterateLora": true, "useRepackedPtq1": repacked}))
    };
    let pq2 = make_plan(false)?;
    let ptq1 = make_plan(true)?;
    let lock = &pq2["Lock"];
    let adapter = Path::new(
        pq2["AdapterPath"]
            .as_str()
            .ok_or("Missing synthetic adapter path.")?,
    );
    fs::create_dir_all(
        adapter
            .parent()
            .ok_or("Missing synthetic adapter parent.")?,
    )
    .map_err(|error| error.to_string())?;
    fs::File::create(adapter)
        .and_then(|file| file.set_len(9_682_464))
        .map_err(|error| error.to_string())?;
    let request = serde_json::from_value(json!({"dataDirectory": data, "evidenceDirectory": evidence,
        "processorName": "Qualcomm Snapdragon X Elite", "runId": "synthetic-screen", "startedUtc": "2026-10-26T00:00:00Z",
        "parentProcessId": std::process::id(), "parentCreationTimeFileTime": 123,
        "pq2Admission": admission(&pq2), "ptq1Admission": admission(&ptq1), "lock": lock,
        "adapter": {"AdapterPath": pq2["AdapterPath"], "Sha256": pq2["AdapterLock"]["adapter"]["sha256"]},
        "runtime": {"Commit": lock["runtime"]["commit"], "RuntimeFiles": [], "Runtime": {
            "archivePath": data.join("downloads").join(lock["runtime"]["asset"]["filename"].as_str().ok_or("Missing synthetic archive.")?),
            "runtimeDirectory": data.join("runtime").join(lock["runtime"]["commit"].as_str().ok_or("Missing synthetic runtime.")?),
            "archiveBytes": lock["runtime"]["asset"]["bytes"], "archiveSha256": lock["runtime"]["asset"]["sha256"]}}}))
        .map_err(|error| error.to_string())?;
    Ok((
        request,
        json!({"githubActions": true, "runUrl": "https://github.com/symbiogenesis/SwiftBonsai2Runner/actions/runs/123",
        "runnerArchitecture": "Arm64", "deviceModel": "Snapdragon X Elite", "runnerName": "synthetic-runner", "sourceRevision": "a".repeat(40)}),
    ))
}

fn completed(native: &Value, count: usize) -> Result<Value> {
    assert_eq!(native["ciOnly"], true);
    assert_eq!(native["memoryLimitBytes"], 12_616_466_432_u64);
    assert_eq!(native["requiredAvailableBytes"], 16_911_433_728_u64);
    assert_eq!(native["physicalReserveBytes"], 4_u64 * 1024 * 1024 * 1024);
    assert_eq!(native["timeoutSeconds"], 600);
    assert_eq!(native["parentCreationTimeFileTime"], 123);
    let root = Path::new(
        native["evidenceDirectory"]
            .as_str()
            .ok_or("Missing synthetic screen root.")?,
    );
    let samples = native["samples"]
        .as_array()
        .ok_or("Missing synthetic screen samples.")?;
    assert_eq!(samples.len(), 14);
    assert_eq!(native["artifacts"].as_array().map(Vec::len), Some(3));
    let mut adapter_count = 0;
    let mut zero_count = 0;
    for sample in samples {
        let arguments = sample["arguments"]
            .as_array()
            .ok_or("Missing synthetic screen arguments.")?;
        let predict = arguments
            .iter()
            .position(|argument| argument == "--n-predict")
            .ok_or("Missing screen output cap.")?;
        assert_eq!(arguments.get(predict + 1), Some(&json!("64")));
        if let Some(index) = arguments
            .iter()
            .position(|argument| argument == "--lora-scaled")
        {
            match arguments.get(index + 1).and_then(Value::as_str) {
                Some("bonsai-abliterate-lora.gguf:1.3") => adapter_count += 1,
                Some("bonsai-abliterate-lora.gguf:0") => zero_count += 1,
                _ => return Err("Screen adapter strength differs from its contract.".into()),
            }
        }
    }
    assert_eq!((adapter_count, zero_count), (6, 2));
    for sample in samples.iter().take(count) {
        let lane = Path::new(
            sample["evidenceDirectory"]
                .as_str()
                .ok_or("Missing synthetic sample lane.")?,
        );
        save(
            &lane.join("capture.json"),
            &json!({"SchemaVersion": 1, "Text": "bounded prefix", "Sha256": digest("bounded prefix"), "Diagnostics": {}}),
            true,
        )?;
        save(
            &lane.join("supervision.json"),
            &json!({"Status": "completed", "Timing": {"DecodeTokens": 63, "DecodeMilliseconds": 1000},
            "PeakJobMemoryBytes": 1_073_741_824, "MinimumAvailablePhysicalBytes": 8_589_934_592_u64}),
            true,
        )?;
    }
    let result = json!({"Status": if count == 14 { "completed" } else { "failed" }, "ModelProcessStarted": count > 0, "RuntimeFiles": []});
    save(&root.join("supervision.json"), &result, true)?;
    Ok(result)
}

#[test]
fn screen_capture_uses_fourteen_bounded_processes_without_promoting() -> Result<()> {
    let (request, context) = fixture()?;
    let receipt = capture_with(&request, &context, |native| completed(&native, 14))?;
    assert_eq!(receipt["Status"], "capture-completed");
    assert_eq!(receipt["ScreenStatus"], "continue");
    assert_eq!(receipt["DefaultMayChange"], false);
    let evidence = read_json(&request.evidence_directory.join("screen-evidence.json"))?;
    assert_eq!(evidence["samples"].as_array().map(Vec::len), Some(12));
    assert_eq!(
        evidence["zeroScaleChecks"].as_array().map(Vec::len),
        Some(2)
    );
    assert_eq!(
        read_json(&request.evidence_directory.join("screen-report.json"))?["Gates"]["Quality"],
        "pending"
    );
    Ok(())
}

#[test]
fn screen_capture_retains_completed_prefix_on_native_failure() -> Result<()> {
    let (request, context) = fixture()?;
    assert!(
        capture_with(&request, &context, |native| {
            completed(&native, 1)?;
            Err("Deliberate synthetic screen failure.".into())
        })
        .is_err()
    );
    let receipt = read_json(&request.evidence_directory.join("screen-receipt.json"))?;
    assert_eq!(receipt["Status"], "failed");
    assert_eq!(receipt["ModelProcessStarted"], true);
    let evidence = read_json(&request.evidence_directory.join("screen-evidence.json"))?;
    assert_eq!(evidence["samples"].as_array().map(Vec::len), Some(1));
    assert!(
        !request
            .evidence_directory
            .join("screen-report.json")
            .exists()
    );
    Ok(())
}

#[test]
fn screen_capture_preflight_and_setup_fail_without_starting_models() -> Result<()> {
    let (mut request, context) = fixture()?;
    request.ptq1_admission["ArtifactSha256"] = json!("0".repeat(64));
    let invoked = Cell::new(false);
    assert!(
        capture_with(&request, &context, |_| {
            invoked.set(true);
            Err("Must not supervise.".into())
        })
        .is_err()
    );
    assert!(!invoked.get());
    assert!(
        !request
            .evidence_directory
            .join("screen-evidence.json")
            .exists()
    );
    let (request, context) = fixture()?;
    fs::remove_file(
        request.adapter["AdapterPath"]
            .as_str()
            .ok_or("Missing synthetic adapter.")?,
    )
    .map_err(|error| error.to_string())?;
    assert!(
        capture_with(&request, &context, |_| {
            invoked.set(true);
            Err("Must not supervise.".into())
        })
        .is_err()
    );
    assert!(!invoked.get());
    let receipt = read_json(&request.evidence_directory.join("screen-receipt.json"))?;
    assert_eq!(receipt["Status"], "failed");
    assert_eq!(receipt["ModelProcessStarted"], false);
    Ok(())
}
