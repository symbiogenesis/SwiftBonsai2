//! Bounded, byte-exact sharing of immutable CPU `LoRA` B vectors.
//!
//! The safe policy stores caller-owned tensor identities without dereferencing
//! them. The optional Rust native loader creates ggml views and loads payloads.
//! Signed zeroes and NaN payloads are compared as bytes, without arithmetic.

#![deny(unsafe_code)]

#[cfg(feature = "engine-loader")]
#[allow(unsafe_code)]
pub mod loader;
mod pool;

pub use pool::{Layout, MAX_CACHE_BYTES, MAX_ENTRIES, MAX_TENSOR_BYTES, Pool};
