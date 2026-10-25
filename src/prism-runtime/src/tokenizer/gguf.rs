//! Safe, bounded tokenization for the tokenizer families declared by GGUF.
//!
//! Algorithms follow the pinned Prism vocabulary contract. Metadata is used
//! directly: no publisher tokenizer files or reference token observations are
//! needed at runtime. The specialized Qwen35 API remains independently strict.
//! Algorithm port: PrismML-Eng/llama.cpp `73bfe6cc485554822f80c473a3c0c4f25e80f41b`,
//! `src/llama-vocab.cpp`; upstream MIT attribution is retained in
//! `tools/prism/upstream/LICENSE`.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

use bonsai_gguf::{Metadata, MetadataArray, MetadataValue};
use fancy_regex::Regex;
use spm_precompiled::Precompiled;
use unicode_normalization_alignments::UnicodeNormalization;

use crate::pretokenizer::Qwen35PreTokenizer;
use crate::vocabulary::{
    MAX_PIECE_BYTES, MAX_VOCABULARY_BYTES, MAX_VOCABULARY_TOKENS, decode_character,
};
use crate::{Result, invalid};

const LIMIT: usize = 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Family {
    Bpe,
    Spm,
    WordPiece,
    Unigram,
    Rwkv,
    Plamo,
}

#[derive(Debug)]
struct Symbol {
    start: usize,
    end: usize,
    previous: Option<usize>,
    next: Option<usize>,
}

#[derive(Debug)]
struct Pair {
    priority: f64,
    left: usize,
    right: usize,
    size: usize,
}
impl PartialEq for Pair {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}
impl Eq for Pair {}
impl PartialOrd for Pair {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Pair {
    fn cmp(&self, other: &Self) -> Ordering {
        self.priority
            .total_cmp(&other.priority)
            .then_with(|| other.left.cmp(&self.left))
    }
}

/// Immutable production tokenizer loaded solely from bounded GGUF metadata.
#[derive(Debug)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "GGUF independently declares these tokenizer flags"
)]
pub struct GgufTokenizer {
    family: Family,
    model: String,
    pre: String,
    tokens: Vec<String>,
    types: Vec<i32>,
    specials: Vec<usize>,
    scores: Vec<f32>,
    lookup: HashMap<String, u32>,
    ranks: HashMap<String, usize>,
    prefixes: Vec<Vec<u32>>,
    raw: Vec<Vec<u8>>,
    regexes: Vec<Regex>,
    charsmap: Option<Precompiled>,
    bos: Option<u32>,
    eos: Option<u32>,
    unknown: Option<u32>,
    separator: Option<u32>,
    add_bos: bool,
    add_eos: bool,
    prefix_space: bool,
    remove_spaces: bool,
    escape_spaces: bool,
    lowercase: bool,
    strip_accents: bool,
    ignore_merges: bool,
}

fn text<'a>(metadata: &'a Metadata, name: &str) -> Option<&'a str> {
    metadata
        .get(&format!("tokenizer.ggml.{name}"))
        .and_then(MetadataValue::as_str)
}
fn flag(metadata: &Metadata, name: &str, default: bool) -> Result<bool> {
    metadata
        .get(&format!("tokenizer.ggml.{name}"))
        .map_or(Ok(default), |value| {
            value
                .as_bool()
                .ok_or_else(|| invalid(format!("tokenizer flag {name} is not BOOL")))
        })
}
fn id(metadata: &Metadata, name: &str, default: Option<u32>, count: usize) -> Result<Option<u32>> {
    let value = metadata
        .get(&format!("tokenizer.ggml.{name}_token_id"))
        .map_or(Ok(default), |value| {
            if let MetadataValue::U32(value) = value {
                Ok(Some(*value))
            } else {
                Err(invalid("tokenizer ID is not UINT32"))
            }
        })?;
    Ok(value.filter(|value| usize::try_from(*value).is_ok_and(|value| value < count)))
}

