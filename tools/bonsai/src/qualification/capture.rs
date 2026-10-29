//! Paired capture orchestration and exact response reconciliation.

use std::fs::{self, OpenOptions};
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};

use bonsai_artifacts::RuntimeSpec;
use bonsai_cpu_policy::hardware::SnapdragonFamily;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::{
    Policy, control_identity, decode, digest, invocation, policy, read_json, read_json_with_limit,
    require, schema::Controls, score, string, validate_path,
};
use crate::Result;

const MIB: u64 = 1024 * 1024;
const CAPTURE_JSON_LIMIT: u64 = 25 * MIB;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    data_directory: PathBuf,
    weight_representation: String,
    admission: Value,
    lock: Value,
    runtime: Value,
    adapter: Value,
    evidence_directory: PathBuf,
    processor_name: String,
    run_id: String,
    started_utc: String,
    parent_process_id: u32,
    parent_creation_time_file_time: u64,
}

struct Context {
    run_url: String,
    runner_name: String,
    source_revision: Option<String>,
}

fn ci_context(processor_name: &str) -> Result<Context> {
    bonsai_cpu_policy::server::validate_environment(
        std::env::vars_os().map(|(name, _)| name.to_string_lossy().into_owned()),
    )?;
    for (name, value) in [
        ("GITHUB_ACTIONS", "true"),
        ("GITHUB_REF", "refs/heads/main"),
        ("GITHUB_REPOSITORY", "symbiogenesis/SwiftBonsai2Runner"),
        ("RUNNER_ARCH", "ARM64"),
    ] {
        require(
            std::env::var(name).as_deref() == Ok(value),
            "Paired capture requires this repository's main ARM64 CI lane.",
        )?;
    }
    let run_id = std::env::var("GITHUB_RUN_ID").unwrap_or_default();
    let runner_name = std::env::var("RUNNER_NAME").unwrap_or_default();
    let device = SnapdragonFamily::from_processor_name(processor_name);
    require(
        cfg!(all(windows, target_arch = "aarch64"))
            && device.is_some()
            && !run_id.is_empty()
            && run_id.bytes().all(|byte| byte.is_ascii_digit())
            && !runner_name.trim().is_empty(),
        "Paired capture requires an identified native Snapdragon X/X2 Elite CI runner.",
    )?;
    Ok(Context {
        run_url: format!(
            "https://github.com/symbiogenesis/SwiftBonsai2Runner/actions/runs/{run_id}"
        ),
        runner_name,
        source_revision: std::env::var("GITHUB_SHA").ok(),
    })
}

fn compiled_plan(request: &Request, policy: &Policy) -> Result<(Value, Value, RuntimeSpec)> {
    let repacked = match request.weight_representation.as_str() {
        "PQ2_0" => false,
        "current-repacked-PTQ1_0" => true,
        _ => return Err("Unpinned capture weight representation.".into()),
    };
    let plan = bonsai_cpu_policy::run(
        json!({"operation": "plan", "dataDirectory": request.data_directory,
        "profile": "swift-pq2-assessment", "useAbliterateLora": true, "useRepackedPtq1": repacked}),
    )?;
    let fixed = &plan["Policy"];
    let lock = &plan["Lock"];
    for (actual, expected) in [
        (&request.admission["Status"], &json!("admission-passed")),
        (
            &request.admission["ModelRevision"],
            &lock["model"]["revision"],
        ),
        (
            &request.admission["RuntimeCommit"],
            &lock["runtime"]["commit"],
        ),
        (&request.admission["ModelPath"], &plan["ModelPath"]),
        (
            &request.admission["ArtifactSha256"],
            &plan["Artifact"]["Sha256"],
        ),
        (
            &request.admission["EstimatedWeightBytes"],
            &plan["Artifact"]["Bytes"],
        ),
        (
            &request.admission["RequiredAvailableBytes"],
            &fixed["RequiredAvailableBytes"],
        ),
        (
            &request.admission["ServiceCeilingBytes"],
            &fixed["ServiceCeilingBytes"],
        ),
        (
            &request.admission["AvailableMemoryFloorBytes"],
            &fixed["AvailableMemoryFloorBytes"],
        ),
        (
            &request.lock["model"]["revision"],
            &lock["model"]["revision"],
        ),
        (
            &request.lock["runtime"]["commit"],
            &lock["runtime"]["commit"],
        ),
        (&request.runtime["Commit"], &lock["runtime"]["commit"]),
        (&request.adapter["AdapterPath"], &plan["AdapterPath"]),
        (
            &request.adapter["Sha256"],
            &plan["AdapterLock"]["adapter"]["sha256"],
        ),
    ] {
        require(
            actual == expected && !actual.is_null(),
            "Paired capture admission/runtime/adapter identities disagree with the compiled plan.",
        )?;
    }
    require(
        request.admission["AvailablePhysicalBytesAtAdmission"]
            .as_u64()
            .is_some_and(|bytes| {
                fixed["RequiredAvailableBytes"]
                    .as_u64()
                    .is_some_and(|required| bytes >= required)
            }),
        "The compiled available-memory admission requirement was not met.",
    )?;
    let runtime = runtime_spec(request, lock)?;
    let mut controls = policy.contract["controls"].clone();
    controls["modelSha256"] =
        policy.contract["representations"][&request.weight_representation].clone();
    controls["weightRepresentation"] = json!(request.weight_representation);
    let decoded: Controls = decode(controls.clone())?;
    control_identity(&decoded, policy)?;
    require(
        controls["modelSha256"] == plan["Artifact"]["Sha256"],
        "Qualification and compiled artifact hashes disagree.",
    )?;
    Ok((plan, controls, runtime))
}

