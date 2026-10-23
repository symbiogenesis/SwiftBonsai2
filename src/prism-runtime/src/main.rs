#![forbid(unsafe_code)]
//! Explicit token-ID driver for the unqualified native CPU reference engine.

use bonsai_prism_runtime::{
    Result,
    graph::Qwen35,
    model::Model,
    runtime::{Cancellation, Finish, Generation, Limits, Runtime},
};
use serde_json::json;
use std::{collections::BTreeMap, io::Write, path::PathBuf, process::ExitCode};

const USAGE: &str = "Native CPU reference; full-model parity and performance are unqualified.\n\
Usage: bonsai-prism-runtime --model <absolute-gguf-path> --bytes <u64>\n\
  --sha256 <64-hex> --memory-bytes <usize> --context <1..262144>\n\
  --checkpoint-bytes <usize> --max-tokens <usize> --tokens <id,id,...>\n\
  [--stop-tokens <id,id,...>]\n\
Input is token IDs; decoding is greedy. Output is one JSON object per line.\n";

struct Arguments {
    path: PathBuf,
    bytes: u64,
    sha256: String,
    memory: usize,
    context: usize,
    checkpoint_bytes: usize,
    generation: Generation,
    tokens: Vec<u32>,
}

fn bad(message: &'static str) -> bonsai_prism_runtime::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidInput, message).into()
}

fn parse(arguments: impl IntoIterator<Item = String>) -> Result<Arguments> {
    let mut values = BTreeMap::new();
    let mut args = arguments.into_iter();
    while let Some(key) = args.next() {
        if !matches!(
            key.as_str(),
            "--model"
                | "--bytes"
                | "--sha256"
                | "--memory-bytes"
                | "--context"
                | "--checkpoint-bytes"
                | "--max-tokens"
                | "--tokens"
                | "--stop-tokens"
        ) || values.contains_key(&key)
        {
            return Err(bad("unknown or repeated option"));
        }
        let value = args.next().ok_or_else(|| bad("missing option value"))?;
        if value.len() > 4 * 1024 * 1024 {
            return Err(bad("option exceeds bounded input size"));
        }
        values.insert(key, value);
    }
    let mut required = |key: &str| {
        values
            .remove(key)
            .ok_or_else(|| bad("missing required option"))
    };
    let path = PathBuf::from(required("--model")?);
    if !path.is_absolute() {
        return Err(bad("model path must be absolute"));
    }
    let bytes = required("--bytes")?.parse::<u64>()?;
    let sha256 = required("--sha256")?;
    let memory = required("--memory-bytes")?.parse::<usize>()?;
    let context = required("--context")?.parse::<usize>()?;
    let checkpoint_bytes = required("--checkpoint-bytes")?.parse::<usize>()?;
    let max_tokens = required("--max-tokens")?.parse::<usize>()?;
    // Reject model-independent errors before opening/hashing a multi-GiB file
    // or allocating model and graph storage. Vocabulary checks still require
    // the verified model and remain the runtime's responsibility.
    if bytes == 0
        || sha256.len() != 64
        || !sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
        || memory == 0
        || !(1..=262_144).contains(&context)
        || checkpoint_bytes == 0
        || max_tokens == 0
        || max_tokens >= context
    {
        return Err(bad("invalid artifact identity or native resource bounds"));
    }
    let tokens = token_ids(&required("--tokens")?, context - max_tokens)?;
    let stop_tokens = values
        .remove("--stop-tokens")
        .map(|text| token_ids(&text, 256))
        .transpose()?
        .unwrap_or_default();
    Ok(Arguments {
        path,
        bytes,
        sha256,
        memory,
        context,
        checkpoint_bytes,
        generation: Generation {
            max_tokens,
            stop_tokens,
        },
        tokens,
    })
}

fn token_ids(text: &str, limit: usize) -> Result<Vec<u32>> {
    let mut result = Vec::new();
    for token in text.split(',') {
        if result.len() == limit
            || token.is_empty()
            || !token.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(bad("invalid or oversized token sequence"));
        }
        result.try_reserve(1)?;
        result.push(token.parse()?);
    }
    Ok(result)
}

