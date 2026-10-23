//! Windows implementation using the separately audited native process boundary.

use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{Read as _, Write as _};
use std::os::windows::fs::OpenOptionsExt as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use bonsai_windows_credentials::validate_external_path;
use bonsai_windows_process::{OwnedProcess, ParentProcess, ProcessSpec, available_memory_bytes};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

use crate::Result;

const INTERVAL: Duration = Duration::from_secs(1);
const STOP_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Artifact {
    path: PathBuf,
    bytes: u64,
    sha256: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Sample {
    id: String,
    arguments: Vec<String>,
    evidence_directory: PathBuf,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    schema_version: u32,
    executable: PathBuf,
    arguments: Vec<String>,
    working_directory: PathBuf,
    evidence_directory: PathBuf,
    artifacts: Vec<Artifact>,
    samples: Option<Vec<Sample>>,
    memory_limit_bytes: usize,
    required_available_bytes: u64,
    physical_reserve_bytes: u64,
    timeout_seconds: u64,
    stdout_limit_bytes: Option<u64>,
    stderr_limit_bytes: Option<u64>,
    require_nonempty_stdout: bool,
    ci_only: bool,
    parent_process_id: u32,
    parent_creation_time_file_time: u64,
}

impl Request {
    fn validate(&self) -> Result<()> {
        if self.schema_version != 1
            || self.memory_limit_bytes == 0
            || !(1..=3600).contains(&self.timeout_seconds)
            || self.physical_reserve_bytes > self.required_available_bytes
            || self.artifacts.len() > 256
            || self.arguments.len() > 4096
        {
            return Err("Invalid completion schema, limits or input count.".into());
        }
        for path in [
            &self.executable,
            &self.working_directory,
            &self.evidence_directory,
        ] {
            validate_external_path(path).map_err(|error| error.to_string())?;
        }
        if !self.executable.is_file()
            || !self.working_directory.is_dir()
            || !self.evidence_directory.is_dir()
        {
            return Err("Executable and external working/evidence directories must exist.".into());
        }
        if self.ci_only {
            validate_ci()?;
        }
        if let Some(samples) = &self.samples {
            if samples.is_empty() || samples.len() > 128 || !self.arguments.is_empty() {
                return Err(
                    "Batch completion requires 1..128 samples and no shared arguments.".into(),
                );
            }
            for sample in samples {
                validate_external_path(&sample.evidence_directory)
                    .map_err(|error| error.to_string())?;
                if !sample.evidence_directory.is_dir()
                    || sample.arguments.len() > 4096
                    || sample.id.is_empty()
                    || sample.id.len() > 128
                    || !sample
                        .id
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte))
                    || sample.evidence_directory == self.evidence_directory
                {
                    return Err(
                        "Invalid batch sample identity, arguments or evidence directory.".into(),
                    );
                }
            }
        }
        Ok(())
    }
}

fn validate_ci() -> Result<()> {
    for (name, value) in [
        ("GITHUB_ACTIONS", "true"),
        ("GITHUB_REF", "refs/heads/main"),
        ("GITHUB_REPOSITORY", "symbiogenesis/SwiftBonsai2"),
        ("RUNNER_ARCH", "ARM64"),
    ] {
        if std::env::var(name).as_deref() != Ok(value) {
            return Err(
                "Model evaluations require this repository's main Windows ARM64 CI lane.".into(),
            );
        }
    }
    let run = std::env::var("GITHUB_RUN_ID").unwrap_or_default();
    if std::env::consts::ARCH != "aarch64"
        || run.is_empty()
        || !run.bytes().all(|byte| byte.is_ascii_digit())
        || std::env::var("RUNNER_NAME")
            .unwrap_or_default()
            .trim()
            .is_empty()
    {
        return Err("Model evaluations require an identified native ARM64 CI runner.".into());
    }
    Ok(())
}

