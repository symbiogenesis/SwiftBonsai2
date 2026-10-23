//! Focused synthetic supervision regressions; no model or benchmark is executed.
#![cfg(windows)]

use std::fs;
use std::os::windows::process::CommandExt as _;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn shell() -> Result<PathBuf> {
    Ok(
        PathBuf::from(std::env::var_os("SystemRoot").ok_or("Missing Windows system root.")?)
            .join("System32/WindowsPowerShell/v1.0/powershell.exe"),
    )
}

fn fixture(name: &str) -> Result<(PathBuf, Value)> {
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let lane = std::env::temp_dir().join(format!(
        "Bonsai-Supervision-{name}-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir(&lane)?;
    let identity = Command::new(shell()?)
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &format!(
                "(Get-Process -Id {}).StartTime.ToUniversalTime().ToFileTimeUtc()",
                std::process::id()
            ),
        ])
        .current_dir(&lane)
        .creation_flags(0x0800_0000)
        .stdin(Stdio::null())
        .output()?;
    if !identity.status.success() {
        return Err("Cannot observe test caller identity.".into());
    }
    let creation = String::from_utf8(identity.stdout)?.trim().parse::<u64>()?;
    let request = json!({
        "schemaVersion": 1, "executable": shell()?, "arguments": ["-NoLogo", "-NoProfile", "-NonInteractive", "-Command", "[Console]::Out.Write('bounded-response')"],
        "workingDirectory": lane, "evidenceDirectory": lane, "artifacts": [],
        "memoryLimitBytes": 536_870_912, "requiredAvailableBytes": 0, "physicalReserveBytes": 0,
        "timeoutSeconds": 10, "stdoutLimitBytes": 4096, "stderrLimitBytes": 4096,
        "requireNonemptyStdout": true, "ciOnly": false,
        "parentProcessId": std::process::id(), "parentCreationTimeFileTime": creation,
    });
    Ok((lane, request))
}

#[test]
fn batch_preserves_outputs_and_retains_verified_inputs() -> Result<()> {
    let (lane, mut request) = fixture("batch")?;
    let artifact = lane.join("pinned.txt");
    fs::write(&artifact, b"abc")?;
    let mut samples = Vec::new();
    for id in ["first", "second"] {
        let evidence = lane.join(id);
        fs::create_dir(&evidence)?;
        samples.push(json!({"id": id, "evidenceDirectory": evidence,
            "arguments": ["-NoLogo", "-NoProfile", "-NonInteractive", "-Command",
                "$ErrorActionPreference='Stop'; try { [IO.File]::WriteAllText('pinned.txt','changed'); exit 9 } catch [IO.IOException] { [Console]::Out.Write('retained-input') }"]}));
    }
    request["arguments"] = json!([]);
    request["samples"] = json!(samples);
    request["artifacts"] = json!([{"path": artifact, "bytes": 3,
        "sha256": "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"}]);
    let result = bonsai_tools::completion::run(request)?;
    assert_eq!(result["Status"], "completed");
    assert_eq!(result["Samples"].as_array().map(Vec::len), Some(2));
    assert_eq!(fs::read(&artifact)?, b"abc");
    for id in ["first", "second"] {
        assert_eq!(
            fs::read_to_string(lane.join(id).join("stdout.txt"))?,
            "retained-input"
        );
        let receipt: Value =
            serde_json::from_slice(&fs::read(lane.join(id).join("supervision.json"))?)?;
        assert_eq!(receipt["Status"], "completed");
        assert_eq!(receipt["ExitCode"], 0);
    }
    Ok(())
}

#[test]
fn exit_output_caps_fail_and_preserve_diagnostic_receipt() -> Result<()> {
    let (lane, mut request) = fixture("output-cap")?;
    request["stdoutLimitBytes"] = json!(1);
    let result = bonsai_tools::completion::run(request);
    assert!(result.is_err());
    let receipt: Value = serde_json::from_slice(&fs::read(lane.join("supervision.json"))?)?;
    assert_eq!(receipt["Status"], "failed");
    assert_eq!(receipt["ModelProcessStarted"], true);
    assert!(
        receipt["Failure"]
            .as_str()
            .is_some_and(|message| message.contains("output limit"))
    );
    assert_eq!(
        fs::read_to_string(lane.join("stdout.txt"))?,
        "bounded-response"
    );
    Ok(())
}

#[test]
fn unknown_request_fields_and_changed_artifacts_cannot_start_children() -> Result<()> {
    let (lane, mut request) = fixture("invalid-request")?;
    request["unknown"] = json!(true);
    assert!(bonsai_tools::completion::run(request.clone()).is_err());
    assert!(!lane.join("stdout.txt").exists());
    request
        .as_object_mut()
        .ok_or("Not an object.")?
        .remove("unknown");
    let artifact = lane.join("bad.txt");
    fs::write(&artifact, b"abd")?;
    request["artifacts"] = json!([{"path": artifact, "bytes": 3,
        "sha256": "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"}]);
    assert!(bonsai_tools::completion::run(request).is_err());
    assert!(!lane.join("stdout.txt").exists());
    Ok(())
}
