//! Fixed development-only backend policy; configuration is compiled into the broker.
//!
//! This plan is not permission to launch and does not qualify the daily profile.

use serde::Deserialize;
use std::{
    ffi::OsString,
    fmt, fs,
    path::{Component, Path, PathBuf},
};

const LOCK_JSON: &str = include_str!("../../../config/runtime.lock.json");
const PROFILE_JSON: &str = include_str!("../../../config/profiles/reference-cpu.json");
/// Fixed process memory ceiling: 18 GiB.
pub const MEMORY_LIMIT_BYTES: usize = 19_327_352_832;
/// Required available physical memory including a 4 GiB host reserve.
pub const REQUIRED_AVAILABLE_BYTES: u64 = 23_622_320_128;

/// A value-free policy failure; inherited environment values are never retained.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PolicyError {
    /// Compiled artifact identities or profile need review.
    Configuration,
    /// An inherited model/runtime override or non-Unicode name was present.
    Environment,
    /// Storage is not an absolute external path without redirects.
    Path,
    /// The backend port is privileged.
    Port,
}
impl fmt::Display for PolicyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Configuration => "Unreviewed compiled launch configuration.",
            Self::Environment => "Inherited model/runtime environment overrides are unsupported.",
            Self::Path => "An absolute external path without Git or reparse ancestors is required.",
            Self::Port => "A nonprivileged backend port is required.",
        })
    }
}
impl std::error::Error for PolicyError {}

/// Reject inherited runtime/model overrides using names alone.
///
/// # Errors
/// Returns [`PolicyError::Environment`] for forbidden or non-Unicode names.
pub fn validate_environment(names: impl IntoIterator<Item = OsString>) -> Result<(), PolicyError> {
    for name in names {
        let name = name
            .to_str()
            .ok_or(PolicyError::Environment)?
            .to_ascii_uppercase();
        if ["LLAMA_", "LLAMACPP_", "GGML_", "AIP_", "MTMD_"]
            .iter()
            .any(|prefix| name.starts_with(prefix))
            || ["HF_TOKEN", "HF_ENDPOINT", "MODEL_ENDPOINT"].contains(&name.as_str())
        {
            return Err(PolicyError::Environment);
        }
    }
    Ok(())
}

/// Fixed, separately tokenized backend invocation; callers cannot add arguments.
#[derive(Debug)]
pub struct LaunchPlan {
    /// Locked installed ARM64 server path.
    pub executable: PathBuf,
    /// Arguments passed directly to the executable, never a shell.
    pub arguments: Vec<OsString>,
    /// Whole-model path verified before launch.
    pub model_path: PathBuf,
    /// Pinned archive path verified before launch.
    pub archive_path: PathBuf,
    /// Complete installed runtime directory.
    pub runtime_directory: PathBuf,
    /// Owner-private backend credential path supplied by the credential owner.
    pub key_file: PathBuf,
    /// Available memory admission threshold, including the host reserve.
    pub required_available_bytes: u64,
    /// Job memory ceiling in bytes.
    pub memory_limit_bytes: usize,
}
impl LaunchPlan {
    /// Build the sole reviewed development plan. This never launches or writes.
    /// The separate launcher must require its explicit development opt-in.
    ///
    /// # Errors
    /// Rejects invalid paths, ports, inherited overrides or compiled policy drift.
    pub fn reference(data: &Path, backend_port: u16, key_file: &Path) -> Result<Self, PolicyError> {
        validate_environment(std::env::vars_os().map(|(name, _)| name))?;
        let pins = reviewed_pins()?;
        validate_external_path(data)?;
        validate_external_path(key_file)?;
        if backend_port < 1024 {
            return Err(PolicyError::Port);
        }
        let runtime_directory = data.join("runtime").join(&pins.runtime.commit);
        // The pinned release archive places the server at the archive root.
        let executable = runtime_directory.join("llama-server.exe");
        let model_path = data
            .join("models")
            .join(&pins.model.revision)
            .join(&pins.model.filename);
        let archive_path = data.join("downloads").join(&pins.runtime.asset.filename);
        for path in [&executable, &model_path, &archive_path] {
            validate_external_path(path)?;
        }
        let mut arguments = vec![
            OsString::from("--model"),
            model_path.as_os_str().to_owned(),
            "--host".into(),
            "127.0.0.1".into(),
            "--port".into(),
            backend_port.to_string().into(),
            "--api-key-file".into(),
            key_file.as_os_str().to_owned(),
            "--cors-origins".into(),
            format!("http://127.0.0.1:{backend_port}").into(),
        ];
        arguments.extend(FIXED_ARGUMENTS.iter().map(OsString::from));
        Ok(Self {
            executable,
            arguments,
            model_path,
            archive_path,
            runtime_directory,
            key_file: key_file.to_path_buf(),
            required_available_bytes: REQUIRED_AVAILABLE_BYTES,
            memory_limit_bytes: MEMORY_LIMIT_BYTES,
        })
    }
}

