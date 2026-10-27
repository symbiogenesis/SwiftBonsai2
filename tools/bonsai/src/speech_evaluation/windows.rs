//! Windows-only TTS session ownership; HTTP requests stay in the evaluator.

use bonsai_artifacts::{ArtifactError, VerifiedFile, validate_external_path};
use bonsai_windows_process::{
    OwnedProcess, ParentProcess, ProcessMemory, ProcessSpec, available_memory_bytes,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    ffi::OsString,
    fs::{self, OpenOptions},
    io::Write as _,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const GIB: u64 = 1024 * 1024 * 1024;
const INTERVAL: Duration = Duration::from_millis(25);
const OUTPUT_LIMIT: u64 = 1024 * 1024;
const PHASE_NAMES: [&str; 3] = ["startup", "bootstrap", "warm"];

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Request {
    data_directory: PathBuf,
    executable: PathBuf,
    executable_bytes: u64,
    executable_sha256: String,
    evidence_directory: PathBuf,
    token_file: PathBuf,
    port: u16,
    threads: usize,
    quantization: String,
    parent_process_id: u32,
    parent_creation_time_file_time: u64,
}

#[derive(Default)]
struct Observation {
    samples: u64,
    maximum_private: usize,
    maximum_working_set: usize,
    minimum_available: Option<u64>,
}

impl Observation {
    fn observe(&mut self, memory: ProcessMemory, available: u64) {
        self.samples += 1;
        self.maximum_private = self.maximum_private.max(memory.private_bytes);
        self.maximum_working_set = self.maximum_working_set.max(memory.working_set_bytes);
        self.minimum_available = Some(self.minimum_available.unwrap_or(available).min(available));
    }

    fn receipt(&self) -> Value {
        json!({"Samples": self.samples, "SampledPeakPrivateBytes": self.maximum_private,
            "SampledPeakWorkingSetBytes": self.maximum_working_set,
            "MinimumAvailablePhysicalBytes": self.minimum_available})
    }
}

fn save(path: &Path, receipt: &Value) -> crate::Result<()> {
    let bytes = serde_json::to_vec_pretty(receipt).map_err(|error| error.to_string())?;
    let pending = path.with_extension("pending");
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&pending)
        .map_err(|error| error.to_string())?;
    file.write_all(&bytes).map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())?;
    drop(file);
    fs::rename(pending, path).map_err(|error| error.to_string())
}

fn validate(request: &Request) -> crate::Result<()> {
    super::validate_context(std::env::consts::ARCH, |key| std::env::var(key).ok())?;
    if request.port == 0
        || !(1..=256).contains(&request.threads)
        || !matches!(request.quantization.as_str(), "Q8_0" | "Q4_K_M")
    {
        return Err("Invalid native TTS evaluation identity or limits.".into());
    }
    for path in [
        &request.data_directory,
        &request.executable,
        &request.evidence_directory,
        &request.token_file,
    ] {
        validate_external_path(path).map_err(|error| error.to_string())?;
    }
    if !request.executable.is_file()
        || !request.token_file.is_file()
        || !request.evidence_directory.is_dir()
    {
        return Err("TTS evaluation inputs and new evidence directory must exist.".into());
    }
    for name in [
        "started.json",
        "memory.json",
        "phase-bootstrap",
        "phase-warm",
        "stop",
        "abort",
        "worker",
    ] {
        if request.evidence_directory.join(name).exists() {
            return Err("TTS evaluation requires a fresh session directory.".into());
        }
    }
    Ok(())
}

