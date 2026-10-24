#![forbid(unsafe_code)]
//! Focused verified-artifact checks of reusable tokenization and accounting.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use bonsai_prism_runtime::Result;
use bonsai_prism_runtime::model::Model;
use bonsai_prism_runtime::special_input::SpecialInputPolicy;
use bonsai_prism_runtime::tokenization::{
    MAX_TOKENIZATION_CONTEXT, SpecialTokenInsertion, TextTokenizationPolicy, TokenBudget,
    TokenizationSession,
};
use sha2::{Digest, Sha256};

const HEADER_BUDGET: usize = 128 * 1024;
static NEXT_ARTIFACT: AtomicU64 = AtomicU64::new(0);

struct Artifact {
    root: PathBuf,
    path: PathBuf,
    bytes: Vec<u8>,
}

impl Artifact {
    fn new(vocab_size: u64, eos: u32, pre: &str) -> Result<Self> {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let sequence = NEXT_ARTIFACT.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "SwiftBonsai-TokenizationSession-{}-{nonce}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&root)?;
        let path = root.join("model.gguf");
        let mut bytes = Vec::new();
        u32_bytes(&mut bytes, 0x4655_4747);
        u32_bytes(&mut bytes, 3);
        u64_bytes(&mut bytes, 1);
        u64_bytes(&mut bytes, 10);
        string_metadata(
            &mut bytes,
            "general.architecture",
            "unsupported-for-generation",
        )?;
        string_metadata(&mut bytes, "tokenizer.ggml.model", "gpt2")?;
        string_metadata(&mut bytes, "tokenizer.ggml.pre", pre)?;
        text(&mut bytes, "tokenizer.ggml.tokens")?;
        u32_bytes(&mut bytes, 9);
        u32_bytes(&mut bytes, 8);
        u64_bytes(&mut bytes, 260);
        let mut next = 256_u32;
        for byte in u8::MIN..=u8::MAX {
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
            let character = char::from_u32(scalar).ok_or("invalid fixture byte alphabet")?;
            text(&mut bytes, &character.to_string())?;
        }
        for token in ["ab", "a!", "<stop>", "<tag>"] {
            text(&mut bytes, token)?;
        }
        text(&mut bytes, "tokenizer.ggml.token_type")?;
        u32_bytes(&mut bytes, 9);
        u32_bytes(&mut bytes, 5);
        u64_bytes(&mut bytes, 260);
        for _ in 0..258 {
            bytes.extend(1_i32.to_le_bytes());
        }
        for kind in [3_i32, 4] {
            bytes.extend(kind.to_le_bytes());
        }
        text(&mut bytes, "tokenizer.ggml.merges")?;
        u32_bytes(&mut bytes, 9);
        u32_bytes(&mut bytes, 8);
        u64_bytes(&mut bytes, 2);
        text(&mut bytes, "a b")?;
        text(&mut bytes, "a !")?;
        for (key, token) in [
            ("tokenizer.ggml.eos_token_id", eos),
            ("tokenizer.ggml.bos_token_id", 259),
        ] {
            text(&mut bytes, key)?;
            u32_bytes(&mut bytes, 4);
            u32_bytes(&mut bytes, token);
        }
        for key in [
            "tokenizer.ggml.add_bos_token",
            "tokenizer.ggml.add_eos_token",
        ] {
            text(&mut bytes, key)?;
            u32_bytes(&mut bytes, 7);
            bytes.push(1);
        }
        text(&mut bytes, "token_embd.weight")?;
        u32_bytes(&mut bytes, 2);
        u64_bytes(&mut bytes, 1024);
        u64_bytes(&mut bytes, vocab_size);
        u32_bytes(&mut bytes, 0);
        u64_bytes(&mut bytes, 0);
        let data_offset = bytes.len().div_ceil(32) * 32;
        let payload_bytes = usize::try_from(vocab_size)? * 1024 * 4;
        bytes.resize(data_offset + payload_bytes, 0);
        bytes[data_offset..data_offset + 4].copy_from_slice(&f32::NAN.to_le_bytes());
        fs::write(&path, &bytes)?;
        Ok(Self { root, path, bytes })
    }

    fn hash(&self) -> String {
        Sha256::digest(&self.bytes)
            .into_iter()
            .flat_map(|byte| [byte >> 4, byte & 15])
            .map(|nibble| char::from(b"0123456789abcdef"[usize::from(nibble)]))
            .collect()
    }

