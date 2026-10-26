//! Shared maintenance commands for Bonsai's PowerShell entry points.
#![forbid(unsafe_code)]

/// A command result suitable for a JSON receipt or a diagnostic on stderr.
pub type Result<T> = std::result::Result<T, String>;

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

mod native_startup;
mod startup_cache;
mod startup_download;
mod startup_process;