fn execute(request: &Request, receipt: &mut Value) -> crate::Result<()> {
    let parent = ParentProcess::open(
        request.parent_process_id,
        request.parent_creation_time_file_time,
    )
    .map_err(|error| error.to_string())?;
    let checkpoint = || parent.ensure_alive().map_err(|_| ArtifactError::Cancelled);
    let _executable = VerifiedFile::verify_with(
        &request.executable,
        request.executable_bytes,
        &request.executable_sha256,
        checkpoint,
    )
    .map_err(|error| error.to_string())?;
    let models = crate::speech::run(json!({"dataDirectory": request.data_directory,
        "model": "QwenTts", "ttsQuantization": request.quantization}))?;
    let plans = models["Plans"]
        .as_array()
        .ok_or("Missing compiled speech plan.")?;
    receipt["Artifacts"] = models["Plans"].clone();
    receipt["Executable"] = json!({"Path": request.executable, "Bytes": request.executable_bytes,
        "Sha256": request.executable_sha256});
    let _guards = plans
        .iter()
        .map(|plan| {
            VerifiedFile::verify_with(
                Path::new(plan["Path"].as_str().ok_or("Missing artifact path.")?),
                plan["Bytes"].as_u64().ok_or("Missing artifact size.")?,
                plan["Sha256"].as_str().ok_or("Missing artifact hash.")?,
                checkpoint,
            )
            .map_err(|error| error.to_string())
        })
        .collect::<crate::Result<Vec<_>>>()?;
    let available = available_memory_bytes().map_err(|error| error.to_string())?;
    receipt["AdmissionAvailablePhysicalBytes"] = json!(available);
    if available < 9 * GIB {
        return Err(
            "TTS evaluation did not meet the unchanged 9 GiB available-memory admission.".into(),
        );
    }
    parent.ensure_alive().map_err(|error| error.to_string())?;
    let working = request.evidence_directory.join("worker");
    fs::create_dir(&working).map_err(|error| error.to_string())?;
    let mut arguments: Vec<OsString> = Vec::new();
    for (flag, value) in [
        ("--model", &models["TtsModelPath"]),
        ("--codec", &models["TtsCodecPath"]),
        ("--voice-model", &models["TtsVoiceModelPath"]),
    ] {
        arguments.extend([
            flag.into(),
            value.as_str().ok_or("Missing speech model path.")?.into(),
        ]);
    }
    arguments.extend([
        OsString::from("--token-file"),
        request.token_file.clone().into_os_string(),
        "--port".into(),
        request.port.to_string().into(),
        "--threads".into(),
        request.threads.to_string().into(),
        "--quantization".into(),
        request.quantization.clone().into(),
    ]);
    let spec = ProcessSpec {
        executable: request.executable.clone(),
        arguments,
        working_directory: working,
        stdout: request.evidence_directory.join("service.stdout"),
        stderr: request.evidence_directory.join("service.stderr"),
        memory_limit_bytes: usize::try_from(16 * GIB).map_err(|error| error.to_string())?,
        active_process_limit: 1,
    };
    let process = OwnedProcess::start(&spec).map_err(|error| error.to_string())?;
    receipt["ModelProcessStarted"] = json!(true);
    let start = Instant::now();
    let result = capture(request, &spec, &process, &parent, start, receipt);
    // Query the Job before destroying it, including failed or aborted sessions.
    let job_peak = process
        .peak_job_memory_bytes()
        .map_err(|error| error.to_string());
    if let Ok(peak) = &job_peak {
        receipt["PeakJobMemoryBytes"] = json!(peak);
    }
    receipt["WallMilliseconds"] = json!(start.elapsed().as_millis());
    let exited_before_stop = process.try_wait().map_err(|error| error.to_string());
    if let Ok(Some(code)) = &exited_before_stop {
        record_exit(receipt, *code, true);
    }
    let stopped = process
        .stop(Duration::from_secs(10))
        .map_err(|error| error.to_string());
    if receipt["ChildExit"].is_null()
        && let Ok(Some(code)) = process.try_wait()
    {
        record_exit(receipt, code, false);
    }
    capture_verdict(result, exited_before_stop)?;
    job_peak?;
    stopped
}

fn capture_verdict(
    captured: crate::Result<()>,
    exited_before_stop: crate::Result<Option<u32>>,
) -> crate::Result<()> {
    captured?;
    if let Some(code) = exited_before_stop? {
        return Err(format!(
            "TTS process exited before supervisor shutdown: {code} (0x{code:08X})."
        ));
    }
    Ok(())
}

fn record_exit(receipt: &mut Value, code: u32, before_supervisor_stop: bool) {
    receipt["ChildExit"] = json!({"Code": code, "Hex": format!("0x{code:08X}"),
        "ObservedBeforeSupervisorStop": before_supervisor_stop});
}

fn select_phase(current: usize, bootstrap: bool, warm: bool) -> crate::Result<usize> {
    if warm {
        if !bootstrap || current == 0 {
            return Err("Warm TTS phase preceded observed bootstrap.".into());
        }
        Ok(2)
    } else if bootstrap {
        if current == 2 {
            return Err("TTS phase markers moved backwards.".into());
        }
        Ok(1)
    } else if current == 0 {
        Ok(0)
    } else {
        Err("TTS phase markers moved backwards.".into())
    }
}

