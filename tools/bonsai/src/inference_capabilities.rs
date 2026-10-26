//! Model-free discovery from the exact compiled Prism release, with owned probes.

#[cfg(any(windows, test))]
use bonsai_cpu_policy::hardware::RuntimeCapabilities;
use serde_json::Value;

#[cfg(windows)]
const LOCK: &str = include_str!("../../../config/prism-inference.lock.json");
#[cfg(windows)]
const MAX_REPORT_BYTES: u64 = 16 * 1024;

#[cfg(any(windows, test))]
fn checked_report(text: &str, version: &str) -> crate::Result<Value> {
    let report = RuntimeCapabilities::from_json(text)?;
    if !report.target().is_windows_arm64() {
        return Err("Runtime discovery requires the native Windows ARM64 target.".into());
    }
    let value: Value = serde_json::from_str(text).map_err(|error| error.to_string())?;
    if value["runtime"]["version"].as_str() != Some(version) {
        return Err("Runtime capability version differs from the compiled release lock.".into());
    }
    Ok(value)
}

/// Verify and retain the pinned release files while probing its capabilities.
///
/// This starts only `prism-inference.exe --capabilities`, with no model input,
/// and never selects a profile or asserts qualification. Arbitrary runtime paths
/// and caller-supplied hashes are not accepted.
///
/// # Errors
/// Rejects unsupported platforms, malformed requests, identity drift, caller
/// death, output/deadline limits and incompatible capability reports.
pub fn run(request: Value) -> crate::Result<Value> {
    #[cfg(windows)]
    {
        windows::run(request)
    }
    #[cfg(not(windows))]
    {
        let _ = request;
        Err("Released-runtime discovery requires Windows.".into())
    }
}

#[cfg(windows)]
mod windows {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    use bonsai_artifacts::{ArtifactError, VerifiedFile, validate_external_path};
    use bonsai_windows_process::{ParentProcess, ProcessSpec};
    use serde::Deserialize;
    use serde_json::{Value, json};

    use crate::Result;

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Request {
        schema_version: u32,
        data_directory: PathBuf,
        parent_process_id: u32,
        parent_creation_time_file_time: u64,
    }

    // The reviewed pin is compiled into this tool; no request can replace it.
    #[derive(Deserialize)]
    struct Release {
        version: String,
        commit: String,
        target: String,
        asset: Asset,
        files: Vec<Identity>,
    }

    #[derive(Deserialize)]
    struct Asset {
        filename: String,
        bytes: u64,
        sha256: String,
    }

    #[derive(Deserialize)]
    struct Identity {
        name: String,
        bytes: u64,
        sha256: String,
    }

    fn probe(executable: &Path, parent: &ParentProcess, scratch: &Path) -> Result<String> {
        let stdout = scratch.join("capabilities.stdout.json");
        let stderr = scratch.join("capabilities.stderr.txt");
        let output = crate::runtime_probe::run(
            &ProcessSpec {
                executable: executable.to_owned(),
                arguments: vec!["--capabilities".into()],
                working_directory: scratch.to_owned(),
                stdout,
                stderr,
                memory_limit_bytes: 512 * 1024 * 1024,
                active_process_limit: 1,
            },
            super::MAX_REPORT_BYTES,
            "Runtime capability",
            || parent.ensure_alive().map_err(|error| error.to_string()),
        )?;
        // Keep diagnostic stderr separate: it must never become part of the JSON.
        Ok(output.stdout)
    }

