#![forbid(unsafe_code)]
//! Model-declared special-token validation independent of tokenizer behavior.

use bonsai_gguf::{Metadata, MetadataArray, MetadataValue};
use bonsai_prism_runtime::{Result, special_tokens::SpecialTokens};

const EOS: &str = "tokenizer.ggml.eos_token_id";
const BOS: &str = "tokenizer.ggml.bos_token_id";
const PADDING: &str = "tokenizer.ggml.padding_token_id";
const EOT: &str = "tokenizer.ggml.eot_token_id";
const EOM: &str = "tokenizer.ggml.eom_token_id";
const ADD_BOS: &str = "tokenizer.ggml.add_bos_token";
const ADD_EOS: &str = "tokenizer.ggml.add_eos_token";

fn metadata() -> Metadata {
    Metadata::from([(EOS.to_owned(), MetadataValue::U32(7))])
}

#[test]
fn eos_is_required_and_optional_declarations_do_not_get_guessed() -> Result<()> {
    assert!(SpecialTokens::from_metadata(&Metadata::new(), 8).is_err());
    assert!(SpecialTokens::from_metadata(&metadata(), 0).is_err());
    let mut metadata = metadata();
    metadata.insert(
        "tokenizer.ggml.tokens".to_owned(),
        MetadataValue::Array(MetadataArray::String(vec![
            "<eot>".to_owned(),
            "<eom>".to_owned(),
        ])),
    );
    let tokens = SpecialTokens::from_metadata(&metadata, 8)?;
    assert_eq!(tokens.vocab_size(), 8);
    assert_eq!(tokens.eos_token(), 7);
    assert_eq!(tokens.bos_token(), None);
    assert_eq!(tokens.padding_token(), None);
    assert_eq!(tokens.eot_token(), None);
    assert_eq!(tokens.eom_token(), None);
    assert_eq!(tokens.add_bos_token(), None);
    assert_eq!(tokens.add_eos_token(), None);
    assert_eq!(tokens.stop_tokens(), &[7]);
    Ok(())
}

#[test]
fn all_declared_ids_require_exact_uint32_type_and_vocabulary_bounds() {
    let wrong_types = [
        MetadataValue::U8(1),
        MetadataValue::U16(1),
        MetadataValue::U64(1),
        MetadataValue::I8(1),
        MetadataValue::I16(1),
        MetadataValue::I32(1),
        MetadataValue::I64(1),
        MetadataValue::I64(-1),
        MetadataValue::F32(1.0),
        MetadataValue::F64(1.0),
        MetadataValue::Bool(true),
        MetadataValue::String("1".to_owned()),
        MetadataValue::Array(MetadataArray::U32(vec![1])),
        MetadataValue::Array(MetadataArray::U32(Vec::new())),
    ];
    for key in [EOS, BOS, PADDING, EOT, EOM] {
        for value in &wrong_types {
            let mut metadata = metadata();
            metadata.insert(key.to_owned(), value.clone());
            assert!(
                SpecialTokens::from_metadata(&metadata, 8).is_err(),
                "accepted wrong GGUF type for {key}: {value:?}"
            );
        }
        for value in [8, u32::MAX] {
            let mut metadata = metadata();
            metadata.insert(key.to_owned(), MetadataValue::U32(value));
            assert!(SpecialTokens::from_metadata(&metadata, 8).is_err());
        }
    }
}

#[test]
fn zero_and_final_vocabulary_ids_are_admitted_and_aliases_are_valid() -> Result<()> {
    let mut metadata = metadata();
    metadata.insert(BOS.to_owned(), MetadataValue::U32(0));
    metadata.insert(PADDING.to_owned(), MetadataValue::U32(0));
    metadata.insert(EOT.to_owned(), MetadataValue::U32(7));
    metadata.insert(EOM.to_owned(), MetadataValue::U32(0));
    metadata.insert(ADD_BOS.to_owned(), MetadataValue::Bool(true));
    metadata.insert(ADD_EOS.to_owned(), MetadataValue::Bool(false));
    let tokens = SpecialTokens::from_metadata(&metadata, 8)?;
    assert_eq!(tokens.bos_token(), Some(0));
    assert_eq!(tokens.padding_token(), Some(0));
    assert_eq!(tokens.eot_token(), Some(7));
    assert_eq!(tokens.eom_token(), Some(0));
    assert_eq!(tokens.add_bos_token(), Some(true));
    assert_eq!(tokens.add_eos_token(), Some(false));
    assert_eq!(tokens.stop_tokens(), &[7, 0]);
    Ok(())
}

#[test]
fn explicit_stop_declarations_are_deduplicated_in_stable_order() -> Result<()> {
    for (turn, message, expected) in [
        (Some(1), Some(2), vec![7, 1, 2]),
        (Some(1), Some(1), vec![7, 1]),
        (Some(7), Some(7), vec![7]),
        (None, Some(2), vec![7, 2]),
        (Some(1), None, vec![7, 1]),
        (None, None, vec![7]),
    ] {
        let mut metadata = metadata();
        if let Some(token) = turn {
            metadata.insert(EOT.to_owned(), MetadataValue::U32(token));
        }
        if let Some(token) = message {
            metadata.insert(EOM.to_owned(), MetadataValue::U32(token));
        }
        let tokens = SpecialTokens::from_metadata(&metadata, 8)?;
        assert_eq!(tokens.stop_tokens(), expected);
    }
    Ok(())
}

#[test]
fn insertion_flags_require_exact_bool_and_enabled_bos_requires_its_id() -> Result<()> {
    for key in [ADD_BOS, ADD_EOS] {
        for value in [
            MetadataValue::U32(0),
            MetadataValue::U32(1),
            MetadataValue::String("true".to_owned()),
            MetadataValue::Array(MetadataArray::Bool(vec![true])),
        ] {
            let mut metadata = metadata();
            metadata.insert(key.to_owned(), value);
            assert!(SpecialTokens::from_metadata(&metadata, 8).is_err());
        }
    }
    let mut metadata = metadata();
    metadata.insert(ADD_BOS.to_owned(), MetadataValue::Bool(true));
    assert!(SpecialTokens::from_metadata(&metadata, 8).is_err());
    metadata.insert(ADD_BOS.to_owned(), MetadataValue::Bool(false));
    metadata.insert(ADD_EOS.to_owned(), MetadataValue::Bool(true));
    let tokens = SpecialTokens::from_metadata(&metadata, 8)?;
    assert_eq!(tokens.add_bos_token(), Some(false));
    assert_eq!(tokens.add_eos_token(), Some(true));
    assert_eq!(tokens.stop_tokens(), &[7]);
    Ok(())
}