    fn load(&self) -> Result<TokenizationSession> {
        TokenizationSession::load(
            &self.path,
            u64::try_from(self.bytes.len())?,
            &self.hash(),
            HEADER_BUDGET,
        )
    }
}

impl Drop for Artifact {
    fn drop(&mut self) {
        // Remove only the exact file and directory owned by this fixture.
        let _ = fs::remove_file(&self.path);
        let _ = fs::remove_dir(&self.root);
    }
}

fn u32_bytes(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend(value.to_le_bytes());
}

fn u64_bytes(bytes: &mut Vec<u8>, value: u64) {
    bytes.extend(value.to_le_bytes());
}

fn text(bytes: &mut Vec<u8>, value: &str) -> Result<()> {
    u64_bytes(bytes, u64::try_from(value.len())?);
    bytes.extend(value.as_bytes());
    Ok(())
}

fn string_metadata(bytes: &mut Vec<u8>, key: &str, value: &str) -> Result<()> {
    text(bytes, key)?;
    u32_bytes(bytes, 8);
    text(bytes, value)
}

const fn policy(context: usize, reserved_output_tokens: usize) -> TextTokenizationPolicy {
    TextTokenizationPolicy {
        special_input: SpecialInputPolicy::Literal,
        insert_special: SpecialTokenInsertion::None,
        budget: TokenBudget {
            context,
            reserved_output_tokens,
        },
    }
}

fn error<T>(result: Result<T>) -> Result<String> {
    Ok(result
        .err()
        .ok_or("invalid request was accepted")?
        .to_string())
}

#[test]
fn session_reuses_owned_tokenizer_after_artifact_removal_without_loading_weights() -> Result<()> {
    let artifact = Artifact::new(260, 258, "qwen35")?;
    assert!(artifact.bytes.len() > HEADER_BUDGET);
    let session = artifact.load()?;
    assert_eq!(session.identity(), artifact.hash());
    assert_eq!(session.vocab_size(), 260);
    assert_eq!(session.special_tokens().stop_tokens(), [258]);
    assert!(
        error(Model::load(
            &artifact.path,
            u64::try_from(artifact.bytes.len())?,
            &artifact.hash(),
            2 * 1024 * 1024,
        ))?
        .contains("nonfinite executable tensor payload")
    );
    // A retained Windows file handle would prevent deleting this artifact.
    fs::remove_file(&artifact.path)?;
    let first = session.encode("ab a!", policy(5, 1))?;
    assert_eq!(first.token_ids, [256, 32, 97, 33]);
    assert_eq!(first.prompt_token_count(), 4);
    assert_eq!(first.context, 5);
    assert_eq!(first.reserved_output_tokens, 1);
    assert_eq!(session.encode("ab a!", policy(5, 1))?, first);
    let exact_text = session.encode("ab\r\né", policy(5, 0))?;
    assert_eq!(exact_text.token_ids, [256, 13, 10, 195, 169]);
    Ok(())
}

#[test]
fn session_keeps_special_parsing_insertion_and_empty_input_explicit() -> Result<()> {
    let artifact = Artifact::new(260, 258, "qwen35")?;
    let session = artifact.load()?;
    let mut request = policy(16, 1);
    let literal = session.encode("<stop>", request)?;
    assert_eq!(literal.token_ids, [60, 115, 116, 111, 112, 62]);
    request.special_input = SpecialInputPolicy::Parse;
    assert_eq!(session.encode("<stop>", request)?.token_ids, [258]);
    request.insert_special = SpecialTokenInsertion::Model;
    assert_eq!(
        session.encode("<stop>", request)?.token_ids,
        [259, 258, 258]
    );
    assert_eq!(session.encode("", request)?.token_ids, [259, 258]);
    assert_eq!(session.encode("", policy(1, 0))?.token_ids, [0_u32; 0]);
    request.budget.context = 2;
    assert!(error(session.encode("", request))?.contains("insertions exceed token limit"));
    Ok(())
}

