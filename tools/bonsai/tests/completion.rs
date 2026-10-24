//! Focused synthetic supervision regressions; no model or benchmark is executed.
#![forbid(unsafe_code)]
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
        assert!(!lane.join(id).join("capture.json").exists());
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

#[cfg(target_arch = "aarch64")]
fn synthetic_ci_child(name: &str) -> Result<bool> {
    const CHILD: &str = "BONSAI_SYNTHETIC_CI_FIXTURE";
    if std::env::var_os(CHILD).is_none() {
        // Supply CI identity only to this synthetic subprocess. It executes no
        // model or performance workload, and preserves the inherited OS/profile.
        let directory = std::env::temp_dir();
        let status = Command::new(std::env::current_exe()?)
            .args(["--exact", name, "--nocapture"])
            .env(CHILD, "1")
            .env("GITHUB_ACTIONS", "true")
            .env("GITHUB_REF", "refs/heads/main")
            .env("GITHUB_REPOSITORY", "symbiogenesis/SwiftBonsai2")
            .env("RUNNER_ARCH", "ARM64")
            .env("GITHUB_RUN_ID", "1")
            .env("RUNNER_NAME", "synthetic-completion-fixture")
            .current_dir(directory)
            .creation_flags(0x0800_0000)
            .stdin(Stdio::null())
            .status()?;
        assert!(status.success());
        return Ok(false);
    }
    Ok(true)
}

#[cfg(target_arch = "aarch64")]
#[test]
fn malformed_first_capture_prevents_the_next_batch_sample() -> Result<()> {
    if !synthetic_ci_child("malformed_first_capture_prevents_the_next_batch_sample")? {
        return Ok(());
    }
    for (name, script, expected) in [
        ("empty", "[Console]::Out.Write(' ')", "no captured response"),
        (
            "utf8",
            "[Console]::OpenStandardOutput().Write([byte[]]@(255),0,1)",
            "valid UTF-8",
        ),
        (
            "timing",
            "[Console]::Out.Write('response')",
            "decode timing",
        ),
    ] {
        let (lane, mut request) = fixture(name)?;
        let first = lane.join("first");
        let second = lane.join("second");
        fs::create_dir(&first)?;
        fs::create_dir(&second)?;
        request["arguments"] = json!([]);
        request["ciOnly"] = json!(true);
        request["samples"] = json!([
            {"id": "first", "evidenceDirectory": first,
                "arguments": ["-NoLogo", "-NoProfile", "-NonInteractive", "-Command", script]},
            {"id": "second", "evidenceDirectory": second,
                "arguments": ["-NoLogo", "-NoProfile", "-NonInteractive", "-Command", "[IO.File]::WriteAllText('next-started.txt','unexpected')"]},
        ]);
        let failure = bonsai_tools::completion::run(request)
            .err()
            .ok_or("Malformed capture was accepted.")?;
        assert!(failure.contains(expected), "{failure}");
        assert!(!lane.join("next-started.txt").exists());
        assert!(!second.join("stdout.txt").exists());
        let receipt: Value = serde_json::from_slice(&fs::read(first.join("supervision.json"))?)?;
        assert_eq!(receipt["Status"], "failed");
        assert!(!first.join("capture.json").exists());
    }
    Ok(())
}

#[cfg(target_arch = "aarch64")]
fn exact_capture_script(text: &str) -> String {
    const LOG: &str = "main: number of tokens in prompt = 15\ncommon_perf_print: prompt eval time = 100.5 ms / 15 tokens ( 6.7 ms per token, 149.25 tokens per second)\ncommon_perf_print: eval time = 200.0 ms / 3 runs ( 66.7 ms per token, 15 tokens per second)\ncommon_perf_print: samplers time = 0.50 ms / 19 tokens\n";
    let bytes = text
        .bytes()
        .map(|byte| byte.to_string())
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "$bytes = [byte[]]@({bytes}); [Console]::OpenStandardOutput().Write($bytes,0,$bytes.Length); [Console]::Error.Write('{LOG}')"
    )
}

