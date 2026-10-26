//! Reviewed development-only backend policies compiled into the broker.
//!
//! This plan is not permission to launch and does not qualify the daily profile.

use serde::Deserialize;
use std::{
    ffi::OsString,
    fmt,
    path::{Path, PathBuf},
};

const LOCK_JSON: &str = include_str!("../../../config/runtime.lock.json");
const PROFILE_JSON: &str = include_str!("../../../config/profiles/reference-cpu.json");
const SWIFT_LOCK_JSON: &str = include_str!("../../../config/swift-assessment.lock.json");
const PTQ1_EXPORT_JSON: &str = include_str!("../../../config/swift.ptq1-export.json");
/// Fixed process memory ceiling: 18 GiB.
pub const MEMORY_LIMIT_BYTES: usize = bonsai_cpu_policy::MEMORY_LIMIT_BYTES;
/// Required available physical memory including a 4 GiB host reserve.
pub const REQUIRED_AVAILABLE_BYTES: u64 = bonsai_cpu_policy::REQUIRED_AVAILABLE_BYTES;

/// The only reviewed development model/runtime selections. Neither qualifies daily use.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LaunchProfile {
    /// Default Qwen reference; its compiled JSON profile remains launch-disabled.
    #[default]
    ReferenceCpu,
    /// Explicit Swift PQ2 assessment, optionally with the pinned abliteration `LoRA`.
    SwiftPq2Assessment,
}
impl LaunchProfile {
    /// Exact command-line name of the compiled selection.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::ReferenceCpu => "reference-cpu",
            Self::SwiftPq2Assessment => "swift-pq2-assessment",
        }
    }

    /// Job-wide committed-memory ceiling; the physical reserve is additional.
    #[must_use]
    pub const fn memory_limit_bytes(self) -> usize {
        self.cpu_profile().memory_limit_bytes()
    }

    /// Physical admission threshold, including a separate 4 GiB host reserve.
    #[must_use]
    pub const fn required_available_bytes(self) -> u64 {
        self.cpu_profile().required_available_bytes()
    }

    const fn cpu_profile(self) -> bonsai_cpu_policy::Profile {
        match self {
            Self::ReferenceCpu => bonsai_cpu_policy::Profile::ReferenceCpu,
            Self::SwiftPq2Assessment => bonsai_cpu_policy::Profile::SwiftPq2Assessment,
        }
    }
}
impl std::str::FromStr for LaunchProfile {
    type Err = PolicyError;
    fn from_str(name: &str) -> Result<Self, Self::Err> {
        match name {
            "reference-cpu" => Ok(Self::ReferenceCpu),
            "swift-pq2-assessment" => Ok(Self::SwiftPq2Assessment),
            _ => Err(PolicyError::Configuration),
        }
    }
}

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
        let name = name.to_str().ok_or(PolicyError::Environment)?;
        if bonsai_cpu_policy::server::forbidden_environment_name(name) {
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
    /// Optional pinned adapter identity; never an arbitrary caller-supplied path.
    pub adapter: Option<bonsai_cpu_policy::AbliterateLoraArtifact>,
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
        Self::for_profile(data, backend_port, key_file, LaunchProfile::ReferenceCpu)
    }

    /// Build one compiled development plan; no model, runtime or argument overrides exist.
    ///
    /// # Errors
    /// Rejects invalid paths, ports, inherited overrides or compiled policy drift.
    pub fn for_profile(
        data: &Path,
        backend_port: u16,
        key_file: &Path,
        profile: LaunchProfile,
    ) -> Result<Self, PolicyError> {
        Self::for_profile_with_lora(data, backend_port, key_file, profile, false)
    }

    /// Build a fixed server plan with the pinned scale-one `LoRA` when requested.
    ///
    /// # Errors
    /// Rejects `LoRA` on the reference profile, invalid paths or compiled policy drift.
    pub fn for_profile_with_lora(
        data: &Path,
        backend_port: u16,
        key_file: &Path,
        profile: LaunchProfile,
        use_abliterate_lora: bool,
    ) -> Result<Self, PolicyError> {
        Self::for_profile_with_options(
            data,
            backend_port,
            key_file,
            profile,
            use_abliterate_lora,
            false,
        )
    }

    /// Build a fixed server plan with explicit pinned adapter and weight packing selections.
    /// The PTQ1 option selects only the verified current-weight export, never the older checkpoint.
    ///
    /// # Errors
    /// Rejects Swift-only options on the reference profile, invalid paths or compiled policy drift.
    pub fn for_profile_with_options(
        data: &Path,
        backend_port: u16,
        key_file: &Path,
        profile: LaunchProfile,
        use_abliterate_lora: bool,
        use_repacked_ptq1: bool,
    ) -> Result<Self, PolicyError> {
        if use_abliterate_lora && profile != LaunchProfile::SwiftPq2Assessment {
            return Err(PolicyError::Configuration);
        }
        validate_environment(std::env::vars_os().map(|(name, _)| name))?;
        let pins = reviewed_pins_for_representation(profile, use_repacked_ptq1)?;
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
        let mut arguments: Vec<OsString> = bonsai_cpu_policy::server::arguments(
            model_path.to_str().ok_or(PolicyError::Path)?,
            key_file.to_str().ok_or(PolicyError::Path)?,
            backend_port,
            profile.cpu_profile(),
        )
        .map_err(|_| PolicyError::Configuration)?
        .into_iter()
        .map(OsString::from)
        .collect();
        let adapter = if use_abliterate_lora {
            let adapter = bonsai_cpu_policy::abliterate_lora_artifact(data)
                .map_err(|_| PolicyError::Configuration)?;
            validate_external_path(&adapter.path)?;
            // --lora applies scale 1 and accepts an absolute Windows path as one
            // argument; --lora-scaled would split the drive-letter colon.
            arguments.extend([
                OsString::from("--lora"),
                adapter.path.as_os_str().to_owned(),
            ]);
            Some(adapter)
        } else {
            None
        };
        Ok(Self {
            executable,
            arguments,
            model_path,
            adapter,
            archive_path,
            runtime_directory,
            key_file: key_file.to_path_buf(),
            required_available_bytes: profile.required_available_bytes(),
            memory_limit_bytes: profile.memory_limit_bytes(),
        })
    }
}

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

