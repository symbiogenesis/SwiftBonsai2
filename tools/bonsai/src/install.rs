//! Offline runtime staging without publication, probes or model execution.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use bonsai_artifacts::{RuntimeSpec, stage_runtime};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    schema_version: u32,
    runtime: RuntimeSpec,
}

/// Stage a locked runtime through the shared bounded archive policy.
///
/// # Errors
/// Rejects unsupported schema/platform, unsafe or changed artifacts, and failed
/// extraction. A failed staging directory is retained as evidence.
pub fn run(request: Value) -> crate::Result<Value> {
    if !request.is_object() || !request["runtime"].is_object() {
        return Err("Runtime staging requires request and runtime objects.".into());
    }
    let request: Request = serde_json::from_value(request).map_err(|error| error.to_string())?;
    if request.schema_version != 1 {
        return Err("Unsupported runtime staging schema.".into());
    }
    let destination = &request.runtime.runtime_directory;
    let name = destination
        .file_name()
        .ok_or("Runtime directory needs a filename.")?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    let mut staging_name = name.to_os_string();
    staging_name.push(format!(".staging-{}-{stamp}", std::process::id()));
    let staging: PathBuf = destination.with_file_name(staging_name);
    let files = stage_runtime(&request.runtime, &staging).map_err(|error| {
        format!(
            "{error} Incomplete runtime staging, if created, retained at {}",
            staging.display()
        )
    })?;
    let server = files
        .iter()
        .find(|file| {
            file.path()
                .file_name()
                .is_some_and(|name| name.eq_ignore_ascii_case("llama-server.exe"))
        })
        .ok_or("Staged runtime has no server.")?;
    Ok(json!({
        "Status": "staged", "ArchivePath": request.runtime.archive_path,
        "RuntimeDirectory": destination, "StagingDirectory": staging, "ServerPath": server.path(),
        "RuntimeFiles": files.iter().map(|file| json!({
            "Path": file.path(), "Bytes": file.bytes(), "Sha256": file.sha256(),
        })).collect::<Vec<_>>(),
    }))
}