const FIXED_ARGUMENTS: &[&str] = &[
    "--cors-methods",
    "GET,POST,OPTIONS",
    "--cors-headers",
    "Authorization,Content-Type",
    "--no-cors-credentials",
    "--no-agent",
    "--no-ui-mcp-proxy",
    "--no-ui",
    "--no-slots",
    "--no-mmproj",
    "--ctx-size",
    "8192",
    "--parallel",
    "1",
    "--batch-size",
    "256",
    "--ubatch-size",
    "128",
    "--cache-type-k",
    "f16",
    "--cache-type-v",
    "f16",
    "--ctx-checkpoints",
    "0",
    "--cache-ram",
    "0",
    "--no-cache-prompt",
    "--no-cache-idle-slots",
    "--no-context-shift",
    "--spec-type",
    "none",
    "--gpu-layers",
    "0",
    "--device",
    "none",
    "--no-kv-offload",
    "--no-op-offload",
    "--fit",
    "off",
    "--load-mode",
    "mmap",
    "--no-repack",
    "--offline",
    "--no-warmup",
    "--threads",
    "4",
    "--threads-batch",
    "4",
    "--poll",
    "0",
    "--poll-batch",
    "0",
    "--jinja",
    "--reasoning",
    "auto",
    "--temp",
    "1.0",
    "--top-p",
    "0.95",
    "--top-k",
    "20",
    "--min-p",
    "0.0",
    "--presence-penalty",
    "0.0",
    "--repeat-penalty",
    "1.0",
    "--n-predict",
    "4096",
    "--timeout",
    "60",
    "--no-perf",
    "--log-disable",
];