fn require_complete(phase: usize, phases: &[Observation; 3]) -> crate::Result<()> {
    if phase != 2 || phases.iter().any(|phase| phase.samples == 0) {
        Err("TTS evaluation did not observe all declared phases.".into())
    } else {
        Ok(())
    }
}

fn capture(
    request: &Request,
    spec: &ProcessSpec,
    process: &OwnedProcess,
    parent: &ParentProcess,
    start: Instant,
    receipt: &mut Value,
) -> crate::Result<()> {
    let mut phases: [Observation; 3] = std::array::from_fn(|_| Observation::default());
    let mut phase = 0;
    save(
        &request.evidence_directory.join("started.json"),
        &json!({"ProcessId": process.id()}),
    )?;
    let result = (|| {
        loop {
            parent.ensure_alive().map_err(|error| error.to_string())?;
            phase = select_phase(
                phase,
                request.evidence_directory.join("phase-bootstrap").exists(),
                request.evidence_directory.join("phase-warm").exists(),
            )?;
            receipt["CurrentPhase"] = json!(PHASE_NAMES[phase]);
            if let Some(code) = process.try_wait().map_err(|error| error.to_string())? {
                record_exit(receipt, code, true);
                return Err(format!(
                    "TTS process exited during {} before the completed evaluation: {code} (0x{code:08X}).",
                    PHASE_NAMES[phase]
                ));
            }
            if start.elapsed() > Duration::from_secs(600) {
                return Err("TTS evaluation exceeded its bounded session deadline.".into());
            }
            let memory = process.memory_usage().map_err(|error| error.to_string())?;
            let available = available_memory_bytes().map_err(|error| error.to_string())?;
            phases[phase].observe(memory, available);
            receipt["PeakWorkingSetBytes"] = json!(memory.peak_working_set_bytes);
            if memory.private_bytes > spec.memory_limit_bytes
                || memory.working_set_bytes > spec.memory_limit_bytes
            {
                return Err("TTS evaluation crossed its unchanged process memory ceiling.".into());
            }
            if available < 5 * GIB {
                return Err("TTS evaluation crossed its unchanged 5 GiB physical reserve.".into());
            }
            for output in [&spec.stdout, &spec.stderr] {
                if fs::metadata(output)
                    .map_err(|error| error.to_string())?
                    .len()
                    > OUTPUT_LIMIT
                {
                    return Err("TTS evaluation exceeded bounded diagnostic output.".into());
                }
            }
            if request.evidence_directory.join("abort").exists() {
                return Err("The caller aborted its TTS evaluation.".into());
            }
            if request.evidence_directory.join("stop").exists() {
                return require_complete(phase, &phases);
            }
            let _exit = process
                .wait_for_exit(INTERVAL)
                .map_err(|error| error.to_string())?;
        }
    })();
    receipt["Phases"] = json!({"startup": phases[0].receipt(), "bootstrap": phases[1].receipt(), "warm": phases[2].receipt()});
    if result.is_err() {
        receipt["FailureObservation"] = json!({"Phase": PHASE_NAMES[phase],
            "SamplesInPhase": phases[phase].samples, "ElapsedMilliseconds": start.elapsed().as_millis()});
    }
    result
}

