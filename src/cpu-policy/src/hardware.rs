//! H0 wire contracts for runtime discovery and future qualified profiles.
//!
//! Parsing checks structure and bounds, not provenance or qualification. A
//! runtime report never authorizes execution. Live memory/power admission and
//! verification of retained evidence remain the caller's responsibility.

use serde::{Deserialize, Serialize};

/// Offline ranking of reviewed, same-workload CPU/GPU/NPU observations.
pub mod ranking;

const MAX_DOCUMENT_BYTES: usize = 16 * 1024;
const MAX_CONTRACTS: usize = 16;

/// Observed Snapdragon marketing family; this never implies SKU or ISA support.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapdragonFamily {
    /// Original Snapdragon X Elite family.
    XElite,
    /// Snapdragon X2 Elite family without the Extreme designation.
    X2Elite,
    /// Snapdragon X2 Elite Extreme family.
    X2EliteExtreme,
}

impl SnapdragonFamily {
    /// Recognize a bounded processor name while keeping X1 and X2 distinct.
    ///
    /// Trademark punctuation and an optional Qualcomm prefix are tolerated.
    /// The name is only an inventory observation, never execution eligibility.
    #[must_use]
    pub fn from_processor_name(name: &str) -> Option<Self> {
        if name.is_empty() || name.len() > 512 || name.chars().any(char::is_control) {
            return None;
        }
        let lowercase = name.to_ascii_lowercase();
        let mut words = lowercase
            .split(|character: char| !character.is_ascii_alphanumeric())
            .filter(|word| !word.is_empty() && !matches!(*word, "r" | "tm"));
        let mut word = words.next()?;
        if word == "qualcomm" {
            word = words.next()?;
        }
        if word != "snapdragon" {
            return None;
        }
        word = words.next()?;
        let generation = word;
        if words.next()? != "elite" {
            return None;
        }
        let extreme = words.next() == Some("extreme");
        match (generation, extreme) {
            ("x" | "x1", false) => Some(Self::XElite),
            ("x2", false) => Some(Self::X2Elite),
            ("x2", true) => Some(Self::X2EliteExtreme),
            _ => None,
        }
    }

    /// Exact evidence label; the legacy X1 spelling remains stable.
    #[must_use]
    pub const fn device_model(self) -> &'static str {
        match self {
            Self::XElite => "Snapdragon X Elite",
            Self::X2Elite => "Snapdragon X2 Elite",
            Self::X2EliteExtreme => "Snapdragon X2 Elite Extreme",
        }
    }

    /// Check an evidence label against the observed name when present.
    ///
    /// Only legacy X1 receipts may omit the processor observation. Newer family
    /// labels require the matching name so relabeling old evidence rejects.
    #[must_use]
    pub fn from_evidence(device_model: &str, processor_name: Option<&str>) -> Option<Self> {
        let family = match device_model {
            "Snapdragon X Elite" => Self::XElite,
            "Snapdragon X2 Elite" => Self::X2Elite,
            "Snapdragon X2 Elite Extreme" => Self::X2EliteExtreme,
            _ => return None,
        };
        let matches_observation = processor_name.map_or(family == Self::XElite, |name| {
            Self::from_processor_name(name) == Some(family)
        });
        matches_observation.then_some(family)
    }
}

fn parse<T: serde::de::DeserializeOwned>(input: &str) -> Result<T, String> {
    if input.len() > MAX_DOCUMENT_BYTES {
        return Err("Hardware contract exceeds its byte limit.".into());
    }
    serde_json::from_str(input).map_err(|_| "Invalid hardware contract.".into())
}

fn require(condition: bool) -> Result<(), String> {
    condition.ok_or_else(|| "Unsupported or incomplete hardware contract.".into())
}

fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 192
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':' | b'+')
        })
}

fn concrete_identifier(value: &str) -> bool {
    identifier(value) && !value.eq_ignore_ascii_case("unknown")
}

/// Exact compiler target facts reported by the runtime, not host SKU inference.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RuntimeTarget {
    architecture: String,
    os: String,
    environment: String,
}

impl RuntimeTarget {
    fn validate(&self) -> Result<(), String> {
        require(
            identifier(&self.architecture) && identifier(&self.os) && identifier(&self.environment),
        )
    }

