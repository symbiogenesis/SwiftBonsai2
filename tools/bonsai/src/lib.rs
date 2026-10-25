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

/// Ordinary Prism source partitioning and bounded revision-bound result merging.
pub mod prism_source;

/// Catalog identities, complete source reports, and nonregression baseline gates.
pub mod prism_baseline;

/// Offline runtime archive staging through the shared artifact policy.
pub mod install;