fn runtime_spec(request: &Request, lock: &Value) -> Result<RuntimeSpec> {
    let runtime: RuntimeSpec = decode(request.runtime["Runtime"].clone())?;
    let archive = request.data_directory.join("downloads").join(
        lock["runtime"]["asset"]["filename"]
            .as_str()
            .ok_or("Missing runtime filename.")?,
    );
    let directory = request.data_directory.join("runtime").join(
        lock["runtime"]["commit"]
            .as_str()
            .ok_or("Missing runtime commit.")?,
    );
    require(
        runtime.archive_path == archive
            && runtime.runtime_directory == directory
            && Some(runtime.archive_bytes) == lock["runtime"]["asset"]["bytes"].as_u64()
            && Some(runtime.archive_sha256.as_str()) == lock["runtime"]["asset"]["sha256"].as_str(),
        "Paired capture runtime descriptor differs from its compiled artifact lock.",
    )?;
    Ok(runtime)
}

pub(super) fn save(path: &Path, value: &Value, initial: bool) -> Result<()> {
    let mut options = OpenOptions::new();
    options.write(true);
    if initial {
        options.create_new(true);
    } else {
        options.truncate(true);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt as _;
        options.share_mode(0);
    }
    let mut file = options.open(path).map_err(|error| error.to_string())?;
    serde_json::to_writer_pretty(&mut file, value).map_err(|error| error.to_string())?;
    file.write_all(b"\n").map_err(|error| error.to_string())
}

struct Capture {
    root: PathBuf,
    evidence: Value,
    receipt: Value,
}

impl Capture {
    fn save(&self, initial: bool) -> Result<()> {
        save(&self.root.join("evidence.json"), &self.evidence, initial)?;
        save(&self.root.join("receipt.json"), &self.receipt, initial)
    }

    fn reconcile(&mut self, policy: &Policy, control_digest: &str, incomplete: bool) -> Result<()> {
        for scheduled in &policy.schedule {
            let Some((supervision, captured)) = read_sample(&self.root, &scheduled.sample_id)?
            else {
                if incomplete {
                    break;
                }
                return Err("Completed paired sample evidence is missing.".into());
            };
            let mut sample = serde_json::to_value(scheduled).map_err(|error| error.to_string())?;
            for (key, value) in [
                ("controlsSha256", json!(control_digest)),
                (
                    "decodeTokens",
                    supervision["Timing"]["DecodeTokens"].clone(),
                ),
                (
                    "decodeMilliseconds",
                    supervision["Timing"]["DecodeMilliseconds"].clone(),
                ),
                (
                    "peakJobMemoryBytes",
                    supervision["PeakJobMemoryBytes"].clone(),
                ),
                (
                    "minimumAvailablePhysicalBytes",
                    supervision["MinimumAvailablePhysicalBytes"].clone(),
                ),
                ("completed", json!(true)),
                ("responseText", captured["Text"].clone()),
                ("responseSha256", captured["Sha256"].clone()),
                ("diagnostics", captured["Diagnostics"].clone()),
            ] {
                sample[key] = value;
            }
            self.evidence["samples"]
                .as_array_mut()
                .ok_or("Missing sample checkpoint array.")?
                .push(sample);
            self.save(false)?;
        }
        for prompt in &policy.prompts {
            let Some((_, captured)) = read_sample(&self.root, &format!("zero-{}", prompt.id))?
            else {
                if incomplete {
                    break;
                }
                return Err("Completed zero-scale sample evidence is missing.".into());
            };
            let baseline = self.evidence["samples"]
                .as_array()
                .and_then(|samples| {
                    samples.iter().find(|sample| {
                        sample["promptId"] == prompt.id
                            && sample["arm"] == "baseline"
                            && sample["repeat"] == 1
                    })
                })
                .ok_or("Zero-scale sample has no completed baseline response.")?;
            let check = json!({"promptId": prompt.id, "seed": 42, "baselineResponseSha256": baseline["responseSha256"],
                "zeroScaleResponseSha256": captured["Sha256"], "baselineLogitsSha256": null, "zeroScaleLogitsSha256": null});
            self.evidence["zeroScaleChecks"]
                .as_array_mut()
                .ok_or("Missing zero-scale checkpoint array.")?
                .push(check);
            self.save(false)?;
        }
        Ok(())
    }
}

