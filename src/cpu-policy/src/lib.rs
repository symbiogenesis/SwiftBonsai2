//! Reviewed CPU completion policy shared by maintenance and qualification tools.
//!
//! Planning never reads artifact bytes, queries memory, or starts a process.
//! Callers retain responsibility for external storage validation, artifact
//! verification, memory admission, and bounded native process supervision.
#![forbid(unsafe_code)]

use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Shared fixed server arguments and nonlaunching reference preflight policy.
pub mod server;

/// Versioned discovery and profile evidence contracts; no automatic selection.
pub mod hardware;

const REFERENCE_LOCK: &str = include_str!("../../../config/runtime.lock.json");
const SWIFT_LOCK: &str = include_str!("../../../config/swift-assessment.lock.json");
const ADAPTER_LOCK: &str = include_str!("../../../config/swift-lora.lock.json");
const PTQ1_EVIDENCE: &str = include_str!("../../../config/swift.ptq1-export.json");
const REFERENCE_PROFILE: &str = include_str!("../../../config/profiles/reference-cpu.json");
const PHYSICAL_RESERVE_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const SWIFT_WEIGHT_BYTES: u64 = 7_206_168_928;
const PTQ1_WEIGHT_BYTES: u64 = 5_946_648_928;
const ADAPTER_BYTES: u64 = 9_682_464;
const PTQ1_FILENAME: &str = "Swift-Bonsai-2-current-repacked-PTQ1_0.gguf";
const ESTIMATE_SCOPE: &str = "Whole file plus persistent caches/transforms; temporary construction, tokenizer, graph/scratch and allocation overhead use remaining allowance. Source arithmetic, not measured peak.";

/// Fixed reference process memory ceiling: 18 GiB.
pub const MEMORY_LIMIT_BYTES: usize = 19_327_352_832;
/// Reference physical admission threshold, including a separate 4 GiB host reserve.
pub const REQUIRED_AVAILABLE_BYTES: u64 = 23_622_320_128;

/// Reviewed development CPU profiles; neither selection qualifies daily use.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Profile {
    /// Default Qwen reference, with an 18 GiB process allowance.
    #[default]
    ReferenceCpu,
    /// Explicit Swift assessment, with a 12 GiB process allowance.
    SwiftPq2Assessment,
}

impl Profile {
    /// Exact serialized and command-line name of this compiled selection.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::ReferenceCpu => "reference-cpu",
            Self::SwiftPq2Assessment => "swift-pq2-assessment",
        }
    }

    /// Job-wide committed-memory ceiling; the host reserve is additional.
    #[must_use]
    pub const fn memory_limit_bytes(self) -> usize {
        match self {
            Self::ReferenceCpu => MEMORY_LIMIT_BYTES,
            Self::SwiftPq2Assessment => 12 * 1024 * 1024 * 1024,
        }
    }

    /// Physical admission threshold including a separate 4 GiB host reserve.
    #[must_use]
    pub const fn required_available_bytes(self) -> u64 {
        match self {
            Self::ReferenceCpu => REQUIRED_AVAILABLE_BYTES,
            Self::SwiftPq2Assessment => 16 * 1024 * 1024 * 1024,
        }
    }

    /// Reviewed prompt batch size shared by server and completion policies.
    #[must_use]
    pub const fn batch_tokens(self) -> u32 {
        match self {
            Self::ReferenceCpu => 256,
            Self::SwiftPq2Assessment => 128,
        }
    }

    /// Reviewed prompt microbatch size shared by server and completion policies.
    #[must_use]
    pub const fn microbatch_tokens(self) -> u32 {
        match self {
            Self::ReferenceCpu => 128,
            Self::SwiftPq2Assessment => 64,
        }
    }
}

impl std::str::FromStr for Profile {
    type Err = String;

    fn from_str(name: &str) -> Result<Self, Self::Err> {
        match name {
            "reference-cpu" => Ok(Self::ReferenceCpu),
            "swift-pq2-assessment" => Ok(Self::SwiftPq2Assessment),
            _ => Err("Unsupported CPU profile.".into()),
        }
    }
}

