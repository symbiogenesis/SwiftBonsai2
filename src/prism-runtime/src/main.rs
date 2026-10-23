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
    let bytes = required("--bytes")?.parse()?;
    let sha256 = required("--sha256")?;
    let memory = required("--memory-bytes")?.parse()?;
    let context = required("--context")?.parse()?;
    let checkpoint_bytes = required("--checkpoint-bytes")?.parse()?;
    let max_tokens = required("--max-tokens")?.parse()?;
    let tokens = token_ids(&required("--tokens")?)?;
    let stop_tokens = values
        .remove("--stop-tokens")
        .map(|text| token_ids(&text))
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

fn token_ids(text: &str) -> Result<Vec<u32>> {
    let mut result = Vec::new();
    for token in text.split(',') {
        if result.len() == 262_144
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