fn verify_artifact(artifact: &Artifact, parent: &ParentProcess) -> Result<File> {
    validate_external_path(&artifact.path).map_err(|error| error.to_string())?;
    if artifact.bytes == 0
        || artifact.sha256.len() != 64
        || !artifact.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("Artifact identity requires positive bytes and SHA-256.".into());
    }
    let mut file = OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&artifact.path)
        .map_err(|error| format!("Open retained artifact: {error}"))?;
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    if !metadata.is_file() || metadata.len() != artifact.bytes {
        return Err("Artifact length or file type differs from its reviewed identity.".into());
    }
    let mut hash = Sha256::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    let mut remaining = artifact.bytes;
    let mut checked = Instant::now();
    while remaining != 0 {
        if checked.elapsed() >= INTERVAL {
            parent.ensure_alive().map_err(|error| error.to_string())?;
            checked = Instant::now();
        }
        let count = usize::try_from(remaining.min(1_048_576)).map_err(|error| error.to_string())?;
        file.read_exact(&mut buffer[..count])
            .map_err(|error| error.to_string())?;
        hash.update(&buffer[..count]);
        remaining -= u64::try_from(count).map_err(|error| error.to_string())?;
    }
    let digest: String = hash
        .finalize()
        .into_iter()
        .flat_map(|byte| [byte >> 4, byte & 15])
        .map(|part| char::from(b"0123456789abcdef"[usize::from(part)]))
        .collect();
    if !digest.eq_ignore_ascii_case(&artifact.sha256) {
        return Err("Artifact SHA-256 differs from its reviewed identity.".into());
    }
    parent.ensure_alive().map_err(|error| error.to_string())?;
    Ok(file)
}

fn check_output(path: &Path, limit: Option<u64>) -> Result<()> {
    if let Some(limit) = limit
        && fs::metadata(path).map_err(|error| error.to_string())?.len() > limit
    {
        return Err("Native operation exceeded its diagnostic output limit.".into());
    }
    Ok(())
}

fn save(path: &Path, receipt: &Value) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(receipt).map_err(|error| error.to_string())?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| format!("Create supervision receipt: {error}"))?;
    file.write_all(&bytes).map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())
}

fn execute(request: &Request, receipt: &mut Value) -> Result<()> {
    let parent = ParentProcess::open(
        request.parent_process_id,
        request.parent_creation_time_file_time,
    )
    .map_err(|error| format!("Open calling process: {error}"))?;
    let _guards: Vec<File> = request
        .artifacts
        .iter()
        .map(|artifact| verify_artifact(artifact, &parent))
        .collect::<Result<_>>()?;
    if let Some(samples) = &request.samples {
        receipt["Samples"] = json!([]);
        for sample in samples {
            let mut captured = json!({"Id": sample.id, "Status": "running", "ModelProcessStarted": false,
                "EvidenceDirectory": sample.evidence_directory});
            let result = execute_one(
                request,
                &sample.arguments,
                &sample.evidence_directory,
                &parent,
                &mut captured,
            );
            if captured["ModelProcessStarted"] == true {
                receipt["ModelProcessStarted"] = json!(true);
            }
            match &result {
                Ok(()) => captured["Status"] = json!("completed"),
                Err(error) => {
                    captured["Status"] = json!("failed");
                    captured["Failure"] = json!(error);
                }
            }
            save(
                &sample.evidence_directory.join("supervision.json"),
                &captured,
            )?;
            receipt["Samples"]
                .as_array_mut()
                .ok_or("Missing batch receipt array.")?
                .push(captured);
            result?;
        }
        return Ok(());
    }
    execute_one(
        request,
        &request.arguments,
        &request.evidence_directory,
        &parent,
        receipt,
    )
}

fn execute_one(
    request: &Request,
    arguments: &[String],
    evidence_directory: &Path,
    parent: &ParentProcess,
    receipt: &mut Value,
) -> Result<()> {
    let available = available_memory_bytes().map_err(|error| error.to_string())?;
    receipt["MinimumAvailablePhysicalBytes"] = json!(available);
    if available < request.required_available_bytes {
        return Err("Available physical memory changed before native operation admission.".into());
    }
    parent.ensure_alive().map_err(|error| error.to_string())?;
    let spec = ProcessSpec {
        executable: request.executable.clone(),
        arguments: arguments.iter().map(OsString::from).collect(),
        working_directory: request.working_directory.clone(),
        stdout: evidence_directory.join("stdout.txt"),
        stderr: evidence_directory.join("stderr.txt"),
        memory_limit_bytes: request.memory_limit_bytes,
        active_process_limit: 1,
    };
    let start = Instant::now();
    let process =
        OwnedProcess::start(&spec).map_err(|error| format!("Start owned process: {error}"))?;
    receipt["ModelProcessStarted"] = json!(true);
    let result = supervise(request, &spec, &process, parent, start, receipt);
    if let Ok(peak) = process.peak_job_memory_bytes() {
        receipt["PeakJobMemoryBytes"] = json!(peak);
    }
    receipt["WallMilliseconds"] = json!(start.elapsed().as_millis());
    let stopped = process
        .stop(STOP_TIMEOUT)
        .map_err(|error| format!("Stop owned process: {error}"));
    if let Err(error) = stopped {
        receipt["CancellationFailure"] = json!(error);
        return result.and(Err(error));
    }
    result
}