impl GgufTokenizer {
    /// Validates and owns a tokenizer without requiring model weights.
    ///
    /// # Errors
    /// Rejects unsupported families, malformed metadata, inconsistent counts,
    /// nonfinite scores, and vocabulary or merge resource limits.
    #[allow(
        clippy::too_many_lines,
        reason = "Validate all bounded GGUF arrays before constructing the tokenizer"
    )]
    pub fn from_metadata(metadata: &Metadata, count: usize) -> Result<Self> {
        let model = text(metadata, "model").ok_or_else(|| invalid("tokenizer model missing"))?;
        let family = match model {
            "gpt2" | "hybriddna" | "gemma4" | "whitespace" => Family::Bpe,
            "llama" => Family::Spm,
            "bert" => Family::WordPiece,
            "t5" => Family::Unigram,
            "rwkv" => Family::Rwkv,
            "plamo2" => Family::Plamo,
            _ => return Err(invalid(format!("unsupported tokenizer model {model}"))),
        };
        let pre = if model == "gemma4" {
            "gemma4"
        } else {
            text(metadata, "pre").unwrap_or("default")
        };
        if count == 0 || count > MAX_VOCABULARY_TOKENS {
            return Err(invalid("tokenizer vocabulary count exceeds bound"));
        }
        let Some(MetadataArray::String(tokens)) = metadata
            .get("tokenizer.ggml.tokens")
            .and_then(MetadataValue::as_array)
        else {
            return Err(invalid("tokenizer tokens require STRING array"));
        };
        if tokens.len() != count
            || tokens.iter().any(|token| token.len() > MAX_PIECE_BYTES)
            || tokens.iter().map(String::len).sum::<usize>() > MAX_VOCABULARY_BYTES
        {
            return Err(invalid("tokenizer vocabulary bytes/count exceed bound"));
        }
        let types = match metadata
            .get("tokenizer.ggml.token_type")
            .and_then(MetadataValue::as_array)
        {
            Some(MetadataArray::I32(types))
                if types.len() == count && types.iter().all(|kind| (1..=6).contains(kind)) =>
            {
                types.clone()
            }
            None => vec![1; count],
            _ => return Err(invalid("tokenizer types require matching INT32 array")),
        };
        let scores = match metadata
            .get("tokenizer.ggml.scores")
            .and_then(MetadataValue::as_array)
        {
            Some(MetadataArray::F32(scores))
                if scores.len() >= count && scores.iter().all(|v| v.is_finite()) =>
            {
                scores[..count].to_vec()
            }
            None => vec![0.0; count],
            _ => return Err(invalid("tokenizer scores require finite FLOAT32 array")),
        };
        let mut lookup = HashMap::new();
        lookup.try_reserve(count)?;
        let mut raw = Vec::new();
        raw.try_reserve_exact(count)?;
        let mut prefixes = vec![Vec::new(); 256];
        for (index, token) in tokens.iter().enumerate() {
            let token_id = u32::try_from(index)?;
            lookup.insert(token.clone(), token_id);
            let bytes = if family == Family::Rwkv {
                unescape_rwkv(token)?
            } else {
                token.as_bytes().to_vec()
            };
            if let Some(first) = bytes.first() {
                prefixes[usize::from(*first)].push(token_id);
            }
            raw.push(bytes);
        }
        for entries in &mut prefixes {
            entries.sort_by_key(|&token| {
                std::cmp::Reverse(raw[usize::try_from(token).unwrap_or(0)].len())
            });
        }
        let mut ranks = HashMap::new();
        if family == Family::Bpe {
            let Some(MetadataArray::String(merges)) = metadata
                .get("tokenizer.ggml.merges")
                .and_then(MetadataValue::as_array)
            else {
                return Err(invalid("BPE requires STRING merges"));
            };
            if merges.len() > 1_000_000
                || merges.iter().map(String::len).sum::<usize>() > 64 * LIMIT
            {
                return Err(invalid("BPE merge metadata exceeds bound"));
            }
            ranks.try_reserve(merges.len())?;
            for (rank, merge) in merges.iter().enumerate() {
                ranks.entry(merge.clone()).or_insert(rank);
            }
        }
        let regexes = if family == Family::Bpe {
            patterns(pre)?
                .iter()
                .map(|pattern| Regex::new(pattern).map_err(Into::into))
                .collect::<Result<Vec<_>>>()?
        } else {
            Vec::new()
        };
        let charsmap = match metadata
            .get("tokenizer.ggml.precompiled_charsmap")
            .and_then(MetadataValue::as_array)
        {
            Some(MetadataArray::U8(bytes)) => Some(Precompiled::from(bytes)?),
            Some(MetadataArray::I8(bytes)) => Some(Precompiled::from(
                &bytes.iter().map(|v| v.to_ne_bytes()[0]).collect::<Vec<_>>(),
            )?),
            None => None,
            _ => return Err(invalid("tokenizer charsmap requires byte array")),
        };
        let (bos, eos, unk, sep) = match family {
            Family::Spm | Family::Plamo => (Some(1), Some(2), Some(0), None),
            Family::WordPiece => (Some(101), None, Some(100), Some(102)),
            Family::Unigram => (None, Some(1), Some(2), None),
            Family::Bpe => (Some(11), Some(11), None, None),
            Family::Rwkv => (None, None, None, None),
        };
        let lowercase = flag(
            metadata,
            "normalizer.lowercase",
            family == Family::WordPiece,
        )?;
        let specials = types
            .iter()
            .enumerate()
            .filter_map(|(index, kind)| matches!(kind, 2..=4).then_some(index))
            .collect();
        Ok(Self {
            family,
            model: model.to_owned(),
            pre: pre.to_owned(),
            tokens: tokens.clone(),
            types,
            specials,
            scores,
            lookup,
            ranks,
            prefixes,
            raw,
            regexes,
            charsmap,
            bos: id(metadata, "bos", bos, count)?,
            eos: id(metadata, "eos", eos, count)?,
            unknown: id(metadata, "unknown", unk, count)?,
            separator: id(metadata, "seperator", sep, count)?,
            add_bos: flag(
                metadata,
                "add_bos_token",
                family == Family::Spm || matches!(pre, "llama3" | "llama-v3" | "llama-bpe"),
            )?,
            add_eos: flag(metadata, "add_eos_token", family == Family::Unigram)?,
            prefix_space: flag(
                metadata,
                "add_space_prefix",
                family == Family::Spm || family == Family::Unigram,
            )?,
            remove_spaces: flag(metadata, "remove_extra_whitespaces", false)?,
            escape_spaces: family == Family::Spm || family == Family::Unigram || pre == "gemma4",
            lowercase,
            strip_accents: flag(metadata, "normalizer.strip_accents", lowercase)?,
            ignore_merges: matches!(pre, "llama3" | "llama-v3" | "llama-bpe"),
        })
    }

    /// Number of validated token IDs.
    #[must_use]
    pub const fn vocab_size(&self) -> usize {
        self.tokens.len()
    }

    /// Pinned GGUF vocabulary family discriminator (SPM=1 through PLaMo=6).
    #[must_use]
    pub const fn vocabulary_type(&self) -> u32 {
        match self.family {
            Family::Spm => 1,
            Family::Bpe => 2,
            Family::WordPiece => 3,
            Family::Unigram => 4,
            Family::Rwkv => 5,
            Family::Plamo => 6,
        }
    }

    /// Validated beginning-of-sequence ID, including family defaults.
    #[must_use]
    pub const fn bos_token(&self) -> Option<u32> {
        self.bos
    }

    /// Encodes a UTF-8 request, with explicit source special-token policies.
    ///
    /// User-defined tokens are always recognized; control tokens are recognized
    /// only with `parse_special`. Insertion uses family defaults overridden by
    /// the immutable metadata. No chat template is applied.
    ///
    /// # Errors
    /// Rejects malformed UTF-8, input/output limits, unavailable byte fallback,
    /// and missing enabled boundary IDs.
    pub fn encode(&self, bytes: &[u8], add_special: bool, parse_special: bool) -> Result<Vec<u32>> {
        if bytes.len() > LIMIT {
            return Err(invalid("tokenizer input exceeds byte limit"));
        }
        let text = match std::str::from_utf8(bytes) {
            Ok(text) => text,
            Err(_) if self.family == Family::Spm => {
                return self.encode_spm_bytes(bytes, add_special, parse_special);
            }
            Err(error) => return Err(error.into()),
        };
        let mut output = Vec::new();
        if add_special && (self.add_bos || self.family == Family::WordPiece) {
            output.push(
                self.bos
                    .ok_or_else(|| invalid("tokenizer BOS unavailable"))?,
            );
        }
        let mut offset = 0;
        while offset < text.len() {
            let remaining = &text[offset..];
            let special = self
                .specials
                .iter()
                .copied()
                .filter(|&index| {
                    !self.tokens[index].is_empty() && (self.types[index] == 4 || parse_special)
                })
                .filter_map(|index| {
                    remaining.find(&self.tokens[index]).map(|position| {
                        (position, std::cmp::Reverse(self.tokens[index].len()), index)
                    })
                })
                .min();
            let end = special.map_or(text.len(), |(position, _, _)| offset + position);
            if end > offset {
                self.ordinary(&text[offset..end], true, &mut output)?;
            }
            if let Some((_, _, index)) = special {
                output.push(u32::try_from(index)?);
                offset = end + self.tokens[index].len();
            } else {
                break;
            }
            if output.len() > 262_144 {
                return Err(invalid("tokenizer output exceeds token bound"));
            }
        }
        if add_special {
            if self.family == Family::WordPiece {
                output.push(
                    self.separator
                        .ok_or_else(|| invalid("tokenizer separator unavailable"))?,
                );
            } else if self.add_eos {
                output.push(
                    self.eos
                        .ok_or_else(|| invalid("tokenizer EOS unavailable"))?,
                );
            }
        }
        if output.len() > 262_144 {
            return Err(invalid("tokenizer output exceeds token bound"));
        }
        Ok(output)
    }

    fn encode_spm_bytes(
        &self,
        bytes: &[u8],
        add_special: bool,
        parse_special: bool,
    ) -> Result<Vec<u32>> {
        let mut output = Vec::new();
        if add_special && self.add_bos {
            output.push(self.bos.ok_or_else(|| invalid("SPM BOS unavailable"))?);
        }
        let mut offset = 0;
        while offset < bytes.len() {
            let special = self
                .specials
                .iter()
                .copied()
                .filter(|&index| {
                    !self.tokens[index].is_empty() && (self.types[index] == 4 || parse_special)
                })
                .filter_map(|index| {
                    bytes[offset..]
                        .windows(self.tokens[index].len())
                        .position(|part| part == self.tokens[index].as_bytes())
                        .map(|position| {
                            (position, std::cmp::Reverse(self.tokens[index].len()), index)
                        })
                })
                .min();
            let end = special.map_or(bytes.len(), |(position, _, _)| offset + position);
            if end > offset {
                let mut escaped = Vec::new();
                if self.prefix_space {
                    escaped.extend_from_slice("▁".as_bytes());
                }
                for &byte in &bytes[offset..end] {
                    if byte == b' ' {
                        escaped.extend_from_slice("▁".as_bytes());
                    } else {
                        escaped.push(byte);
                    }
                }
                self.merge_spm_bytes(&escaped, &mut output)?;
            }
            if let Some((_, _, index)) = special {
                output.push(u32::try_from(index)?);
                offset = end + self.tokens[index].len();
            } else {
                break;
            }
        }
        if add_special && self.add_eos {
            output.push(self.eos.ok_or_else(|| invalid("SPM EOS unavailable"))?);
        }
        if output.len() > 262_144 {
            return Err(invalid("tokenizer output exceeds token bound"));
        }
        Ok(output)
    }

    fn merge_spm_bytes(&self, bytes: &[u8], output: &mut Vec<u32>) -> Result<()> {
        let mut nodes: Vec<Symbol> = Vec::new();
        let mut offset = 0;
        while offset < bytes.len() {
            let length = match bytes[offset] {
                0..=0xbf => 1,
                0xc0..=0xdf => 2,
                0xe0..=0xef => 3,
                _ => 4,
            };
            let index = nodes.len();
            let end = (offset + length).min(bytes.len());
            nodes.push(Symbol {
                start: offset,
                end,
                previous: index.checked_sub(1),
                next: (end < bytes.len()).then_some(index + 1),
            });
            offset = end;
        }
        let mut queue = BinaryHeap::new();
        let candidate =
            |nodes: &[Symbol], left: usize, queue: &mut BinaryHeap<Pair>| -> Result<()> {
                let Some(right) = nodes[left].next else {
                    return Ok(());
                };
                if let Ok(text) = std::str::from_utf8(&bytes[nodes[left].start..nodes[right].end])
                    && let Some(&id) = self.lookup.get(text)
                {
                    queue.push(Pair {
                        priority: f64::from(self.scores[usize::try_from(id)?]),
                        left,
                        right,
                        size: nodes[right].end - nodes[left].start,
                    });
                }
                Ok(())
            };
        for index in 0..nodes.len().saturating_sub(1) {
            candidate(&nodes, index, &mut queue)?;
        }
        while let Some(pair) = queue.pop() {
            if nodes[pair.left].start == nodes[pair.left].end
                || nodes[pair.right].start == nodes[pair.right].end
                || nodes[pair.left].next != Some(pair.right)
                || nodes[pair.right].end - nodes[pair.left].start != pair.size
            {
                continue;
            }
            let next = nodes[pair.right].next;
            nodes[pair.left].end = nodes[pair.right].end;
            nodes[pair.left].next = next;
            nodes[pair.right].end = nodes[pair.right].start;
            if let Some(next) = next {
                nodes[next].previous = Some(pair.left);
            }
            if let Some(previous) = nodes[pair.left].previous {
                candidate(&nodes, previous, &mut queue)?;
            }
            candidate(&nodes, pair.left, &mut queue)?;
        }
        let mut current = (!nodes.is_empty()).then_some(0);
        while let Some(index) = current {
            let piece = &bytes[nodes[index].start..nodes[index].end];
            if let Ok(text) = std::str::from_utf8(piece)
                && let Some(&id) = self.lookup.get(text)
            {
                output.push(id);
            } else {
                for &byte in piece {
                    output.push(self.byte(byte)?);
                }
            }
            current = nodes[index].next;
        }
        Ok(())
    }

    fn ordinary(&self, text: &str, previous_special: bool, output: &mut Vec<u32>) -> Result<()> {
        match self.family {
            Family::Bpe => self.bpe(text, output),
            Family::Spm => {
                let text = format!(
                    "{}{}",
                    if self.prefix_space && previous_special {
                        " "
                    } else {
                        ""
                    },
                    text
                )
                .replace(' ', "▁");
                self.merge(&text, false, output)
            }
            Family::WordPiece => self.wordpiece(text, output),
            Family::Unigram => self.unigram(text, output),
            Family::Rwkv => self.greedy(text.as_bytes(), output),
            Family::Plamo => self.plamo(text, output),
        }
    }

    fn byte(&self, value: u8) -> Result<u32> {
        self.lookup
            .get(&format!("<0x{value:02X}>"))
            .copied()
            .or_else(|| self.lookup.get(&char::from(value).to_string()).copied())
            .ok_or_else(|| invalid(format!("tokenizer byte {value} unavailable")))
    }

    fn greedy(&self, bytes: &[u8], output: &mut Vec<u32>) -> Result<()> {
        let mut offset = 0;
        while offset < bytes.len() {
            let found = self.prefixes[usize::from(bytes[offset])]
                .iter()
                .find(|&&id| {
                    bytes[offset..].starts_with(&self.raw[usize::try_from(id).unwrap_or(0)])
                });
            let id = found
                .copied()
                .ok_or_else(|| invalid("tokenizer has no matching byte token"))?;
            offset += self.raw[usize::try_from(id)?].len();
            output.push(id);
        }
        Ok(())
    }

    fn bpe(&self, input: &str, output: &mut Vec<u32>) -> Result<()> {
        if self.model == "hybriddna" && input.contains("<dna>") {
            return self.dna(input, output);
        }
        let text = if self.escape_spaces {
            input.replace(' ', "▁")
        } else {
            input.to_owned()
        };
        let pieces = if self.pre == "qwen35" {
            let metadata = Metadata::from([
                ("tokenizer.ggml.model".into(), "gpt2".into()),
                ("tokenizer.ggml.pre".into(), "qwen35".into()),
            ]);
            Qwen35PreTokenizer::from_metadata(&metadata)?.split(&text)?
        } else {
            let mut pieces = vec![text.as_str()];
            for regex in &self.regexes {
                let mut split = Vec::new();
                for piece in pieces {
                    let mut start = 0;
                    for found in regex.find_iter(piece) {
                        let found = found?;
                        if found.start() > start {
                            split.push(&piece[start..found.start()]);
                        }
                        if found.end() > found.start() {
                            split.push(found.as_str());
                        }
                        start = found.end();
                    }
                    if start < piece.len() {
                        split.push(&piece[start..]);
                    }
                }
                pieces = split;
            }
            pieces
        };
        for piece in pieces {
            let encoded = if self.pre == "gemma4" || self.model == "whitespace" {
                piece.to_owned()
            } else {
                encode_bytes(piece.as_bytes())?
            };
            if self.ignore_merges
                && let Some(&id) = self.lookup.get(&encoded)
            {
                output.push(id);
                continue;
            }
            self.merge(&encoded, true, output)?;
        }
        Ok(())
    }

    fn merge(&self, text: &str, ranked: bool, output: &mut Vec<u32>) -> Result<()> {
        let mut nodes: Vec<_> = text
            .char_indices()
            .enumerate()
            .map(|(index, (start, value))| Symbol {
                start,
                end: start + value.len_utf8(),
                previous: index.checked_sub(1),
                next: Some(index + 1),
            })
            .collect();
        let count = nodes.len();
        if count == 0 {
            return Ok(());
        }
        nodes[count - 1].next = None;
        let mut queue = BinaryHeap::new();
        for left in 0..count.saturating_sub(1) {
            self.candidate(text, &nodes, left, ranked, &mut queue)?;
        }
        while let Some(pair) = queue.pop() {
            if nodes[pair.left].start == nodes[pair.left].end
                || nodes[pair.left].next != Some(pair.right)
                || nodes[pair.right].start == nodes[pair.right].end
                || nodes[pair.left].end - nodes[pair.left].start + nodes[pair.right].end
                    - nodes[pair.right].start
                    != pair.size
            {
                continue;
            }
            let next = nodes[pair.right].next;
            nodes[pair.left].end = nodes[pair.right].end;
            nodes[pair.left].next = next;
            nodes[pair.right].end = nodes[pair.right].start;
            if let Some(next) = next {
                nodes[next].previous = Some(pair.left);
            }
            if let Some(previous) = nodes[pair.left].previous {
                self.candidate(text, &nodes, previous, ranked, &mut queue)?;
            }
            self.candidate(text, &nodes, pair.left, ranked, &mut queue)?;
        }
        let mut current = Some(0);
        while let Some(index) = current {
            let piece = &text[nodes[index].start..nodes[index].end];
            if let Some(&id) = self.lookup.get(piece) {
                output.push(id);
            } else if ranked && self.pre != "gemma4" {
                for value in piece.chars() {
                    output.push(
                        *self
                            .lookup
                            .get(&value.to_string())
                            .ok_or_else(|| invalid("BPE byte alphabet token unavailable"))?,
                    );
                }
            } else {
                for &value in piece.as_bytes() {
                    output.push(self.byte(value)?);
                }
            }
            current = nodes[index].next;
        }
        Ok(())
    }

    fn candidate(
        &self,
        text: &str,
        nodes: &[Symbol],
        left: usize,
        ranked: bool,
        queue: &mut BinaryHeap<Pair>,
    ) -> Result<()> {
        let Some(right) = nodes[left].next else {
            return Ok(());
        };
        let priority = if ranked {
            let key = format!(
                "{} {}",
                &text[nodes[left].start..nodes[left].end],
                &text[nodes[right].start..nodes[right].end]
            );
            let Some(&rank) = self.ranks.get(&key) else {
                return Ok(());
            };
            -f64::from(u32::try_from(rank)?)
        } else {
            let Some(&id) = self.lookup.get(&text[nodes[left].start..nodes[right].end]) else {
                return Ok(());
            };
            f64::from(self.scores[usize::try_from(id)?])
        };
        queue.push(Pair {
            priority,
            left,
            right,
            size: nodes[right].end - nodes[left].start,
        });
        Ok(())
    }

    fn wordpiece(&self, text: &str, output: &mut Vec<u32>) -> Result<()> {
        let normalized = if self.strip_accents {
            text.nfd()
                .map(|value| value.0)
                .filter(|value| !unicode_normalization_alignments::char::is_combining_mark(*value))
                .collect::<String>()
        } else {
            text.to_owned()
        };
        let mut cleaned = String::new();
        for value in normalized.chars() {
            if value.is_whitespace() {
                cleaned.push(' ');
            } else if value != '\0' && value != '\u{fffd}' && !value.is_control() {
                let scalar = u32::from(value);
                let chinese = matches!(scalar, 0x4e00..=0x9fff | 0x3400..=0x4dbf | 0x20000..=0x2a6df | 0x2a700..=0x2b73f | 0x2b740..=0x2b81f | 0x2b920..=0x2ceaf | 0xf900..=0xfaff | 0x2f800..=0x2fa1f);
                if chinese {
                    cleaned.push(' ');
                }
                if self.lowercase {
                    cleaned.extend(value.to_lowercase());
                } else {
                    cleaned.push(value);
                }
                if chinese {
                    cleaned.push(' ');
                }
            }
        }
        let regex = Regex::new(r"\p{P}|[!-/:-@\[-`{-~]|[^\s\p{P}!-/:-@\[-`{-~]+")?;
        for word in regex.find_iter(&cleaned) {
            let word = format!("▁{}", word?.as_str());
            let start = output.len();
            let mut offset = 0;
            while offset < word.len() {
                let found = (offset + 1..=word.len())
                    .rev()
                    .filter(|&end| word.is_char_boundary(end))
                    .find_map(|end| {
                        self.lookup
                            .get(&word[offset..end])
                            .copied()
                            .map(|id| (end, id))
                    });
                if let Some((end, id)) = found {
                    output.push(id);
                    offset = end;
                } else {
                    output.truncate(start);
                    output.push(
                        self.unknown
                            .ok_or_else(|| invalid("WordPiece unknown token unavailable"))?,
                    );
                    break;
                }
            }
        }
        Ok(())
    }

    fn unigram(&self, input: &str, output: &mut Vec<u32>) -> Result<()> {
        let mut text = self.charsmap.as_ref().map_or_else(
            || input.to_owned(),
            |charsmap| charsmap.normalize_string(input),
        );
        if self.remove_spaces {
            text = text
                .split(' ')
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join(" ");
        }
        if self.prefix_space && !text.is_empty() {
            text.insert(0, ' ');
        }
        if self.escape_spaces {
            text = text.replace(' ', "▁");
        }
        if text.is_empty() {
            return Ok(());
        }
        let unknown = self
            .unknown
            .ok_or_else(|| invalid("Unigram unknown token unavailable"))?;
        let minimum = self
            .scores
            .iter()
            .zip(&self.types)
            .filter(|(_, kind)| **kind == 1)
            .map(|(score, _)| *score)
            .reduce(f32::min)
            .unwrap_or(0.0);
        let mut best = vec![(f64::NEG_INFINITY, 0_usize, unknown); text.len() + 1];
        best[0].0 = 0.0;
        for (offset, value) in text.char_indices() {
            let mut single = false;
            for &id in &self.prefixes[usize::from(text.as_bytes()[offset])] {
                let index = usize::try_from(id)?;
                let token = &self.tokens[index];
                if !matches!(self.types[index], 1 | 4 | 5) || !text[offset..].starts_with(token) {
                    continue;
                }
                let end = offset + token.len();
                single |= token.len() == value.len_utf8();
                let score = best[offset].0
                    + if self.types[index] == 4 {
                        0.0
                    } else {
                        f64::from(self.scores[index])
                    };
                if score > best[end].0 {
                    best[end] = (score, offset, id);
                }
            }
            if !single {
                let end = offset + value.len_utf8();
                let score = best[offset].0 + f64::from(minimum - 10.0);
                if score > best[end].0 {
                    best[end] = (score, offset, unknown);
                }
            }
        }
        let mut reverse = Vec::new();
        let mut end = text.len();
        while end > 0 {
            let (_, start, id) = best[end];
            if start >= end {
                return Err(invalid("Unigram path unavailable"));
            }
            if id != unknown || reverse.last() != Some(&unknown) {
                reverse.push(id);
            }
            end = start;
        }
        output.extend(reverse.into_iter().rev());
        Ok(())
    }

    fn plamo(&self, input: &str, output: &mut Vec<u32>) -> Result<()> {
        let text = input.strip_prefix('\u{feff}').unwrap_or(input);
        let chars: Vec<_> = text.char_indices().collect();
        let mut best = vec![(i64::MAX / 2, 0_usize, None); chars.len() + 1];
        best[chars.len()].0 = 0;
        for index in (0..chars.len()).rev() {
            let (offset, value) = chars[index];
            best[index] = (best[index + 1].0 + 10_000_000, index + 1, None);
            for &id in &self.prefixes[usize::from(text.as_bytes()[offset])] {
                let token = &self.tokens[usize::try_from(id)?];
                if token.is_empty() || !text[offset..].starts_with(token) {
                    continue;
                }
                let end = index + token.chars().count();
                if end > chars.len() {
                    continue;
                }
                let score = rounded_score(self.scores[usize::try_from(id)?])?;
                if score <= -20_000_000 {
                    continue;
                }
                let total = best[end].0 - i64::from(score);
                if total < best[index].0 {
                    best[index] = (total, end, Some(id));
                }
            }
            let _ = value;
        }
        let mut index = 0;
        while index < chars.len() {
            let (_, next, id) = best[index];
            if let Some(id) = id {
                output.push(id);
            } else {
                let mut storage = [0; 4];
                for &value in chars[index].1.encode_utf8(&mut storage).as_bytes() {
                    output.push(self.byte(value)?);
                }
            }
            index = next;
        }
        Ok(())
    }

    fn dna(&self, input: &str, output: &mut Vec<u32>) -> Result<()> {
        let mut position = 0;
        while let Some(start) = input[position..].find("<dna>") {
            let start = position + start;
            if start > position {
                self.bpe(&input[position..start], output)?;
            }
            output.push(
                *self
                    .lookup
                    .get("<dna>")
                    .ok_or_else(|| invalid("DNA begin token unavailable"))?,
            );
            let begin = start + 5;
            let end = input[begin..]
                .find("</dna>")
                .map_or(input.len(), |v| begin + v);
            let sequence = input[begin..end].to_ascii_uppercase();
            for chunk in sequence.as_bytes().chunks(6) {
                let mut kmer = std::str::from_utf8(chunk)?.to_owned();
                while kmer.len() < 6 {
                    kmer.push('A');
                }
                kmer.push('\u{e000}');
                output.push(
                    self.lookup
                        .get(&kmer)
                        .or_else(|| self.lookup.get("<oov>"))
                        .copied()
                        .ok_or_else(|| invalid("DNA fallback unavailable"))?,
                );
            }
            if end == input.len() {
                return Ok(());
            }
            output.push(
                *self
                    .lookup
                    .get("</dna>")
                    .ok_or_else(|| invalid("DNA end token unavailable"))?,
            );
            position = end + 6;
        }
        if position < input.len() {
            self.bpe(&input[position..], output)?;
        }
        Ok(())
    }

    /// Returns the bytes of one token, with explicit control rendering.
    ///
    /// # Errors
    /// Rejects an invalid token ID or malformed byte notation/alphabet.
    pub fn piece(&self, id: u32, special: bool) -> Result<Vec<u8>> {
        let index = usize::try_from(id)?;
        let token = self
            .tokens
            .get(index)
            .ok_or_else(|| invalid("tokenizer piece ID out of range"))?;
        let kind = self.types[index];
        if matches!(kind, 2 | 3) {
            return Ok(if special {
                token.as_bytes().to_vec()
            } else {
                Vec::new()
            });
        }
        if kind == 5 {
            return Ok(Vec::new());
        }
        if kind == 4 {
            return Ok(token.as_bytes().to_vec());
        }
        if kind == 6 {
            return Ok(vec![parse_byte(token)?]);
        }
        match self.family {
            Family::Bpe if self.pre != "gemma4" && self.model != "whitespace" && kind == 1 => {
                token.chars().map(decode_character).collect()
            }
            Family::Rwkv => Ok(self.raw[index].clone()),
            Family::Spm | Family::Unigram | Family::WordPiece => {
                Ok(token.replace('▁', " ").into_bytes())
            }
            _ => Ok(token
                .trim_end_matches('\u{e000}')
                .replace('▁', " ")
                .into_bytes()),
        }
    }

    /// Concatenates decoded pieces using model boundary and cleanup policy.
    ///
    /// # Errors
    /// Rejects invalid IDs or more than 1 MiB of decoded output.
    pub fn decode(
        &self,
        ids: &[u32],
        remove_special: bool,
        unparse_special: bool,
    ) -> Result<Vec<u8>> {
        let mut ids = ids;
        if remove_special && self.add_bos && ids.first().copied() == self.bos {
            ids = &ids[1..];
        }
        if remove_special && self.add_eos && ids.last().copied() == self.eos {
            ids = &ids[..ids.len() - 1];
        }
        let mut output = Vec::new();
        for &id in ids {
            output.extend(self.piece(id, unparse_special)?);
            if output.len() > LIMIT {
                return Err(invalid("tokenizer decoded bytes exceed bound"));
            }
        }
        if self.prefix_space && output.first() == Some(&b' ') {
            output.remove(0);
        }
        Ok(output)
    }
}

