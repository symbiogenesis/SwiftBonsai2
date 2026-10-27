//! Synthetic evidence assembly; these tests never start a native process.

use std::cell::Cell;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use super::{Context, Request, capture_with, digest, read_json, save};
use crate::Result;

const TEXT: &str = "\u{feff}caf\u{e9} \u{1f331} [end of text]\r\n\n";

fn fixture() -> Result<(Request, Context)> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "SwiftBonsai-CaptureFixture-{}-{stamp}",
        std::process::id()
    ));
    let data = root.join("data");
    let evidence = root.join("evidence");
    fs::create_dir_all(&data).map_err(|error| error.to_string())?;
    fs::create_dir(&evidence).map_err(|error| error.to_string())?;
    let plan = bonsai_cpu_policy::run(json!({"operation": "plan", "dataDirectory": data,
        "profile": "swift-pq2-assessment", "useAbliterateLora": true, "useRepackedPtq1": false}))?;
    let policy = &plan["Policy"];
    let lock = &plan["Lock"];
    let adapter = std::path::Path::new(
        plan["AdapterPath"]
            .as_str()
            .ok_or("Missing adapter fixture path.")?,
    );
    fs::create_dir_all(adapter.parent().ok_or("Missing adapter parent.")?)
        .map_err(|error| error.to_string())?;
    fs::File::create(adapter)
        .and_then(|file| {
            file.set_len(
                plan["AdapterLock"]["adapter"]["bytes"]
                    .as_u64()
                    .unwrap_or(0),
            )
        })
        .map_err(|error| error.to_string())?;
    let request = serde_json::from_value(json!({
        "dataDirectory": data, "weightRepresentation": "PQ2_0", "evidenceDirectory": evidence,
        "processorName": "Qualcomm Snapdragon X Elite", "runId": "synthetic-capture", "startedUtc": "2026-10-26T00:00:00.0000000Z",
        "parentProcessId": std::process::id(), "parentCreationTimeFileTime": 123,
        "lock": lock, "adapter": {"AdapterPath": plan["AdapterPath"], "Sha256": plan["AdapterLock"]["adapter"]["sha256"]},
        "admission": {"Status": "admission-passed", "ModelRevision": lock["model"]["revision"], "RuntimeCommit": lock["runtime"]["commit"],
            "ModelPath": plan["ModelPath"], "ArtifactSha256": plan["Artifact"]["Sha256"], "EstimatedWeightBytes": plan["Artifact"]["Bytes"],
            "RequiredAvailableBytes": policy["RequiredAvailableBytes"], "ServiceCeilingBytes": policy["ServiceCeilingBytes"],
            "AvailableMemoryFloorBytes": policy["AvailableMemoryFloorBytes"], "AvailablePhysicalBytesAtAdmission": 20_u64 * 1024 * 1024 * 1024},
        "runtime": {"Commit": lock["runtime"]["commit"], "RuntimeFiles": [], "Runtime": {
            "archivePath": data.join("downloads").join(lock["runtime"]["asset"]["filename"].as_str().ok_or("Missing archive filename.")?),
            "runtimeDirectory": data.join("runtime").join(lock["runtime"]["commit"].as_str().ok_or("Missing runtime commit.")?),
            "archiveBytes": lock["runtime"]["asset"]["bytes"], "archiveSha256": lock["runtime"]["asset"]["sha256"],
        }},
    })).map_err(|error| error.to_string())?;
    Ok((
        request,
        Context {
            run_url: "https://github.com/symbiogenesis/SwiftBonsai2Runner/actions/runs/123".into(),
            runner_name: "synthetic-runner".into(),
            source_revision: Some("a".repeat(40)),
        },
    ))
}

fn completed(request: &Value, count: usize, failed: bool) -> Result<Value> {
    assert_eq!(request["ciOnly"], true);
    assert_eq!(request["memoryLimitBytes"], 12_348_030_976_u64);
    assert_eq!(request["requiredAvailableBytes"], 16_642_998_272_u64);
    assert_eq!(request["physicalReserveBytes"], 4_u64 * 1024 * 1024 * 1024);
    assert_eq!(request["timeoutSeconds"], 600);
    assert_eq!(request["stdoutLimitBytes"], 4 * 1024 * 1024);
    assert_eq!(request["stderrLimitBytes"], 64 * 1024 * 1024);
    assert_eq!(request["parentCreationTimeFileTime"], 123);
    let root = std::path::Path::new(
        request["evidenceDirectory"]
            .as_str()
            .ok_or("Missing evidence root.")?,
    );
    assert_eq!(
        read_json(&root.join("receipt.json"))?["Status"],
        "capturing"
    );
    assert_eq!(
        read_json(&root.join("evidence.json"))?["samples"],
        json!([])
    );
    let samples = request["samples"]
        .as_array()
        .ok_or("Missing synthetic samples.")?;
    assert_eq!(samples.len(), 28);
    for sample in samples.iter().take(count) {
        let lane = std::path::Path::new(
            sample["evidenceDirectory"]
                .as_str()
                .ok_or("Missing sample lane.")?,
        );
        save(
            &lane.join("capture.json"),
            &json!({"SchemaVersion": 1, "Text": TEXT, "Sha256": digest(TEXT),
            "Diagnostics": {"LogitsAvailable": false}}),
            true,
        )?;
        save(
            &lane.join("supervision.json"),
            &json!({"Status": "completed", "Timing": {"DecodeTokens": 64, "DecodeMilliseconds": 1000},
            "PeakJobMemoryBytes": 9_u64 * 1024 * 1024 * 1024, "MinimumAvailablePhysicalBytes": 5_u64 * 1024 * 1024 * 1024}),
            true,
        )?;
    }
    let result = json!({"Status": if failed { "failed" } else { "completed" }, "ModelProcessStarted": count > 0, "RuntimeFiles": []});
    save(&root.join("supervision.json"), &result, true)?;
    Ok(result)
}

