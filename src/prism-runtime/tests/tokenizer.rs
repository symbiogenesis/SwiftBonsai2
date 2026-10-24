#![forbid(unsafe_code)]
//! Focused end-to-end plain-text tokenizer composition regressions.

use bonsai_gguf::{Metadata, MetadataValue};
use bonsai_prism_runtime::Result;
use bonsai_prism_runtime::bpe::MAX_BPE_INPUT_BYTES;
use bonsai_prism_runtime::special_input::SpecialInputPolicy;
use bonsai_prism_runtime::tokenizer::Tokenizer;
use bonsai_prism_runtime::vocabulary::Vocabulary;

fn metadata() -> Result<Metadata> {
    let mut next = 256_u32;
    let mut tokens = (u8::MIN..=u8::MAX)
        .map(|byte| {
            let scalar = if (b'!'..=b'~').contains(&byte)
                || (0xa1..=0xac).contains(&byte)
                || (0xae..=0xff).contains(&byte)
            {
                u32::from(byte)
            } else {
                let scalar = next;
                next += 1;
                scalar
            };
            char::from_u32(scalar)
                .map(|character| character.to_string())
                .ok_or_else(|| "invalid fixture byte alphabet".into())
        })
        .collect::<Result<Vec<_>>>()?;
    tokens.extend(["ab", "a!", "<stop>", "<tag>"].map(str::to_owned));
    let mut kinds = vec![1_i32; 258];
    kinds.extend([3, 4]);
    Ok(Metadata::from([
        ("tokenizer.ggml.model".to_owned(), "gpt2".into()),
        ("tokenizer.ggml.pre".to_owned(), "qwen35".into()),
        ("tokenizer.ggml.tokens".to_owned(), tokens.into()),
        ("tokenizer.ggml.token_type".to_owned(), kinds.into()),
        (
            "tokenizer.ggml.merges".to_owned(),
            vec!["a b", "a !"].into(),
        ),
        (
            "tokenizer.ggml.bos_token_id".to_owned(),
            MetadataValue::U32(259),
        ),
        (
            "tokenizer.ggml.eos_token_id".to_owned(),
            MetadataValue::U32(258),
        ),
        ("tokenizer.ggml.add_bos_token".to_owned(), true.into()),
        ("tokenizer.ggml.add_eos_token".to_owned(), true.into()),
    ]))
}

fn decoded(vocabulary: &Vocabulary, tokens: &[u32]) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    for &token in tokens {
        bytes.extend_from_slice(vocabulary.piece(token)?);
    }
    Ok(bytes)
}

#[test]
fn text_encoding_respects_pretokenizer_boundaries_and_never_inserts_tokens() -> Result<()> {
    let tokenizer = Tokenizer::from_metadata(&metadata()?, 260)?;
    assert_eq!(
        tokenizer.encode("ab a!", 4, SpecialInputPolicy::Literal)?,
        [256, 32, 97, 33]
    );
    assert_eq!(tokenizer.encode("ab", 1, SpecialInputPolicy::Parse)?, [256]);
    assert!(
        tokenizer
            .encode("a!", 1, SpecialInputPolicy::Literal)
            .is_err()
    );
    assert_eq!(
        tokenizer.encode("", 0, SpecialInputPolicy::Literal)?,
        Vec::<u32>::new()
    );
    Ok(())
}

#[test]
fn text_encoding_specials_require_explicit_opt_in_and_preserve_surrounding_bytes() -> Result<()> {
    let values = metadata()?;
    let tokenizer = Tokenizer::from_metadata(&values, 260)?;
    let vocabulary = Vocabulary::from_metadata(&values, 260)?;
    let text = "ab<stop><tag>a!";
    let parsed = tokenizer.encode(text, 5, SpecialInputPolicy::Parse)?;
    assert_eq!(parsed, [256, 258, 259, 97, 33]);
    assert_eq!(decoded(&vocabulary, &parsed)?, text.as_bytes());
    let literal = tokenizer.encode(text, text.len(), SpecialInputPolicy::Literal)?;
    assert!(!literal.contains(&258));
    assert!(!literal.contains(&259));
    assert_eq!(decoded(&vocabulary, &literal)?, text.as_bytes());
    assert!(
        tokenizer
            .encode(text, 4, SpecialInputPolicy::Parse)
            .is_err()
    );
    // A rejected call must not retain a partial token stream or mutate rules.
    assert_eq!(
        tokenizer.encode(text, 5, SpecialInputPolicy::Parse)?,
        parsed
    );
    Ok(())
}

#[test]
fn text_encoding_unicode_and_control_bytes_round_trip_with_bounded_output() -> Result<()> {
    let values = metadata()?;
    let tokenizer = Tokenizer::from_metadata(&values, 260)?;
    let vocabulary = Vocabulary::from_metadata(&values, 260)?;
    for text in [
        " café\n\t\0",
        "λ🦀 中文",
        "I'LL revise １２３.",
        "a\u{301}\u{85}\u{a0}",
    ] {
        for policy in [SpecialInputPolicy::Literal, SpecialInputPolicy::Parse] {
            let tokens = tokenizer.encode(text, text.len(), policy)?;
            assert_eq!(decoded(&vocabulary, &tokens)?, text.as_bytes());
            assert!(tokenizer.encode(text, 0, policy).is_err());
        }
    }
    assert!(
        tokenizer
            .encode(
                &"a".repeat(MAX_BPE_INPUT_BYTES + 1),
                usize::MAX,
                SpecialInputPolicy::Literal
            )
            .is_err()
    );
    Ok(())
}

#[test]
fn text_encoding_charges_piece_limits_across_special_boundaries() -> Result<()> {
    let tokenizer = Tokenizer::from_metadata(&metadata()?, 260)?;
    let text = "1".repeat(65_536) + "<stop>1";
    let error = tokenizer
        .encode(&text, text.len(), SpecialInputPolicy::Parse)
        .err()
        .ok_or("global piece limit must include all ordinary and special segments")?;
    assert!(error.to_string().contains("total piece count"), "{error}");
    Ok(())
}
