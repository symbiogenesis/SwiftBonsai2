//! Ranked GPT-2 byte-pair merges for **already pretokenized byte pieces**.
//!
//! This is only the merge core. It does not implement the Qwen35 regex splitter,
//! chat templates, special-token recognition, or an arbitrary-text tokenizer.
//! Callers must supply the exact pretokenizer boundaries: merges never cross
//! those boundaries. Only normal vocabulary IDs can be emitted.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

use bonsai_gguf::{Metadata, MetadataArray};

use crate::vocabulary::{MAX_PIECE_BYTES, Vocabulary};
use crate::{Result, invalid};

/// Maximum number of ordered merge rules admitted from metadata.
pub const MAX_BPE_MERGES: usize = 1_000_000;
/// Maximum aggregate UTF-8 bytes in serialized merge rules.
pub const MAX_BPE_MERGE_BYTES: usize = 64 * 1024 * 1024;
/// Maximum bytes in one already pretokenized input piece.
pub const MAX_BPE_PIECE_BYTES: usize = 64 * 1024;
/// Maximum total input bytes admitted by [`Bpe::encode_pieces`].
pub const MAX_BPE_INPUT_BYTES: usize = 1024 * 1024;
/// Maximum pretokenized pieces admitted in one call, including empty pieces.
pub const MAX_BPE_INPUT_PIECES: usize = 64 * 1024;

#[derive(Clone, Copy, Debug)]
struct Merge {
    rank: usize,
    token: u32,
}

#[derive(Debug)]
struct Node {
    token: u32,
    previous: Option<usize>,
    next: Option<usize>,
    alive: bool,
}

// Rank first, original left position second: equal-ranked overlapping pairs
// deterministically merge at the leftmost surviving position.
type Candidate = (usize, usize, usize, u32, u32, u32);

/// Bounded normal-token BPE rules, independent of pretokenization and templates.
///
/// Every byte starts as its normal single-byte vocabulary ID. At each step the
/// lowest-ranked adjacent rule wins, with leftmost ties. Multi-byte vocabulary
/// entries without a matching merge path are never selected by longest match.
#[derive(Debug)]
pub struct Bpe {
    bytes: [Option<u32>; 256],
    merges: HashMap<(u32, u32), Merge>,
}