// Struct deserialization rejects duplicate and unknown fields. Object wrappers
// also reject Serde's positional-array representation at every object boundary.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Pins {
    schema_version: u8,
    decision: String,
    pub(crate) model: super::Object<ModelPin>,
    pub(crate) runtime: super::Object<RuntimePin>,
    fallback: super::Object<Fallback>,
    qualification: super::Object<Qualification>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ModelPin {
    repository: String,
    pub(crate) revision: String,
    pub(crate) filename: String,
    pub(crate) bytes: u64,
    pub(crate) sha256: String,
    digest_source: String,
    license: String,
    architecture: String,
    expected_metadata: super::Object<Metadata>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Metadata {
    block_count_including_mtp: u32,
    mtp_blocks: u32,
    full_attention_interval: u32,
    kv_heads: u32,
    key_length: u32,
    value_length: u32,
    ssm_state_size: u32,
    ssm_inner_size: u32,
    ssm_group_count: u32,
    ssm_convolution_kernel: u32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RuntimePin {
    repository: String,
    pub(crate) commit: String,
    release: String,
    license: String,
    pub(crate) asset: super::Object<AssetPin>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AssetPin {
    pub(crate) filename: String,
    url: String,
    pub(crate) bytes: u64,
    pub(crate) sha256: String,
    digest_source: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Fallback {
    repository: String,
    revision: String,
    filename: String,
    bytes: u64,
    sha256: String,
    status: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Qualification {
    source_compatibility: String,
    model_download: String,
    full_gguf_inspection: String,
    cpu_loading: String,
    daily_security: String,
    device_performance: String,
}

impl<T> std::ops::Deref for super::Object<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.0
    }
}

pub(crate) fn reviewed_pins() -> Result<Pins, PolicyError> {
    let pins = parse_pins(LOCK_JSON)?;
    validate_profile(PROFILE_JSON)?;
    Ok(pins)
}

fn parse_pins(input: &str) -> Result<Pins, PolicyError> {
    let super::Object(pins): super::Object<Pins> =
        serde_json::from_str(input).map_err(|_| PolicyError::Configuration)?;
    let m = &pins.model;
    let r = &pins.runtime;
    let metadata = &m.expected_metadata;
    let f = &pins.fallback;
    let q = &pins.qualification;
    if pins.schema_version != 1
        || pins.decision != "docs/model-selection.md"
        || m.repository != "unsloth/Qwen3.8-27B-GGUF"
        || m.revision != "4ca720788d1e01f1bff70c033e0d0028fd02e502"
        || m.filename != "Qwen3.8-27B-UD-Q3_K_XL.gguf"
        || m.bytes != 13_146_393_504
        || m.sha256 != "8c2a45ff85e7674ca185ec8eb6cdeab0e617ed9d8018caed0b64380eb2a67a5e"
        || m.digest_source
            != "https://huggingface.co/api/models/unsloth/Qwen3.8-27B-GGUF/revision/4ca720788d1e01f1bff70c033e0d0028fd02e502?blobs=true"
        || m.license != "Apache-2.0"
        || m.architecture != "qwen35"
        || [
            metadata.block_count_including_mtp,
            metadata.mtp_blocks,
            metadata.full_attention_interval,
            metadata.kv_heads,
            metadata.key_length,
            metadata.value_length,
            metadata.ssm_state_size,
            metadata.ssm_inner_size,
            metadata.ssm_group_count,
            metadata.ssm_convolution_kernel,
        ] != [65, 1, 4, 4, 256, 256, 128, 6144, 16, 4]
        || r.repository != "https://github.com/ggml-org/llama.cpp.git"
        || r.commit != "620d2225cc436ab0b001e3200e590e3b286ef9ab"
        || r.release != "b11669"
        || r.license != "MIT"
        || r.asset.filename != "llama-b11669-bin-win-cpu-arm64.zip"
        || r.asset.bytes != 12_442_177
        || r.asset.sha256 != "9cff39ffed17edcd03a7d876073f74d7b48cd43a1e5eafdd59ee094185ad871b"
        || r.asset.url
            != "https://github.com/ggml-org/llama.cpp/releases/download/b11669/llama-b11669-bin-win-cpu-arm64.zip"
        || r.asset.digest_source
            != "https://api.github.com/repos/ggml-org/llama.cpp/releases/tags/b11669"
        || f.repository != "google/gemma-4-12B-it-qat-q4_0-gguf"
        || f.revision != "29d097773436b69ff9feafd636ab4cf873786537"
        || f.filename != "gemma-4-12b-it-qat-q4_0.gguf"
        || f.bytes != 6_975_879_296
        || f.sha256 != "93567e57a8fe10b23569b9d9ec38cd005deedf71e29477c421a4b83f418a538b"
        || f.status != "literature-nominated-only; not downloaded or selected"
        || q.source_compatibility != "passed"
        || q.model_download != "size-and-sha256-verified-2026-10-23"
        || q.full_gguf_inspection != "verified-config/model.metadata.json"
        || q.cpu_loading != "pending"
        || q.daily_security != "pending"
        || q.device_performance != "pending-self-hosted-ci"
    {
        return Err(PolicyError::Configuration);
    }
    Ok(pins)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Profile {
    schema_version: u8,
    name: String,
    status: String,
    backend: String,
    context_tokens: u32,
    parallel: u32,
    batch_tokens: u32,
    microbatch_tokens: u32,
    cache_type_k: String,
    cache_type_v: String,
    context_checkpoints: u32,
    prompt_cache_mi_b: u32,
    cache_prompt: bool,
    speculation: String,
    service_ceiling_bytes: u64,
    estimated_weight_bytes: u64,
    estimated_kv_bytes: u64,
    estimated_recurrent_bytes: u64,
    estimated_runtime_headroom_bytes: u64,
    estimate_scope: String,
    launch_enabled: bool,
    remaining_gates: Vec<String>,
}
fn validate_profile(input: &str) -> Result<(), PolicyError> {
    let super::Object(p): super::Object<Profile> =
        serde_json::from_str(input).map_err(|_| PolicyError::Configuration)?;
    if p.schema_version != 1
        || p.name != "reference-cpu"
        || p.status != "development-only-unqualified"
        || p.backend != "cpu"
        || p.context_tokens != 8192
        || p.parallel != 1
        || p.batch_tokens != 256
        || p.microbatch_tokens != 128
        || p.cache_type_k != "f16"
        || p.cache_type_v != "f16"
        || p.context_checkpoints != 0
        || p.prompt_cache_mi_b != 0
        || p.cache_prompt
        || p.speculation != "none"
        || p.service_ceiling_bytes != 19_327_352_832
        || p.estimated_weight_bytes != 13_146_393_504
        || p.estimated_kv_bytes != 536_870_912
        || p.estimated_recurrent_bytes != 156_893_184
        || p.estimated_runtime_headroom_bytes != 5_487_195_232
        || p.launch_enabled
        || p.estimate_scope.trim().is_empty()
        || p.remaining_gates.is_empty()
        || p.remaining_gates.iter().any(|gate| gate.trim().is_empty())
    {
        return Err(PolicyError::Configuration);
    }
    Ok(())
}

/// Validate path spelling and existing ancestors without following redirects.
/// Native launch code additionally verifies the volume is a recognized local disk.
///
/// # Errors
/// Rejects nonabsolute, namespace, ADS, unresolved-variable, Git and reparse paths.
pub fn validate_external_path(path: &Path) -> Result<(), PolicyError> {
    let spelling = path.to_str().ok_or(PolicyError::Path)?;
    if !path.is_absolute()
        || spelling.contains('%')
        || spelling.contains('\0')
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
    {
        return Err(PolicyError::Path);
    }
    #[cfg(windows)]
    {
        use std::path::Prefix;
        let Some(Component::Prefix(prefix)) = path.components().next() else {
            return Err(PolicyError::Path);
        };
        if !matches!(prefix.kind(), Prefix::Disk(_))
            || spelling.get(2..).is_none_or(|rest| rest.contains(':'))
            || path.parent().is_none()
        {
            return Err(PolicyError::Path);
        }
    }
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) => {
                if is_redirect(&metadata) {
                    return Err(PolicyError::Path);
                }
                if metadata.is_dir() {
                    match fs::symlink_metadata(ancestor.join(".git")) {
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                        _ => return Err(PolicyError::Path),
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(PolicyError::Path),
        }
    }
    Ok(())
}

pub(crate) fn is_redirect(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compiled_configuration_is_reviewed() {
        assert!(reviewed_pins().is_ok());
        assert!(validate_profile(PROFILE_JSON).is_ok());
    }
    #[test]
    fn profile_drift_and_coercions_fail_closed() {
        for changed in [
            PROFILE_JSON.replace("\"launchEnabled\": false", "\"launchEnabled\": true"),
            PROFILE_JSON.replace("\"contextTokens\": 8192", "\"contextTokens\": \"8192\""),
            PROFILE_JSON.replace(
                "\"schemaVersion\": 1",
                "\"schemaVersion\": 1, \"extraArgs\": []",
            ),
            PROFILE_JSON.replace("\"parallel\": 1", "\"parallel\": 1, \"parallel\": 1"),
            PROFILE_JSON.replace("\"parallel\": 1", "\"parallel\": 1.0"),
        ] {
            assert_eq!(validate_profile(&changed), Err(PolicyError::Configuration));
        }
    }
    #[test]
    fn lock_drift_and_shapes_fail_closed() {
        for changed in [
            LOCK_JSON.replace("12442177", "12442178"),
            LOCK_JSON.replace("\"bytes\": 12442177", "\"bytes\": \"12442177\""),
            LOCK_JSON.replace(
                "\"schemaVersion\": 1",
                "\"schemaVersion\": 1, \"schemaVersion\": 1",
            ),
            LOCK_JSON.replace("\"model\": {", "\"model\": {\"argument\": \"--unsafe\","),
            "[]".to_owned(),
        ] {
            assert!(parse_pins(&changed).is_err());
        }
    }
}
