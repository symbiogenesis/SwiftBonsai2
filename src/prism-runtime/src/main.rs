#![forbid(unsafe_code)]
//! Explicit token-ID driver for the unqualified native CPU reference engine.

use bonsai_prism_runtime::{
    MAX_PREFILL_BLOCK_TOKENS, Result,
    graph::{MAX_STATE_BYTES, Qwen35},
    kernels::{MatvecBackend, MatvecDiagnostics, MatvecFallbackReason, MatvecStatistics},
    model::Model,
    runtime::{Cancellation, Finish, Generation, Limits, Prefill, Runtime},
    sampling::{Greedy, ReferenceSampler, Sampler, Sampling},
    special_tokens::SpecialTokens,
    vocabulary::Vocabulary,
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
  [--timeout-seconds <1..3600>] [--output <tokens|text>]\n\
  [--state-bytes <usize>]\n\
  [--prefill-block-tokens <1..256>]\n\
  [--sampling <greedy|reference>] [--seed <u64>]\n\
  [--temperature <positive-f32>] [--top-k <0..1048576>] [--top-p <0..1>]\n\
State budget defaults to 4 GiB for live KV/recurrent element payload only;\n\
model storage, graph scratch, checkpoints and allocator overhead are separate.\n\
Timeout covers generation after loading. Checks are cooperative between operations;\n\
streamed tokens are provisional until the final completion record.\n\
Backend defaults to auto. NEON requires the experimental-neon feature and\n\
native Windows ARM64 MSVC; unsupported builds, targets and formats use scalar.\n\
Selection is per matrix operation; backend_requested reports only the request.\n\
backend_last_operation records just the latest projection, including scalar fallback.\n\
backend_statistics counts all attempted matrix work, including provisional work.\n\
Prefill blocks default to one token and preserve sequential reference arithmetic.\n\
Input is token IDs; decoding defaults to greedy. Output is one JSON object per line.\n\
Reference sampling is experimental, requires --seed, and uses temperature, top-k,\n\
then top-p with SplitMix64. Defaults are 1, 20 and 0.95; top-k 0 retains all IDs.\n\
Top-p must be greater than zero. Sampling controls require --sampling reference.\n\
This reference RNG/sampler does not establish Prism sampling or quality parity.\n\
Model-declared EOS/EOT/EOM always stop generation; --stop-tokens adds IDs.\n\
Text output adds lossless token bytes and incremental UTF-8 text (invalid bytes\n\
display as replacement characters). Control token spellings remain visible.\n";

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
    prefill: Prefill,
    text_output: bool,
    sampling: SamplingMode,
}

#[derive(Clone, Copy)]
enum SamplingMode {
    Greedy,
    Reference { options: Sampling, seed: u64 },
}

impl SamplingMode {
    fn sampler(self) -> Result<DriverSampler> {
        match self {
            Self::Greedy => Ok(DriverSampler::Greedy(Greedy)),
            Self::Reference { options, seed } => Ok(DriverSampler::Reference(
                ReferenceSampler::new(options, seed)?,
            )),
        }
    }

    fn record(self) -> serde_json::Value {
        match self {
            Self::Greedy => json!({"mode": "greedy"}),
            Self::Reference { options, seed } => json!({
                "mode": "reference",
                "rng": "splitmix64",
                "seed": seed,
                "temperature": options.temperature,
                "top_k": options.top_k,
                "top_p": options.top_p,
            }),
        }
    }
}

#[derive(Clone)]
enum DriverSampler {
    Greedy(Greedy),
    Reference(ReferenceSampler),
}

impl Sampler for DriverSampler {
    fn sample(&mut self, logits: &[f32]) -> Result<u32> {
        match self {
            Self::Greedy(sampler) => sampler.sample(logits),
            Self::Reference(sampler) => sampler.sample(logits),
        }
    }
}

fn bad(message: &'static str) -> bonsai_prism_runtime::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidInput, message).into()
}

