//! CI-only capture using the existing verified native process supervisor.

use std::fs;
use std::path::{Path, PathBuf};

use bonsai_artifacts::RuntimeSpec;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::{REPRESENTATIONS, controls_identity, decode, plan, require, score};
use crate::Result;
use crate::qualification::{
    capture, invocation, read_json, schema::Controls, string, validate_path,
};

const MIB: u64 = 1024 * 1024;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    data_directory: PathBuf,
    pq2_admission: Value,
    ptq1_admission: Value,
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

fn compiled(request: &Request, index: usize) -> Result<(Value, Value, RuntimeSpec)> {
    capture::screen_compiled_plan(json!({
        "dataDirectory": request.data_directory, "weightRepresentation": REPRESENTATIONS[index],
        "admission": if index == 0 { &request.pq2_admission } else { &request.ptq1_admission },
        "lock": request.lock, "runtime": request.runtime, "adapter": request.adapter,
        "evidenceDirectory": request.evidence_directory, "processorName": request.processor_name,
        "runId": request.run_id, "startedUtc": request.started_utc,
        "parentProcessId": request.parent_process_id,
        "parentCreationTimeFileTime": request.parent_creation_time_file_time,
    }))
}

fn invocations(
    root: &Path,
    plans: &[Value; 2],
    controls: &[Value; 2],
    screen: &Value,
) -> Result<Vec<Value>> {
    let mut scheduled = screen["Schedule"]
        .as_array()
        .ok_or("Missing screen schedule.")?
        .clone();
    for representation in REPRESENTATIONS {
        scheduled.push(json!({"sampleId": format!("zero-{representation}"),
            "weightRepresentation": representation, "arm": "zero"}));
    }
    let mut samples = Vec::with_capacity(14);
    for sample in scheduled {
        let index = usize::from(sample["weightRepresentation"] == REPRESENTATIONS[1]);
        let decoded: Controls = decode(controls[index].clone())?;
        controls_identity(&controls[index])?;
        let arguments = invocation::screen_arguments(
            &decoded,
            plans[index]["ModelPath"]
                .as_str()
                .ok_or("Missing verified screen model path.")?,
            sample["arm"].as_str().ok_or("Missing screen arm.")?,
            screen["Contract"]["prompt"]["text"]
                .as_str()
                .ok_or("Missing screen prompt.")?,
        )?;
        let id = sample["sampleId"]
            .as_str()
            .ok_or("Missing screen sample ID.")?;
        let lane = root.join(id);
        fs::create_dir(&lane).map_err(|error| error.to_string())?;
        capture::save(&lane.join("arguments.json"), &json!(arguments), true)?;
        samples.push(json!({"id": id, "arguments": arguments, "evidenceDirectory": lane}));
    }
    Ok(samples)
}

fn reconcile(root: &Path, evidence: &mut Value, screen: &Value, incomplete: bool) -> Result<()> {
    for scheduled in screen["Schedule"]
        .as_array()
        .ok_or("Missing screen schedule.")?
    {
        let id = scheduled["sampleId"]
            .as_str()
            .ok_or("Missing screen sample ID.")?;
        let Some((supervision, captured)) = capture::read_sample(root, id)? else {
            require(incomplete, "Completed decision screen sample is missing.")?;
            break;
        };
        let representation = scheduled["weightRepresentation"]
            .as_str()
            .ok_or("Missing screen representation.")?;
        let mut sample = scheduled.clone();
        for (key, value) in [
            (
                "controlsSha256",
                json!(controls_identity(&evidence["controls"][representation])?),
            ),
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
        ] {
            sample[key] = value;
        }
        evidence["samples"]
            .as_array_mut()
            .ok_or("Missing screen sample array.")?
            .push(sample);
        capture::save(&root.join("screen-evidence.json"), evidence, false)?;
    }
    for representation in REPRESENTATIONS {
        let Some((_, captured)) = capture::read_sample(root, &format!("zero-{representation}"))?
        else {
            require(
                incomplete,
                "Completed decision screen zero-scale control is missing.",
            )?;
            break;
        };
        let baseline = evidence["samples"]
            .as_array()
            .and_then(|samples| {
                samples.iter().find(|sample| {
                    sample["weightRepresentation"] == representation
                        && sample["arm"] == "baseline"
                        && sample["repeat"] == 1
                })
            })
            .ok_or("Zero-scale screen control lacks a baseline.")?;
        let check = json!({"weightRepresentation": representation,
            "baselineResponseSha256": baseline["responseSha256"], "zeroScaleResponseSha256": captured["Sha256"]});
        evidence["zeroScaleChecks"]
            .as_array_mut()
            .ok_or("Missing screen zero controls.")?
            .push(check);
        capture::save(&root.join("screen-evidence.json"), evidence, false)?;
    }
    Ok(())
}

fn device_model(request: &Request, context: &Value) -> Result<&'static str> {
    let device =
        bonsai_cpu_policy::hardware::SnapdragonFamily::from_processor_name(&request.processor_name)
            .ok_or("Screen requires a recognized Snapdragon X/X2 Elite processor name.")?;
    require(
        context["deviceModel"] == device.device_model()
            && context
                .get("processorName")
                .is_none_or(|name| name == &request.processor_name),
        "Screen context and observed processor identity disagree.",
    )?;
    Ok(device.device_model())
}