fn supervise(
    request: &Request,
    spec: &ProcessSpec,
    process: &OwnedProcess,
    parent: &ParentProcess,
    start: Instant,
    receipt: &mut Value,
) -> Result<()> {
    let mut minimum = receipt["MinimumAvailablePhysicalBytes"]
        .as_u64()
        .ok_or("Missing admission memory observation.")?;
    let exit = loop {
        parent.ensure_alive().map_err(|error| error.to_string())?;
        if let Some(exit) = process
            .wait_for_exit(INTERVAL)
            .map_err(|error| error.to_string())?
        {
            break exit;
        }
        if start.elapsed() >= Duration::from_secs(request.timeout_seconds) {
            return Err("Native operation exceeded its safety timeout.".into());
        }
        check_output(&spec.stdout, request.stdout_limit_bytes)?;
        check_output(&spec.stderr, request.stderr_limit_bytes)?;
        observe_memory(request, process, &mut minimum, receipt)?;
    };
    receipt["ExitCode"] = json!(exit);
    receipt["WallMilliseconds"] = json!(start.elapsed().as_millis());
    parent.ensure_alive().map_err(|error| error.to_string())?;
    check_output(&spec.stdout, request.stdout_limit_bytes)?;
    check_output(&spec.stderr, request.stderr_limit_bytes)?;
    observe_memory(request, process, &mut minimum, receipt)?;
    if exit != 0 {
        return Err(format!("Native operation failed with exit code {exit}."));
    }
    if request.ci_only {
        let text = read_capture(&spec.stdout)?;
        if text.trim().is_empty() {
            return Err("Paired native sample returned no captured response.".into());
        }
        let parsed = super::timing::parse(&read_capture(&spec.stderr)?)?;
        receipt["Timing"] = parsed["Timing"].clone();
        receipt["Counts"] = parsed["Counts"].clone();
    } else if request.require_nonempty_stdout {
        let mut file = OpenOptions::new()
            .read(true)
            .share_mode(3)
            .open(&spec.stdout)
            .map_err(|error| error.to_string())?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        let text = String::from_utf8_lossy(&bytes);
        if text.trim_start_matches('\u{feff}').trim().is_empty() {
            return Err("Native operation returned no captured response.".into());
        }
    }
    Ok(())
}

fn read_capture(path: &Path) -> Result<String> {
    let mut file = OpenOptions::new()
        .read(true)
        .share_mode(3)
        .open(path)
        .map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    String::from_utf8(bytes)
        .map_err(|error| format!("Native captured output is not valid UTF-8: {error}"))
}

fn observe_memory(
    request: &Request,
    process: &OwnedProcess,
    minimum: &mut u64,
    receipt: &mut Value,
) -> Result<()> {
    *minimum = (*minimum).min(available_memory_bytes().map_err(|error| error.to_string())?);
    receipt["MinimumAvailablePhysicalBytes"] = json!(*minimum);
    if *minimum < request.physical_reserve_bytes {
        return Err("Native operation crossed its physical memory reserve.".into());
    }
    // A root can exit between the preceding wait and this observation.
    match process.memory_usage() {
        Ok(memory)
            if memory.private_bytes > request.memory_limit_bytes
                || memory.working_set_bytes > request.memory_limit_bytes =>
        {
            Err("Native operation crossed its current process memory ceiling.".into())
        }
        Ok(_) => Ok(()),
        Err(_)
            if process
                .try_wait()
                .map_err(|error| error.to_string())?
                .is_some() =>
        {
            Ok(())
        }
        Err(error) => Err(format!("Observe process memory: {error}")),
    }
}

pub(super) fn run(value: Value) -> Result<Value> {
    let request: Request = serde_json::from_value(value)
        .map_err(|error| format!("Invalid completion request: {error}"))?;
    request.validate()?;
    let path = request.evidence_directory.join("supervision.json");
    if path.exists() {
        return Err("Supervision evidence must be new; refusing to replace a receipt.".into());
    }
    let mut receipt = json!({
        "SchemaVersion": 1, "Status": "running", "ModelProcessStarted": false,
        "EvidenceDirectory": request.evidence_directory,
    });
    match execute(&request, &mut receipt) {
        Ok(()) => {
            receipt["Status"] = json!("completed");
            save(&path, &receipt)?;
            Ok(receipt)
        }
        Err(error) => {
            receipt["Status"] = json!("failed");
            receipt["Failure"] = json!(error);
            if let Err(evidence_error) = save(&path, &receipt) {
                return Err(format!("{error}; {evidence_error}"));
            }
            Err(format!(
                "{error} Evidence: {}",
                request.evidence_directory.display()
            ))
        }
    }
}