fn option_values(arguments: impl IntoIterator<Item = String>) -> Result<BTreeMap<String, String>> {
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
                | "--prefill-block-tokens"
                | "--output"
                | "--sampling"
                | "--seed"
                | "--temperature"
                | "--top-k"
                | "--top-p"
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
    Ok(values)
}

fn parse(arguments: impl IntoIterator<Item = String>) -> Result<Arguments> {
    let mut values = option_values(arguments)?;
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
    let prefill = prefill_options(values.remove("--prefill-block-tokens").as_deref())?;
    let text_output = match values.remove("--output").as_deref().unwrap_or("tokens") {
        "tokens" => false,
        "text" => true,
        _ => return Err(bad("output must be tokens or text")),
    };
    let sampling = sampling_options(&mut values)?;
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
        prefill,
        text_output,
        sampling,
    })
}

fn sampling_options(values: &mut BTreeMap<String, String>) -> Result<SamplingMode> {
    match values.remove("--sampling").as_deref().unwrap_or("greedy") {
        "greedy" => {
            if ["--seed", "--temperature", "--top-k", "--top-p"]
                .iter()
                .any(|key| values.contains_key(*key))
            {
                return Err(bad("sampling controls require --sampling reference"));
            }
            Ok(SamplingMode::Greedy)
        }
        "reference" => {
            let seed = values
                .remove("--seed")
                .ok_or_else(|| bad("reference sampling requires an explicit seed"))?;
            if seed.is_empty() || !seed.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(bad("seed must be an unsigned decimal integer"));
            }
            let seed = seed.parse::<u64>()?;
            let defaults = Sampling::default();
            let temperature = values
                .remove("--temperature")
                .map(|value| value.parse::<f32>())
                .transpose()?
                .unwrap_or(defaults.temperature);
            let top_k = values
                .remove("--top-k")
                .map(|value| {
                    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                        return Err(bad("top-k must be an unsigned decimal integer"));
                    }
                    Ok(value.parse::<usize>()?)
                })
                .transpose()?
                .unwrap_or(defaults.top_k);
            let top_p = values
                .remove("--top-p")
                .map(|value| value.parse::<f32>())
                .transpose()?
                .unwrap_or(defaults.top_p);
            let sampling = SamplingMode::Reference {
                options: Sampling {
                    temperature,
                    top_k,
                    top_p,
                },
                seed,
            };
            // Reject configuration errors before model hashing or graph allocation.
            let _validated = sampling.sampler()?;
            Ok(sampling)
        }
        _ => Err(bad("sampling must be greedy or reference")),
    }
}

fn prefill_options(value: Option<&str>) -> Result<Prefill> {
    let value = value.unwrap_or("1");
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(bad("prefill block tokens must be from 1 through 256"));
    }
    let block_tokens = value.parse::<usize>()?;
    if !(1..=MAX_PREFILL_BLOCK_TOKENS).contains(&block_tokens) {
        return Err(bad("prefill block tokens must be from 1 through 256"));
    }
    Ok(Prefill { block_tokens })
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

fn generation_options(options: &Generation, model_stops: &[u32]) -> Result<Generation> {
    let mut stop_tokens = Vec::new();
    stop_tokens.try_reserve_exact(256)?;
    for &token in model_stops.iter().chain(&options.stop_tokens) {
        if !stop_tokens.contains(&token) {
            if stop_tokens.len() == 256 {
                return Err(bad("combined model and caller stop set exceeds 256 tokens"));
            }
            stop_tokens.push(token);
        }
    }
    Ok(Generation {
        max_tokens: options.max_tokens,
        stop_tokens,
    })
}

#[derive(Default)]
struct Utf8Output {
    // Only an incomplete UTF-8 suffix survives each push (at most three bytes).
    pending: Vec<u8>,
}

