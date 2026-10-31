//! Offline, fail-closed review of the evidence required for default-runtime promotion.

use std::collections::BTreeMap;
use std::io::Read as _;
use std::path::{Component, Path, PathBuf};

use bonsai_artifacts::{open_locked_file, validate_external_path};
use bonsai_cpu_policy::hardware::SnapdragonFamily;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::identity::{is_lower_hex, sha256};

const RUNNER: &str = "symbiogenesis/SwiftBonsai2Runner";
const PRISM: &str = "symbiogenesis/prism-inference";
const MANIFEST_LIMIT: u64 = 1024 * 1024;
const REPORT_LIMIT: u64 = 16 * 1024 * 1024;

struct Gate {
    id: &'static str,
    repository: &'static str,
    device: bool,
    human: bool,
    requirement: &'static str,
}

const GATES: &[Gate] = &[
    Gate {
        id: "released-binary-provenance",
        repository: PRISM,
        device: false,
        human: false,
        requirement: "Reviewed main source, immutable release archive and every executable match the selected lock and required green Foundation run.",
    },
    Gate {
        id: "runtime-foundation",
        repository: PRISM,
        device: false,
        human: false,
        requirement: "Complete required source correctness, feature variants, analyzers and packaging pass on the exact released source revision.",
    },
    Gate {
        id: "runtime-conformance",
        repository: PRISM,
        device: false,
        human: false,
        requirement: "Complete required upstream conformance passes without weakening baselines, assertions, tolerances or unsupported-result handling.",
    },
    Gate {
        id: "runner-foundation",
        repository: RUNNER,
        device: false,
        human: false,
        requirement: "Required application, broker, provisioning and released-runtime integration gates pass on settled main.",
    },
    Gate {
        id: "full-context-numerical-parity",
        repository: PRISM,
        device: true,
        human: false,
        requirement: "Full 8192-token Swift scalar/NEON logits, complete live FP16 KV and recurrent/convolution state match pinned native reference within 1e-3 + 1e-3 * abs(reference).",
    },
    Gate {
        id: "prompt-sampler-termination",
        repository: PRISM,
        device: true,
        human: false,
        requirement: "Exact prompt/template tokens, supplied reasoning history, MT19937 draw ordering, seeded replay and stop/output-limit semantics match the pinned native contract.",
    },
    Gate {
        id: "full-output-lifecycle-recovery",
        repository: RUNNER,
        device: true,
        human: false,
        requirement: "4096-token output at the 8192-token context bound, long conversations, count/generation identity, Stop, abandonment, deadlines, backend loss and fresh retry preserve committed history without replay.",
    },
    Gate {
        id: "full-model-resources",
        repository: RUNNER,
        device: true,
        human: false,
        requirement: "Actual-model full-context peak private/Job/resident/system memory obeys 15.5 GiB admission, 11.5 GiB Job commit and 4 GiB physical reserve without sustained paging; pressure shuts down the owned worker.",
    },
    Gate {
        id: "adapter-basis-and-logits",
        repository: PRISM,
        device: true,
        human: false,
        requirement: "Pinned adapter executes in the full Swift graph with verified residual-writer/SSM basis, loaded-at-zero response/logits identity and nonzero logits application.",
    },
    Gate {
        id: "adapter-standard-scale-throughput",
        repository: RUNNER,
        device: true,
        human: false,
        requirement: "Paired same-engine standard LoRA scale 1.3 trials preserve every required per-prompt decode-TPS gate and bounded adapter memory premium; retain losing outcomes.",
    },
    Gate {
        id: "adapter-standard-scale-quality",
        repository: RUNNER,
        device: true,
        human: true,
        requirement: "Independent human reviews of exact hashed paired scale-1.3 responses pass each required quality dimension without refusal/quality tradeoffs.",
    },
    Gate {
        id: "sandbox-and-transport",
        repository: RUNNER,
        device: true,
        human: false,
        requirement: "Actual-model restricted identity denies unrelated files/credentials/egress and same-user bypass, proves broker-only communication, authenticated numeric-loopback framing, bounded consumption and owned cleanup without weaker fallback.",
    },
    Gate {
        id: "speech-offline-and-recovery",
        repository: RUNNER,
        device: true,
        human: false,
        requirement: "Actual native application qualifies speech/chat overlap, encrypted recovery, offline restart, explicit retry, stalled consumers and descendant cleanup with the released runtime.",
    },
    Gate {
        id: "sustained-device-and-creative-acceptance",
        repository: RUNNER,
        device: true,
        human: true,
        requirement: "Owner-reviewed sustained responsiveness/heat/fan and creative usefulness show no material quality, security, memory or comfort regression versus the retained control.",
    },
    Gate {
        id: "installation-and-rollback",
        repository: RUNNER,
        device: true,
        human: false,
        requirement: "Clean-machine ARM64 install, exact package/dependency notices, offline launch, upgrade/recovery and explicit pinned native fallback reproduce on settled main.",
    },
    Gate {
        id: "paired-target-device-benefit",
        repository: RUNNER,
        device: true,
        human: false,
        requirement: "Matched native/Rust artifact and standard-adapter trials retain cold load, prefill, decode and complete-task latency, resources and losing outcomes; demonstrate at least 10% lower end-to-end latency or a reviewed capacity/energy benefit within accepted latency and quality limits.",
    },
    Gate {
        id: "released-adapter-service-integration",
        repository: PRISM,
        device: true,
        human: false,
        requirement: "The selected released private service and worker explicitly verify the pinned adapter identity, advertise matching readiness and support scale 1.3 plus bounded 0..2 changes with fresh compatible state. Canonical count/generation, zero/nonzero streaming, cancellation, reset and rollback reject stale adapter/scale state. A source-only graph or adapter-disabled service cannot satisfy this gate.",
    },
];

