#![forbid(unsafe_code)]
//! Explicit token-ID driver for the unqualified native CPU reference engine.

use bonsai_prism_runtime::{
    Result,
    graph::{MAX_STATE_BYTES, Qwen35},
    kernels::{MatvecBackend, MatvecDiagnostics, MatvecFallbackReason},
    model::Model,
    runtime::{Cancellation, Finish, Generation, Limits, Runtime},
};
use serde_json::json;
use std::{
    collections::BTreeMap,
    io::Write,
    path::PathBuf,
    process::ExitCode,
    time::{Duration, Instant},
};

const USAGE: &str = "Native CPU reference; full-model parity and performance are unqualified.\n\
Usage: bonsai-prism-runtime --model <absolute-gguf-path> --bytes <u64>\n\
  --sha256 <64-hex> --memory-bytes <usize> --context <1..262144>\n\
  --checkpoint-bytes <usize> --max-tokens <usize> --tokens <id,id,...>\n\
  [--stop-tokens <id,id,...>] [--backend <auto|scalar|neon>]\n\
  [--timeout-seconds <1..3600>]\n\
  [--state-bytes <usize>]\n\
State budget defaults to 4 GiB for live KV/recurrent element payload only;\n\
model storage, graph scratch, checkpoints and allocator overhead are separate.\n\
Timeout covers generation after loading. Checks are cooperative between operations;\n\
streamed tokens are provisional until the final completion record.\n\
Backend defaults to auto. NEON requires the experimental-neon feature and\n\
native Windows ARM64 MSVC; unsupported builds, targets and formats use scalar.\n\
Selection is per matrix operation; backend_requested reports only the request.\n\
backend_last_operation records just the latest projection, including scalar fallback.\n\
Input is token IDs; decoding is greedy. Output is one JSON object per line.\n";

struct Arguments {
    path: PathBuf,
    bytes: u64,
    sha256: String,
    memory: usize,
    context: usize,
    checkpoint_bytes: usize,
    state_bytes: usize,
    generation: Generation,
    tokens: Vec<u32>,
    backend: MatvecBackend,
    timeout: Option<Duration>,
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
                | "--backend"
                | "--timeout-seconds"
                | "--state-bytes"
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
    let state_bytes = values
        .remove("--state-bytes")
        .map(|value| value.parse::<usize>())
        .transpose()?
        .unwrap_or(MAX_STATE_BYTES);
    if state_bytes == 0 || state_bytes > MAX_STATE_BYTES {
        return Err(bad("invalid native live-state payload budget"));
    }
    let backend = match values.remove("--backend").as_deref().unwrap_or("auto") {
        "auto" => MatvecBackend::Auto,
        "scalar" => MatvecBackend::Scalar,
        "neon" => MatvecBackend::ExperimentalNeon,
        _ => return Err(bad("backend must be auto, scalar or neon")),
    };
    let timeout = values
        .remove("--timeout-seconds")
        .as_deref()
        .map(timeout_seconds)
        .transpose()?;
    Ok(Arguments {
        path,
        bytes,
        sha256,
        memory,
        context,
        checkpoint_bytes,
        state_bytes,
        generation: Generation {
            max_tokens,
            stop_tokens,
        },
        tokens,
        backend,
        timeout,
    })
}

fn timeout_seconds(value: &str) -> Result<Duration> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(bad(
            "timeout seconds must be an integer from 1 through 3600",
        ));
    }
    let seconds = value.parse::<u64>()?;
    if !(1..=3600).contains(&seconds) {
        return Err(bad(
            "timeout seconds must be an integer from 1 through 3600",
        ));
    }
    Ok(Duration::from_secs(seconds))
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
    let decoder = Qwen35::with_state_budget(
        model,
        arguments.context,
        arguments.state_bytes,
        arguments.backend,
    )?;
    let backend_requested = backend_name(decoder.requested_backend());
    let mut runtime = Runtime::new(
        decoder,
        Limits {
            context_tokens: arguments.context,
            checkpoints: 0,
            checkpoint_bytes: arguments.checkpoint_bytes,
        },
    )?;
    let mut output = std::io::stdout().lock();
    let cancellation = Cancellation::default();
    let emit = |token| {
        serde_json::to_writer(&mut output, &json!({"token": token}))?;
        output.write_all(b"\n")?;
        output.flush()?;
        Ok(())
    };
    let result = if let Some(timeout) = arguments.timeout {
        let deadline = Instant::now()
            .checked_add(timeout)
            .ok_or_else(|| bad("generation deadline exceeds monotonic clock bounds"))?;
        runtime.generate_until(
            &arguments.tokens,
            &arguments.generation,
            &cancellation,
            deadline,
            emit,
        )?
    } else {
        runtime.generate(
            &arguments.tokens,
            &arguments.generation,
            &cancellation,
            emit,
        )?
    };
    serde_json::to_writer(
        &mut output,
        &json!({
            "finish_reason": match result.finish { Finish::Stop => "stop", Finish::Length => "length" },
            "prompt_tokens": result.prompt_tokens,
            "generated_tokens": result.generated_tokens,
            "reused_tokens": result.reused_tokens,
            "backend_requested": backend_requested,
            "state_budget_bytes": arguments.state_bytes,
            "backend_last_operation": backend_record(runtime.decoder().backend_diagnostics()),
        }),
    )?;
    output.write_all(b"\n")?;
    output.flush()?;
    Ok(())
}