impl Utf8Output {
    fn push(&mut self, bytes: &[u8]) -> Result<String> {
        self.pending.try_reserve(bytes.len())?;
        self.pending.extend_from_slice(bytes);
        let mut output = String::new();
        let mut consumed = 0;
        while consumed < self.pending.len() {
            match std::str::from_utf8(&self.pending[consumed..]) {
                Ok(text) => {
                    output.try_reserve(text.len())?;
                    output.push_str(text);
                    consumed = self.pending.len();
                }
                Err(error) => {
                    let valid_end = consumed + error.valid_up_to();
                    let text = std::str::from_utf8(&self.pending[consumed..valid_end])?;
                    output.try_reserve(text.len() + 3)?;
                    output.push_str(text);
                    consumed = valid_end;
                    if let Some(invalid_bytes) = error.error_len() {
                        output.push('\u{fffd}');
                        consumed += invalid_bytes;
                    } else {
                        break;
                    }
                }
            }
        }
        self.pending.drain(..consumed);
        Ok(output)
    }

    fn finish(&mut self) -> &'static str {
        if self.pending.is_empty() {
            ""
        } else {
            self.pending.clear();
            "\u{fffd}"
        }
    }
}

fn load_decoder(arguments: &Arguments) -> Result<(Qwen35, Generation, Option<Vocabulary>)> {
    let model = Model::load(
        &arguments.path,
        arguments.bytes,
        &arguments.sha256,
        arguments.memory,
    )?;
    let vocab_size = model
        .tensors
        .get("token_embd.weight")
        .filter(|tensor| tensor.dimensions.len() == 2)
        .and_then(|tensor| tensor.dimensions.get(1))
        .copied()
        .ok_or_else(|| bad("missing token embedding vocabulary dimension"))?;
    let special_tokens = SpecialTokens::from_metadata(&model.metadata, vocab_size)?;
    let generation = generation_options(&arguments.generation, special_tokens.stop_tokens())?;
    let vocabulary = arguments
        .text_output
        .then(|| Vocabulary::from_metadata(&model.metadata, vocab_size))
        .transpose()?;
    let decoder = Qwen35::with_state_budget(
        model,
        arguments.context,
        arguments.state_bytes,
        arguments.backend,
    )?;
    Ok((decoder, generation, vocabulary))
}