    /// Whether this is the native Windows ARM64 MSVC target required by Runner.
    ///
    /// This does not establish host ISA eligibility or artifact trust.
    #[must_use]
    pub fn is_windows_arm64(&self) -> bool {
        self.architecture == "aarch64" && self.os == "windows" && self.environment == "msvc"
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct RuntimeIdentity {
    name: String,
    version: String,
    target: RuntimeTarget,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct CpuCapabilities {
    scalar: bool,
    neon_build_eligible: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum BackendAvailability {
    Unavailable,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Backends {
    vulkan: BackendAvailability,
    hexagon: BackendAvailability,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum DiscoveryQualification {
    Unqualified,
}

/// Checked v1 Prism report; claims describe support and never device acceptance.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct RuntimeReport {
    schema_version: u32,
    kind: String,
    runtime: RuntimeIdentity,
    cpu: CpuCapabilities,
    backends: Backends,
    supported_contracts: Vec<String>,
    qualification: DiscoveryQualification,
}

/// Structurally checked runtime discovery, without any qualification verdict.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeCapabilities(RuntimeReport);

impl RuntimeCapabilities {
    /// Parse a bounded report with exact fields, types and schema identity.
    ///
    /// # Errors
    /// Rejects malformed/oversized reports, unknown fields or versions and
    /// contradictory v1 CPU claims. It does not verify the reporting binary.
    pub fn from_json(input: &str) -> Result<Self, String> {
        let report: RuntimeReport = parse(input)?;
        require(report.schema_version == 1 && report.kind == "prism-runtime-capabilities")?;
        require(report.runtime.name == "prism-inference" && identifier(&report.runtime.version))?;
        report.runtime.target.validate()?;
        require(report.cpu.scalar)?;
        require(!report.cpu.neon_build_eligible || report.runtime.target.is_windows_arm64())?;
        require(
            !report.supported_contracts.is_empty()
                && report.supported_contracts.len() <= MAX_CONTRACTS,
        )?;
        for (index, contract) in report.supported_contracts.iter().enumerate() {
            require(
                identifier(contract) && !report.supported_contracts[..index].contains(contract),
            )?;
        }
        Ok(Self(report))
    }

    /// Compiler target used to build the reporting executable.
    #[must_use]
    pub const fn target(&self) -> &RuntimeTarget {
        &self.0.runtime.target
    }

    /// Whether the runtime's existing compile-time NEON guard admits this build.
    ///
    /// A true result is neither a host ISA probe nor numerical qualification.
    #[must_use]
    pub const fn neon_build_eligible(&self) -> bool {
        self.0.cpu.neon_build_eligible
    }

    /// Whether the runtime declares the exact graph contract string.
    ///
    /// Model-specific contracts still require the matching verified model.
    #[must_use]
    pub fn declares_contract(&self, contract: &str) -> bool {
        self.0
            .supported_contracts
            .iter()
            .any(|value| value == contract)
    }
}

/// Measured objective for a future profile; ordinary startup does not tune it.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Objective {
    /// Minimize complete-task latency within the qualified constraints.
    Interactive,
    /// Minimize measured energy within the qualified latency constraints.
    Battery,
}

/// Explicit absence differs from a missing or unknown adapter identity.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum AdapterIdentity {
    /// The profile deliberately executes without an adapter.
    None,
    /// The exact verified adapter bytes are part of compatibility.
    Artifact {
        /// Lowercase SHA-256 of the complete adapter artifact.
        sha256: String,
    },
}

impl<'de> Deserialize<'de> for AdapterIdentity {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // Serde's internally tagged unit variants otherwise ignore extra
        // fields, which would conflate explicit absence and an unknown hash.
        let wire = AdapterWire::deserialize(deserializer)?;
        match (wire.kind.as_str(), wire.sha256) {
            ("none", None) => Ok(Self::None),
            ("artifact", Some(sha256)) => Ok(Self::Artifact { sha256 }),
            _ => Err(serde::de::Error::custom("Invalid adapter identity.")),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AdapterWire {
    kind: String,
    #[serde(default, deserialize_with = "present_digest")]
    sha256: Option<String>,
}

fn present_digest<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    // Absent is distinct from an explicitly null/ill-typed digest. The derived
    // wire parser also rejects duplicate kind/hash fields before projection.
    String::deserialize(deserializer).map(Some)
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct RuntimeBuild {
    archive_sha256: String,
    source_revision: String,
    target: RuntimeTarget,
}

/// State treatment is part of a workload's retained evidence identity.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum StateUse {
    /// Empty state with the complete prompt ingested for this request.
    Fresh,
    /// Qualified restoration of the complete matching hybrid state.
    Reused,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Workload {
    context_tokens: u32,
    minimum_prompt_tokens: u32,
    maximum_prompt_tokens: u32,
    maximum_output_tokens: u32,
    state: StateUse,
}

/// Immutable identity for future profile evidence, separate from live budgets.
///
/// The capability-class digest excludes timestamps, tool paths, changing free
/// memory and power snapshots. Exact model bytes cover derived exports too;
/// settings bind placement, threads, batching, precision and migration policy.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct ProfileKey {
    schema_version: u32,
    objective: Objective,
    model_sha256: String,
    adapter: AdapterIdentity,
    runtime: RuntimeBuild,
    capability_class_sha256: String,
    backend_driver_sha256: String,
    graph_contract: String,
    state_layout: String,
    settings_sha256: String,
    workload: Workload,
}

/// Validated immutable profile identity. Parsing does not verify provenance.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct ProfileCompatibilityKey(ProfileKey);

impl ProfileCompatibilityKey {
    /// Parse a concrete key. Unknown identities must remain outside a key.
    ///
    /// # Errors
    /// Rejects unknown fields/versions, missing identities, invalid digests and
    /// workload bounds that cannot reserve the entire declared output.
    pub fn from_json(input: &str) -> Result<Self, String> {
        let key: ProfileKey = parse(input)?;
        key.validate()?;
        Ok(Self(key))
    }
}

impl ProfileKey {
    fn validate(&self) -> Result<(), String> {
        require(self.schema_version == 1)?;
        for digest in [
            &self.model_sha256,
            &self.runtime.archive_sha256,
            &self.capability_class_sha256,
            &self.backend_driver_sha256,
            &self.settings_sha256,
        ] {
            require(super::hex(digest, 64))?;
        }
        if let AdapterIdentity::Artifact { sha256 } = &self.adapter {
            require(super::hex(sha256, 64))?;
        }
        require(super::hex(&self.runtime.source_revision, 40))?;
        self.runtime.target.validate()?;
        require(
            concrete_identifier(&self.runtime.target.architecture)
                && concrete_identifier(&self.runtime.target.os)
                && concrete_identifier(&self.runtime.target.environment)
                && concrete_identifier(&self.graph_contract)
                && concrete_identifier(&self.state_layout),
        )?;
        let workload = &self.workload;
        require(
            workload.minimum_prompt_tokens > 0
                && workload.minimum_prompt_tokens <= workload.maximum_prompt_tokens
                && workload.maximum_output_tokens > 0
                && workload
                    .maximum_prompt_tokens
                    .checked_add(workload.maximum_output_tokens)
                    .is_some_and(|total| total <= workload.context_tokens),
        )
    }
}

/// Retained outcome; absence of a verdict is represented explicitly.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Verdict {
    /// Evidence is missing or has not been reviewed.
    Unknown,
    /// The declared evidence scope passed review.
    Passed,
    /// The declared evidence scope failed review.
    Failed,
    /// Retained evidence rules out this candidate for its declared scope.
    NoGo,
}

/// Evidence reference bound to one exact compatibility key and tested revision.
///
/// This schema is a receipt contract, not an attestation verifier or selector.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Evidence {
    schema_version: u32,
    compatibility: ProfileKey,
    tested_main_revision: String,
    ci_run_id: String,
    capture_sha256: String,
    verdict: Verdict,
    energy_verdict: Verdict,
}

/// Validated evidence receipt structure, without trusting its assertions.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct ProfileEvidence(Evidence);

impl ProfileEvidence {
    /// Parse a receipt without treating its assertions as trusted qualification.
    ///
    /// # Errors
    /// Rejects invalid identities and a claimed battery pass without an energy
    /// pass. Run existence, main ancestry, signatures and captures need review.
    pub fn from_json(input: &str) -> Result<Self, String> {
        let evidence: Evidence = parse(input)?;
        require(evidence.schema_version == 1)?;
        evidence.compatibility.validate()?;
        require(
            super::hex(&evidence.tested_main_revision, 40)
                && super::hex(&evidence.capture_sha256, 64)
                && !evidence.ci_run_id.is_empty()
                && evidence.ci_run_id.len() <= 32
                && evidence.ci_run_id.bytes().all(|byte| byte.is_ascii_digit()),
        )?;
        require(
            evidence.compatibility.objective != Objective::Battery
                || evidence.verdict != Verdict::Passed
                || evidence.energy_verdict == Verdict::Passed,
        )?;
        Ok(Self(evidence))
    }

    /// Exact identity match, independent of the receipt's asserted verdict.
    ///
    /// A match alone never authorizes a profile or replaces evidence review.
    #[must_use]
    pub fn applies_to(&self, current: &ProfileCompatibilityKey) -> bool {
        self.0.compatibility == current.0
    }
}

/// Value-free diagnostics reserved for future fallback and strict requests.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum FallbackReason {
    /// A required capability could not be observed.
    UnknownCapability,
    /// The required backend is unavailable.
    UnavailableBackend,
    /// Runtime bytes or compiler target differ from retained evidence.
    IncompatibleRuntime,
    /// Driver/backend compatibility differs from retained evidence.
    IncompatibleDriver,
    /// Retained evidence does not apply to the current immutable key.
    StaleEvidence,
    /// Current resource admission cannot reserve the full declared workload.
    InsufficientMemory,
    /// An explicit strict-backend requirement cannot be met.
    StrictBackendUnavailable,
}

#[cfg(test)]
mod tests;
