#![forbid(unsafe_code)]
//! Focused production tokenizer regressions using immutable GGUF corpora.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use bonsai_gguf::{MetadataArray, MetadataValue, VerifiedModel};
use bonsai_prism_runtime::Result;
use bonsai_prism_runtime::tokenizer::gguf::GgufTokenizer;
use sha2::{Digest, Sha256};

fn windows_text(raw: &[u8]) -> Vec<u8> {
    raw.iter()
        .copied()
        .enumerate()
        .take_while(|(_, byte)| *byte != 0x1a)
        .filter_map(|(index, byte)| {
            (byte != b'\r' || raw.get(index + 1) != Some(&b'\n')).then_some(byte)
        })
        .collect()
}

fn check(path: &Path, expected_hash: &str) -> Result<Vec<String>> {
    let bytes = std::fs::read(path)?;
    let mut digest = String::new();
    for byte in Sha256::digest(&bytes) {
        write!(digest, "{byte:02x}")?;
    }
    assert_eq!(
        digest,
        expected_hash,
        "immutable fixture hash differs: {}",
        path.display()
    );
    let metadata = VerifiedModel::open(
        path,
        "Stock",
        u64::try_from(bytes.len())?,
        &digest,
        128 * 1024 * 1024,
    )?
    .into_metadata();
    let Some(MetadataArray::String(tokens)) = metadata
        .get("tokenizer.ggml.tokens")
        .and_then(MetadataValue::as_array)
    else {
        return Err("missing test tokens".into());
    };
    let tokenizer = GgufTokenizer::from_metadata(&metadata, tokens.len())?;
    let input = windows_text(&std::fs::read(path.with_extension("gguf.inp"))?);
    let output = windows_text(&std::fs::read(path.with_extension("gguf.out"))?);
    let separator = b"\n__ggml_vocab_test__\n";
    let mut inputs = Vec::new();
    let mut position = 0;
    while position < input.len() {
        if let Some(next) = input[position..]
            .windows(separator.len())
            .position(|bytes| bytes == separator)
        {
            inputs.push(&input[position..position + next]);
            position += next + separator.len();
        } else {
            inputs.push(&input[position..]);
            break;
        }
    }
    let mut lines: Vec<_> = output.split(|byte| *byte == b'\n').collect();
    if output.last() == Some(&b'\n') {
        lines.pop();
    }
    assert_eq!(inputs.len(), lines.len());
    let cases: BTreeMap<_, _> = inputs.into_iter().zip(lines).collect();
    let mut failures = Vec::new();
    for (index, (text, line)) in cases.into_iter().enumerate() {
        let expected = std::str::from_utf8(line)?
            .split_ascii_whitespace()
            .map(str::parse::<u32>)
            .collect::<std::result::Result<Vec<_>, _>>()?;
        match tokenizer.encode(text, false, false) {
            Ok(actual) if actual == expected => {}
            Ok(actual) => failures.push(format!(
                "{} case {index}: {:?}: expected {expected:?}, actual {actual:?}",
                path.display(),
                String::from_utf8_lossy(text)
            )),
            Err(error) => failures.push(format!("{} case {index}: {error}", path.display())),
        }
    }
    Ok(failures)
}

#[test]
#[ignore = "requires hash-verified original source assets in PRISM_TOKENIZER_SOURCE"]
fn original_bundled_vocabulary_corpora() -> Result<()> {
    let directory = std::env::var("PRISM_TOKENIZER_SOURCE")?;
    let names = [
        "bert-bge",
        "command-r",
        "deepseek-coder",
        "deepseek-llm",
        "falcon",
        "gemma-4",
        "gpt-2",
        "llama-bpe",
        "llama-spm",
        "mpt",
        "phi-3",
        "qwen2",
        "qwen35",
        "refact",
        "starcoder",
    ];
    let manifest: serde_json::Value = serde_json::from_str(include_str!(
        "../examples/upstream_prism/tokenizer-assets.json"
    ))?;
    let mut failures = Vec::new();
    for name in names {
        let path = Path::new(&directory).join(format!("models/ggml-vocab-{name}.gguf"));
        let hash = manifest["Assets"]
            .as_array()
            .and_then(|assets| assets.iter().find(|asset| asset["Name"] == name))
            .and_then(|asset| asset["Files"][0]["Sha256"].as_str())
            .ok_or("fixture hash missing")?;
        match check(&path, hash) {
            Ok(values) => failures.extend(values),
            Err(error) => failures.push(format!("{name}: {error}")),
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    Ok(())
}

#[test]
#[ignore = "requires hash-verified repository assets in PRISM_TOKENIZER_REPOSITORY"]
fn original_repository_vocabulary_corpora() -> Result<()> {
    let directory = std::env::var("PRISM_TOKENIZER_REPOSITORY")?;
    let names = [
        "HybridDNA/ggml-vocab-carbon",
        "PLaMo2/ggml-vocab-plamo2",
        "RWKV/ggml-vocab-rwkv-7-world",
        "SPM/ggml-vocab-gemma-3",
        "UGM/ggml-vocab-nomic-bert-moe",
        "WPM/ggml-vocab-jina-v2-en",
    ];
    let manifest: serde_json::Value = serde_json::from_str(include_str!(
        "../examples/upstream_prism/tokenizer-repository-assets.json"
    ))?;
    let mut failures = Vec::new();
    for name in names {
        let path = Path::new(&directory).join(format!("{name}.gguf"));
        let filename = format!("{name}.gguf");
        let hash = manifest["Files"]
            .as_array()
            .and_then(|files| files.iter().find(|file| file["Path"] == filename))
            .and_then(|file| file["Sha256"].as_str())
            .ok_or("fixture hash missing")?;
        match check(&path, hash) {
            Ok(values) => failures.extend(values),
            Err(error) => failures.push(format!("{name}: {error}")),
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    Ok(())
}