pub(super) fn read_sample(root: &Path, id: &str) -> Result<Option<(Value, Value)>> {
    let lane = validate_path(&root.join(id))?;
    let path = lane.join("supervision.json");
    if !path.try_exists().map_err(|error| error.to_string())? {
        return Ok(None);
    }
    let supervision = read_json(&path)?;
    if supervision["Status"] != "completed" {
        return Ok(None);
    }
    let captured = read_json_with_limit(&lane.join("capture.json"), CAPTURE_JSON_LIMIT)?;
    require(
        captured["SchemaVersion"] == 1
            && captured["Text"].is_string()
            && captured["Diagnostics"].is_object(),
        "Unsupported paired native capture schema.",
    )?;
    require(
        captured["Sha256"].as_str() == captured["Text"].as_str().map(digest).as_deref(),
        "Paired response text and recorded SHA-256 disagree.",
    )?;
    Ok(Some((supervision, captured)))
}

pub(super) fn copy_adapter(source: &Path, destination: &Path, bytes: u64) -> Result<()> {
    validate_path(source)?;
    let mut input = OpenOptions::new();
    input.read(true);
    let mut output = OpenOptions::new();
    output.write(true).create_new(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt as _;
        input.share_mode(1);
        output.share_mode(0);
    }
    let input = input.open(source).map_err(|error| error.to_string())?;
    let mut output = output
        .open(destination)
        .map_err(|error| error.to_string())?;
    let count = std::io::copy(&mut input.take(bytes + 1), &mut output)
        .map_err(|error| error.to_string())?;
    require(
        count == bytes,
        "Adapter copy differs from its exact compiled byte length.",
    )
}

fn execute(
    request: &Request,
    plan: &Value,
    controls: &Value,
    runtime: &RuntimeSpec,
    policy: &Policy,
    capture: &mut Capture,
    supervise: impl FnOnce(Value) -> Result<Value>,
) -> Result<()> {
    let copy = capture.root.join("bonsai-abliterate-lora.gguf");
    let adapter = &plan["AdapterLock"]["adapter"];
    copy_adapter(
        Path::new(
            plan["AdapterPath"]
                .as_str()
                .ok_or("Missing compiled adapter path.")?,
        ),
        &copy,
        adapter["bytes"]
            .as_u64()
            .ok_or("Missing compiled adapter bytes.")?,
    )?;
    let decoded: Controls = decode(controls.clone())?;
    let (control_digest, _) = control_identity(&decoded, policy)?;
    let invocations = serde_json::to_value(invocation::samples(
        policy,
        &decoded,
        plan["ModelPath"]
            .as_str()
            .ok_or("Missing compiled model path.")?,
    )?)
    .map_err(|error| error.to_string())?;
    let mut samples = Vec::new();
    for sample in invocations
        .as_array()
        .ok_or("Missing fixed invocation samples.")?
    {
        let lane = capture
            .root
            .join(sample["id"].as_str().ok_or("Missing invocation ID.")?);
        fs::create_dir(&lane).map_err(|error| error.to_string())?;
        save(&lane.join("arguments.json"), &sample["arguments"], true)?;
        samples.push(json!({"id": sample["id"], "arguments": sample["arguments"], "evidenceDirectory": lane}));
    }
    let fixed = &plan["Policy"];
    let result = supervise(json!({
        "schemaVersion": 1, "executable": runtime.runtime_directory.join("llama-completion.exe"),
        "arguments": [], "samples": samples, "workingDirectory": capture.root, "evidenceDirectory": capture.root,
        "runtime": runtime, "artifacts": [
            {"path": plan["ModelPath"], "bytes": plan["Artifact"]["Bytes"], "sha256": controls["modelSha256"]},
            {"path": copy, "bytes": adapter["bytes"], "sha256": controls["adapterSha256"]}],
        "memoryLimitBytes": fixed["ServiceCeilingBytes"], "requiredAvailableBytes": fixed["RequiredAvailableBytes"],
        "physicalReserveBytes": fixed["AvailableMemoryFloorBytes"], "timeoutSeconds": fixed["TimeoutSeconds"],
        "stdoutLimitBytes": 4 * MIB, "stderrLimitBytes": 64 * MIB, "requireNonemptyStdout": false, "ciOnly": true,
        "parentProcessId": request.parent_process_id, "parentCreationTimeFileTime": request.parent_creation_time_file_time,
    }));
    if let Ok(supervision) = &result {
        capture.receipt["ModelProcessStarted"] = supervision["ModelProcessStarted"].clone();
        capture.receipt["RuntimeFiles"] = supervision["RuntimeFiles"].clone();
    } else {
        let path = capture.root.join("supervision.json");
        if path.try_exists().map_err(|error| error.to_string())? {
            capture.receipt["ModelProcessStarted"] =
                read_json(&path)?["ModelProcessStarted"].clone();
        }
    }
    capture.reconcile(policy, &control_digest, result.is_err())?;
    result?;
    let qualification = score::score(capture.evidence.clone(), None, policy)?;
    save(
        &capture.root.join("qualification.json"),
        &qualification,
        true,
    )?;
    capture.receipt["Status"] = json!("capture-completed");
    capture.receipt["QualificationStatus"] = qualification["Status"].clone();
    capture.save(false)
}

