//! Shared maintenance commands for Bonsai's PowerShell entry points.
#![forbid(unsafe_code)]

/// A command result suitable for a JSON receipt or a diagnostic on stderr.
pub type Result<T> = std::result::Result<T, String>;

/// Private credential creation through the existing audited Windows boundary.
pub mod credentials;