fn rounded_score(score: f32) -> Result<i32> {
    let value = f64::from(score) * 10_000.0;
    value.round().to_string().parse().map_err(Into::into)
}
fn parse_byte(token: &str) -> Result<u8> {
    u8::from_str_radix(
        token
            .strip_prefix("<0x")
            .and_then(|v| v.strip_suffix('>'))
            .filter(|v| v.len() == 2)
            .ok_or_else(|| invalid("byte token notation malformed"))?,
        16,
    )
    .map_err(Into::into)
}
fn encode_bytes(bytes: &[u8]) -> Result<String> {
    let mut table = ['\0'; 256];
    let mut next = 256;
    for value in 0_u16..256 {
        let byte = u8::try_from(value)?;
        table[usize::from(byte)] = if matches!(byte, 33..=126 | 161..=172 | 174..=255) {
            char::from(byte)
        } else {
            let value =
                char::from_u32(next).ok_or_else(|| invalid("byte alphabet scalar invalid"))?;
            next += 1;
            value
        };
    }
    Ok(bytes.iter().map(|&byte| table[usize::from(byte)]).collect())
}
fn unescape_rwkv(token: &str) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    let mut bytes = token.bytes();
    while let Some(value) = bytes.next() {
        if value != b'\\' {
            output.push(value);
            continue;
        }
        match bytes.next() {
            Some(b'n') => output.push(b'\n'),
            Some(b'r') => output.push(b'\r'),
            Some(b't') => output.push(b'\t'),
            Some(b'x') => {
                let hex = [
                    bytes.next().ok_or_else(|| invalid("RWKV truncated hex"))?,
                    bytes.next().ok_or_else(|| invalid("RWKV truncated hex"))?,
                ];
                output.push(u8::from_str_radix(std::str::from_utf8(&hex)?, 16)?);
            }
            Some(value) => output.push(value),
            None => return Err(invalid("RWKV trailing escape")),
        }
    }
    Ok(output)
}