#[test]
fn capture_completion_preserves_exact_responses_and_pending_proofs() -> Result<()> {
    let (request, context) = fixture()?;
    let result = capture_with(&request, &context, |native| completed(&native, 28, false))?;
    assert_eq!(result["Status"], "capture-completed");
    assert_eq!(result["QualificationStatus"], "pending");
    assert_eq!(result["ModelProcessStarted"], true);
    let evidence = read_json(&request.evidence_directory.join("evidence.json"))?;
    assert_eq!(evidence["samples"].as_array().map(Vec::len), Some(24));
    assert_eq!(
        evidence["zeroScaleChecks"].as_array().map(Vec::len),
        Some(4)
    );
    for sample in evidence["samples"]
        .as_array()
        .ok_or("Missing saved samples.")?
    {
        assert_eq!(sample["responseText"], TEXT);
        assert_eq!(
            sample["responseSha256"],
            "df83f021218f20b60f48cbb5306a794f327336012fd749567bd1a8094531a0c1"
        );
    }
    assert_eq!(evidence["basis"]["status"], "pending");
    assert!(evidence["nonzeroAdapterCheck"]["logitsChanged"].is_null());
    assert!(evidence["zeroScaleChecks"][0]["zeroScaleLogitsSha256"].is_null());
    let qualification = read_json(&request.evidence_directory.join("qualification.json"))?;
    assert_eq!(qualification["StandardProfileMayEnable"], false);
    assert_eq!(qualification["Gates"]["Quality"], "pending");
    assert_eq!(qualification["Gates"]["AuthoringBasis"], "pending");
    Ok(())
}

#[test]
fn capture_failure_retains_completed_prefix_and_ignores_uncommitted_stdout() -> Result<()> {
    let (request, context) = fixture()?;
    let result = capture_with(&request, &context, |native| {
        completed(&native, 1, true)?;
        let next = std::path::Path::new(
            native["samples"][1]["evidenceDirectory"]
                .as_str()
                .ok_or("Missing unfinished sample lane.")?,
        );
        fs::write(next.join("stdout.txt"), "unfinished response")
            .map_err(|error| error.to_string())?;
        save(
            &next.join("capture.json"),
            &json!({"SchemaVersion": 1, "Text": TEXT, "Sha256": digest(TEXT), "Diagnostics": {}}),
            true,
        )?;
        Err("Deliberate synthetic batch failure.".into())
    });
    assert!(result.is_err());
    let receipt = read_json(&request.evidence_directory.join("receipt.json"))?;
    let evidence = read_json(&request.evidence_directory.join("evidence.json"))?;
    assert_eq!(receipt["Status"], "failed");
    assert_eq!(receipt["ModelProcessStarted"], true);
    assert_eq!(receipt["QualificationStatus"], "pending");
    assert_eq!(evidence["samples"].as_array().map(Vec::len), Some(1));
    assert_eq!(evidence["samples"][0]["responseText"], TEXT);
    assert_eq!(evidence["zeroScaleChecks"], json!([]));
    assert!(
        !request
            .evidence_directory
            .join("qualification.json")
            .exists()
    );
    Ok(())
}

#[test]
fn capture_setup_failure_saves_initial_and_failed_evidence_without_supervision() -> Result<()> {
    let (request, context) = fixture()?;
    fs::remove_file(
        request.adapter["AdapterPath"]
            .as_str()
            .ok_or("Missing adapter fixture.")?,
    )
    .map_err(|error| error.to_string())?;
    let invoked = Cell::new(false);
    assert!(
        capture_with(&request, &context, |_| {
            invoked.set(true);
            Err("Supervisor must not run.".into())
        })
        .is_err()
    );
    assert!(!invoked.get());
    let receipt = read_json(&request.evidence_directory.join("receipt.json"))?;
    assert_eq!(receipt["Status"], "failed");
    assert_eq!(receipt["ModelProcessStarted"], false);
    assert_eq!(
        read_json(&request.evidence_directory.join("evidence.json"))?["samples"],
        json!([])
    );
    Ok(())
}

#[test]
fn capture_rejects_mismatched_runtime_or_admission_before_supervision() -> Result<()> {
    for field in ["admission", "lock", "runtime", "adapter"] {
        let (mut request, context) = fixture()?;
        match field {
            "admission" => request.admission["ArtifactSha256"] = json!("0".repeat(64)),
            "lock" => request.lock["model"]["revision"] = json!("0".repeat(40)),
            "runtime" => request.runtime["Runtime"]["archiveSha256"] = json!("0".repeat(64)),
            _ => request.adapter["Sha256"] = json!("0".repeat(64)),
        }
        let invoked = Cell::new(false);
        assert!(
            capture_with(&request, &context, |_| {
                invoked.set(true);
                Err("Supervisor must not run.".into())
            })
            .is_err()
        );
        assert!(!invoked.get());
        assert!(!request.evidence_directory.join("evidence.json").exists());
    }
    Ok(())
}