#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "kebab-case", deny_unknown_fields)]
enum Request {
    Plan {
        #[serde(rename = "runnerRevision")]
        runner_revision: String,
    },
    FileScore {
        #[serde(rename = "runnerRevision")]
        runner_revision: String,
        #[serde(rename = "evidencePath")]
        evidence_path: PathBuf,
    },
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Evidence {
    schema_version: u32,
    policy_sha256: String,
    identities: Value,
    gates: Vec<Record>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Record {
    id: String,
    status: Status,
    note: String,
    report: Option<Report>,
    execution: Option<Execution>,
    reviewer_id: Option<String>,
    #[serde(default)]
    human_reviewed: bool,
}

#[derive(Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum Status {
    Passed,
    Pending,
    Rejected,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Report {
    file: PathBuf,
    sha256: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Execution {
    repository: String,
    revision: String,
    git_ref: String,
    run_id: String,
    run_attempt: u32,
    architecture: String,
    device_model: String,
    #[serde(default)]
    processor_name: Option<String>,
    self_hosted: bool,
}

fn require(condition: bool, message: &str) -> crate::Result<()> {
    condition.ok_or_else(|| message.into())
}

fn policy(revision: &str) -> crate::Result<Value> {
    require(
        is_lower_hex(revision, 40),
        "Runner revision must be a full lowercase Git commit.",
    )?;
    let profile = bonsai_cpu_policy::Profile::SwiftPq2Assessment;
    require(
        profile.required_available_bytes() == 16_642_998_272
            && profile.memory_limit_bytes() == 12_348_030_976,
        "Promotion resource contract needs review after a compiled profile change.",
    )?;
    let runtime: Value =
        serde_json::from_str(include_str!("../../../config/prism-inference.lock.json"))
            .map_err(|error| error.to_string())?;
    let reference: Value =
        serde_json::from_str(include_str!("../../../config/swift-assessment.lock.json"))
            .map_err(|error| error.to_string())?;
    let adapter: Value = serde_json::from_str(include_str!("../../../config/swift-lora.lock.json"))
        .map_err(|error| error.to_string())?;
    let metadata: Value = serde_json::from_str(include_str!("../../../config/swift.metadata.json"))
        .map_err(|error| error.to_string())?;
    let server = runtime["files"]
        .as_array()
        .and_then(|files| {
            files
                .iter()
                .find(|file| file["name"] == "prism-inference-server.exe")
        })
        .ok_or("Release lock has no server identity.")?;
    for (value, length) in [
        (&runtime["commit"], 40),
        (&reference["runtime"]["commit"], 40),
        (&runtime["asset"]["sha256"], 64),
        (&server["sha256"], 64),
        (&reference["model"]["sha256"], 64),
        (&adapter["adapter"]["sha256"], 64),
        (
            &metadata["EmbeddedTemplates"]["tokenizer.chat_template"]["Sha256"],
            64,
        ),
    ] {
        require(
            value
                .as_str()
                .is_some_and(|identity| is_lower_hex(identity, length)),
            "Promotion policy is missing a complete pinned artifact identity.",
        )?;
    }
    Ok(json!({
        "schemaVersion": 1,
        "profile": "swift-pq2-standard-lora-1.3",
        "identities": {
            "runnerRevision": revision, "runtimeRevision": runtime["commit"],
            "runtimeLockSha256": sha256(include_bytes!("../../../config/prism-inference.lock.json")),
            "assessmentLockSha256": sha256(include_bytes!("../../../config/swift-assessment.lock.json")),
            "adapterLockSha256": sha256(include_bytes!("../../../config/swift-lora.lock.json")),
            "runtimeArchiveSha256": runtime["asset"]["sha256"], "serverSha256": server["sha256"],
            "modelSha256": reference["model"]["sha256"], "referenceRuntimeRevision": reference["runtime"]["commit"],
            "adapterSha256": adapter["adapter"]["sha256"], "adapterScale": 1.3,
            "templateSha256": metadata["EmbeddedTemplates"]["tokenizer.chat_template"]["Sha256"],
            "contextTokens": 8192, "maxCompletionTokens": 4096,
            "admissionBytes": 16_642_998_272_u64, "jobCommitLimitBytes": 12_348_030_976_u64,
            "physicalReserveBytes": 4_294_967_296_u64,
            "target": "aarch64-pc-windows-msvc"
        },
        "requiredGates": GATES.iter().map(|gate| json!({"id": gate.id, "repository": gate.repository, "snapdragonRequired": gate.device, "humanReviewRequired": gate.human, "requirement": gate.requirement})).collect::<Vec<_>>()
    }))
}

fn read(path: &Path, limit: u64) -> crate::Result<Vec<u8>> {
    validate_external_path(path).map_err(|error| error.to_string())?;
    let file = open_locked_file(path).map_err(|error| error.to_string())?;
    require(
        file.metadata().map_err(|error| error.to_string())?.len() <= limit,
        "Promotion evidence exceeds its byte bound.",
    )?;
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    require(
        u64::try_from(bytes.len()).map_err(|error| error.to_string())? <= limit,
        "Promotion evidence exceeds its byte bound.",
    )?;
    Ok(bytes)
}

fn validate_record(
    gate: &Gate,
    record: &Record,
    policy: &Value,
    directory: &Path,
) -> crate::Result<()> {
    require(
        !record.note.trim().is_empty() && record.note.len() <= 4096,
        "Every supplied promotion gate needs a bounded review note.",
    )?;
    if record.status == Status::Pending {
        return Ok(());
    }
    let report = record
        .report
        .as_ref()
        .ok_or("Completed promotion gate is missing its report.")?;
    require(
        !report.file.as_os_str().is_empty()
            && report
                .file
                .components()
                .all(|part| matches!(part, Component::Normal(_))),
        "Promotion report must be a relative path without traversal.",
    )?;
    require(
        is_lower_hex(&report.sha256, 64),
        "Promotion report digest is invalid.",
    )?;
    let bytes = read(&directory.join(&report.file), REPORT_LIMIT)?;
    require(
        !bytes.is_empty() && sha256(&bytes) == report.sha256,
        "Promotion report is empty or its digest does not match.",
    )?;
    let execution = record
        .execution
        .as_ref()
        .ok_or("Completed promotion gate is missing CI provenance.")?;
    let revision = if gate.repository == PRISM {
        &policy["identities"]["runtimeRevision"]
    } else {
        &policy["identities"]["runnerRevision"]
    };
    require(
        execution.repository == gate.repository
            && revision.as_str() == Some(&execution.revision)
            && execution.git_ref == "refs/heads/main"
            && execution.run_attempt > 0
            && !execution.run_id.is_empty()
            && execution.run_id.len() <= 24
            && execution.run_id.bytes().all(|byte| byte.is_ascii_digit())
            && execution.run_id.bytes().any(|byte| byte != b'0'),
        "Promotion gate does not identify the expected exact main CI revision/run.",
    )?;
    require(
        !gate.device
            || (execution.architecture == "ARM64"
                && SnapdragonFamily::from_evidence(
                    &execution.device_model,
                    execution.processor_name.as_deref(),
                )
                .is_some()
                && execution.self_hosted),
        "Promotion gate requires matching self-hosted Snapdragon X/X2 Elite ARM64 identity.",
    )?;
    if record.status == Status::Passed {
        require(
            record
                .reviewer_id
                .as_ref()
                .is_some_and(|id| !id.trim().is_empty() && id.len() <= 256),
            "Passed promotion gate requires an identified reviewer.",
        )?;
        require(
            !gate.human || record.human_reviewed,
            "Promotion gate requires direct human review.",
        )?;
    }
    Ok(())
}

fn score(value: Value, policy: &Value, directory: &Path) -> crate::Result<Value> {
    require(
        value.is_object()
            && value["gates"].as_array().is_some_and(|gates| {
                gates.iter().all(|gate| {
                    gate.is_object()
                        && gate
                            .get("report")
                            .is_none_or(|report| report.is_null() || report.is_object())
                        && gate
                            .get("execution")
                            .is_none_or(|execution| execution.is_null() || execution.is_object())
                })
            }),
        "Promotion evidence and gate records must be JSON objects.",
    )?;
    let evidence: Evidence = serde_json::from_value(value).map_err(|error| error.to_string())?;
    require(
        evidence.schema_version == 1
            && evidence.policy_sha256
                == sha256(&serde_json::to_vec(policy).map_err(|error| error.to_string())?)
            && evidence.identities == policy["identities"],
        "Promotion evidence does not match the selected policy and exact artifact identities.",
    )?;
    require(
        evidence.gates.len() <= GATES.len(),
        "Too many promotion gate records.",
    )?;
    let mut records = BTreeMap::new();
    for record in &evidence.gates {
        require(
            records.insert(record.id.as_str(), record).is_none(),
            "Duplicate promotion gate record.",
        )?;
        require(
            GATES.iter().any(|gate| gate.id == record.id),
            "Unknown promotion gate record.",
        )?;
    }
    let mut status = Status::Passed;
    let mut gates = Vec::new();
    let mut device_family = None;
    for gate in GATES {
        let record = records.get(gate.id);
        let verdict = if let Some(record) = record {
            validate_record(gate, record, policy, directory)?;
            if gate.device && record.status != Status::Pending {
                let execution = record
                    .execution
                    .as_ref()
                    .ok_or("Missing completed device execution.")?;
                let family = SnapdragonFamily::from_evidence(
                    &execution.device_model,
                    execution.processor_name.as_deref(),
                )
                .ok_or("Unknown promotion device identity.")?;
                require(
                    device_family.is_none_or(|existing| existing == family),
                    "Device-dependent promotion gates cannot mix Snapdragon families.",
                )?;
                device_family = Some(family);
            }
            record.status
        } else {
            Status::Pending
        };
        if verdict == Status::Rejected || (verdict == Status::Pending && status == Status::Passed) {
            status = verdict;
        }
        let run_url = record
            .and_then(|record| record.execution.as_ref())
            .map(|execution| {
                format!(
                    "https://github.com/{}/actions/runs/{}/attempts/{}",
                    execution.repository, execution.run_id, execution.run_attempt
                )
            });
        gates.push(json!({"id": gate.id, "status": match verdict {Status::Passed => "passed", Status::Pending => "pending", Status::Rejected => "rejected"}, "runUrl": run_url, "deviceModel": record.and_then(|record| record.execution.as_ref()).map(|execution| &execution.device_model), "processorName": record.and_then(|record| record.execution.as_ref()).and_then(|execution| execution.processor_name.as_ref()), "reportSha256": record.and_then(|record| record.report.as_ref()).map(|report| &report.sha256)}));
    }
    Ok(
        json!({"schemaVersion": 1, "status": match status {Status::Passed => "passed", Status::Pending => "pending", Status::Rejected => "rejected"}, "promotionReady": status == Status::Passed, "gates": gates, "scope": "Offline integrity and reviewer-attested evidence completeness only. CI provenance and human identity require independent review. Does not collect qualification, authenticate attestations, enable serving defaults or qualify PTQ1."}),
    )
}

/// Produce the exact promotion plan or score an external, bounded evidence bundle.
///
/// # Errors
/// Rejects malformed records, stale identities, unreviewed passes, wrong devices,
/// missing report bytes and report tampering. Missing gates remain pending.
pub fn run(request: Value) -> crate::Result<Value> {
    let request: Request = serde_json::from_value(request).map_err(|error| error.to_string())?;
    match request {
        Request::Plan { runner_revision } => {
            let policy = policy(&runner_revision)?;
            let digest = sha256(&serde_json::to_vec(&policy).map_err(|error| error.to_string())?);
            Ok(json!({"policySha256": digest, "policy": policy,
                    "evidenceTemplate": {"schemaVersion": 1, "policySha256": digest, "identities": policy["identities"], "gates": GATES.iter().map(|gate| json!({"id": gate.id, "status": "pending", "note": "Evidence has not yet been collected and reviewed."})).collect::<Vec<_>>()}}))
        }
        Request::FileScore {
            runner_revision,
            evidence_path,
        } => {
            let policy = policy(&runner_revision)?;
            let value = serde_json::from_slice(&read(&evidence_path, MANIFEST_LIMIT)?)
                .map_err(|error| error.to_string())?;
            score(
                value,
                &policy,
                evidence_path
                    .parent()
                    .ok_or("Promotion evidence needs an external parent directory.")?,
            )
        }
    }
}

#[cfg(test)]
mod tests;
