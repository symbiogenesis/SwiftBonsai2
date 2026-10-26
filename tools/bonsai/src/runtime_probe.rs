//! Bounded model-free native probes with retained Job ownership through cleanup.
#![expect(
    clippy::redundant_pub_crate,
    reason = "Private maintenance modules expose only a crate-local implementation API."
)]

use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use bonsai_windows_process::{OwnedProcess, ProcessSpec};

use crate::Result;

pub(super) struct Output {
    pub(super) stdout: String,
    pub(super) stderr: String,
}

fn check_output(path: &Path, maximum: u64, label: &str) -> Result<()> {
    if fs::metadata(path).map_err(|error| error.to_string())?.len() > maximum {
        return Err(format!("{label} probe exceeded its output bound."));
    }
    Ok(())
}

fn read_output(path: &Path, maximum: u64, label: &str) -> Result<String> {
    check_output(path, maximum, label)?;
    fs::read_to_string(path).map_err(|error| error.to_string())
}

pub(super) fn run(
    spec: &ProcessSpec,
    maximum: u64,
    label: &str,
    mut checkpoint: impl FnMut() -> Result<()>,
) -> Result<Output> {
    let process = OwnedProcess::start(spec).map_err(|error| error.to_string())?;
    let deadline = Instant::now() + Duration::from_secs(10);
    let result = (|| loop {
        checkpoint()?;
        if Instant::now() >= deadline {
            return Err(format!("{label} probe timed out."));
        }
        for path in [&spec.stdout, &spec.stderr] {
            check_output(path, maximum, label)?;
        }
        if let Some(code) = process
            .wait_for_exit(Duration::from_millis(25))
            .map_err(|error| error.to_string())?
        {
            return if code == 0 {
                Ok(())
            } else {
                Err(format!("{label} probe failed."))
            };
        }
    })();
    // Stop/reap the owned Job even when cancellation, limits or waiting failed.
    let stopped = process
        .stop(Duration::from_secs(10))
        .map_err(|error| error.to_string());
    result.and(stopped)?;
    Ok(Output {
        stdout: read_output(&spec.stdout, maximum, label)?,
        stderr: read_output(&spec.stderr, maximum, label)?,
    })
}
