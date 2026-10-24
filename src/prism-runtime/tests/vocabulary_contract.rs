#![forbid(unsafe_code)]
//! Shared strict vocabulary validation across independent and composed callers.

use bonsai_gguf::Metadata;
use bonsai_prism_runtime::Result;
use bonsai_prism_runtime::bpe::Bpe;
use bonsai_prism_runtime::special_input::SpecialInput;
use bonsai_prism_runtime::tokenizer::Tokenizer;
use bonsai_prism_runtime::vocabulary::{MAX_PIECE_BYTES, Vocabulary};

fn metadata(piece: &str, kind: i32) -> Metadata {
    Metadata::from([
        ("tokenizer.ggml.model".to_owned(), "gpt2".into()),
        ("tokenizer.ggml.pre".to_owned(), "qwen35".into()),
        ("tokenizer.ggml.tokens".to_owned(), vec!["a", piece].into()),
        (
            "tokenizer.ggml.token_type".to_owned(),
            vec![1_i32, kind].into(),
        ),
        (
            "tokenizer.ggml.merges".to_owned(),
            Vec::<String>::new().into(),
        ),
    ])
}

#[test]
fn every_constructor_validates_pieces_outside_its_own_token_types() -> Result<()> {
    for (piece, kind) in [
        ("b", 1),
        ("", 2),
        ("<control>", 3),
        ("<user>", 4),
        ("", 5),
        ("<0x41>", 6),
    ] {
        let values = metadata(piece, kind);
        Vocabulary::from_metadata(&values, 2)?;
        Bpe::from_metadata(&values, 2)?;
        SpecialInput::from_metadata(&values, 2)?;
        Tokenizer::from_metadata(&values, 2)?;
    }
    for (piece, kind) in [
        ("", 1),
        ("🦀", 1),
        ("", 3),
        ("", 4),
        ("<0xGG>", 6),
        ("<0x041>", 6),
        ("b", 7),
    ] {
        let values = metadata(piece, kind);
        assert!(
            Vocabulary::from_metadata(&values, 2).is_err(),
            "{piece:?}/{kind}"
        );
        assert!(Bpe::from_metadata(&values, 2).is_err(), "{piece:?}/{kind}");
        assert!(
            SpecialInput::from_metadata(&values, 2).is_err(),
            "{piece:?}/{kind}"
        );
        assert!(
            Tokenizer::from_metadata(&values, 2).is_err(),
            "{piece:?}/{kind}"
        );
    }
    Ok(())
}

#[test]
fn input_constructors_enforce_the_decoded_piece_limit_without_output_storage() {
    let piece = "b".repeat(MAX_PIECE_BYTES + 1);
    let values = metadata(&piece, 1);
    for result in [
        Bpe::from_metadata(&values, 2).map(|_| ()),
        SpecialInput::from_metadata(&values, 2).map(|_| ()),
        Tokenizer::from_metadata(&values, 2).map(|_| ()),
    ] {
        assert!(result.is_err_and(|error| error.to_string().contains("decoded byte limit")));
    }
}
