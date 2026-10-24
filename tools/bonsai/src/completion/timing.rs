//! Exact pinned completion logger grammar and fresh-sample counter validation.

use serde_json::{Value, json};

use crate::Result;

fn horizontal(text: &str) -> &str {
    text.trim_start_matches([' ', '\t'])
}

fn required_space(text: &str) -> Option<&str> {
    let rest = horizontal(text);
    (rest.len() != text.len()).then_some(rest)
}

fn integer(text: &str) -> Option<(&str, &str)> {
    let length = text.bytes().take_while(u8::is_ascii_digit).count();
    (length != 0).then(|| text.split_at(length))
}

fn number(text: &str, require_fraction: bool) -> Option<(&str, &str)> {
    let (whole, remaining) = integer(text)?;
    if let Some(fraction) = remaining.strip_prefix('.') {
        let (fraction, rest) = integer(fraction)?;
        return Some((&text[..whole.len() + 1 + fraction.len()], rest));
    }
    (!require_fraction).then_some((whole, remaining))
}

fn common(line: &str) -> Option<&str> {
    required_space(line.strip_prefix("common_perf_print:")?)
}

fn timing_record<'a>(line: &'a str, label: &str, unit: &str) -> Option<(&'a str, &'a str)> {
    let text = horizontal(common(line)?.strip_prefix(label)?);
    let text = horizontal(text.strip_prefix('=')?);
    let (milliseconds, text) = number(text, false)?;
    let text = horizontal(text.strip_prefix(" ms")?);
    let text = horizontal(text.strip_prefix('/')?);
    let (count, text) = integer(text)?;
    let text = text.strip_prefix(' ')?.strip_prefix(unit)?;
    let text = required_space(text)?.strip_prefix('(')?;
    let (_, text) = number(horizontal(text), false)?;
    let text = text.strip_prefix(" ms per token,")?;
    let (_, text) = number(horizontal(text), false)?;
    text.strip_prefix(" tokens per second)")?
        .is_empty()
        .then_some((milliseconds, count))
}

fn prompt_record(line: &str) -> Option<&str> {
    let (count, text) = integer(line.strip_prefix("main: number of tokens in prompt = ")?)?;
    horizontal(text).is_empty().then_some(count)
}

fn sampler_record(line: &str) -> Option<&str> {
    let text = horizontal(common(line)?.strip_prefix("samplers time")?);
    let (_, text) = number(horizontal(text.strip_prefix('=')?), true)?;
    let text = horizontal(text.strip_prefix(" ms")?);
    let (count, text) = integer(horizontal(text.strip_prefix('/')?))?;
    horizontal(text.strip_prefix(" tokens")?)
        .is_empty()
        .then_some(count)
}

fn unique<T>(records: Vec<T>, description: &str) -> Result<T> {
    let mut values = records.into_iter();
    match (values.next(), values.next()) {
        (Some(record), None) => Ok(record),
        _ => Err(format!(
            "Expected exactly one complete {description} record from the pinned logger."
        )),
    }
}

pub(super) fn parse(text: &str) -> Result<Value> {
    // ReadAllText consumed only an initial UTF-8 preamble in the former caller.
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut decode = Vec::new();
    let mut prompt = Vec::new();
    let mut prompt_count = Vec::new();
    let mut sampler_count = Vec::new();
    for line in text.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if let Some(record) = timing_record(line, "eval time", "runs") {
            decode.push(record);
        }
        if let Some(record) = timing_record(line, "prompt eval time", "tokens") {
            prompt.push(record);
        }
        if let Some(record) = prompt_record(line) {
            prompt_count.push(record);
        }
        if let Some(record) = sampler_record(line) {
            sampler_count.push(record);
        }
    }
    let (decode_ms, decode_tokens) = unique(decode, "decode timing")?;
    let (prompt_ms, prompt_tokens) = unique(prompt, "prompt timing")?;
    let milliseconds = decode_ms
        .parse::<f64>()
        .map_err(|error| error.to_string())?;
    let count = decode_tokens
        .parse::<u32>()
        .map_err(|error| error.to_string())?;
    let prompt_milliseconds = prompt_ms
        .parse::<f64>()
        .map_err(|error| error.to_string())?;
    let prompt_tokens = prompt_tokens
        .parse::<u32>()
        .map_err(|error| error.to_string())?;
    if !milliseconds.is_finite()
        || milliseconds <= 0.0
        || !(1..=512).contains(&count)
        || !prompt_milliseconds.is_finite()
        || prompt_milliseconds <= 0.0
        || !(1..=8192).contains(&prompt_tokens)
    {
        return Err("Invalid or out-of-contract native timing counts.".into());
    }
    let input = unique(prompt_count, "prompt counter")?
        .parse::<u32>()
        .map_err(|error| error.to_string())?;
    let accepted = unique(sampler_count, "sampler counter")?
        .parse::<u32>()
        .map_err(|error| error.to_string())?;
    let generated = accepted
        .checked_sub(input)
        .ok_or("Native sample counters disagree.")?;
    if !(2..=8192).contains(&input)
        || !(1..=512).contains(&generated)
        || accepted != prompt_tokens + count + 1
    {
        return Err(
            "Native prompt, sampler and decode counters disagree for a fresh single-turn sample."
                .into(),
        );
    }
    // The pinned logger counts decode eval runs. The first sampled token follows
    // prompt evaluation, so the rate uses eval runs rather than sampled tokens.
    Ok(json!({
        "Timing": {"DecodeTokens": count, "DecodeMilliseconds": milliseconds,
            "DecodeTokensPerSecond": 1000.0 * f64::from(count) / milliseconds,
            "PromptTokens": prompt_tokens, "PromptMilliseconds": prompt_milliseconds},
        "Counts": {"PromptTokenCount": input, "SamplerAcceptedTokens": accepted,
            "GeneratedSampledTokens": generated},
    }))
}

