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
    /// Include one token and replace a caller-owned reusable logits buffer.
    ///
    /// The caller must finish consuming the previous output before this call.
    /// On success the buffer contains exactly [`Self::vocab_size`] finite logits;
    /// on error it is empty. Implementations may retain independent graph scratch,
    /// but checkpoints must never borrow either scratch or this output storage.
    /// The compatibility implementation copies the owned evaluation result;
    /// decoders can override it to write directly into the retained allocation.
    ///
    /// # Errors
    /// Returns evaluation or output-allocation errors. Decoder state may have
    /// changed on failure and requires transaction restoration before reuse.
    fn evaluate_into(&mut self, token: u32, output: &mut Vec<f32>) -> Result<()> {
        output.clear();
        let logits = self.evaluate(token)?;
        if logits.len() != self.vocab_size() || logits.iter().any(|value| !value.is_finite()) {
            return Err(invalid("native decoder returned invalid logits"));
        }
        output.try_reserve_exact(logits.len())?;
        output.extend_from_slice(&logits);
        Ok(())
    }
    /// Include one token in sequence state without retaining next-token logits.
    ///
    /// Success advances exactly the same recurrent/convolution/KV state and
    /// position as [`Self::evaluate_into`]. An override may skip output-only
    /// normalization and vocabulary projection, but must still validate the
    /// token, context bounds and every computed state value. Checkpoints must
    /// remain complete and independent of scratch storage.
    ///
    /// The compatibility implementation calls [`Self::evaluate_into`] and
    /// discards its output only after independently checking that it contains
    /// exactly [`Self::vocab_size`] finite logits. Thus an invalid output from
    /// an existing decoder remains an error even when no logits are needed.
    ///
    /// # Errors
    /// Rejects invalid tokens, exhausted context, numerical and allocation
    /// errors. State may have changed on failure and requires transaction
    /// restoration before reuse.
    fn ingest(&mut self, token: u32) -> Result<()> {
        let mut logits = Vec::new();
        self.evaluate_into(token, &mut logits)?;
        if logits.len() != self.vocab_size() || logits.iter().any(|value| !value.is_finite()) {
            return Err(invalid("native decoder returned invalid logits"));
        }
        Ok(())
    }
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
