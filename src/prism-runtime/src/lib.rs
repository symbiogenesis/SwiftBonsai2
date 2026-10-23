//! Native Rust CPU reference execution and bounded token conversation ownership.
//!
//! This experimental engine uses the pinned Prism numerical contract. It has no
//! C/C++ inference dependency. Full-model parity, tokenizer/chat integration and
//! target-device performance qualification remain separate acceptance gates.

#![forbid(unsafe_code)]

pub mod graph;
pub mod kernels;
pub mod model;
pub mod runtime;

/// A rejected model, numerical contract, resource limit, or I/O operation.
pub type Error = Box<dyn std::error::Error + Send + Sync>;
/// A checked native runtime operation.
pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn invalid(message: impl Into<String>) -> Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, message.into()).into()
}

/// Incremental token decoder with complete, independently owned checkpoints.
///
/// An evaluation must include exactly one token in the state and return the next
/// token logits. Errors may mutate state: [`runtime::Runtime`] restores its
/// transaction checkpoint before allowing another request. A checkpoint includes
/// all recurrent/convolution/KV state and position, never references mutable
/// decoder storage, and is valid only for its originating decoder identity.
pub trait Decoder {
    /// Complete state snapshot, including the current sequence position.
    type Checkpoint;

    /// Stable model, layout and cache-contract identity.
    fn identity(&self) -> &str;
    /// Number of model output tokens.
    fn vocab_size(&self) -> usize;
    /// Conservative maximum bytes allocated when capturing the current state.
    fn checkpoint_bytes(&self) -> usize;
    /// Clear all sequence state without changing model identity.
    ///
    /// # Errors
    /// Returns an error if resetting cannot establish an empty sequence.
    fn reset(&mut self) -> Result<()>;
    /// Include one token in the sequence and compute the next-token logits.
    ///
    /// # Errors
    /// Rejects invalid tokens, exhausted context, numerical and allocation errors.
    fn evaluate(&mut self, token: u32) -> Result<Vec<f32>>;
    /// Capture every mutable component needed for exact prefix restoration.
    ///
    /// # Errors
    /// Returns an error if the snapshot cannot be allocated.
    fn checkpoint(&self) -> Result<Self::Checkpoint>;
    /// Restore a snapshot from this exact model and cache contract.
    ///
    /// # Errors
    /// Rejects foreign or malformed snapshots and allocation failures.
    fn restore(&mut self, checkpoint: &Self::Checkpoint) -> Result<()>;
}