    pub(super) fn run(value: Value) -> Result<Value> {
        let request: Request = serde_json::from_value(value).map_err(|error| error.to_string())?;
        if request.schema_version != 1 {
            return Err("Unsupported runtime discovery schema.".into());
        }
        validate_external_path(&request.data_directory).map_err(|error| error.to_string())?;
        let release: Release =
            serde_json::from_str(super::LOCK).map_err(|error| error.to_string())?;
        let parent = ParentProcess::open(
            request.parent_process_id,
            request.parent_creation_time_file_time,
        )
        .map_err(|error| error.to_string())?;
        let started = Instant::now();
        let mut checkpoint = || {
            if started.elapsed() >= Duration::from_secs(60) {
                return Err(ArtifactError::Cancelled);
            }
            parent.ensure_alive().map_err(|_| ArtifactError::Cancelled)
        };
        let archive = request
            .data_directory
            .join("downloads")
            .join(&release.asset.filename);
        let mut guards = vec![
            VerifiedFile::verify_with(
                &archive,
                release.asset.bytes,
                &release.asset.sha256,
                &mut checkpoint,
            )
            .map_err(|error| error.to_string())?,
        ];
        let directory = request
            .data_directory
            .join("inference")
            .join(&release.commit)
            .join(&release.target);
        let mut inventory = Vec::new();
        for identity in &release.files {
            let path = directory.join(&identity.name);
            guards.push(
                VerifiedFile::verify_with(&path, identity.bytes, &identity.sha256, &mut checkpoint)
                    .map_err(|error| error.to_string())?,
            );
            inventory
                .push(json!({"Path": path, "Bytes": identity.bytes, "Sha256": identity.sha256}));
        }
        let entries = fs::read_dir(&directory).map_err(|error| error.to_string())?;
        let mut count = 0;
        for entry in entries {
            let entry = entry.map_err(|error| error.to_string())?;
            if !release
                .files
                .iter()
                .any(|file| entry.file_name() == file.name.as_str())
            {
                return Err("Installed inference inventory differs from the compiled lock.".into());
            }
            count += 1;
        }
        if count != release.files.len() {
            return Err("Installed inference inventory differs from the compiled lock.".into());
        }
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_nanos();
        let scratch = std::env::temp_dir().join(format!(
            "SwiftBonsai-InferenceCapabilities-{}-{stamp}",
            std::process::id()
        ));
        validate_external_path(&scratch).map_err(|error| error.to_string())?;
        fs::create_dir(&scratch).map_err(|error| error.to_string())?;
        let executable = directory.join("prism-inference.exe");
        let text = probe(&executable, &parent, &scratch)?;
        let capabilities = super::checked_report(&text, &release.version)?;
        let receipt = json!({
            "Status": "verified", "Commit": release.commit, "Version": release.version,
            "Target": release.target, "Executable": executable, "ArchivePath": archive,
            "ArchiveSha256": release.asset.sha256, "RuntimeFiles": inventory,
            "Capabilities": capabilities, "EvidenceDirectory": scratch,
            "RuntimeVerification": "verified-and-retained-through-exit"
        });
        fs::write(
            scratch.join("verification.json"),
            serde_json::to_vec_pretty(&receipt).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        drop(guards);
        Ok(receipt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_discovery_rejects_version_target_and_wire_drift() -> crate::Result<()> {
        let fixture =
            include_str!("../../../src/cpu-policy/tests/fixtures/runtime-capabilities-v1.json");
        assert!(checked_report(fixture, "0.1.2").is_ok());
        assert!(checked_report(fixture, "0.1.3").is_err());
        let value: Value = serde_json::from_str(fixture).map_err(|error| error.to_string())?;
        for (pointer, replacement) in [
            ("/runtime/target/architecture", Value::from("x86_64")),
            ("/qualification", Value::from("passed")),
            ("/backends/vulkan", Value::from("available")),
            ("/schema_version", Value::from(2)),
        ] {
            let mut changed = value.clone();
            *changed
                .pointer_mut(pointer)
                .ok_or("Missing fixture field")? = replacement;
            assert!(checked_report(&changed.to_string(), "0.1.2").is_err());
        }
        assert!(checked_report(&(" ".repeat(16 * 1024) + fixture), "0.1.2").is_err());
        assert!(checked_report(&(fixture.to_owned() + fixture), "0.1.2").is_err());
        Ok(())
    }

    #[cfg(windows)]
    #[test]
    fn release_discovery_rejects_untrusted_request_overrides_before_launch() {
        let request = serde_json::json!({
            "schemaVersion": 2, "dataDirectory": std::env::temp_dir(),
            "parentProcessId": 0, "parentCreationTimeFileTime": 0,
        });
        assert!(run(request.clone()).is_err());
        for field in ["executable", "arguments", "lockPath", "sha256"] {
            let mut changed = request.clone();
            changed["schemaVersion"] = 1.into();
            changed[field] = "untrusted".into();
            assert!(run(changed).is_err());
        }
    }
}
