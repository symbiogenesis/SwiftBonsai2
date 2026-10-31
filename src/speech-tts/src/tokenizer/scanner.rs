//! Exact ordered predicates and byte positions of pinned Qwen pretokenization.

use std::io;

fn codepoint(bytes: &[u8], index: usize) -> io::Result<(u32, usize)> {
    let first = bytes[index];
    let (width, mask) = match first {
        0xc0..=0xdf => (2, 0x1f),
        0xe0..=0xef => (3, 0x0f),
        0xf0..=0xf7 => (4, 0x07),
        _ => return Ok((u32::from(first), 1)),
    };
    let available = bytes
        .get(index..index + width)
        .ok_or_else(|| super::invalid("Truncated Qwen tokenizer codepoint."))?;
    let mut value = u32::from(first & mask);
    for &byte in &available[1..] {
        value = (value << 6) | u32::from(byte & 0x3f);
    }
    Ok((value, width))
}

const fn letter(codepoint: u32) -> bool {
    matches!(codepoint, 0x41..=0x5a | 0x61..=0x7a | 0xc0..=0xd6 | 0xd8..=0xf6 | 0xf8..=0x24f | 0x370..=0x1fff | 0x2c00..=0x2dff | 0x3040..=0x9fff | 0xac00..=0xd7af | 0xf900..=0xfaff | 0x1_0000..)
}

const fn digit(codepoint: u32) -> bool {
    matches!(codepoint, 0x30..=0x39)
}

const fn whitespace(codepoint: u32) -> bool {
    matches!(codepoint, 0x20 | 0x09..=0x0d | 0xa0 | 0x2000..=0x2002 | 0x200b)
}

const fn newline(codepoint: u32) -> bool {
    matches!(codepoint, 0x0a | 0x0d)
}

fn letters_end(text: &[u8], mut index: usize) -> io::Result<usize> {
    while index < text.len() {
        let (next, width) = codepoint(text, index)?;
        if !letter(next) {
            break;
        }
        index += width;
    }
    Ok(index)
}

fn contraction(text: &[u8], start: usize) -> io::Result<Option<usize>> {
    for suffix in [b"ll".as_slice(), b"re", b"ve", b"s", b"t", b"m", b"d"] {
        let end = start + suffix.len();
        if text
            .get(start..end)
            .is_some_and(|bytes| bytes.eq_ignore_ascii_case(suffix))
            && (end == text.len() || !letter(codepoint(text, end)?.0))
        {
            return Ok(Some(end));
        }
    }
    Ok(None)
}

#[allow(
    clippy::too_many_lines,
    reason = "Keep the native scanner's ordered rules and byte-level whitespace quirks together for review."
)]
pub(super) fn pretokenize(text: &[u8]) -> io::Result<Vec<&[u8]>> {
    let mut chunks = Vec::new();
    chunks
        .try_reserve_exact(text.len())
        .map_err(io::Error::other)?;
    let mut index = 0;
    while index < text.len() {
        let start = index;
        let (current, width) = codepoint(text, index)?;
        if matches!(current, 0x27 | 0x2019)
            && index + width < text.len()
            && let Some(end) = contraction(text, index + width)?
        {
            chunks.push(&text[index..end]);
            index = end;
            continue;
        }
        if letter(current) {
            index = letters_end(text, index + width)?;
            chunks.push(&text[start..index]);
            continue;
        }
        if !newline(current) && !letter(current) && !digit(current) && !whitespace(current) {
            let after = index + width;
            if after < text.len() {
                let (next, next_width) = codepoint(text, after)?;
                if letter(next) {
                    index = letters_end(text, after + next_width)?;
                    chunks.push(&text[start..index]);
                    continue;
                }
            }
        }
        if digit(current) {
            while index < text.len() && digit(u32::from(text[index])) {
                chunks.push(&text[index..=index]);
                index += 1;
            }
            continue;
        }
        if newline(current) {
            while index < text.len() && newline(u32::from(text[index])) {
                index += 1;
            }
            chunks.push(&text[start..index]);
            continue;
        }
        if whitespace(current) {
            let mut whitespace_end = index + width;
            while whitespace_end < text.len()
                && whitespace(u32::from(text[whitespace_end]))
                && !newline(u32::from(text[whitespace_end]))
            {
                whitespace_end += 1;
            }
            let followed = whitespace_end < text.len()
                && !whitespace(u32::from(text[whitespace_end]))
                && !newline(u32::from(text[whitespace_end]));
            if followed && whitespace_end - start > 1 {
                index = whitespace_end - 1;
                chunks.push(&text[start..index]);
                continue;
            }
            index = start + width;
            if index < text.len() {
                let (next, next_width) = codepoint(text, index)?;
                if letter(next) {
                    index = letters_end(text, index + next_width)?;
                    chunks.push(&text[start..index]);
                    continue;
                }
                if digit(next) {
                    chunks.push(&text[start..index]);
                    continue;
                }
                if !whitespace(next) && !newline(next) {
                    while index < text.len() {
                        let (next, next_width) = codepoint(text, index)?;
                        if whitespace(next) || letter(next) || digit(next) {
                            break;
                        }
                        index += next_width;
                    }
                    while index < text.len() && newline(u32::from(text[index])) {
                        index += 1;
                    }
                    chunks.push(&text[start..index]);
                    continue;
                }
            }
            index = whitespace_end;
            while index < text.len() {
                let (next, next_width) = codepoint(text, index)?;
                if !whitespace(next) {
                    break;
                }
                index += next_width;
            }
            chunks.push(&text[start..index]);
            continue;
        }
        index += width;
        while index < text.len() {
            let (next, next_width) = codepoint(text, index)?;
            if whitespace(next) || letter(next) || digit(next) || newline(next) {
                break;
            }
            index += next_width;
        }
        while index < text.len() && newline(u32::from(text[index])) {
            index += 1;
        }
        chunks.push(&text[start..index]);
    }
    Ok(chunks)
}
