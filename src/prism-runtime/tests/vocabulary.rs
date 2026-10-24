#![forbid(unsafe_code)]
//! Focused output vocabulary validation and lossless byte decoding regressions.

use bonsai_gguf::{Metadata, MetadataArray, MetadataValue};
use bonsai_prism_runtime::Result;
use bonsai_prism_runtime::vocabulary::{
    MAX_PIECE_BYTES, MAX_VOCABULARY_BYTES, MAX_VOCABULARY_TOKENS, Vocabulary,
};

fn metadata(tokens: &[(&str, i32)]) -> Metadata {
    Metadata::from([
        ("tokenizer.ggml.model".to_owned(), "gpt2".into()),
        ("tokenizer.ggml.pre".to_owned(), "qwen35".into()),
        (
            "tokenizer.ggml.tokens".to_owned(),
            tokens
                .iter()
                .map(|(text, _)| *text)
                .collect::<Vec<_>>()
                .into(),
        ),
        (
            "tokenizer.ggml.token_type".to_owned(),
            tokens
                .iter()
                .map(|(_, kind)| *kind)
                .collect::<Vec<_>>()
                .into(),
        ),
    ])
}

// Independent forward GPT-2 alphabet construction: preserve the printable byte
// ranges, then assign all remaining bytes consecutive scalars above Latin-1.
fn encode_bytes(bytes: &[u8]) -> Result<String> {
    let printable = (b'!'..=b'~')
        .chain(0xa1..=0xac)
        .chain(0xae..=0xff)
        .collect::<Vec<_>>();
    let mut alphabet = [None; 256];
    for &byte in &printable {
        alphabet[usize::from(byte)] = Some(char::from(byte));
    }
    let mut next = 256_u32;
    for byte in u8::MIN..=u8::MAX {
        if !printable.contains(&byte) {
            alphabet[usize::from(byte)] = char::from_u32(next);
            next += 1;
        }
    }
    bytes
        .iter()
        .map(|&byte| alphabet[usize::from(byte)].ok_or_else(|| "invalid forward alphabet".into()))
        .collect()
}

#[test]
fn all_256_bytes_round_trip_the_complete_gpt2_alphabet() -> Result<()> {
    let expected = (u8::MIN..=u8::MAX).collect::<Vec<_>>();
    let encoded = encode_bytes(&expected)?;
    let vocabulary = Vocabulary::from_metadata(&metadata(&[(&encoded, 1)]), 1)?;
    assert_eq!(vocabulary.vocab_size(), 1);
    assert_eq!(vocabulary.piece(0)?, expected);
    assert!(!vocabulary.is_control(0)?);
    Ok(())
}

#[test]
fn normal_pieces_restore_spaces_newlines_tabs_and_nul() -> Result<()> {
    let vocabulary = Vocabulary::from_metadata(&metadata(&[("ĠhelloĊĉčĀ", 1)]), 1)?;
    assert_eq!(vocabulary.piece(0)?, b" hello\n\t\r\0");
    Ok(())
}

#[test]
fn split_unicode_bytes_are_preserved_until_joined() -> Result<()> {
    let encoded = "λ🦀"
        .as_bytes()
        .iter()
        .map(|&byte| encode_bytes(&[byte]))
        .collect::<Result<Vec<_>>>()?;
    let records = encoded
        .iter()
        .map(|piece| (piece.as_str(), 1))
        .collect::<Vec<_>>();
    let vocabulary = Vocabulary::from_metadata(&metadata(&records), records.len())?;
    let mut decoded = Vec::new();
    for id in 0..u32::try_from(records.len())? {
        let piece = vocabulary.piece(id)?;
        assert_eq!(piece.len(), 1);
        assert!(std::str::from_utf8(piece).is_err());
        decoded.extend_from_slice(piece);
    }
    assert_eq!(std::str::from_utf8(&decoded)?, "λ🦀");
    Ok(())
}

#[test]
fn declared_types_control_literal_and_byte_piece_behavior() -> Result<()> {
    let records = [
        ("<|im_end|>", 1),
        ("<|im_end|>", 3),
        ("Ġλ", 4),
        ("<0x00>", 6),
        ("<0xFf>", 6),
        ("<unknown>", 2),
        ("", 5),
    ];
    let vocabulary = Vocabulary::from_metadata(&metadata(&records), records.len())?;
    assert_eq!(vocabulary.piece(0)?, b"<|im_end|>");
    assert_eq!(vocabulary.piece(1)?, b"<|im_end|>");
    assert!(!vocabulary.is_control(0)?);
    assert!(vocabulary.is_control(1)?);
    assert_eq!(vocabulary.piece(2)?, "Ġλ".as_bytes());
    assert_eq!(vocabulary.piece(3)?, &[0]);
    assert_eq!(vocabulary.piece(4)?, &[255]);
    assert!(vocabulary.piece(5).is_err());
    assert!(vocabulary.piece(6).is_err());
    assert!(!vocabulary.is_control(5)?);
    assert!(!vocabulary.is_control(6)?);
    assert!(vocabulary.piece(7).is_err());
    assert!(vocabulary.is_control(7).is_err());
    assert!(vocabulary.piece(u32::MAX).is_err());
    assert!(vocabulary.is_control(u32::MAX).is_err());
    Ok(())
}