fn execute(arguments: &Arguments) -> Result<()> {
    let model = Model::load(
        &arguments.path,
        arguments.bytes,
        &arguments.sha256,
        arguments.memory,
    )?;
    let decoder = Qwen35::new(model, arguments.context)?;
    let mut runtime = Runtime::new(
        decoder,
        Limits {
            context_tokens: arguments.context,
            checkpoints: 0,
            checkpoint_bytes: arguments.checkpoint_bytes,
        },
    )?;
    let mut output = std::io::stdout().lock();
    let result = runtime.generate(
        &arguments.tokens,
        &arguments.generation,
        &Cancellation::default(),
        |token| {
            serde_json::to_writer(&mut output, &json!({"token": token}))?;
            output.write_all(b"\n")?;
            output.flush()?;
            Ok(())
        },
    )?;
    serde_json::to_writer(
        &mut output,
        &json!({
            "finish_reason": match result.finish { Finish::Stop => "stop", Finish::Length => "length" },
            "prompt_tokens": result.prompt_tokens,
            "generated_tokens": result.generated_tokens,
            "reused_tokens": result.reused_tokens,
        }),
    )?;
    output.write_all(b"\n")?;
    output.flush()?;
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() == 1 && args[0] == "--help" {
        return match std::io::stdout().lock().write_all(USAGE.as_bytes()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(_) => ExitCode::FAILURE,
        };
    }
    match parse(args).and_then(|arguments| execute(&arguments)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            // No argument, prompt, token sequence or credential is logged.
            let _written = writeln!(std::io::stderr().lock(), "native runtime failed: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Result, bad, parse};

    fn arguments() -> Vec<String> {
        [
            "--model".to_owned(),
            std::env::temp_dir()
                .join("unopened-native-model.gguf")
                .to_string_lossy()
                .into_owned(),
            "--bytes".to_owned(),
            "32".to_owned(),
            "--sha256".to_owned(),
            "ab".repeat(32),
            "--memory-bytes".to_owned(),
            "4096".to_owned(),
            "--context".to_owned(),
            "8".to_owned(),
            "--checkpoint-bytes".to_owned(),
            "4096".to_owned(),
            "--max-tokens".to_owned(),
            "1".to_owned(),
            "--tokens".to_owned(),
            "1,2".to_owned(),
        ]
        .into()
    }

    fn replace(arguments: &mut [String], key: &str, value: &str) -> Result<()> {
        let position = arguments
            .iter()
            .position(|argument| argument == key)
            .ok_or_else(|| bad("missing test argument"))?;
        arguments[position + 1] = value.to_owned();
        Ok(())
    }

    #[test]
    fn cli_preflight_rejects_invalid_identity_and_resource_bounds() -> Result<()> {
        for (key, value) in [
            ("--bytes", "0"),
            ("--sha256", "bad"),
            ("--sha256", &"z".repeat(64)),
            ("--memory-bytes", "0"),
            ("--context", "0"),
            ("--context", "262145"),
            ("--checkpoint-bytes", "0"),
            ("--max-tokens", "0"),
            ("--max-tokens", "8"),
            ("--max-tokens", &usize::MAX.to_string()),
        ] {
            let mut input = arguments();
            replace(&mut input, key, value)?;
            assert!(parse(input).is_err(), "accepted invalid {key}");
        }
        Ok(())
    }

    #[test]
    fn cli_preflight_bounds_prompt_and_stop_lists_before_model_loading() -> Result<()> {
        let mut input = arguments();
        replace(&mut input, "--tokens", "0,1,2,3,4,5,6")?;
        assert!(parse(input.clone()).is_ok());
        replace(&mut input, "--tokens", "0,1,2,3,4,5,6,7")?;
        assert!(parse(input).is_err());

        for count in [256, 257] {
            let mut input = arguments();
            input.extend(["--stop-tokens".to_owned(), vec!["1"; count].join(",")]);
            assert_eq!(parse(input).is_ok(), count == 256);
        }
        for tokens in ["", "1,", "-1", "4294967296"] {
            let mut input = arguments();
            replace(&mut input, "--tokens", tokens)?;
            assert!(parse(input).is_err());
        }
        Ok(())
    }

    #[test]
    fn cli_preflight_accepts_exact_context_boundary_and_uppercase_digest() -> Result<()> {
        let mut input = arguments();
        replace(&mut input, "--sha256", &"AB".repeat(32))?;
        replace(&mut input, "--max-tokens", "6")?;
        let parsed = parse(input)?;
        assert_eq!(parsed.tokens.len() + parsed.generation.max_tokens, 8);
        assert_eq!(parsed.generation.stop_tokens.len(), 0);
        Ok(())
    }
}