#[cfg(target_arch = "aarch64")]
#[test]
fn completed_capture_survives_failed_and_missing_batch_samples() -> Result<()> {
    if !synthetic_ci_child("completed_capture_survives_failed_and_missing_batch_samples")? {
        return Ok(());
    }
    let (lane, mut request) = fixture("capture-recovery")?;
    let text = "\u{feff}caf\u{e9} \u{1f331} [end of text]\r\n\n";
    let mut samples = Vec::new();
    for (id, script) in [
        ("first", exact_capture_script(text)),
        (
            "failed",
            "[Console]::Out.Write('partial-response'); exit 7".into(),
        ),
        (
            "missing",
            "[IO.File]::WriteAllText('next-started.txt','unexpected')".into(),
        ),
    ] {
        let evidence = lane.join(id);
        fs::create_dir(&evidence)?;
        samples.push(json!({"id": id, "evidenceDirectory": evidence,
            "arguments": ["-NoLogo", "-NoProfile", "-NonInteractive", "-Command", script]}));
    }
    request["arguments"] = json!([]);
    request["ciOnly"] = json!(true);
    request["samples"] = json!(samples);
    let failure = bonsai_tools::completion::run(request)
        .err()
        .ok_or("Failed batch was accepted.")?;
    assert!(failure.contains("exit code 7"), "{failure}");
    let first = lane.join("first");
    assert_eq!(fs::read(first.join("stdout.txt"))?, text.as_bytes());
    let captured: Value = serde_json::from_slice(&fs::read(first.join("capture.json"))?)?;
    let supervision: Value = serde_json::from_slice(&fs::read(first.join("supervision.json"))?)?;
    let diagnostics: Value = serde_json::from_slice(&fs::read(first.join("diagnostics.json"))?)?;
    assert_eq!(supervision["Status"], "completed");
    assert_eq!(captured["SchemaVersion"], 1);
    assert_eq!(captured["Text"], text);
    assert_eq!(
        captured["Sha256"],
        "df83f021218f20b60f48cbb5306a794f327336012fd749567bd1a8094531a0c1"
    );
    assert_eq!(captured["Diagnostics"], diagnostics);
    assert_eq!(
        diagnostics["WallMilliseconds"],
        supervision["WallMilliseconds"]
    );
    assert_eq!(diagnostics["PromptTokens"], 15);
    assert_eq!(diagnostics["PromptMilliseconds"], 100.5);
    assert_eq!(diagnostics["DecodeTokensPerSecond"], 15.0);
    assert_eq!(diagnostics["EogMarkerObserved"], true);
    assert_eq!(diagnostics["GeneratedSampledTokens"], 4);
    assert_eq!(diagnostics["LogitsAvailable"], false);
    assert!(supervision.get("Text").is_none());
    let failed = lane.join("failed");
    let receipt: Value = serde_json::from_slice(&fs::read(failed.join("supervision.json"))?)?;
    assert_eq!(receipt["Status"], "failed");
    assert_eq!(fs::read(failed.join("stdout.txt"))?, b"partial-response");
    assert!(!failed.join("capture.json").exists());
    let batch: Value = serde_json::from_slice(&fs::read(lane.join("supervision.json"))?)?;
    assert_eq!(batch["Status"], "failed");
    assert_eq!(batch["Samples"].as_array().map(Vec::len), Some(2));
    assert_eq!(batch["Samples"][0]["Status"], "completed");
    assert!(batch["Samples"][0].get("Text").is_none());
    for file in ["supervision.json", "stdout.txt", "capture.json"] {
        assert!(!lane.join("missing").join(file).exists());
    }
    assert!(!lane.join("next-started.txt").exists());
    Ok(())
}

#[cfg(target_arch = "aarch64")]
#[test]
fn capture_write_failure_cannot_publish_a_completed_sample() -> Result<()> {
    if !synthetic_ci_child("capture_write_failure_cannot_publish_a_completed_sample")? {
        return Ok(());
    }
    let (lane, mut request) = fixture("capture-write")?;
    let diagnostics = lane.join("diagnostics.json");
    fs::write(&diagnostics, b"retained-evidence")?;
    request["arguments"] = json!([
        "-NoLogo",
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        exact_capture_script("response\n")
    ]);
    request["ciOnly"] = json!(true);
    assert!(bonsai_tools::completion::run(request).is_err());
    assert!(lane.join("capture.json").exists());
    assert_eq!(fs::read(diagnostics)?, b"retained-evidence");
    let receipt: Value = serde_json::from_slice(&fs::read(lane.join("supervision.json"))?)?;
    assert_eq!(receipt["Status"], "failed");
    Ok(())
}