fn capture_with(
    request: &Request,
    context: &Value,
    supervise: impl FnOnce(Value) -> Result<Value>,
) -> Result<Value> {
    let device = device_model(request, context)?;
    let (pq2, pq2_controls, runtime) = compiled(request, 0)?;
    let (ptq1, ptq1_controls, _) = compiled(request, 1)?;
    let plans = [pq2, ptq1];
    let mut controls = [pq2_controls, ptq1_controls];
    for control in &mut controls {
        control["maxOutputTokens"] = json!(64);
    }
    let screen = plan()?;
    let root = validate_path(&request.evidence_directory)?;
    require(
        root.is_dir(),
        "Screen evidence directory must already exist.",
    )?;
    for name in [
        "screen-evidence.json",
        "screen-receipt.json",
        "screen-report.json",
        "supervision.json",
        "bonsai-abliterate-lora.gguf",
    ] {
        require(
            !root
                .join(name)
                .try_exists()
                .map_err(|error| error.to_string())?,
            "Screen evidence must be new.",
        )?;
    }
    string(&request.run_id, 128)?;
    string(&request.started_utc, 128)?;
    let mut evidence = json!({"schemaVersion": 1, "runId": request.run_id,
        "contractSha256": screen["ContractSha256"], "execution": context,
        "controls": {REPRESENTATIONS[0]: controls[0], REPRESENTATIONS[1]: controls[1]},
        "samples": [], "zeroScaleChecks": []});
    let mut receipt = json!({"Status": "capturing", "StartedUtc": request.started_utc,
        "EvidenceDirectory": root, "ModelProcessStarted": false, "DefaultMayChange": false,
        "DeviceModel": device, "ProcessorName": request.processor_name,
        "StandardProfileMayEnable": false});
    capture::save(&root.join("screen-evidence.json"), &evidence, true)?;
    capture::save(&root.join("screen-receipt.json"), &receipt, true)?;
    let result: Result<(Value, Value)> = (|| {
        let adapter = &plans[0]["AdapterLock"]["adapter"];
        let copy = root.join("bonsai-abliterate-lora.gguf");
        capture::copy_adapter(
            Path::new(
                plans[0]["AdapterPath"]
                    .as_str()
                    .ok_or("Missing screen adapter path.")?,
            ),
            &copy,
            adapter["bytes"]
                .as_u64()
                .ok_or("Missing screen adapter bytes.")?,
        )?;
        let samples = invocations(&root, &plans, &controls, &screen)?;
        let fixed = &plans[0]["Policy"];
        receipt["ModelProcessStarted"] = Value::Null;
        capture::save(&root.join("screen-receipt.json"), &receipt, false)?;
        let result = supervise(json!({"schemaVersion": 1,
        "executable": runtime.runtime_directory.join("llama-completion.exe"), "arguments": [],
        "samples": samples, "workingDirectory": root, "evidenceDirectory": root,
        "runtime": runtime, "artifacts": [
            {"path": plans[0]["ModelPath"], "bytes": plans[0]["Artifact"]["Bytes"], "sha256": controls[0]["modelSha256"]},
            {"path": plans[1]["ModelPath"], "bytes": plans[1]["Artifact"]["Bytes"], "sha256": controls[1]["modelSha256"]},
            {"path": copy, "bytes": adapter["bytes"], "sha256": controls[0]["adapterSha256"]}],
        "memoryLimitBytes": fixed["ServiceCeilingBytes"], "requiredAvailableBytes": fixed["RequiredAvailableBytes"],
        "physicalReserveBytes": fixed["AvailableMemoryFloorBytes"], "timeoutSeconds": fixed["TimeoutSeconds"],
        "stdoutLimitBytes": 4 * MIB, "stderrLimitBytes": 64 * MIB, "requireNonemptyStdout": false, "ciOnly": true,
        "parentProcessId": request.parent_process_id, "parentCreationTimeFileTime": request.parent_creation_time_file_time}));
        reconcile(&root, &mut evidence, &screen, result.is_err())?;
        let supervision = result?;
        require(
            supervision["Status"] == "completed",
            "The native screen batch did not complete.",
        )?;
        let report = score::score(&evidence, None)?;
        capture::save(&root.join("screen-report.json"), &report, true)?;
        Ok((supervision, report))
    })();
    match result {
        Ok((supervision, report)) => {
            receipt["Status"] = json!("capture-completed");
            receipt["ScreenStatus"] = report["Status"].clone();
            receipt["ModelProcessStarted"] = supervision["ModelProcessStarted"].clone();
            receipt["RuntimeFiles"] = supervision["RuntimeFiles"].clone();
            capture::save(&root.join("screen-receipt.json"), &receipt, false)?;
            Ok(receipt)
        }
        Err(error) => {
            if let Ok(supervision) = read_json(&root.join("supervision.json")) {
                receipt["ModelProcessStarted"] = supervision["ModelProcessStarted"].clone();
            }
            receipt["Status"] = json!("failed");
            receipt["Failure"] = json!(error);
            capture::save(&root.join("screen-receipt.json"), &receipt, false)?;
            Err(format!("{error} Screen evidence: {}", root.display()))
        }
    }
}

#[cfg(test)]
mod tests;

pub(super) fn run(request: Map<String, Value>) -> Result<Value> {
    let request: Request = decode(Value::Object(request))?;
    let context = capture::screen_context(&request.processor_name)?;
    capture_with(&request, &context, crate::completion::run)
}