impl Bpe {
    /// Validates the pinned `gpt2` / `qwen35` vocabulary and ordered merge array.
    ///
    /// `tokenizer.ggml.merges` must be a GGUF `STRING` array. Each entry is two
    /// nonempty serialized GPT-2 alphabet pieces separated by exactly one ASCII
    /// space. Both operands and their concatenation must be normal vocabulary
    /// tokens. Array position is the rank; duplicate pairs and duplicate normal
    /// vocabulary spellings are rejected. An empty merge array is valid.
    ///
    /// Missing normal single-byte tokens are allowed at construction, but any
    /// input using such a byte fails. Control, user-defined and byte-type tokens
    /// cannot participate in ordinary byte encoding, even with matching text.
    /// Metadata bounds are checked before allocating merge storage. Retained
    /// storage is a 256-entry byte table and at most [`MAX_BPE_MERGES`] rules.
    /// Construction additionally uses the bounded [`Vocabulary`] allocation, a
    /// borrowed normal-token lookup table, and one concatenated rule string of
    /// at most `2 * MAX_PIECE_BYTES` UTF-8 bytes (the alphabet uses at most two
    /// UTF-8 bytes per decoded byte).
    ///
    /// # Errors
    /// Rejects invalid vocabulary metadata, malformed or missing merges, missing
    /// normal operands/results, duplicates, resource limits and allocation errors.
    pub fn from_metadata(metadata: &Metadata, vocab_size: usize) -> Result<Self> {
        let Some(MetadataArray::String(rules)) = metadata
            .get("tokenizer.ggml.merges")
            .and_then(|value| value.as_array())
        else {
            return Err(invalid("BPE requires a STRING merge array"));
        };
        if rules.len() > MAX_BPE_MERGES {
            return Err(invalid("BPE merge count exceeds supported bounds"));
        }
        let mut rule_bytes = 0_usize;
        for rule in rules {
            if rule.len() > 2 * MAX_PIECE_BYTES + 1 {
                return Err(invalid("BPE merge exceeds serialized rule byte limit"));
            }
            rule_bytes = rule_bytes
                .checked_add(rule.len())
                .filter(|&length| length <= MAX_BPE_MERGE_BYTES)
                .ok_or_else(|| invalid("BPE merges exceed serialized byte limit"))?;
        }
        let vocabulary = Vocabulary::from_metadata(metadata, vocab_size)?;
        let Some(MetadataArray::String(tokens)) = metadata
            .get("tokenizer.ggml.tokens")
            .and_then(|value| value.as_array())
        else {
            return Err(invalid("BPE requires a STRING token array"));
        };
        let Some(MetadataArray::I32(types)) = metadata
            .get("tokenizer.ggml.token_type")
            .and_then(|value| value.as_array())
        else {
            return Err(invalid("BPE requires an INT32 token type array"));
        };
        let mut normal = HashMap::new();
        normal.try_reserve(vocab_size)?;
        let mut bytes = [None; 256];
        for (index, (token, &kind)) in tokens.iter().zip(types).enumerate() {
            if kind != 1 {
                continue;
            }
            let id = u32::try_from(index)?;
            if normal.insert(token.as_str(), id).is_some() {
                return Err(invalid("BPE has duplicate normal vocabulary pieces"));
            }
            if let [byte] = vocabulary.piece(id)? {
                bytes[usize::from(*byte)] = Some(id);
            }
        }
        let mut merges = HashMap::new();
        merges.try_reserve(rules.len())?;
        let mut combined = String::new();
        for (rank, rule) in rules.iter().enumerate() {
            let Some((left, right)) = rule.split_once(' ') else {
                return Err(invalid("BPE merge requires one ASCII space separator"));
            };
            if left.is_empty() || right.is_empty() || right.contains(' ') {
                return Err(invalid("BPE merge has empty operands or extra separators"));
            }
            let left_id = *normal
                .get(left)
                .ok_or_else(|| invalid("BPE merge left operand is not a normal token"))?;
            let right_id = *normal
                .get(right)
                .ok_or_else(|| invalid("BPE merge right operand is not a normal token"))?;
            combined.clear();
            combined.try_reserve(rule.len() - 1)?;
            combined.push_str(left);
            combined.push_str(right);
            let token = *normal
                .get(combined.as_str())
                .ok_or_else(|| invalid("BPE merge result is not a normal token"))?;
            if merges
                .insert((left_id, right_id), Merge { rank, token })
                .is_some()
            {
                return Err(invalid("BPE has duplicate merge pairs"));
            }
        }
        Ok(Self { bytes, merges })
    }

