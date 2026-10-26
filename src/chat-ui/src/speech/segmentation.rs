//! Shared Unicode clip bounds for complete and incremental read aloud.

use super::{MAX_SEGMENT, MAX_TEXT, VecDeque};

pub(super) fn segments(text: &str) -> Option<VecDeque<String>> {
    if text.trim().is_empty()
        || text.contains('\0')
        || text.encode_utf16().take(MAX_TEXT + 1).count() > MAX_TEXT
    {
        return None;
    }
    let mut remaining = text.trim();
    let mut result = VecDeque::new();
    while !remaining.is_empty() {
        let boundary =
            maximum(remaining).map_or(remaining.len(), |limit| word_boundary(remaining, limit));
        result.push_back(remaining[..boundary].trim().to_owned());
        remaining = remaining[boundary..].trim_start();
    }
    Some(result)
}

/// Return the byte offset only when another character exceeds the clip bound.
pub(super) fn maximum(text: &str) -> Option<usize> {
    text.char_indices().nth(MAX_SEGMENT).map(|(index, _)| index)
}

/// Prefer a word boundary in the latter half of the bounded UTF-8 prefix.
pub(super) fn word_boundary(text: &str, maximum: usize) -> usize {
    text[..maximum]
        .char_indices()
        .rev()
        .find(|(index, character)| *index >= maximum / 2 && character.is_whitespace())
        .map_or(maximum, |(index, _)| index)
}