fn capture_with(
    request: &Request,
    context: &Context,
    supervise: impl FnOnce(Value) -> Result<Value>,
) -> Result<Value> {
    let device = SnapdragonFamily::from_processor_name(&request.processor_name)
        .ok_or("Capture requires a recognized Snapdragon X/X2 Elite processor name.")?;
    let policy = policy()?;
    let (plan, controls, runtime) = compiled_plan(request, &policy)?;
    let root = validate_path(&request.evidence_directory)?;
    require(
        root.is_dir(),
        "Capture evidence directory must already exist.",
    )?;
    for name in [
        "evidence.json",
        "receipt.json",
        "supervision.json",
        "qualification.json",
    ] {
        require(
            !root
                .join(name)
                .try_exists()
                .map_err(|error| error.to_string())?,
            "Capture evidence must be new.",
        )?;
    }
    string(&request.run_id, 128)?;
    string(&request.started_utc, 128)?;
    let mut capture = Capture {
        root,
        evidence: json!({
            "schemaVersion": 1, "runId": request.run_id, "contractSha256": policy.contract_sha256,
            "execution": {"githubActions": true, "runUrl": context.run_url, "runnerArchitecture": "Arm64",
                "deviceModel": device.device_model(), "processorName": request.processor_name,
                "runnerName": context.runner_name},
            "controls": controls, "samples": [], "zeroScaleChecks": [],
            "basis": {"status": "pending", "evidenceSha256": null, "reviewerId": null,
                "note": "The artifact does not declare the source SSM basis; residual-writer verification remains required."},
            "nonzeroAdapterCheck": {"status": "pending", "scale": 1, "adapterSha256": controls["adapterSha256"],
                "logitsChanged": null, "evidenceSha256": null},
        }),
        receipt: json!({
            "Status": "capturing", "StartedUtc": request.started_utc, "EvidenceDirectory": request.evidence_directory,
            "ProcessorName": request.processor_name, "RuntimePackage": plan["Lock"]["runtime"]["asset"],
            "DeviceModel": device.device_model(),
            "RuntimeFiles": request.runtime["RuntimeFiles"], "ModelBytes": plan["Artifact"]["Bytes"],
            "SourceRevision": context.source_revision,
            "TimingScope": "native single-token eval runs/ms, excluding prompt evaluation; not exact sampled token count or first-token latency",
            "QualificationStatus": "pending", "ModelProcessStarted": false,
        }),
    };
    capture.save(true)?;
    match execute(
        request,
        &plan,
        &controls,
        &runtime,
        &policy,
        &mut capture,
        supervise,
    ) {
        Ok(()) => Ok(capture.receipt),
        Err(error) => {
            capture.receipt["Status"] = json!("failed");
            capture.receipt["Failure"] = json!(error);
            if let Err(save_error) = capture.save(false) {
                return Err(format!("{error}; {save_error}"));
            }
            Err(format!("{error} Evidence: {}", capture.root.display()))
        }
    }
}

pub(super) fn run(request: Map<String, Value>) -> Result<Value> {
    let request: Request = decode(Value::Object(request))?;
    let context = ci_context(&request.processor_name)?;
    capture_with(&request, &context, crate::completion::run)
}

pub(super) fn screen_context(processor_name: &str) -> Result<Value> {
    let context = ci_context(processor_name)?;
    let device = SnapdragonFamily::from_processor_name(processor_name)
        .ok_or("Screen requires a recognized Snapdragon X/X2 Elite processor name.")?;
    Ok(json!({"githubActions": true, "runUrl": context.run_url,
        "runnerArchitecture": "Arm64", "deviceModel": device.device_model(), "processorName": processor_name,
        "runnerName": context.runner_name, "sourceRevision": context.source_revision}))
}

pub(super) fn screen_compiled_plan(request: Value) -> Result<(Value, Value, RuntimeSpec)> {
    let request: Request = decode(request)?;
    compiled_plan(&request, &policy()?)
}

#[cfg(test)]
mod tests;
