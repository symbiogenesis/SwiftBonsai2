//! Owned byte-level tokenizer policy from pinned Qwen `src/bpe.h`.
//!
//! Ported from qwentts.cpp `5916bf7c79dbb4e66fc059cdc507a79a23a28bb4`,
//! under the MIT license in `LICENSE.qwen-core`. GGUF access remains native;
//! these owned tables, special matching, segmentation and merges are safe Rust.
#![forbid(unsafe_code)]
#![allow(
    clippy::redundant_pub_crate,
    reason = "Keep the tokenizer and model-free boundary observations private to this crate."
)]

use std::{collections::HashMap, io, ops::Range};

mod scanner;

const MAX_INPUT_BYTES: usize = 64 * 1024;
const MAX_ENTRIES: usize = 1_000_000;
const MAX_TABLE_BYTES: usize = 64 * 1024 * 1024;
const MAX_SPELLING_BYTES: usize = 4096;
const MAX_SPECIALS: usize = 64;
const MAX_MERGE_PROBES: usize = 64 * 1024 * 1024;

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn copy_bytes(bytes: &[u8]) -> io::Result<Vec<u8>> {
    let mut owned = Vec::new();
    owned
        .try_reserve_exact(bytes.len())
        .map_err(io::Error::other)?;
    owned.extend_from_slice(bytes);
    Ok(owned)
}

/// The pinned scanner's borrowed chunks, which can split multibyte whitespace.
pub(super) fn pretokenize(text: &[u8]) -> io::Result<Vec<&[u8]>> {
    if text.len() > MAX_INPUT_BYTES || std::str::from_utf8(text).is_err() {
        return Err(invalid(
            "Qwen tokenizer input is oversized or invalid UTF-8.",
        ));
    }
    scanner::pretokenize(text)
}

/// One context's immutable finalized vocabulary and merge policy.
pub(super) struct Tokenizer {
    vocabulary: HashMap<Vec<u8>, i32>,
    merges: HashMap<Vec<u8>, i32>,
    reverse: Vec<Vec<u8>>,
    specials: Vec<(Vec<u8>, i32)>,
    eos: i32,
    table_bytes: usize,
    max_merge_bytes: usize,
    finished: bool,
}

impl Tokenizer {
    /// Create an empty owner without allocating model tables.
    pub(super) fn new() -> Self {
        Self {
            vocabulary: HashMap::new(),
            merges: HashMap::new(),
            reverse: Vec::new(),
            specials: Vec::new(),
            eos: -1,
            table_bytes: 0,
            max_merge_bytes: 0,
            finished: false,
        }
    }

    fn insert(&mut self, bytes: &[u8], value: i32, merge: bool) -> io::Result<()> {
        if self.finished
            || bytes.len() > MAX_SPELLING_BYTES
            || value < 0
            || usize::try_from(value).map_or(true, |value| value >= MAX_ENTRIES)
        {
            return Err(invalid("Qwen tokenizer table entry is outside its bounds."));
        }
        let table = if merge {
            &mut self.merges
        } else {
            &mut self.vocabulary
        };
        if let Some(existing) = table.get_mut(bytes) {
            // Native GGUF insertion overwrites repeated spellings, including ranks.
            *existing = value;
            return Ok(());
        }
        if table.len() >= MAX_ENTRIES || bytes.len() > MAX_TABLE_BYTES - self.table_bytes {
            return Err(invalid("Qwen tokenizer table exceeds its aggregate bound."));
        }
        let owned = copy_bytes(bytes)?;
        table.try_reserve(1).map_err(io::Error::other)?;
        table.insert(owned, value);
        self.table_bytes += bytes.len();
        if merge {
            self.max_merge_bytes = self.max_merge_bytes.max(bytes.len());
        }
        Ok(())
    }

    /// Add a raw vocabulary spelling; repeated spellings replace their ID.
    pub(super) fn add_token(&mut self, bytes: &[u8], id: i32) -> io::Result<()> {
        self.insert(bytes, id, false)
    }