pub(crate) struct ReviewedPins {
    pub(crate) model: ModelArtifact,
    pub(crate) runtime: super::Object<RuntimePin>,
}
pub(crate) struct ModelArtifact {
    pub(crate) revision: String,
    pub(crate) filename: String,
    pub(crate) bytes: u64,
    pub(crate) sha256: String,
}

pub(crate) fn reviewed_pins(profile: LaunchProfile) -> Result<ReviewedPins, PolicyError> {
    match profile {
        LaunchProfile::ReferenceCpu => {
            let pins = parse_pins(LOCK_JSON)?;
            validate_profile(PROFILE_JSON)?;
            Ok(ReviewedPins {
                model: ModelArtifact {
                    revision: pins.model.0.revision,
                    filename: pins.model.0.filename,
                    bytes: pins.model.0.bytes,
                    sha256: pins.model.0.sha256,
                },
                runtime: pins.runtime,
            })
        }
        LaunchProfile::SwiftPq2Assessment => parse_swift_pins(SWIFT_LOCK_JSON),
    }
}

pub(crate) fn reviewed_pins_for_representation(
    profile: LaunchProfile,
    use_repacked_ptq1: bool,
) -> Result<ReviewedPins, PolicyError> {
    let mut pins = reviewed_pins(profile)?;
    if use_repacked_ptq1 {
        let lock = serde_json::from_str(SWIFT_LOCK_JSON).map_err(|_| PolicyError::Configuration)?;
        let evidence =
            serde_json::from_str(PTQ1_EXPORT_JSON).map_err(|_| PolicyError::Configuration)?;
        let selected =
            bonsai_cpu_policy::select_artifact(&lock, profile.cpu_profile(), true, Some(&evidence))
                .map_err(|_| PolicyError::Configuration)?;
        let selected: SelectedModel =
            serde_json::from_value(selected).map_err(|_| PolicyError::Configuration)?;
        pins.model.filename = selected.filename;
        pins.model.bytes = selected.bytes;
        pins.model.sha256 = selected.sha256;
    }
    Ok(pins)
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase", deny_unknown_fields)]
struct SelectedModel {
    filename: String,
    bytes: u64,
    sha256: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SwiftPins {
    schema_version: u8,
    decision: String,
    model: super::Object<SwiftModelPin>,
    runtime: super::Object<RuntimePin>,
    qualification: super::Object<SwiftQualification>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SwiftModelPin {
    repository: String,
    revision: String,
    filename: String,
    bytes: u64,
    sha256: String,
    digest_source: String,
    license: String,
    architecture: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SwiftQualification {
    purpose: String,
    model_download: String,
    full_gguf_inspection: String,
    ternary_symbol_census: String,
    cpu_loading: String,
    daily_security: String,
    device_performance: String,
}

fn parse_swift_pins(input: &str) -> Result<ReviewedPins, PolicyError> {
    let super::Object(pins): super::Object<SwiftPins> =
        serde_json::from_str(input).map_err(|_| PolicyError::Configuration)?;
    let m = &pins.model;
    let r = &pins.runtime;
    let q = &pins.qualification;
    if pins.schema_version != 1
        || pins.decision != "docs/swift-memory.md"
        || m.repository != "ukisai/Swift-Bonsai-2-GGUF"
        || m.revision != "a3bdac086bbb6b04d87d0108d18943b5f63868f7"
        || m.filename != "Swift-Bonsai-2-PQ2_0.gguf"
        || m.bytes != 7_206_168_928
        || m.sha256 != "5912bb739217cf25b4283a098c7b893d9baff36bdf2e72d3dbf4cb999d5633d5"
        || m.digest_source
            != "https://huggingface.co/api/models/ukisai/Swift-Bonsai-2-GGUF/revision/a3bdac086bbb6b04d87d0108d18943b5f63868f7?blobs=true"
        || m.license != "Apache-2.0"
        || m.architecture != "qwen35"
        || r.repository != "https://github.com/PrismML-Eng/llama.cpp.git"
        || r.commit != "73bfe6cc485554822f80c473a3c0c4f25e80f41b"
        || r.release != "prism-b10787-73bfe6c"
        || r.license != "MIT"
        || r.asset.filename != "llama-prism-b10787-73bfe6c-bin-win-cpu-arm64.zip"
        || r.asset.bytes != 12_412_126
        || r.asset.sha256 != "cc808b33d35c72a3bd5c69a0d609d422426feb9dce23868e77992b6c33b8ddb3"
        || r.asset.url
            != "https://github.com/PrismML-Eng/llama.cpp/releases/download/prism-b10787-73bfe6c/llama-prism-b10787-73bfe6c-bin-win-cpu-arm64.zip"
        || r.asset.digest_source
            != "https://api.github.com/repos/PrismML-Eng/llama.cpp/releases/tags/prism-b10787-73bfe6c"
        || q.purpose != "owner-prioritized Swift memory and exact ternary repacking assessment"
        || q.model_download != "verified-size-and-sha256-2026-10-23"
        || q.full_gguf_inspection != "config/swift.metadata.json"
        || q.ternary_symbol_census != "config/swift.pq2-census.json"
        || q.cpu_loading
            != "bounded-16-token-native-decode-completed; PQ2/current-repacked-PTQ1 with and without approved adapter; quality and TPS unqualified"
        || q.daily_security != "not-qualified"
        || q.device_performance != "pending-self-hosted-ci"
    {
        return Err(PolicyError::Configuration);
    }
    Ok(ReviewedPins {
        model: ModelArtifact {
            revision: pins.model.0.revision,
            filename: pins.model.0.filename,
            bytes: pins.model.0.bytes,
            sha256: pins.model.0.sha256,
        },
        runtime: pins.runtime,
    })
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
        || r.commit != "36e36e63a54fc8f85b95e9c499a022b597e478dd"
        || r.release != "b11840"
        || r.license != "MIT"
        || r.asset.filename != "llama-b11840-bin-win-cpu-arm64.zip"
        || r.asset.bytes != 12_781_980
        || r.asset.sha256 != "9bfdb90329cd190c8286e5e53ed74391f8455c7cf7a6370725817440dd7ff085"
        || r.asset.url
            != "https://github.com/ggml-org/llama.cpp/releases/download/b11840/llama-b11840-bin-win-cpu-arm64.zip"
        || r.asset.digest_source
            != "https://api.github.com/repos/ggml-org/llama.cpp/releases/tags/b11840"
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
    bonsai_artifacts::validate_external_path(path).map_err(|_| PolicyError::Path)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compiled_configuration_is_reviewed() {
        assert!(reviewed_pins(LaunchProfile::ReferenceCpu).is_ok());
        assert!(reviewed_pins(LaunchProfile::SwiftPq2Assessment).is_ok());
        assert!(validate_profile(PROFILE_JSON).is_ok());
    }

    #[test]
    fn repacked_identity_is_the_exact_current_weight_export_not_the_older_checkpoint()
    -> Result<(), PolicyError> {
        let profile = LaunchProfile::SwiftPq2Assessment;
        let original = reviewed_pins_for_representation(profile, false)?;
        let repacked = reviewed_pins_for_representation(profile, true)?;
        assert_eq!(original.model.filename, "Swift-Bonsai-2-PQ2_0.gguf");
        assert_eq!(
            repacked.model.filename,
            "Swift-Bonsai-2-current-repacked-PTQ1_0.gguf"
        );
        assert_eq!(repacked.model.bytes, 5_946_648_928);
        assert_eq!(
            repacked.model.sha256,
            "e658f0f0736a03a4549535a439f9b07254ebf8ab88ae27cbe6e52d098ca5d1f5"
        );
        assert_eq!(repacked.model.revision, original.model.revision);
        assert_eq!(repacked.runtime.commit, original.runtime.commit);
        assert!(reviewed_pins_for_representation(LaunchProfile::ReferenceCpu, true).is_err());
        Ok(())
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
            LOCK_JSON.replace("12781980", "12781981"),
            LOCK_JSON.replace("\"bytes\": 12781980", "\"bytes\": \"12781980\""),
            LOCK_JSON.replace(
                "\"schemaVersion\": 1",
                "\"schemaVersion\": 1, \"schemaVersion\": 1",
            ),
            LOCK_JSON.replace("\"model\": {", "\"model\": {\"argument\": \"--unsafe\","),
            "[]".to_owned(),
        ] {
            assert_ne!(changed, LOCK_JSON, "lock mutation must change the fixture");
            assert!(parse_pins(&changed).is_err());
        }
    }

    #[test]
    fn swift_lock_drift_and_shapes_fail_closed() {
        for changed in [
            SWIFT_LOCK_JSON.replace("7206168928", "7206168929"),
            SWIFT_LOCK_JSON.replace("12412126", "12412127"),
            SWIFT_LOCK_JSON.replace("PQ2_0.gguf", "PTQ1_0.gguf"),
            SWIFT_LOCK_JSON.replace("ukisai/Swift-Bonsai-2-GGUF", "other/Swift-Bonsai-2-GGUF"),
            SWIFT_LOCK_JSON.replace("not-qualified", "passed"),
            SWIFT_LOCK_JSON.replace(
                "73bfe6cc485554822f80c473a3c0c4f25e80f41b",
                "36e36e63a54fc8f85b95e9c499a022b597e478dd",
            ),
            SWIFT_LOCK_JSON.replace("\"bytes\": 7206168928", "\"bytes\": \"7206168928\""),
            SWIFT_LOCK_JSON.replace("\"bytes\": 7206168928", "\"bytes\": 7206168928.0"),
            SWIFT_LOCK_JSON.replace(
                "\"schemaVersion\": 1",
                "\"schemaVersion\": 1, \"schemaVersion\": 1",
            ),
            SWIFT_LOCK_JSON.replace("\"model\": {", "\"model\": {\"lora\": \"arbitrary\","),
            SWIFT_LOCK_JSON.replace("\"runtime\": {", "\"runtime\": {\"arguments\": [],"),
            SWIFT_LOCK_JSON.replace(
                "\"qualification\": {",
                "\"qualification\": {\"unknown\": true,",
            ),
            LOCK_JSON.to_owned(),
            "[]".to_owned(),
        ] {
            assert_ne!(changed, SWIFT_LOCK_JSON, "mutation must change the fixture");
            assert!(parse_swift_pins(&changed).is_err());
        }
        assert!(parse_pins(SWIFT_LOCK_JSON).is_err());
    }
}