    /// Encodes one **already pretokenized** byte piece using ranked merges.
    ///
    /// No special tokens are recognized or inserted. Arbitrary UTF-8 and binary
    /// bytes are preserved; callers can recover the bytes with [`Vocabulary`].
    /// The 64 KiB piece bound limits work to fewer than `3 * bytes.len()` heap
    /// insertions and `O(n log n)` comparisons, including stale candidates.
    /// Scratch allocations reserve for at most `n` nodes, `3 * n` candidates and
    /// `n` output IDs, independently of the untrusted caller's token limit.
    ///
    /// # Errors
    /// Rejects oversized pieces, bytes missing normal single-byte vocabulary
    /// tokens, final outputs above `max_tokens`, and allocation failures.
    pub fn encode_piece(&self, bytes: &[u8], max_tokens: usize) -> Result<Vec<u32>> {
        if bytes.len() > MAX_BPE_PIECE_BYTES {
            return Err(invalid("BPE input piece exceeds byte limit"));
        }
        if bytes.is_empty() {
            return Ok(Vec::new());
        }
        if max_tokens == 0 {
            return Err(invalid("BPE output exceeds token limit"));
        }
        let mut nodes = Vec::new();
        nodes.try_reserve_exact(bytes.len())?;
        for (index, &byte) in bytes.iter().enumerate() {
            let token = self.bytes[usize::from(byte)]
                .ok_or_else(|| invalid("BPE input byte has no normal single-byte token"))?;
            nodes.push(Node {
                token,
                previous: index.checked_sub(1),
                next: (index + 1 < bytes.len()).then_some(index + 1),
                alive: true,
            });
        }
        let mut candidates = BinaryHeap::new();
        candidates.try_reserve_exact(bytes.len() * 3)?;
        for left in 0..bytes.len() - 1 {
            if let Some(candidate) = self.candidate(&nodes, left) {
                candidates.push(candidate);
            }
        }
        let mut remaining = nodes.len();
        while let Some(Reverse((_, left, right, left_token, right_token, token))) = candidates.pop()
        {
            if !nodes[left].alive
                || !nodes[right].alive
                || nodes[left].next != Some(right)
                || nodes[left].token != left_token
                || nodes[right].token != right_token
            {
                continue;
            }
            let next = nodes[right].next;
            nodes[left].token = token;
            nodes[left].next = next;
            nodes[right].alive = false;
            if let Some(next) = next {
                nodes[next].previous = Some(left);
            }
            remaining -= 1;
            if let Some(previous) = nodes[left].previous
                && let Some(candidate) = self.candidate(&nodes, previous)
            {
                candidates.push(candidate);
            }
            if let Some(candidate) = self.candidate(&nodes, left) {
                candidates.push(candidate);
            }
        }
        if remaining > max_tokens {
            return Err(invalid("BPE output exceeds token limit"));
        }
        let mut output = Vec::new();
        output.try_reserve_exact(remaining)?;
        let mut current = Some(0);
        while let Some(index) = current {
            output.push(nodes[index].token);
            current = nodes[index].next;
        }
        Ok(output)
    }

    /// Encodes supplied pretokenizer boundaries without merging across pieces.
    ///
    /// This does not find the boundaries itself. At most 64 Ki pieces and 1 MiB
    /// total bytes are admitted, with the same per-piece bound as
    /// [`Self::encode_piece`]. Empty pieces contribute no output.
    ///
    /// # Errors
    /// Rejects input bounds, missing normal byte tokens, aggregate outputs above
    /// `max_tokens`, and allocation failures. No partial output is returned.
    pub fn encode_pieces(&self, pieces: &[&[u8]], max_tokens: usize) -> Result<Vec<u32>> {
        if pieces.len() > MAX_BPE_INPUT_PIECES {
            return Err(invalid("BPE input piece count exceeds limit"));
        }
        let mut total_bytes = 0_usize;
        for piece in pieces {
            if piece.len() > MAX_BPE_PIECE_BYTES {
                return Err(invalid("BPE input piece exceeds byte limit"));
            }
            total_bytes = total_bytes
                .checked_add(piece.len())
                .filter(|&length| length <= MAX_BPE_INPUT_BYTES)
                .ok_or_else(|| invalid("BPE total input exceeds byte limit"))?;
        }
        let mut output = Vec::new();
        output.try_reserve_exact(total_bytes.min(max_tokens))?;
        for piece in pieces {
            let encoded = self.encode_piece(piece, max_tokens - output.len())?;
            output.extend_from_slice(&encoded);
        }
        Ok(output)
    }

    fn candidate(&self, nodes: &[Node], left: usize) -> Option<Reverse<Candidate>> {
        let right = nodes[left].next?;
        let left_token = nodes[left].token;
        let right_token = nodes[right].token;
        let merge = self.merges.get(&(left_token, right_token))?;
        Some(Reverse((
            merge.rank,
            left,
            right,
            left_token,
            right_token,
            merge.token,
        )))
    }
}