    /// Add the complete `left + space + right` key and its native rank.
    pub(super) fn add_merge(&mut self, bytes: &[u8], rank: i32) -> io::Result<()> {
        self.insert(bytes, rank, true)
    }

    /// Build native-compatible reverse slots and register the end-of-text token.
    pub(super) fn finish(&mut self) -> io::Result<()> {
        if self.finished {
            return Err(invalid("Qwen tokenizer tables were already finalized."));
        }
        let mut reverse = Vec::new();
        reverse
            .try_reserve_exact(self.vocabulary.len())
            .map_err(io::Error::other)?;
        reverse.resize_with(self.vocabulary.len(), Vec::new);
        let mut assigned = Vec::new();
        assigned
            .try_reserve_exact(reverse.len())
            .map_err(io::Error::other)?;
        assigned.resize(reverse.len(), false);
        for (spelling, &id) in &self.vocabulary {
            let id = usize::try_from(id).map_err(|_| invalid("Invalid Qwen vocabulary ID."))?;
            if let Some(slot) = reverse.get_mut(id) {
                if assigned[id] {
                    return Err(invalid("Qwen vocabulary contains ambiguous reverse IDs."));
                }
                *slot = copy_bytes(spelling)?;
                assigned[id] = true;
            }
        }
        self.reverse = reverse;
        self.eos = self
            .vocabulary
            .get(b"<|endoftext|>".as_slice())
            .copied()
            .unwrap_or(-1);
        if self.eos >= 0 {
            self.add_special(b"<|endoftext|>", self.eos)?;
        }
        self.finished = true;
        Ok(())
    }

    /// Register a nonempty spelling; the first registration of each spelling wins.
    pub(super) fn add_special(&mut self, bytes: &[u8], id: i32) -> io::Result<()> {
        if self.specials.iter().any(|(spelling, _)| spelling == bytes) {
            return Ok(());
        }
        if bytes.is_empty()
            || bytes.len() > MAX_SPELLING_BYTES
            || self.specials.len() >= MAX_SPECIALS
        {
            return Err(invalid("Qwen tokenizer special exceeds its bounds."));
        }
        let owned = copy_bytes(bytes)?;
        self.specials.try_reserve(1).map_err(io::Error::other)?;
        self.specials.push((owned, id));
        Ok(())
    }

    /// Register a speech special by reverse ID, skipping invalid or empty slots.
    pub(super) fn add_special_id(&mut self, id: i32) -> io::Result<()> {
        if !self.finished {
            return Err(invalid("Qwen tokenizer tables are not finalized."));
        }
        let Some(spelling) = usize::try_from(id).ok().and_then(|id| self.reverse.get(id)) else {
            return Ok(());
        };
        if spelling.is_empty() {
            return Ok(());
        }
        let spelling = copy_bytes(spelling)?;
        self.add_special(&spelling, id)
    }

    fn encode_segment(
        &self,
        bytes: &[u8],
        ids: &mut Vec<i32>,
        probes: &mut usize,
    ) -> io::Result<()> {
        for chunk in pretokenize(bytes)? {
            self.encode_chunk(chunk, ids, probes)?;
        }
        Ok(())
    }

    /// Encode valid UTF-8 using the pinned byte scanner, special order and merges.
    ///
    /// Tables, input, output and merge work have explicit limits. Invalid or
    /// truncated UTF-8 is rejected rather than reproducing unchecked native reads.
    pub(super) fn encode(&self, text: &[u8], add_eos: bool) -> io::Result<Vec<i32>> {
        if !self.finished || text.len() > MAX_INPUT_BYTES || std::str::from_utf8(text).is_err() {
            return Err(invalid(
                "Qwen tokenizer input or table state is unsupported.",
            ));
        }
        let mut ids = Vec::new();
        // A byte expands to at most two encoded bytes; even the native raw-byte
        // fallback cannot emit more than this many IDs, plus the optional EOS.
        ids.try_reserve_exact(text.len() * 2 + 1)
            .map_err(io::Error::other)?;
        let mut position = 0;
        let mut probes = 0;
        while position < text.len() {
            let mut best: Option<(usize, usize)> = None;
            for (index, (spelling, _)) in self.specials.iter().enumerate() {
                if let Some(offset) = text[position..]
                    .windows(spelling.len())
                    .position(|window| window == spelling)
                {
                    let found = position + offset;
                    if best.is_none_or(|(previous, _)| found < previous) {
                        best = Some((found, index));
                    }
                }
            }
            let Some((found, index)) = best else {
                self.encode_segment(&text[position..], &mut ids, &mut probes)?;
                break;
            };
            self.encode_segment(&text[position..found], &mut ids, &mut probes)?;
            let (spelling, id) = &self.specials[index];
            ids.push(*id);
            position = found + spelling.len();
        }
        if add_eos && self.eos >= 0 {
            ids.push(self.eos);
        }
        Ok(ids)
    }

