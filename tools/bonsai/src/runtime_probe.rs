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

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use std::fs;

    use bonsai_windows_process::ProcessSpec;

    use super::run;
    use crate::Result;

    #[test]
    fn probes_keep_streams_separate_and_reject_failure_limits_and_cancellation() -> Result<()> {
        let scratch =
            std::env::temp_dir().join(format!("Bonsai-Probe-{}", crate::startup_process::unique()));
        fs::create_dir(&scratch).map_err(|error| error.to_string())?;
        let executable = std::path::PathBuf::from(
            std::env::var_os("SystemRoot").ok_or("Missing Windows directory.")?,
        )
        .join("System32")
        .join("WindowsPowerShell")
        .join("v1.0")
        .join("powershell.exe");
        let mut spec = ProcessSpec {
            executable,
            arguments: vec![
                OsString::from("-NoLogo"),
                OsString::from("-NoProfile"),
                OsString::from("-NonInteractive"),
                OsString::from("-Command"),
                OsString::from(
                    "[Console]::Out.Write('output'); [Console]::Error.Write('diagnostic'); exit 0",
                ),
            ],
            working_directory: scratch.clone(),
            stdout: scratch.join("stdout.txt"),
            stderr: scratch.join("stderr.txt"),
            memory_limit_bytes: 512 * 1024 * 1024,
            active_process_limit: 1,
        };
        let output = run(&spec, 1024, "Fixture", || Ok(()))?;
        assert_eq!(output.stdout, "output");
        assert_eq!(output.stderr, "diagnostic");
        spec.stdout = scratch.join("limit.stdout");
        spec.stderr = scratch.join("limit.stderr");
        assert_eq!(
            run(&spec, 1, "Fixture", || Ok(())).err().as_deref(),
            Some("Fixture probe exceeded its output bound.")
        );
        spec.stdout = scratch.join("cancel.stdout");
        spec.stderr = scratch.join("cancel.stderr");
        assert_eq!(
            run(&spec, 1024, "Fixture", || Err("cancelled".into()))
                .err()
                .as_deref(),
            Some("cancelled")
        );
        spec.stdout = scratch.join("failed.stdout");
        spec.stderr = scratch.join("failed.stderr");
        spec.arguments[4] = "exit 7".into();
        assert_eq!(
            run(&spec, 1024, "Fixture", || Ok(())).err().as_deref(),
            Some("Fixture probe failed.")
        );
        // All process and redirected-output handles must be released on errors.
        fs::remove_dir_all(scratch).map_err(|error| error.to_string())
    }
}