#[cfg(test)]
mod tests {
    use super::parse;

    const LOG: &str = "main: number of tokens in prompt = 15\ncommon_perf_print: prompt eval time = 100.5 ms / 15 tokens ( 6.7 ms per token, 149.25 tokens per second)\ncommon_perf_print: eval time = 200.0 ms / 3 runs ( 66.7 ms per token, 15 tokens per second)\ncommon_perf_print: samplers time = 0.50 ms / 19 tokens\n";

    const QUALIFICATION_LOG: &str = "main: number of tokens in prompt = 80\ncommon_perf_print:    samplers time =       5.00 ms /   208 tokens\ncommon_perf_print: prompt eval time =     125.50 ms /    80 tokens ( 1.57 ms per token, 637.45 tokens per second)\ncommon_perf_print:        eval time =    1000.25 ms /   127 runs   ( 7.88 ms per token, 126.97 tokens per second)\n";

    #[test]
    fn exact_logger_records_separate_sampled_tokens_and_decode_runs() -> crate::Result<()> {
        let parsed = parse(&format!("\u{feff}{}", LOG.replace('\n', "\r\n")))?;
        assert_eq!(parsed["Timing"]["DecodeTokens"], 3);
        assert_eq!(parsed["Timing"]["PromptTokens"], 15);
        assert_eq!(parsed["Counts"]["GeneratedSampledTokens"], 4);
        assert_eq!(parsed["Timing"]["DecodeTokensPerSecond"], 15.0);
        Ok(())
    }

    #[test]
    fn fractional_timings_compute_unrounded_decode_rate() -> crate::Result<()> {
        let parsed = parse(QUALIFICATION_LOG)?;
        assert_eq!(parsed["Timing"]["DecodeTokens"], 127);
        assert_eq!(parsed["Timing"]["DecodeMilliseconds"], 1000.25);
        assert_eq!(parsed["Timing"]["PromptTokens"], 80);
        assert_eq!(parsed["Timing"]["PromptMilliseconds"], 125.5);
        assert_eq!(parsed["Counts"]["GeneratedSampledTokens"], 128);
        assert_eq!(parsed["Counts"]["SamplerAcceptedTokens"], 208);
        assert_eq!(parsed["Counts"]["PromptTokenCount"], 80);
        assert_eq!(
            parsed["Timing"]["DecodeTokensPerSecond"],
            127_000.0 / 1000.25
        );
        assert_ne!(parsed["Timing"]["DecodeTokensPerSecond"], 126.97);
        Ok(())
    }

    #[test]
    fn malformed_duplicate_and_inconsistent_records_are_rejected() {
        for text in [
            LOG.replace("19 tokens", "18 tokens"),
            format!("{LOG}main: number of tokens in prompt = 15\n"),
            LOG.replace("200.0 ms", "NaN ms"),
            LOG.replace("200.0 ms", "0.0 ms"),
            LOG.replace("3 runs", "513 runs"),
            LOG.replace("15 tokens (", "0 tokens ("),
            LOG.replace("common_perf_print:", "llama_perf_context_print:"),
            LOG.replace("0.50 ms", "0 ms"),
            LOG.replace("15 tokens (", "15 tokens (invalid "),
            format!("{LOG}{LOG}"),
            LOG.replace("eval time = 200.0", "eval time = +200.0"),
            LOG.replace("tokens per second)", "tokens per second) "),
        ] {
            assert!(parse(&text).is_err());
        }
    }
}