#[test]
fn unsupported_or_missing_tokenizer_contract_is_rejected() {
    for (key, value) in [
        ("tokenizer.ggml.model", "llama".into()),
        ("tokenizer.ggml.model", MetadataValue::U32(1)),
        ("tokenizer.ggml.pre", "qwen2".into()),
        ("tokenizer.ggml.pre", MetadataValue::Bool(true)),
    ] {
        let mut values = metadata(&[("a", 1)]);
        values.insert(key.to_owned(), value);
        assert!(Vocabulary::from_metadata(&values, 1).is_err());
    }
    for key in ["tokenizer.ggml.model", "tokenizer.ggml.pre"] {
        let mut values = metadata(&[("a", 1)]);
        values.remove(key);
        assert!(Vocabulary::from_metadata(&values, 1).is_err());
    }
}

#[test]
fn token_and_type_arrays_must_have_exact_types_and_graph_counts() {
    let valid = metadata(&[("a", 1)]);
    for count in [0, 2, MAX_VOCABULARY_TOKENS + 1] {
        assert!(Vocabulary::from_metadata(&valid, count).is_err());
    }
    for (key, value) in [
        (
            "tokenizer.ggml.tokens",
            MetadataValue::String("a".to_owned()),
        ),
        ("tokenizer.ggml.tokens", vec![1_i32].into()),
        ("tokenizer.ggml.tokens", Vec::<String>::new().into()),
        ("tokenizer.ggml.token_type", vec![1_u32].into()),
        ("tokenizer.ggml.token_type", vec![1_i64].into()),
        ("tokenizer.ggml.token_type", vec![1_f32].into()),
        ("tokenizer.ggml.token_type", vec![true].into()),
        ("tokenizer.ggml.token_type", vec![1_i32, 1].into()),
        ("tokenizer.ggml.token_type", Vec::<i32>::new().into()),
    ] {
        let mut values = valid.clone();
        values.insert(key.to_owned(), value);
        assert!(Vocabulary::from_metadata(&values, 1).is_err());
    }
    for key in ["tokenizer.ggml.tokens", "tokenizer.ggml.token_type"] {
        let mut values = valid.clone();
        values.remove(key);
        assert!(Vocabulary::from_metadata(&values, 1).is_err());
    }
}

#[test]
fn malformed_piece_spelling_and_unknown_type_codes_fail_closed() {
    for token in ["", " ", "\n", "\0", "λ", "🦀", "Ŕ"] {
        assert!(Vocabulary::from_metadata(&metadata(&[(token, 1)]), 1).is_err());
    }
    for token in [
        "", "<0x0>", "<0x100>", "<0xGG>", "<0xλ>", "<0XFF>", "x<0xFF>", "<0xFF>x",
    ] {
        assert!(Vocabulary::from_metadata(&metadata(&[(token, 6)]), 1).is_err());
    }
    for kind in [0, 7, -1, i32::MAX] {
        assert!(Vocabulary::from_metadata(&metadata(&[("a", kind)]), 1).is_err());
    }
    for kind in [3, 4] {
        assert!(Vocabulary::from_metadata(&metadata(&[("", kind)]), 1).is_err());
    }
}

#[test]
fn piece_limits_are_checked_before_decoded_storage_is_allocated() -> Result<()> {
    let at_limit = "a".repeat(MAX_PIECE_BYTES);
    let vocabulary = Vocabulary::from_metadata(&metadata(&[(&at_limit, 1)]), 1)?;
    assert_eq!(vocabulary.piece(0)?.len(), MAX_PIECE_BYTES);
    let too_long = "a".repeat(MAX_PIECE_BYTES + 1);
    for kind in [1, 3, 4] {
        assert!(Vocabulary::from_metadata(&metadata(&[(&too_long, kind)]), 1).is_err());
    }
    Ok(())
}

#[test]
fn aggregate_vocabulary_bytes_are_bounded() -> Result<()> {
    let count = MAX_VOCABULARY_BYTES / MAX_PIECE_BYTES;
    let mut tokens = vec!["a".repeat(MAX_PIECE_BYTES); count];
    tokens.push("a".to_owned());
    let values = Metadata::from([
        ("tokenizer.ggml.model".to_owned(), "gpt2".into()),
        ("tokenizer.ggml.pre".to_owned(), "qwen35".into()),
        ("tokenizer.ggml.tokens".to_owned(), tokens.into()),
        (
            "tokenizer.ggml.token_type".to_owned(),
            MetadataValue::Array(MetadataArray::I32(vec![4; count + 1])),
        ),
    ]);
    let Err(error) = Vocabulary::from_metadata(&values, count + 1) else {
        return Err("over-limit decoded vocabulary was accepted".into());
    };
    assert!(error.to_string().contains("decoded byte limit"));
    Ok(())
}
