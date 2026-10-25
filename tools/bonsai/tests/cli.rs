//! Packaged JSON command discovery and compatibility with legacy scoring limits.
#![forbid(unsafe_code)]

use std::fs;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

const HEX: &[u8; 16] = b"0123456789abcdef";

#[test]
fn version_advertises_policy_commands_without_resolving_a_request_file()
-> Result<(), Box<dyn std::error::Error>> {
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let scratch = std::env::temp_dir().join(format!(
        "SwiftBonsai-CommandDiscovery-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir(&scratch)?;
    let output = Command::new(env!("CARGO_BIN_EXE_bonsai-tools"))
        .arg("--version")
        .current_dir(&scratch)
        .output()?;
    fs::remove_dir(scratch)?;
    assert!(output.status.success());
    assert_eq!(output.stderr, Vec::<u8>::new());
    let version: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(version["Name"], "bonsai-tools");
    assert_eq!(version["ProtocolVersion"], 5);
    let commands = version["Commands"]
        .as_array()
        .ok_or("Missing command capabilities")?;
    for name in ["server-policy", "prism-baseline"] {
        assert!(commands.iter().any(|command| command == name));
    }
    Ok(())
}

#[test]
fn prism_baseline_cli_matches_frozen_powershell_reports() -> Result<(), Box<dyn std::error::Error>>
{
    let golden: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/prism-baseline-v1.json"
    ))?;
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let scratch = std::env::temp_dir().join(format!(
        "SwiftBonsai-BaselineCommand-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir(&scratch)?;
    let request_path = scratch.join("request.json");
    for (request, expected) in [
        (
            json!({"Operation":"catalog", "Catalog":golden["Catalog"]}),
            &golden["Catalog"],
        ),
        (
            json!({"Operation":"report", "Catalog":golden["Catalog"],
            "TargetResults":golden["TargetResults"], "ReferenceResults":golden["ReferenceResults"]}),
            &golden["Report"],
        ),
        (
            json!({"Operation":"baseline", "Catalog":golden["Catalog"],
            "Report":golden["Report"], "Baseline":golden["Baseline"]}),
            &golden["Gate"],
        ),
    ] {
        fs::write(&request_path, serde_json::to_vec(&request)?)?;
        let output = Command::new(env!("CARGO_BIN_EXE_bonsai-tools"))
            .args(["prism-baseline", "--request"])
            .arg(&request_path)
            .current_dir(&scratch)
            .output()?;
        assert!(
            output.status.success(),
            "{}. Evidence: {}",
            String::from_utf8_lossy(&output.stderr),
            scratch.display()
        );
        assert_eq!(output.stderr, Vec::<u8>::new());
        assert_eq!(&serde_json::from_slice::<Value>(&output.stdout)?, expected);
    }
    fs::remove_file(request_path)?;
    fs::remove_dir(scratch)?;
    Ok(())
}

#[test]
fn large_legacy_in_memory_evidence_remains_accepted()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let golden: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/lora-qualification-v1.json"
    ))?;
    let mut evidence = golden["scenarios"][0]["evidence"].clone();
    let mut review = golden["scenarios"][0]["review"].clone();
    let text = "x".repeat(750_000);
    let digest: String = Sha256::digest(text.as_bytes())
        .iter()
        .flat_map(|byte| {
            [
                char::from(HEX[usize::from(byte >> 4)]),
                char::from(HEX[usize::from(byte & 15)]),
            ]
        })
        .collect();
    for sample in evidence["samples"]
        .as_array_mut()
        .ok_or("Missing samples")?
    {
        sample["responseText"] = text.clone().into();
        sample["responseSha256"] = digest.clone().into();
    }
    for record in review["reviews"].as_array_mut().ok_or("Missing reviews")? {
        record["responseSha256"] = digest.clone().into();
    }
    for check in evidence["zeroScaleChecks"]
        .as_array_mut()
        .ok_or("Missing zero-scale checks")?
    {
        check["baselineResponseSha256"] = digest.clone().into();
        check["zeroScaleResponseSha256"] = digest.clone().into();
    }
    let bytes = serde_json::to_vec(&json!({
        "operation": "score", "evidence": evidence, "review": review
    }))?;
    assert!(bytes.len() > 16 * 1024 * 1024);
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let scratch = std::env::temp_dir().join(format!(
        "SwiftBonsai-LargeScore-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir(&scratch)?;
    let request = scratch.join("request.json");
    fs::write(&request, bytes)?;
    let output = Command::new(env!("CARGO_BIN_EXE_bonsai-tools"))
        .args(["qualification", "--request"])
        .arg(&request)
        .current_dir(&scratch)
        .output()?;
    assert!(
        output.status.success(),
        "Packaged score failed: {}. Evidence: {}",
        String::from_utf8_lossy(&output.stderr),
        scratch.display()
    );
    let result: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(result["Status"], "accepted");
    fs::remove_file(request)?;
    fs::remove_dir(scratch)?;
    Ok(())
}