fn require(condition: bool, message: &str) -> Result<(), String> {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}

fn parse(input: &str) -> Result<Value, String> {
    serde_json::from_str(input).map_err(|_| "Invalid compiled CPU configuration.".into())
}

fn hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn validate_options(
    profile: Profile,
    use_abliterate_lora: bool,
    use_repacked_ptq1: bool,
) -> Result<(), String> {
    require(
        !use_abliterate_lora || profile == Profile::SwiftPq2Assessment,
        "The pinned adapter assessment requires the Swift profile.",
    )?;
    require(
        !use_repacked_ptq1 || profile == Profile::SwiftPq2Assessment,
        "The current-weight PTQ1 export requires the Swift profile.",
    )
}

/// Return the complete fixed policy and source-based memory accounting.
///
/// # Errors
/// Rejects Swift-only adapter or derived-weight options on the reference profile.
pub fn policy(
    profile: Profile,
    use_abliterate_lora: bool,
    use_repacked_ptq1: bool,
) -> Result<Value, String> {
    validate_options(profile, use_abliterate_lora, use_repacked_ptq1)?;
    let swift = profile == Profile::SwiftPq2Assessment;
    let ceiling = profile.required_available_bytes() - PHYSICAL_RESERVE_BYTES;
    let weights = if use_repacked_ptq1 {
        PTQ1_WEIGHT_BYTES
    } else if swift {
        SWIFT_WEIGHT_BYTES
    } else {
        13_146_393_504
    };
    let kv = 8192_u64 * 16 * 4 * 256 * 2 * 2;
    let recurrent = 48_u64 * 4 * (128 * 6144 + 3 * (6144 + 2 * 16 * 128));
    let transforms = if swift {
        // Shared F32 rotation and sign vectors plus retained int32 metadata.
        4_u64 * 1024 * 1024 + 8 * (5120 + 6144 + 17408)
    } else {
        0
    };
    let adapter = if use_abliterate_lora {
        ADAPTER_BYTES
    } else {
        0
    };
    let known = weights + kv + recurrent + transforms + adapter;
    Ok(json!({
        "Name": profile.name(),
        "LockRelativePath": if swift { "../config/swift-assessment.lock.json" } else { "../config/runtime.lock.json" },
        "ServiceCeilingBytes": ceiling,
        "AvailableMemoryFloorBytes": PHYSICAL_RESERVE_BYTES,
        "RequiredAvailableBytes": profile.required_available_bytes(),
        "ContextTokens": 8192,
        "BatchTokens": profile.batch_tokens(),
        "MicrobatchTokens": profile.microbatch_tokens(),
        "TokenLimit": 16,
        "TimeoutSeconds": 600,
        "EstimatedWeightBytes": weights,
        "EstimatedKvBytes": kv,
        "EstimatedRecurrentBytes": recurrent,
        "EstimatedTransformBytes": transforms,
        "EstimatedAdapterBytes": adapter,
        "WeightRepresentation": if use_repacked_ptq1 { "current-repacked-PTQ1_0" } else if swift { "PQ2_0" } else { "reference-mixed" },
        "EstimatedTransformConstructionTemporaryBytes": if swift { 4 * 1024 * 1024 } else { 0 },
        "EstimatedKnownSubtotalBytes": known,
        "EstimatedRuntimeHeadroomBytes": ceiling - known,
        "EstimateScope": ESTIMATE_SCOPE,
    }))
}

