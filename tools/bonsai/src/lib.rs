//! Shared maintenance commands for Bonsai's PowerShell entry points.
#![forbid(unsafe_code)]

/// A command result suitable for a JSON receipt or a diagnostic on stderr.
pub type Result<T> = std::result::Result<T, String>;

/// Commands exposed by the maintenance executable and its checked bootstrap receipts.
pub const COMMANDS: &[&str] = &[
    "credentials",
    "memory",
    "cpu-check-plan",
    "cpu-check-report",
    "server-policy",
    "runtime-stage",
    "runtime-verify",
    "completion",
    "qualification",
    "speech-models",
    "inference-lock",
    "inference-capabilities",
    "speech-build",
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

/// Offline runtime verification and bounded probes with retained artifact guards.
pub mod runtime;

/// Fixed qualification plans, canonical identities, and offline gate scoring.
pub mod qualification;

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
