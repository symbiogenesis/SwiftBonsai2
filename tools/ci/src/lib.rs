//! Rust-only workspace impact planning and aggregate CI acceptance.
#![forbid(unsafe_code)]

pub mod command;
pub mod gate;
pub mod impact;

/// A CI operation's diagnostic result.
pub type Result<T> = std::result::Result<T, String>;