fn execute(arguments: &Arguments) -> Result<()> {
    let mut sampler = arguments.sampling.sampler()?;
    let (decoder, generation, vocabulary) = load_decoder(arguments)?;
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
    let mut text_output = Utf8Output::default();
    let emit = |token| {
        let record = if let Some(vocabulary) = &vocabulary {
            let bytes = vocabulary.piece(token)?;
            json!({
                "token": token,
                "bytes": bytes,
                "text": text_output.push(bytes)?,
                "special": vocabulary.is_control(token)?,
            })
        } else {
            json!({"token": token})
        };
        serde_json::to_writer(&mut output, &record)?;
        output.write_all(b"\n")?;
        output.flush()?;
        Ok(())
    };
    let result = if let Some(timeout) = arguments.timeout {
        let deadline = Instant::now()
            .checked_add(timeout)
            .ok_or_else(|| bad("generation deadline exceeds monotonic clock bounds"))?;
        runtime.generate_sampled_until(
            &arguments.tokens,
            &generation,
            arguments.prefill,
            &cancellation,
            deadline,
            &mut sampler,
            emit,
        )?
    } else {
        runtime.generate_sampled(
            &arguments.tokens,
            &generation,
            arguments.prefill,
            &cancellation,
            &mut sampler,
            emit,
        )?
    };
    let mut completion = json!({
        "finish_reason": match result.finish { Finish::Stop => "stop", Finish::Length => "length" },
        "prompt_tokens": result.prompt_tokens,
        "generated_tokens": result.generated_tokens,
        "reused_tokens": result.reused_tokens,
        "backend_requested": backend_requested,
        "state_budget_bytes": arguments.state_bytes,
        "prefill_block_tokens": arguments.prefill.block_tokens,
        "stop_tokens": generation.stop_tokens,
        "sampling": arguments.sampling.record(),
        "backend_last_operation": backend_record(runtime.decoder().backend_diagnostics()),
        "backend_statistics": statistics_record(runtime.decoder().backend_statistics()),
    });
    if arguments.text_output {
        completion["text"] = json!(text_output.finish());
    }
    serde_json::to_writer(&mut output, &completion)?;
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

fn statistics_record(statistics: MatvecStatistics) -> serde_json::Value {
    json!({
        "scope": "decoder_lifetime_including_provisional_work",
        "preparations": statistics.preparations,
        "preparation_failures": statistics.preparation_failures,
        "matrix_attempts": statistics.matrix_attempts,
        "matrix_successes": statistics.matrix_successes,
        "rejected_before_dispatch": statistics.rejected_before_dispatch,
        "scalar_operations": statistics.scalar_operations,
        "neon_operations": statistics.neon_operations,
        "fallbacks": {
            "scalar_requested": statistics.scalar_requested,
            "tensor_kind_unsupported": statistics.tensor_kind_unsupported,
            "feature_disabled": statistics.feature_disabled,
            "target_unsupported": statistics.target_unsupported,
        },
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
    use super::{
        Duration, Generation, MAX_STATE_BYTES, MatvecBackend, Result, SamplingMode, Utf8Output,
        bad, generation_options, parse,
    };

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
    fn cli_sampling_requires_explicit_mode_and_seed_and_records_effective_options() -> Result<()> {
        assert!(matches!(parse(arguments())?.sampling, SamplingMode::Greedy));
        let mut input = arguments();
        input.extend(["--sampling", "reference", "--seed", "42"].map(str::to_owned));
        let mode = parse(input)?.sampling;
        assert_eq!(mode.record()["mode"], "reference");
        assert_eq!(mode.record()["rng"], "splitmix64");
        assert_eq!(mode.record()["seed"], 42);
        assert_eq!(mode.record()["temperature"], 1.0);
        assert_eq!(mode.record()["top_k"], 20);
        assert_eq!(mode.record()["top_p"], serde_json::json!(0.95_f32));
        let mut input = arguments();
        input.extend(
            [
                "--sampling",
                "reference",
                "--seed",
                "18446744073709551615",
                "--temperature",
                "0.7",
                "--top-k",
                "0",
                "--top-p",
                "1",
            ]
            .map(str::to_owned),
        );
        let mode = parse(input)?.sampling;
        assert_eq!(mode.record()["seed"], u64::MAX);
        assert_eq!(mode.record()["temperature"], serde_json::json!(0.7_f32));
        assert_eq!(mode.record()["top_k"], 0);
        assert_eq!(mode.record()["top_p"], 1.0);
        Ok(())
    }

    #[test]
    fn cli_sampling_rejects_inactive_controls_and_invalid_configuration_before_loading() {
        for tail in [
            vec!["--sampling", "reference"],
            vec!["--sampling", "Reference", "--seed", "1"],
            vec!["--seed", "1"],
            vec!["--sampling", "greedy", "--temperature", "1"],
            vec!["--sampling", "greedy", "--top-k", "1"],
            vec!["--sampling", "greedy", "--top-p", "1"],
            vec!["--sampling", "reference", "--seed", "-1"],
            vec!["--sampling", "reference", "--seed", "+1"],
            vec!["--sampling", "reference", "--seed", "18446744073709551616"],
            vec!["--sampling", "reference", "--seed", "1", "--seed", "2"],
        ] {
            let mut input = arguments();
            input.extend(tail.into_iter().map(str::to_owned));
            assert!(parse(input).is_err());
        }
        for (key, value) in [
            ("--temperature", "0"),
            ("--temperature", "NaN"),
            ("--temperature", "inf"),
            ("--top-k", "1048577"),
            ("--top-k", "+1"),
            ("--top-k", "-1"),
            ("--top-p", "0"),
            ("--top-p", "1.01"),
            ("--top-p", "NaN"),
        ] {
            let mut input = arguments();
            input.extend(["--sampling", "reference", "--seed", "1", key, value].map(str::to_owned));
            assert!(parse(input).is_err(), "accepted invalid {key}");
        }
    }

    #[test]
    fn cli_text_output_is_explicit_and_rejects_invalid_options() -> Result<()> {
        assert!(!parse(arguments())?.text_output);
        for (value, expected) in [("tokens", false), ("text", true)] {
            let mut input = arguments();
            input.extend(["--output".to_owned(), value.to_owned()]);
            assert_eq!(parse(input)?.text_output, expected);
        }
        for tail in [
            vec!["--output"],
            vec!["--output", ""],
            vec!["--output", "Text"],
            vec!["--output", "text", "--output", "tokens"],
        ] {
            let mut input = arguments();
            input.extend(tail.into_iter().map(str::to_owned));
            assert!(parse(input).is_err());
        }
        Ok(())
    }

    #[test]
    fn cli_stop_union_preserves_declared_stops_and_bounds_unique_ids() -> Result<()> {
        let mut options = Generation {
            max_tokens: 8,
            stop_tokens: vec![2, 3, 2],
        };
        let merged = generation_options(&options, &[1, 2])?;
        assert_eq!(merged.max_tokens, 8);
        assert_eq!(merged.stop_tokens, [1, 2, 3]);
        assert_eq!(options.stop_tokens, [2, 3, 2]);
        options.stop_tokens = (0..256).collect();
        assert_eq!(generation_options(&options, &[1])?.stop_tokens.len(), 256);
        assert!(generation_options(&options, &[256]).is_err());
        Ok(())
    }

    #[test]
    fn cli_utf8_output_preserves_split_characters_and_bounds_pending_suffix() -> Result<()> {
        for text in ["a café\n", "🌱 bonsai", "\0中文"] {
            let bytes = text.as_bytes();
            for split in 0..=bytes.len() {
                let mut output = Utf8Output::default();
                let mut rendered = output.push(&bytes[..split])?;
                assert!(output.pending.len() <= 3);
                rendered.push_str(&output.push(&bytes[split..])?);
                rendered.push_str(output.finish());
                assert_eq!(rendered, text);
                assert_eq!(output.pending, Vec::<u8>::new());
            }
            let mut output = Utf8Output::default();
            let mut rendered = String::new();
            for byte in bytes {
                rendered.push_str(&output.push(&[*byte])?);
                assert!(output.pending.len() <= 3);
            }
            rendered.push_str(output.finish());
            assert_eq!(rendered, text);
        }
        Ok(())
    }

    #[test]
    fn cli_utf8_output_matches_lossy_rendering_for_invalid_and_truncated_bytes() -> Result<()> {
        for bytes in [
            &[0xff, b'a', 0xc3, b'b'][..],
            &[0xf0, 0x9f, 0x8c][..],
            &[0xed, 0xa0, 0x80][..],
            &[0xc0, 0xaf][..],
            &[0xe2, 0x82, b'!', 0x80][..],
        ] {
            for split in 0..=bytes.len() {
                let mut output = Utf8Output::default();
                let mut rendered = output.push(&bytes[..split])?;
                rendered.push_str(&output.push(&bytes[split..])?);
                rendered.push_str(output.finish());
                assert_eq!(rendered, String::from_utf8_lossy(bytes));
                assert_eq!(output.finish(), "");
            }
        }
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
    fn cli_prefill_blocks_validate_bounds_before_model_loading() -> Result<()> {
        assert_eq!(parse(arguments())?.prefill.block_tokens, 1);
        for tokens in ["1", "2", "256"] {
            let mut input = arguments();
            input.extend(["--prefill-block-tokens".to_owned(), tokens.to_owned()]);
            assert_eq!(parse(input)?.prefill.block_tokens, tokens.parse::<usize>()?);
        }
        for tokens in ["0", "257", "-1", "+1", "1.5", "", "18446744073709551616"] {
            let mut input = arguments();
            input.extend(["--prefill-block-tokens".to_owned(), tokens.to_owned()]);
            assert!(parse(input).is_err());
        }
        let mut input = arguments();
        input.push("--prefill-block-tokens".to_owned());
        assert!(parse(input).is_err());
        let mut input = arguments();
        input.extend(
            ["--prefill-block-tokens", "2", "--prefill-block-tokens", "4"].map(str::to_owned),
        );
        assert!(parse(input).is_err());
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