const fn backend_name(backend: MatvecBackend) -> &'static str {
    match backend {
        MatvecBackend::Auto => "auto",
        MatvecBackend::Scalar => "scalar",
        MatvecBackend::ExperimentalNeon => "neon",
    }
}

fn backend_record(diagnostics: MatvecDiagnostics) -> serde_json::Value {
    json!({
        "selected": backend_name(diagnostics.selected_backend),
        "neon_build_eligible": diagnostics.neon_build_eligible,
        "fallback_reason": diagnostics.fallback_reason.map(|reason| match reason {
            MatvecFallbackReason::ScalarRequested => "scalar_requested",
            MatvecFallbackReason::TensorKindUnsupported => "tensor_kind_unsupported",
            MatvecFallbackReason::FeatureDisabled => "feature_disabled",
            MatvecFallbackReason::TargetUnsupported => "target_unsupported",
        }),
        "dispatched": diagnostics.dispatched,
        "succeeded": diagnostics.succeeded,
    })
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
    use super::{Duration, MAX_STATE_BYTES, MatvecBackend, Result, bad, parse};

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

    #[test]
    fn cli_backend_selection_defaults_to_auto_and_accepts_explicit_requests() -> Result<()> {
        assert_eq!(parse(arguments())?.backend, MatvecBackend::Auto);
        for (value, expected) in [
            ("auto", MatvecBackend::Auto),
            ("scalar", MatvecBackend::Scalar),
            ("neon", MatvecBackend::ExperimentalNeon),
        ] {
            let mut input = arguments();
            input.extend(["--backend".to_owned(), value.to_owned()]);
            assert_eq!(parse(input)?.backend, expected);
        }
        Ok(())
    }

    #[test]
    fn cli_backend_selection_rejects_invalid_repeated_and_missing_values() {
        for value in ["", "NEON", "Scalar", "avx2", "neon ", "--scalar"] {
            let mut input = arguments();
            input.extend(["--backend".to_owned(), value.to_owned()]);
            assert!(parse(input).is_err(), "accepted invalid backend {value}");
        }
        let mut input = arguments();
        input.push("--backend".to_owned());
        assert!(parse(input).is_err());
        for second in ["auto", "scalar", "neon"] {
            let mut input = arguments();
            input.extend(["--backend", "scalar", "--backend", second].map(str::to_owned));
            assert!(parse(input).is_err());
        }
    }

    #[test]
    fn cli_state_budget_accepts_explicit_bounds_and_preserves_default() -> Result<()> {
        assert_eq!(parse(arguments())?.state_bytes, MAX_STATE_BYTES);
        for bytes in [1, 4096, MAX_STATE_BYTES] {
            let mut input = arguments();
            input.extend(["--state-bytes".to_owned(), bytes.to_string()]);
            assert_eq!(parse(input)?.state_bytes, bytes);
        }
        Ok(())
    }

    #[test]
    fn cli_state_budget_rejects_invalid_repeated_and_missing_values() {
        for value in [
            "",
            "0",
            "-1",
            "1.5",
            " 1",
            "4294967297",
            "18446744073709551616",
        ] {
            let mut input = arguments();
            input.extend(["--state-bytes".to_owned(), value.to_owned()]);
            assert!(parse(input).is_err());
        }
        let mut input = arguments();
        input.push("--state-bytes".to_owned());
        assert!(parse(input).is_err());
        let mut input = arguments();
        input.extend(["--state-bytes", "4096", "--state-bytes", "8192"].map(str::to_owned));
        assert!(parse(input).is_err());
    }

    #[test]
    fn cli_timeout_accepts_bounded_seconds_and_preserves_default() -> Result<()> {
        assert_eq!(parse(arguments())?.timeout, None);
        for seconds in [1, 3600] {
            let mut input = arguments();
            input.extend(["--timeout-seconds".to_owned(), seconds.to_string()]);
            assert_eq!(parse(input)?.timeout, Some(Duration::from_secs(seconds)));
        }
        Ok(())
    }

    #[test]
    fn cli_timeout_rejects_invalid_repeated_and_missing_values() {
        for value in [
            "",
            "0",
            "3601",
            "-1",
            "+1",
            "1.5",
            " 1",
            "1s",
            "18446744073709551616",
        ] {
            let mut input = arguments();
            input.extend(["--timeout-seconds".to_owned(), value.to_owned()]);
            assert!(parse(input).is_err());
        }
        let mut input = arguments();
        input.push("--timeout-seconds".to_owned());
        assert!(parse(input).is_err());
        let mut input = arguments();
        input.extend(["--timeout-seconds", "1", "--timeout-seconds", "2"].map(str::to_owned));
        assert!(parse(input).is_err());
    }
}