    fn encode_chunk(&self, chunk: &[u8], ids: &mut Vec<i32>, probes: &mut usize) -> io::Result<()> {
        let mut encoded = Vec::new();
        encoded
            .try_reserve_exact(chunk.len() * 2)
            .map_err(io::Error::other)?;
        let mut symbols: Vec<Range<usize>> = Vec::new();
        symbols
            .try_reserve_exact(chunk.len())
            .map_err(io::Error::other)?;
        for &byte in chunk {
            let start = encoded.len();
            let bytes = BYTE_ENCODER[usize::from(byte)];
            encoded.push(bytes[0]);
            if bytes[1] != 0 {
                encoded.push(bytes[1]);
            }
            symbols.push(start..encoded.len());
        }
        let mut key = Vec::new();
        key.try_reserve_exact(self.max_merge_bytes)
            .map_err(io::Error::other)?;
        while symbols.len() > 1 {
            let mut best_rank = i32::MAX;
            let mut best = None;
            for (index, pair) in symbols.windows(2).enumerate() {
                *probes += 1;
                if *probes > MAX_MERGE_PROBES {
                    return Err(invalid("Qwen tokenizer merge work exceeds its bound."));
                }
                if pair[1].end - pair[0].start + 1 > self.max_merge_bytes {
                    continue;
                }
                key.clear();
                key.extend_from_slice(&encoded[pair[0].clone()]);
                key.push(b' ');
                key.extend_from_slice(&encoded[pair[1].clone()]);
                if let Some(&rank) = self.merges.get(key.as_slice()) {
                    if rank < best_rank {
                        best_rank = rank;
                        best = Some(index);
                    }
                }
            }
            let Some(index) = best else { break };
            symbols[index].end = symbols[index + 1].end;
            symbols.remove(index + 1);
        }
        for symbol in symbols {
            let piece = &encoded[symbol];
            if let Some(&id) = self.vocabulary.get(piece) {
                ids.push(id);
            } else {
                // Native fallback probes the raw bytes of the encoded spelling,
                // not the original input bytes or the GPT-2 byte encoder again.
                for byte in piece {
                    if let Some(&id) = self.vocabulary.get(std::slice::from_ref(byte)) {
                        ids.push(id);
                    }
                }
            }
        }
        Ok(())
    }
}

const fn visible(byte: u16) -> bool {
    matches!(byte, 33..=126 | 161..=172 | 174..=255)
}

const fn byte_encoder() -> [[u8; 2]; 256] {
    let mut result = [[0; 2]; 256];
    let mut byte = 0_usize;
    let mut omitted = 0_u16;
    while byte < 256 {
        let byte_value = u16::from_le_bytes([byte.to_le_bytes()[0], 0]);
        let codepoint = if visible(byte_value) {
            byte_value
        } else {
            let codepoint = 256 + omitted;
            omitted += 1;
            codepoint
        };
        // Every GPT-2 byte spelling is an ASCII or two-byte UTF-8 codepoint.
        let [low, _] = codepoint.to_le_bytes();
        if codepoint < 128 {
            result[byte] = [low, 0];
        } else {
            let [lead, _] = (0xc0 | (codepoint >> 6)).to_le_bytes();
            result[byte] = [lead, 0x80 | (low & 0x3f)];
        }
        byte += 1;
    }
    result
}

const BYTE_ENCODER: [[u8; 2]; 256] = byte_encoder();
