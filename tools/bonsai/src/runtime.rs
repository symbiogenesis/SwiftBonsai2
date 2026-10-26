//! Shared runtime verification with guarded, bounded version probes.

/// Verify an installed runtime and its reported build identity without loading a model.
/// The archive, installed files and ancestors remain guarded until the probe exits.
///
/// # Errors
/// Rejects malformed requests, unsupported platforms, changed artifacts, caller
/// death, output/deadline limits, or a reported commit that differs from the lock.
pub fn run(request: serde_json::Value) -> crate::Result<serde_json::Value> {
    #[cfg(windows)]
    {
        windows::run(request)
    }
    #[cfg(not(windows))]
    {
        let _ = request;
        Err("Runtime verification requires Windows.".into())
    }
}

#[cfg(windows)]
pub(crate) fn file_receipt(runtime: &bonsai_artifacts::VerifiedRuntime) -> serde_json::Value {
    serde_json::json!(
        runtime
            .files()
            .iter()
            .map(|file| serde_json::json!({
                "Path": file.path(), "Bytes": file.bytes(), "Sha256": file.sha256(),
            }))
            .collect::<Vec<_>>()
    )
}

#[cfg(windows)]
mod windows {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    use bonsai_artifacts::{ArtifactError, RuntimeSpec, VerifiedRuntime, validate_external_path};
    use bonsai_windows_process::{ParentProcess, ProcessSpec};
    use serde::Deserialize;
    use serde_json::{Value, json};

    use crate::Result;

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Request {
        schema_version: u32,
        runtime: RuntimeSpec,
        commit: String,
        parent_process_id: u32,
        parent_creation_time_file_time: u64,
        #[serde(default)]
        include_help: bool,
    }

    fn matches_commit(text: &str, commit: &str) -> bool {
        text.match_indices("commit ").any(|(start, _)| {
            let boundary = text[..start]
                .chars()
                .next_back()
                .is_none_or(|character| !character.is_ascii_alphanumeric() && character != '_');
            let Some((reported, _)) = text[start + 7..].split_once(')') else {
                return false;
            };
            boundary
                && (7..=40).contains(&reported.len())
                && reported
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                && commit.starts_with(reported)
        })
    }

    fn probe(
        runtime: &VerifiedRuntime,
        parent: &ParentProcess,
        scratch: &Path,
        kind: &str,
    ) -> Result<String> {
        let stdout = scratch.join(format!("{kind}.stdout.txt"));
        let stderr = scratch.join(format!("{kind}.stderr.txt"));
        let output = crate::runtime_probe::run(
            &ProcessSpec {
                executable: runtime.executable().to_owned(),
                arguments: vec![format!("--{kind}").into()],
                working_directory: scratch.to_owned(),
                stdout,
                stderr,
                memory_limit_bytes: 512 * 1024 * 1024,
                active_process_limit: 1,
            },
            1_048_576,
            "Runtime version",
            || parent.ensure_alive().map_err(|error| error.to_string()),
        )?;
        Ok(output.stdout + &output.stderr)
    }

    pub(super) fn run(value: Value) -> Result<Value> {
        if !value.is_object() {
            return Err("Runtime request must be an object.".into());
        }
        let request: Request = serde_json::from_value(value).map_err(|error| error.to_string())?;
        if request.schema_version != 1 || !crate::identity::is_lower_hex(&request.commit, 40) {
            return Err("Runtime verification requires a reviewed schema and full commit.".into());
        }
        let parent = ParentProcess::open(
            request.parent_process_id,
            request.parent_creation_time_file_time,
        )
        .map_err(|error| error.to_string())?;
        let started = Instant::now();
        let runtime = VerifiedRuntime::verify_with(&request.runtime, || {
            if started.elapsed() >= Duration::from_secs(60) {
                return Err(ArtifactError::Cancelled);
            }
            parent.ensure_alive().map_err(|_| ArtifactError::Cancelled)
        })
        .map_err(|error| error.to_string())?;
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_nanos();
        let scratch: PathBuf = std::env::temp_dir().join(format!(
            "SwiftBonsai-RuntimeProbe-{}-{stamp}",
            std::process::id()
        ));
        validate_external_path(&scratch).map_err(|error| error.to_string())?;
        fs::create_dir(&scratch).map_err(|error| error.to_string())?;
        let version = probe(&runtime, &parent, &scratch, "version")?;
        if !matches_commit(&version, &request.commit) {
            return Err("Runtime reported build identity differs from the lock file.".into());
        }
        let help = request
            .include_help
            .then(|| probe(&runtime, &parent, &scratch, "help"))
            .transpose()?;
        Ok(json!({"Status": "verified", "Commit": request.commit,
            "ServerPath": runtime.executable(), "ArchivePath": runtime.archive_path(),
            "ReportedVersion": version.trim(), "Runtime": request.runtime,
            "RuntimeFiles": super::file_receipt(&runtime), "RuntimeHelp": help, "EvidenceDirectory": scratch}))
    }

    #[cfg(test)]
    mod tests {
        use super::matches_commit;

        #[test]
        fn version_identity_requires_a_complete_bounded_commit_marker() {
            let commit = "73bfe6cc485554822f80c473a3c0c4f25e80f41b";
            assert!(matches_commit("version: build (commit 73bfe6c)", commit));
            for invalid in [
                "commit 73bfe6)",
                "notcommit 73bfe6c)",
                "commit 73bfe6d)",
                "commit 73BFE6C)",
                "commit 73bfe6c",
                "commit 73bfe6c extra)",
            ] {
                assert!(!matches_commit(invalid, commit));
            }
        }
    }
}
