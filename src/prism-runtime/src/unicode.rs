//! Bounded UTF-8 regular-expression splitting and GPT byte encoding.

use regex::RegexBuilder;

use crate::{Result, invalid};

/// Splits text sequentially with each expression, retaining unmatched spans.
///
/// Expressions use the linear-time Rust regex syntax, including Unicode
/// categories. Each later expression subdivides the existing pieces without
/// recombining them. Optional GPT byte encoding maps every original UTF-8 byte
/// to its reversible vocabulary character. No normalization is performed.
///
/// # Errors
/// Rejects unsupported regex syntax and bounded input/expression/output limits.
pub fn regex_split(text: &str, expressions: &[&str], byte_encoding: bool) -> Result<Vec<String>> {
    if text.len() > 65536
        || expressions.is_empty()
        || expressions.len() > 16
        || expressions.iter().any(|expression| expression.len() > 8192)
    {
        return Err(invalid("Unicode split input or expression bound exceeded"));
    }
    let mut pieces = if text.is_empty() {
        Vec::new()
    } else {
        vec![text]
    };
    for expression in expressions {
        let regex = RegexBuilder::new(expression)
            .size_limit(8 * 1024 * 1024)
            .build()?;
        let mut next = Vec::new();
        for piece in pieces {
            let mut end = 0;
            for matched in regex.find_iter(piece) {
                if matched.start() > end {
                    next.push(&piece[end..matched.start()]);
                }
                if !matched.is_empty() {
                    next.push(matched.as_str());
                }
                end = matched.end();
            }
            if end < piece.len() {
                next.push(&piece[end..]);
            }
            if next.len() > 65536 {
                return Err(invalid("Unicode split output piece bound exceeded"));
            }
        }
        pieces = next;
    }
    if byte_encoding {
        let mut missing = 256_u32;
        let mut table = ['\0'; 256];
        for byte in 0..=u8::MAX {
            let codepoint =
                if (b'!'..=b'~').contains(&byte) || (0xa1..=0xac).contains(&byte) || byte >= 0xae {
                    u32::from(byte)
                } else {
                    let codepoint = missing;
                    missing += 1;
                    codepoint
                };
            table[usize::from(byte)] =
                char::from_u32(codepoint).ok_or_else(|| invalid("invalid byte encoding scalar"))?;
        }
        Ok(pieces
            .into_iter()
            .map(|piece| piece.bytes().map(|byte| table[usize::from(byte)]).collect())
            .collect())
    } else {
        Ok(pieces.into_iter().map(str::to_owned).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regex_keeps_unmatched_text_and_subdivides_without_loss() -> Result<()> {
        assert_eq!(
            regex_split(" ~foo", &[r"[~][A-Za-z]+| ?[\p{S}]+|\s+"], false)?,
            [" ~", "foo"]
        );
        assert_eq!(
            regex_split("ab12 cd", &[r"\w+", r"[0-9]+"], false)?.concat(),
            "ab12 cd"
        );
        assert_eq!(regex_split(" ", &[r"\s+"], true)?, ["Ġ"]);
        assert!(regex_split("text", &["["], false).is_err());
        Ok(())
    }
}
