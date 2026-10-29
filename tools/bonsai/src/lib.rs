//! Shared maintenance commands for Bonsai's PowerShell entry points.
#![forbid(unsafe_code)]

/// A command result suitable for a JSON receipt or a diagnostic on stderr.
pub type Result<T> = std::result::Result<T, String>;

/// Commands exposed by the maintenance executable and its checked bootstrap receipts.
pub const COMMANDS: &[&str] = &[
    "credentials",
    "memory",
    "host-cpu",
    "cpu-evaluation",
    "cpu-check-plan",
    "cpu-check-report",
    "server-policy",
    "runtime-stage",
    "runtime-verify",
    "completion",
    "qualification",
    "runtime-promotion",
    "speech-models",
    "inference-lock",
    "inference-capabilities",
    "speech-build",
    "speech-evaluation",
    "source-build",
    "chat-launch",
    "chat-prepare",
    "chat-package",
];

/// Returns the native maintenance protocol capabilities without reading a request.
#[must_use]
pub fn version() -> serde_json::Value {
    serde_json::json!({
        "Name": "bonsai-tools", "Version": env!("CARGO_PKG_VERSION"),
        "ProtocolVersion": 5, "StartupProtocolVersion": 1,
        "BootstrapProtocolVersion": 1, "ChatLaunchProtocolVersion": 1, "Commands": COMMANDS
    })
}

/// Private credential creation through the existing audited Windows boundary.
pub mod credentials;

/// Bounded completion supervision with retained process and artifact ownership.
pub mod completion;

/// Model-free native CPU topology and current-process availability observations.
pub mod host_cpu;

/// Bounded paired CPU tuning plans for the X2 Elite Extreme CI lane.
pub mod cpu_evaluation;

/// Offline runtime verification and bounded probes with retained artifact guards.
pub mod runtime;

/// Fixed qualification plans, canonical identities, and offline gate scoring.
pub mod qualification;

/// Exact standard-profile promotion policy and offline evidence completeness review.
pub mod runtime_promotion;

/// Pure server preflight policy shared with the development broker.
pub mod server;

/// Offline runtime archive staging through the shared artifact policy.
pub mod install;

/// Native speech artifact plans shared with the provisioning wrappers.
pub mod speech;

/// Offline review of a release receipt, exact source ZIP and ARM64 bundle.
pub mod inference_lock;

/// Model-free capability discovery from the exact pinned inference release.
pub mod inference_capabilities;

/// Native speech build locks and bounded source archive staging.
pub mod speech_build;

/// CI-only paired speech plans and bounded native memory-capture sessions.
pub mod speech_evaluation;

/// Checked source builds for the maintenance tool and native chat executables.
pub mod source_build;

/// Native chat validation, provisioning and launch orchestration.
pub mod chat_launch;

/// Installed native chat package selection and integrity checks.
pub mod chat_package;

/// Retained caller identity for cancellation of native startup work.
pub mod startup_cancel;

mod identity;
mod native_startup;
#[cfg(windows)]
mod runtime_probe;
mod startup_cache;
mod startup_download;
mod startup_process;