/// Build the common CPU-only completion prefix without selecting a sampler.
///
/// # Errors
/// Rejects empty model paths and embedded NULs. Filesystem verification belongs
/// to the caller; shell metacharacters remain part of one ordinary argument.
pub fn completion_arguments(model_path: &str, profile: Profile) -> Result<Vec<String>, String> {
    require(
        !model_path.trim().is_empty() && !model_path.contains('\0'),
        "A model path is required.",
    )?;
    let mut arguments: Vec<String> = [
        "--model",
        model_path,
        "--ctx-size",
        "8192",
        "--parallel",
        "1",
        "--cache-type-k",
        "f16",
        "--cache-type-v",
        "f16",
        "--gpu-layers",
        "0",
        "--device",
        "none",
        "--no-kv-offload",
        "--no-op-offload",
        "--fit",
        "off",
        "--threads",
        "4",
        "--threads-batch",
        "4",
        "--poll",
        "0",
        "--poll-batch",
        "0",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    arguments.extend([
        "--batch-size".into(),
        profile.batch_tokens().to_string(),
        "--ubatch-size".into(),
        profile.microbatch_tokens().to_string(),
    ]);
    arguments.extend(
        [
            "--load-mode",
            "mmap",
            "--no-repack",
            "--no-context-shift",
            "--offline",
            "--no-warmup",
        ]
        .map(str::to_owned),
    );
    Ok(arguments)
}

fn check_arguments(
    model_path: &str,
    profile: Profile,
    use_abliterate_lora: bool,
    adapter_path: Option<&str>,
) -> Result<Vec<String>, String> {
    validate_options(profile, use_abliterate_lora, false)?;
    require(
        use_abliterate_lora == adapter_path.is_some()
            && adapter_path.is_none_or(|path| !path.trim().is_empty() && !path.contains('\0')),
        "Adapter arguments require the explicit switch and verified adapter path together.",
    )?;
    let verbosity = if profile == Profile::SwiftPq2Assessment {
        "3"
    } else {
        "2"
    };
    let mut arguments = completion_arguments(model_path, profile)?;
    arguments.extend(
        [
            "--no-perf",
            "--log-verbosity",
            verbosity,
            "--color",
            "off",
            "--simple-io",
            "--no-display-prompt",
            "--conversation",
            "--jinja",
            "--single-turn",
            "--n-predict",
            "16",
            "--reasoning",
            "on",
            "--reasoning-effort",
            "xhigh",
            "--reasoning-budget",
            "-1",
            "--temp",
            "1.0",
            "--top-p",
            "0.95",
            "--top-k",
            "20",
            "--min-p",
            "0.0",
            "--repeat-penalty",
            "1.0",
            "--presence-penalty",
            "0.0",
            "--frequency-penalty",
            "0.0",
        ]
        .map(str::to_owned),
    );
    if let Some(adapter) = adapter_path {
        arguments.extend(["--lora".into(), adapter.into()]);
    }
    // Swift rejects system-only messages; preserve the publisher user template.
    // Paired qualification deliberately supplies separate immutable controls.
    arguments.extend(["--prompt".into(), "Reply with the word ready.".into()]);
    Ok(arguments)
}

const fn expected_source_identity(profile: Profile) -> [(&'static str, &'static str); 9] {
    match profile {
        Profile::SwiftPq2Assessment => [
            ("/model/repository", "ukisai/Swift-Bonsai-2-GGUF"),
            (
                "/model/revision",
                "a3bdac086bbb6b04d87d0108d18943b5f63868f7",
            ),
            ("/model/filename", "Swift-Bonsai-2-PQ2_0.gguf"),
            (
                "/model/sha256",
                "5912bb739217cf25b4283a098c7b893d9baff36bdf2e72d3dbf4cb999d5633d5",
            ),
            (
                "/runtime/repository",
                "https://github.com/PrismML-Eng/llama.cpp.git",
            ),
            (
                "/runtime/commit",
                "73bfe6cc485554822f80c473a3c0c4f25e80f41b",
            ),
            ("/runtime/release", "prism-b10787-73bfe6c"),
            (
                "/runtime/asset/filename",
                "llama-prism-b10787-73bfe6c-bin-win-cpu-arm64.zip",
            ),
            (
                "/runtime/asset/sha256",
                "cc808b33d35c72a3bd5c69a0d609d422426feb9dce23868e77992b6c33b8ddb3",
            ),
        ],
        Profile::ReferenceCpu => [
            ("/model/repository", "unsloth/Qwen3.8-27B-GGUF"),
            (
                "/model/revision",
                "4ca720788d1e01f1bff70c033e0d0028fd02e502",
            ),
            ("/model/filename", "Qwen3.8-27B-UD-Q3_K_XL.gguf"),
            (
                "/model/sha256",
                "8c2a45ff85e7674ca185ec8eb6cdeab0e617ed9d8018caed0b64380eb2a67a5e",
            ),
            (
                "/runtime/repository",
                "https://github.com/ggml-org/llama.cpp.git",
            ),
            (
                "/runtime/commit",
                "36e36e63a54fc8f85b95e9c499a022b597e478dd",
            ),
            ("/runtime/release", "b11840"),
            (
                "/runtime/asset/filename",
                "llama-b11840-bin-win-cpu-arm64.zip",
            ),
            (
                "/runtime/asset/sha256",
                "9bfdb90329cd190c8286e5e53ed74391f8455c7cf7a6370725817440dd7ff085",
            ),
        ],
    }
}

fn validate_source(lock: &Value, profile: Profile) -> Result<(), String> {
    require(
        lock.is_object() && lock["schemaVersion"].as_u64() == Some(1),
        "Invalid compiled model/runtime identity.",
    )?;
    for (pointer, value) in expected_source_identity(profile) {
        require(
            lock.pointer(pointer).and_then(Value::as_str) == Some(value),
            "Unreviewed compiled model/runtime identity.",
        )?;
    }
    let swift = profile == Profile::SwiftPq2Assessment;
    let model_bytes = if swift {
        SWIFT_WEIGHT_BYTES
    } else {
        13_146_393_504
    };
    let runtime_bytes = if swift { 12_412_126 } else { 12_781_980 };
    let repository = lock["runtime"]["repository"]
        .as_str()
        .ok_or("Invalid runtime repository.")?;
    let release = lock["runtime"]["release"]
        .as_str()
        .ok_or("Invalid runtime release.")?;
    let filename = lock["runtime"]["asset"]["filename"]
        .as_str()
        .ok_or("Invalid runtime filename.")?;
    let expected_url = format!(
        "{}/releases/download/{release}/{filename}",
        repository.trim_end_matches(".git")
    );
    require(
        lock["model"]["bytes"].as_u64() == Some(model_bytes)
            && lock["runtime"]["asset"]["bytes"].as_u64() == Some(runtime_bytes)
            && lock["runtime"]["asset"]["url"].as_str() == Some(expected_url.as_str()),
        "Invalid compiled artifact size or runtime download URL.",
    )
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase", deny_unknown_fields)]
struct Derived {
    schema_version: u8,
    codec_commit: String,
    source_bytes: u64,
    output_bytes: u64,
    output_sha256: String,
    converted_tensor_count: u32,
    converted_blocks: u64,
    exact_pq2_reconstruction_verified: bool,
    non_pq2_tensor_bytes_copied_verbatim: bool,
    metadata_changes: String,
    working_buffer_bytes: u64,
    qualification: String,
    source_sha256: String,
    source_revision: String,
    tokenizer_metadata_sha256: String,
    output_filename: String,
    source_filename: String,
    source_repository: String,
    saving_bytes: u64,
    model_process_started: bool,
}

/// Select the pinned source or verified PTQ1 identity without reading model bytes.
///
/// # Errors
/// Rejects unreviewed locks, malformed or mismatched PTQ1 evidence, implicit
/// derived-weight selection, and any discrepancy with the fixed weight estimate.
pub fn select_artifact(
    lock: &Value,
    profile: Profile,
    use_repacked_ptq1: bool,
    derived: Option<&Value>,
) -> Result<Value, String> {
    validate_options(profile, false, use_repacked_ptq1)?;
    validate_source(lock, profile)?;
    if !use_repacked_ptq1 {
        require(
            derived.is_none(),
            "Derived weights require the explicit PTQ1 policy.",
        )?;
        return Ok(json!({
            "Filename": lock["model"]["filename"],
            "Bytes": lock["model"]["bytes"],
            "Sha256": lock["model"]["sha256"],
        }));
    }
    let evidence = derived
        .filter(|value| value.is_object())
        .ok_or("Derived PTQ1 evidence is required.")?;
    let evidence: Derived = serde_json::from_value(evidence.clone())
        .map_err(|_| "Invalid derived PTQ1 evidence shape.")?;
    require(
        evidence.schema_version == 1
            && Some(evidence.source_revision.as_str()) == lock["model"]["revision"].as_str()
            && Some(evidence.source_sha256.as_str()) == lock["model"]["sha256"].as_str()
            && Some(evidence.source_bytes) == lock["model"]["bytes"].as_u64()
            && Some(evidence.codec_commit.as_str()) == lock["runtime"]["commit"].as_str()
            && Some(evidence.source_filename.as_str()) == lock["model"]["filename"].as_str()
            && Some(evidence.source_repository.as_str()) == lock["model"]["repository"].as_str()
            && evidence.output_filename == PTQ1_FILENAME
            && hex(&evidence.output_sha256, 64)
            && hex(&evidence.tokenizer_metadata_sha256, 64)
            && evidence.exact_pq2_reconstruction_verified
            && evidence.non_pq2_tensor_bytes_copied_verbatim
            && evidence.converted_tensor_count == 402
            && evidence.converted_blocks == 209_920_000
            && evidence.working_buffer_bytes == 258_720
            && evidence.metadata_changes
                == "general.file_type UINT32 -> 143; PQ2 tensor type/offset fields; alignment padding normalized"
            && evidence.qualification
                == "weight-symbol-and-scale parity only; PTQ1 native activation quantization, logits, quality and performance unqualified"
            && !evidence.model_process_started,
        "Derived PTQ1 evidence differs from its pinned source/runtime.",
    )?;
    require(
        evidence.output_bytes == PTQ1_WEIGHT_BYTES
            && evidence.source_bytes.checked_sub(evidence.output_bytes)
                == Some(evidence.saving_bytes),
        "Review this check before changing the assessment weight estimate.",
    )?;
    Ok(json!({
        "Filename": evidence.output_filename,
        "Bytes": evidence.output_bytes,
        "Sha256": evidence.output_sha256,
    }))
}

fn validate_adapter(adapter: &Value, lock: &Value) -> Result<(), String> {
    require(
        adapter.is_object()
            && adapter["schemaVersion"].as_u64() == Some(1)
            && adapter["adapter"]["repository"] == "Continuum-AI-Corp/OrcaBonsai-27B-Uncensored"
            && adapter["adapter"]["revision"] == "703028bb7fba03496e796b144839b161cf0f5409"
            && adapter["adapter"]["path"] == "gguf/bonsai-abliterate-lora.gguf"
            && adapter["adapter"]["filename"] == "bonsai-abliterate-lora.gguf"
            && adapter["adapter"]["bytes"].as_u64() == Some(ADAPTER_BYTES)
            && adapter["adapter"]["sha256"]
                == "f1669534803d340a496015f5c45125f3437b4d13ec764f40e34488ce83967f42"
            && adapter["adapter"]["url"]
                == "https://raw.githubusercontent.com/Continuum-AI-Corp/OrcaBonsai-27B-Uncensored/703028bb7fba03496e796b144839b161cf0f5409/gguf/bonsai-abliterate-lora.gguf"
            && adapter["compatibility"]["modelRepository"] == lock["model"]["repository"]
            && adapter["compatibility"]["modelRevision"] == lock["model"]["revision"]
            && adapter["compatibility"]["runtimeCommit"] == lock["runtime"]["commit"],
        "Review adapter compatibility and memory accounting for the pinned model/runtime.",
    )
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    operation: String,
    data_directory: String,
    profile: Profile,
    use_abliterate_lora: bool,
    use_repacked_ptq1: bool,
}

fn artifact_path(
    data: &Path,
    category: &str,
    pin: &Value,
    filename: &str,
) -> Result<String, String> {
    let revision = pin["revision"]
        .as_str()
        .ok_or("Invalid compiled artifact revision.")?;
    data.join(category)
        .join(revision)
        .join(filename)
        .into_os_string()
        .into_string()
        .map_err(|_| "Artifact paths must be Unicode.".into())
}

fn unexpanded_variable(path: &str) -> bool {
    let mut previous = None;
    for (index, _) in path.match_indices('%') {
        if previous.is_some_and(|start| index > start + 1) {
            return true;
        }
        previous = Some(index);
    }
    false
}

/// Build the fixed completion plan for one explicit profile and option selection.
///
/// The request object contains exactly `operation: "plan"`, `dataDirectory`,
/// `profile`, `useAbliterateLora`, and `useRepackedPtq1`. Paths are constructed
/// lexically. No artifact or operating-system state is consulted.
///
/// # Errors
/// Rejects malformed requests, unsupported options, nonabsolute data paths, and
/// inconsistent compiled model, adapter, runtime, or derived-weight evidence.
pub fn run(request: Value) -> Result<Value, String> {
    require(request.is_object(), "CPU planning requires a JSON object.")?;
    let request: Request =
        serde_json::from_value(request).map_err(|_| "Invalid CPU planning request.")?;
    require(
        request.operation == "plan",
        "Unsupported CPU policy operation.",
    )?;
    let data = Path::new(&request.data_directory);
    require(
        data.is_absolute()
            && !request.data_directory.contains('\0')
            && !unexpanded_variable(&request.data_directory),
        "An expanded absolute data directory is required.",
    )?;
    let selected_policy = policy(
        request.profile,
        request.use_abliterate_lora,
        request.use_repacked_ptq1,
    )?;
    let lock = parse(if request.profile == Profile::ReferenceCpu {
        REFERENCE_LOCK
    } else {
        SWIFT_LOCK
    })?;
    if request.profile == Profile::ReferenceCpu {
        require(
            parse(REFERENCE_PROFILE)?["serviceCeilingBytes"]
                == selected_policy["ServiceCeilingBytes"],
            "Review this check before changing the reference service allowance.",
        )?;
    }
    let derived = if request.use_repacked_ptq1 {
        Some(parse(PTQ1_EVIDENCE)?)
    } else {
        None
    };
    let artifact = select_artifact(
        &lock,
        request.profile,
        request.use_repacked_ptq1,
        derived.as_ref(),
    )?;
    let adapter_lock = if request.use_abliterate_lora {
        let adapter = parse(ADAPTER_LOCK)?;
        validate_adapter(&adapter, &lock)?;
        Some(adapter)
    } else {
        None
    };
    let model_filename = artifact["Filename"]
        .as_str()
        .ok_or("Invalid selected artifact filename.")?;
    let model_path = artifact_path(data, "models", &lock["model"], model_filename)?;
    let adapter_path = adapter_lock
        .as_ref()
        .map(|adapter| {
            let filename = adapter["adapter"]["filename"]
                .as_str()
                .ok_or("Invalid adapter filename.")?;
            artifact_path(data, "adapters", &adapter["adapter"], filename)
        })
        .transpose()?;
    let arguments = check_arguments(
        &model_path,
        request.profile,
        request.use_abliterate_lora,
        adapter_path.as_deref(),
    )?;
    Ok(json!({
        "Policy": selected_policy,
        "Artifact": artifact,
        "Lock": lock,
        "AdapterLock": adapter_lock,
        "ModelPath": model_path,
        "AdapterPath": adapter_path,
        "Arguments": arguments,
    }))
}

#[cfg(test)]
mod tests;