#[test]
fn session_checks_context_and_reservation_before_encoding_and_remains_reusable() -> Result<()> {
    let artifact = Artifact::new(260, 258, "qwen35")?;
    let session = artifact.load()?;
    let oversized_text = "a".repeat(1024 * 1024 + 1);
    for (context, reservation) in [
        (0, 0),
        (MAX_TOKENIZATION_CONTEXT + 1, 0),
        (usize::MAX, 0),
        (1, 1),
        (1, usize::MAX),
    ] {
        let request = policy(context, reservation);
        assert!(
            error(session.encode(&oversized_text, request))?
                .contains("invalid tokenization context or output reservation")
        );
        assert!(
            error(session.check_tokens(&[u32::MAX], request.budget))?
                .contains("invalid tokenization context or output reservation")
        );
    }
    assert!(
        error(session.encode(&oversized_text, policy(8, 0)))?.contains("input exceeds byte limit")
    );
    assert!(error(session.encode("ab a!", policy(4, 1)))?.contains("token limit"));
    let accepted = session.encode("ab a!", policy(5, 1))?;
    assert_eq!(accepted.token_ids, [256, 32, 97, 33]);
    assert_eq!(
        policy(MAX_TOKENIZATION_CONTEXT, 0)
            .budget
            .prompt_token_limit()?,
        MAX_TOKENIZATION_CONTEXT
    );
    Ok(())
}

#[test]
fn raw_ids_share_accounting_without_special_insertion_or_parsing() -> Result<()> {
    let artifact = Artifact::new(260, 258, "qwen35")?;
    let session = artifact.load()?;
    let budget = policy(4, 1).budget;
    let source = [0, 258, 259];
    let result = session.check_tokens(&source, budget)?;
    assert_eq!(result.token_ids, source);
    assert_eq!(result.prompt_token_count(), 3);
    assert_eq!(result.context, 4);
    assert_eq!(result.reserved_output_tokens, 1);
    assert!(error(session.check_tokens(&[0, 1, 2, 3], budget))?.contains("token limit"));
    assert!(error(session.check_tokens(&[260], budget))?.contains("outside the model vocabulary"));
    assert!(
        error(session.check_tokens(&[u32::MAX], budget))?.contains("outside the model vocabulary")
    );
    assert_eq!(session.check_tokens(&[], budget)?.token_ids, [0_u32; 0]);
    assert_eq!(session.check_tokens(&source, budget)?, result);
    Ok(())
}

#[test]
fn borrowed_raw_validation_checks_budget_and_vocabulary_without_changing_input() -> Result<()> {
    let artifact = Artifact::new(260, 258, "qwen35")?;
    let session = artifact.load()?;
    let budget = policy(4, 1).budget;
    let source = [259, 259, 0];
    session.validate_tokens(&source, budget)?;
    assert_eq!(source, [259, 259, 0]);
    session.validate_tokens(&[], budget)?;
    assert!(error(session.validate_tokens(&[u32::MAX; 4], budget))?.contains("token limit"));
    for source in [&[260][..], &[u32::MAX]] {
        assert!(
            error(session.validate_tokens(source, budget))?
                .contains("outside the model vocabulary")
        );
    }
    for budget in [policy(0, 0).budget, policy(1, 1).budget] {
        assert!(
            error(session.validate_tokens(&[u32::MAX], budget))?
                .contains("invalid tokenization context or output reservation")
        );
    }
    Ok(())
}

#[test]
fn session_load_rejects_identity_header_and_tokenizer_contract_failures() -> Result<()> {
    let artifact = Artifact::new(260, 258, "qwen35")?;
    let bytes = u64::try_from(artifact.bytes.len())?;
    let identity = artifact.hash();
    for (bytes, identity, memory) in [
        (bytes + 1, identity.clone(), HEADER_BUDGET),
        (bytes, "ab".repeat(32), HEADER_BUDGET),
        (bytes, identity, 1),
    ] {
        assert!(TokenizationSession::load(&artifact.path, bytes, &identity, memory).is_err());
    }
    let mismatched_vocab = Artifact::new(261, 258, "qwen35")?;
    assert!(error(mismatched_vocab.load())?.contains("vocabulary"));
    let invalid_special = Artifact::new(260, 260, "qwen35")?;
    assert!(error(invalid_special.load())?.contains("outside the decoder vocabulary"));
    let unsupported = Artifact::new(260, 258, "unsupported-pretokenizer")?;
    assert!(error(unsupported.load())?.contains("tokenizer.ggml.pre"));
    Ok(())
}
