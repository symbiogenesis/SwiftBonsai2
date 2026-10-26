//! Native model-free probe regressions under the production headless policy.
#![forbid(unsafe_code)]
#![cfg(all(windows, feature = "test-probe"))]

use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use bonsai_windows_process::ProcessSpec;

// Exercise the private implementation without exposing a production test API.
#[path = "../src/runtime_probe.rs"]
mod runtime_probe;

type Result<T> = std::result::Result<T, String>;

#[test]
fn probes_keep_streams_separate_and_reject_failure_limits_and_cancellation() -> Result<()> {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    let scratch = std::env::temp_dir().join(format!("Bonsai-Probe-{}-{nonce}", std::process::id()));
    fs::create_dir(&scratch).map_err(|error| error.to_string())?;
    let mut spec = ProcessSpec {
        executable: env!("CARGO_BIN_EXE_bonsai-completion-test-probe").into(),
        arguments: vec!["streams".into()],
        working_directory: scratch.clone(),
        stdout: scratch.join("stdout.txt"),
        stderr: scratch.join("stderr.txt"),
        memory_limit_bytes: 512 * 1024 * 1024,
        active_process_limit: 1,
    };
    let output = runtime_probe::run(&spec, 1024, "Fixture", || Ok(()))?;
    assert_eq!(output.stdout, "output");
    assert_eq!(output.stderr, "diagnostic");
    spec.stdout = scratch.join("limit.stdout");
    spec.stderr = scratch.join("limit.stderr");
    assert_eq!(
        runtime_probe::run(&spec, 1, "Fixture", || Ok(()))
            .err()
            .as_deref(),
        Some("Fixture probe exceeded its output bound.")
    );
    spec.stdout = scratch.join("cancel.stdout");
    spec.stderr = scratch.join("cancel.stderr");
    assert_eq!(
        runtime_probe::run(&spec, 1024, "Fixture", || Err("cancelled".into()))
            .err()
            .as_deref(),
        Some("cancelled")
    );
    spec.stdout = scratch.join("failed.stdout");
    spec.stderr = scratch.join("failed.stderr");
    spec.arguments = vec!["partial-failure".into()];
    assert_eq!(
        runtime_probe::run(&spec, 1024, "Fixture", || Ok(()))
            .err()
            .as_deref(),
        Some("Fixture probe failed.")
    );
    // All process and redirected-output handles must be released on errors.
    fs::remove_dir_all(scratch).map_err(|error| error.to_string())
}