pub(super) fn run(request: &Request) -> crate::Result<Value> {
    validate(request)?;
    let mut receipt = json!({"SchemaVersion": 1, "Status": "running", "ModelProcessStarted": false,
        "Quantization": request.quantization, "Threads": request.threads,
        "CurrentPhase": "startup", "ChildExit": null, "FailureObservation": null,
        "SampleIntervalMilliseconds": 25, "Limits": {"RequiredAvailableBytes": 9 * GIB,
            "PhysicalReserveBytes": 5 * GIB, "CommitLimitBytes": 16 * GIB},
        "Containment": "One-process, kill-on-close Windows Job and headless mitigations; diagnostic evaluation does not establish daily-service isolation."});
    let result = execute(request, &mut receipt);
    receipt["Status"] = json!(if result.is_ok() {
        "completed"
    } else {
        "failed"
    });
    if let Err(error) = &result {
        receipt["Failure"] = json!(error);
        if receipt["FailureObservation"].is_null() {
            let phase = receipt["CurrentPhase"]
                .as_str()
                .ok_or("Missing TTS failure phase.")?;
            receipt["FailureObservation"] = json!({"Phase": phase,
                "SamplesInPhase": receipt["Phases"][phase]["Samples"].as_u64().unwrap_or(0),
                "ElapsedMilliseconds": receipt["WallMilliseconds"]});
        }
    }
    save(&request.evidence_directory.join("memory.json"), &receipt)?;
    result.map(|()| receipt)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_peaks_keep_private_and_resident_observations_separate() {
        let mut observation = Observation::default();
        observation.observe(
            ProcessMemory {
                private_bytes: 40,
                working_set_bytes: 90,
                peak_working_set_bytes: 100,
            },
            300,
        );
        observation.observe(
            ProcessMemory {
                private_bytes: 60,
                working_set_bytes: 70,
                peak_working_set_bytes: 100,
            },
            250,
        );
        let result = observation.receipt();
        assert_eq!(result["Samples"], 2);
        assert_eq!(result["SampledPeakPrivateBytes"], 60);
        assert_eq!(result["SampledPeakWorkingSetBytes"], 90);
        assert_eq!(result["MinimumAvailablePhysicalBytes"], 250);
        assert!(Observation::default().receipt()["MinimumAvailablePhysicalBytes"].is_null());
    }

    #[test]
    fn phase_control_rejects_skips_regressions_and_incomplete_stops() -> crate::Result<()> {
        assert_eq!(select_phase(0, false, false)?, 0);
        assert_eq!(select_phase(0, true, false)?, 1);
        assert_eq!(select_phase(1, true, true)?, 2);
        assert!(select_phase(0, false, true).is_err());
        assert!(select_phase(0, true, true).is_err());
        assert!(select_phase(1, false, false).is_err());
        assert!(select_phase(2, true, false).is_err());
        let mut phases = std::array::from_fn(|_| Observation::default());
        assert!(require_complete(2, &phases).is_err());
        for observation in &mut phases {
            observation.samples = 1;
        }
        assert!(require_complete(1, &phases).is_err());
        require_complete(2, &phases)
    }

    #[test]
    fn invalid_session_policy_never_creates_evidence_or_launches_a_process() {
        let root = std::env::temp_dir().join(format!("tts-invalid-session-{}", std::process::id()));
        let request = Request {
            data_directory: root.clone(),
            executable: root.join("absent.exe"),
            executable_bytes: 1,
            executable_sha256: "0".repeat(64),
            evidence_directory: root.clone(),
            token_file: root.join("absent.key"),
            port: 0,
            threads: 0,
            quantization: "Q2".into(),
            parent_process_id: 0,
            parent_creation_time_file_time: 0,
        };
        assert!(run(&request).is_err());
        assert!(!root.exists());
    }

    #[test]
    fn exit_receipt_preserves_unsigned_windows_status_and_stop_origin() {
        let mut receipt = json!({});
        record_exit(&mut receipt, 70, true);
        assert_eq!(receipt["ChildExit"]["Code"], 70);
        assert_eq!(receipt["ChildExit"]["Hex"], "0x00000046");
        assert_eq!(receipt["ChildExit"]["ObservedBeforeSupervisorStop"], true);
        record_exit(&mut receipt, 0xC000_0005, false);
        assert_eq!(receipt["ChildExit"]["Code"], 3_221_225_477_u32);
        assert_eq!(receipt["ChildExit"]["Hex"], "0xC0000005");
        assert_eq!(receipt["ChildExit"]["ObservedBeforeSupervisorStop"], false);
    }

    #[test]
    fn completed_capture_rejects_every_spontaneous_pre_stop_exit() {
        for (code, hex) in [
            (0, "0x00000000"),
            (70, "0x00000046"),
            (0xC000_0005, "0xC0000005"),
        ] {
            let mut receipt = json!({});
            record_exit(&mut receipt, code, true);
            assert!(capture_verdict(Ok(()), Ok(Some(code))).is_err());
            assert_eq!(receipt["ChildExit"]["Code"], code);
            assert_eq!(receipt["ChildExit"]["Hex"], hex);
            assert_eq!(receipt["ChildExit"]["ObservedBeforeSupervisorStop"], true);
            assert_eq!(
                capture_verdict(Err("original capture failure".into()), Ok(Some(code))),
                Err("original capture failure".into())
            );
        }
        assert!(capture_verdict(Ok(()), Ok(None)).is_ok());
        assert_eq!(
            capture_verdict(Ok(()), Err("exit observation failed".into())),
            Err("exit observation failed".into())
        );
    }
}