#[allow(
    clippy::unicode_not_nfc,
    reason = "Preserve the pinned upstream regex scalar ranges exactly"
)]
fn patterns(pre: &str) -> Result<Vec<&'static str>> {
    let gpt = r"'s|'t|'re|'ve|'m|'ll|'d| ?\p{L}+| ?\p{N}+| ?[^\s\p{L}\p{N}]+|\s+(?!\S)";
    let llama = r"(?:'[sS]|'[tT]|'[rR][eE]|'[vV][eE]|'[mM]|'[lL][lL]|'[dD])|[^\r\n\p{L}\p{N}]?\p{L}+|\p{N}{1,3}| ?[^\s\p{L}\p{N}]+[\r\n]*|\s*[\r\n]+|\s+(?!\S)|\s+";
    Ok(match pre {
        "qwen35" => vec![],
        "gpt-2" | "phi-2" | "mpt" | "olmo" | "jais" | "jina-v2-es" | "jina-v2-de"
        | "jina-v2-code" | "jina-v1-en" | "roberta-bpe" => vec![gpt],
        "llama3" | "llama-v3" | "llama-bpe" | "smaug-bpe" => vec![llama],
        "qwen2" | "stablelm2" => vec![
            r"(?:'[sS]|'[tT]|'[rR][eE]|'[vV][eE]|'[mM]|'[lL][lL]|'[dD])|[^\r\n\p{L}\p{N}]?\p{L}+|\p{N}| ?[^\s\p{L}\p{N}]+[\r\n]*|\s*[\r\n]+|\s+(?!\S)|\s+",
        ],
        "starcoder" | "refact" | "command-r" => vec![r"\p{N}", gpt],
        "falcon" => vec![r"[\p{P}\$\+<=>\^~\|`]+", gpt, r"[0-9][0-9][0-9]"],
        "deepseek-coder" => vec![
            r"[\r\n]",
            r"\s?\p{L}+",
            r"\s?\p{P}+",
            "[一-龥ࠀ-一가-퟿]+",
            r"\p{N}",
        ],
        "deepseek-llm" => vec![
            r"[\r\n]",
            "\\s?[A-Za-zµÀ-ÖØ-öø-ƺƼ-ƿǄ-ʓʕ-ʯͰ-ͳͶͷͻ-ͽͿΆΈ-ΊΌΎ-ΡΣ-ϵϷ-ҁҊ-ԯԱ-ՖႠ-ჅᎠ-Ᏽᏸ-ᏽᲐ-ᲺᲽ-Ჿᴀ-ᴫᵫ-ᵷᵹ-ᶚḀ-ἕἘ-Ἕἠ-ὅὈ-Ὅὐ-ὗὙὛὝὟ-ώᾀ-ᾴᾶ-ᾼιῂ-ῄῆ-ῌῐ-ΐῖ-Ίῠ-Ῥῲ-ῴῶ-ῼℂℇℊ-ℓℕℙ-ℝℤΩℨK-ℭℯ-ℴℹℼ-ℿⅅ-ⅉⅎↃↄⰀ-ⱻⱾ-ⳤⳫ-ⳮⳲⳳꙀ-ꙭꚀ-ꚛꜢ-ꝯꝱ-ꞇꞋ-ꞎꭰ-ꮿﬀ-ﬆﬓ-ﬗＡ-Ｚａ-ｚ𐐀-𐑏𐒰-𐓓𐓘-𐓻𐲀-𐲲𐳀-𐳲𑢠-𑣟𞤀-𞥃]+",
            "\\s?[!-/:-~！-／：-～‘-‟　-。]+",
            r"\s+$",
            "[一-龥ࠀ-一가-퟿]+",
            r"\p{N}+",
        ],
        "poro-chat" | "bloom" => vec![r" ?[^(\s|.,!?…。，、।۔،)]+"],
        "viking" => vec![r" ?[^(\s|.,!?…。，、।۔،)]+", r"\p{N}"],
        "gemma4" => vec![r"[^\n]+|[\n]+"],
        "whitespace" => vec![r"\S+"],
        "default" => vec![r"[\p{P}\$\+<=>\^~\|]+", gpt, r"\p{N}+", r"[0-9][0-9][0-9]"],
        other => return Err(invalid(format!("unsupported BPE pretokenizer {other}"))),
    })
}
