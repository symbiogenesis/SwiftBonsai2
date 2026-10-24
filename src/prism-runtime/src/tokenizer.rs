//! Bounded plain-text encoding for the experimental GPT-2/Qwen35 reference.
//!
//! This composes explicit special-token handling, ordinary-text pretokenization
//! and ranked byte-pair merges. It never applies a chat template or inserts
//! BOS/EOS; callers supply the complete text they intend to encode.

use bonsai_gguf::Metadata;

use crate::bpe::{Bpe, MAX_BPE_INPUT_BYTES, MAX_BPE_INPUT_PIECES};
use crate::pretokenizer::Qwen35PreTokenizer;
use crate::special_input::{InputSegment, SpecialInput, SpecialInputPolicy};
use crate::{Result, invalid};

/// Validated, bounded plain-text encoder without implicit template or insertion.
///
/// Special spellings become token IDs only with [`SpecialInputPolicy::Parse`].
/// Literal mode sends every byte through ordinary pretokenization and BPE.
/// Model-declared insertion flags are not acted on by this interface; exact
/// caller-provided text is the complete input contract.
#[derive(Debug)]
pub struct Tokenizer {
    bpe: Bpe,
    pretokenizer: Qwen35PreTokenizer,
    specials: SpecialInput,
}

impl Tokenizer {
    /// Validate vocabulary, ordered merges and the exact tokenizer identity.
    ///
    /// # Errors
    /// Rejects malformed or unsupported tokenizer metadata, inconsistent graph
    /// vocabulary size, resource limits and allocation failures.
    pub fn from_metadata(metadata: &Metadata, vocab_size: usize) -> Result<Self> {
        Ok(Self {
            bpe: Bpe::from_metadata(metadata, vocab_size)?,
            pretokenizer: Qwen35PreTokenizer::from_metadata(metadata)?,
            specials: SpecialInput::from_metadata(metadata, vocab_size)?,
        })
    }

    /// Encode complete UTF-8 text with an explicit special-token policy.
    ///
    /// At most 1 MiB of input and 65,536 combined special tokens and ordinary
    /// pretokenizer pieces are admitted across the entire request. Ordinary
    /// pieces additionally obey BPE's 64 KiB piece limit. Merges never cross
    /// pretokenizer or recognized special-token boundaries. Empty input returns
    /// no IDs, including with a zero output allowance.
    ///
    /// # Errors
    /// Rejects input/piece/output bounds, missing normal byte tokens and
    /// allocation failures. Errors return no partial token output.
    pub fn encode(
        &self,
        text: &str,
        max_tokens: usize,
        policy: SpecialInputPolicy,
    ) -> Result<Vec<u32>> {
        if text.len() > MAX_BPE_INPUT_BYTES {
            return Err(invalid("tokenizer input exceeds byte limit"));
        }
        let segments = self.specials.split(text, policy, MAX_BPE_INPUT_PIECES)?;
        let mut output = Vec::new();
        output.try_reserve_exact(text.len().min(max_tokens))?;
        let mut piece_count = 0_usize;
        for segment in segments {
            match segment {
                InputSegment::Special(token) => {
                    charge_pieces(&mut piece_count, 1)?;
                    if output.len() == max_tokens {
                        return Err(invalid("tokenizer output exceeds token limit"));
                    }
                    output.push(token);
                }
                InputSegment::Text(text) => {
                    let pieces = self.pretokenizer.split(text)?;
                    charge_pieces(&mut piece_count, pieces.len())?;
                    for piece in pieces {
                        let encoded = self
                            .bpe
                            .encode_piece(piece.as_bytes(), max_tokens - output.len())?;
                        output.extend_from_slice(&encoded);
                    }
                }
            }
        }
        Ok(output)
    }
}

fn charge_pieces(total: &mut usize, count: usize) -> Result<()> {
    *total = total
        .checked_add(count)
        .filter(|&value| value <= MAX_BPE_INPUT_PIECES)
        .ok_or_else(|| invalid("tokenizer total piece count exceeds limit"))?;
    Ok(())
}
