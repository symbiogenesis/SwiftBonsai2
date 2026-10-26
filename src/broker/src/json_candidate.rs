//! Correctness and CI-only timing for the pinned optional JSON parser.

use crate::{AuthorizedHead, MAX_BODY_BYTES, MAX_CONTENT_PARTS, MAX_MESSAGES, Rejection};
use serde::Serialize;
use std::{error::Error, hint::black_box, path::PathBuf, time::Instant};

fn canonical(body: &[u8], candidate: bool, pinned: bool) -> Result<Vec<u8>, Rejection> {
    let head = AuthorizedHead {
        length: body.len(),
        pinned_abliteration: pinned,
    };
    if candidate {
        head.prepare_simd_json(body)
    } else {
        head.prepare(body)
    }
    .map(|chat| chat.body)
}

fn parity(body: &[u8], pinned: bool) {
    let original = body.to_vec();
    assert_eq!(
        canonical(body, true, pinned),
        canonical(body, false, pinned)
    );
    assert_eq!(body, original);
}

#[test]
fn fixed_candidate_preserves_unicode_and_rejections() {
    for content in [
        r"plain",
        r#"quote: \"; slash: \\; newline: \n"#,
        r"\u0000\u007f\u0080\u07ff\u0800\uffff",
        r"\ud800\udc00\udbff\udfff\ud83c\udf33",
        "雨、雪、🌳 and café",
    ] {
        let body = format!(r#"{{"messages":[{{"role":"user","content":"{content}"}}]}}"#);
        parity(body.as_bytes(), false);
        assert!(canonical(body.as_bytes(), true, false).is_ok());
    }
    for content in [
        r"\ud800",
        r"\udbff",
        r"\udc00",
        r"\udfff",
        r"\ud800x",
        r"\ud800\n",
        r"\ud800\u0000",
        r"\ud800\ud800",
        r"\ud800\uFFFF",
        r"\ud800\u12xz",
        r"\ud800\u123",
        r"\x00",
    ] {
        let body = format!(r#"{{"messages":[{{"role":"user","content":"{content}"}}]}}"#);
        parity(body.as_bytes(), false);
        assert_eq!(
            canonical(body.as_bytes(), true, false),
            Err(Rejection::ChatSchema)
        );
    }
    for body in [
        b"\xff".as_slice(),
        b"{\"messages\":[{\"role\":\"user\",\"content\":\"\xc0\xaf\"}]}",
        b"{\"messages\":[{\"role\":\"user\",\"content\":\"\xed\xa0\x80\"}]}",
        b"{\"messages\":[{\"role\":\"user\",\"content\":\"\x00\"}]}",
    ] {
        parity(body, false);
        assert_eq!(canonical(body, true, false), Err(Rejection::ChatSchema));
    }
}

#[test]
fn fixed_candidate_preserves_schema_history_and_bounds() -> Result<(), Box<dyn Error>> {
    for body in [
        r#"{"messages":[{"role":"user","content":"a","content":"b"}]}"#,
        r#"{"messages":[{"role":"user","content":"a"}],"messages":[]}"#,
        r#"{"messages":[{"role":"user","content":"a"}],"stream":true,"stream":false}"#,
        r#"{"messages":[{"role":"user","content":"a"}],"tools":[]}"#,
        r#"{"messages":[{"role":"user","content":"a","name":"x"}]}"#,
        r#"{"messages":[{"role":"user","content":null}]}"#,
        r#"{"messages":[{"role":{"user":null},"content":"a"}]}"#,
        r#"{"messages":[["user","a"]]}"#,
        r#"[[[{"role":"user","content":"a"}]],1,true]"#,
        r#"{"messages":[{"role":"assistant","content":"a"}]}"#,
        r#"{"messages":[{"role":"user","content":"a","reasoning_content":"x"}]}"#,
        r#"{"messages":[{"role":"user","content":"a"}],"max_completion_tokens":1.0}"#,
        r#"{"messages":[{"role":"user","content":"a"}],"max_completion_tokens":null}"#,
        r#"{"messages":[{"role":"user","content":"a"}],"abliteration_scale":null}"#,
        r#"{"messages":[{"role":"user","content":"a"}],"abliteration_scale":2.0000001}"#,
        r#"{"messages":[{"role":"user","content":"a"}],"enable_thinking":1}"#,
        r#"{"messages":[{"role":"user","content":"a"}]}{}"#,
    ] {
        parity(body.as_bytes(), false);
        parity(body.as_bytes(), true);
        assert!(canonical(body.as_bytes(), true, true).is_err());
    }
    for count in [0, 1, 4096, 4097, u32::MAX] {
        let body = format!(
            r#"{{"messages":[{{"role":"user","content":"a"}}],"max_completion_tokens":{count}}}"#
        );
        parity(body.as_bytes(), false);
    }
    for scale in ["0", "-0.0", "0.1", "1.3", "2"] {
        let body = format!(
            r#"{{"messages":[{{"role":"user","content":"a"}}],"abliteration_scale":{scale}}}"#
        );
        parity(body.as_bytes(), false);
        parity(body.as_bytes(), true);
        assert!(canonical(body.as_bytes(), true, true).is_ok());
    }
    for count in [MAX_CONTENT_PARTS, MAX_CONTENT_PARTS + 1] {
        let parts = vec![serde_json::json!({"type":"text", "text":"雨\n"}); count];
        let body =
            serde_json::to_vec(&serde_json::json!({"messages":[{"role":"user","content":parts}]}))?;
        parity(&body, false);
        assert_eq!(
            canonical(&body, true, false).is_ok(),
            count == MAX_CONTENT_PARTS
        );
    }
    for count in [MAX_MESSAGES, MAX_MESSAGES + 2] {
        let body = history(count)?;
        parity(&body, false);
        assert_eq!(canonical(&body, true, false).is_ok(), count == MAX_MESSAGES);
    }
    for (_, body) in fixtures()? {
        parity(&body, false);
        assert!(canonical(&body, true, false).is_ok());
    }
    let oversized = vec![b' '; MAX_BODY_BYTES + 1];
    parity(&oversized, false);
    assert_eq!(canonical(&oversized, true, false), Err(Rejection::BodySize));
    let near_limit = serde_json::to_vec(
        &serde_json::json!({"messages":[{"role":"user","content":"x".repeat(MAX_BODY_BYTES - 100)}]}),
    )?;
    parity(&near_limit, false);
    assert_eq!(
        canonical(&near_limit, true, false),
        Err(Rejection::BodySize)
    );
    Ok(())
}

#[test]
fn fixed_candidate_preserves_serde_scale_lexemes() {
    for key in ["abliteration_scale", r"abliteration_\u0073cale"] {
        for scale in [
            "1.2785641882898677",
            "1.6447349639294147",
            "1.8830876439670798",
            "1.9999999999999999",
            "2.0000000000000001",
            "-1e-400",
            "0e9999",
            "null",
            r#""1.3""#,
            "[]",
            "{}",
            "true",
            "-0.0001",
            "2.0000000000000004",
            "1e9999",
        ] {
            let body =
                format!(r#"{{"messages":[{{"role":"user","content":"a"}}],"{key}":{scale}}}"#);
            parity(body.as_bytes(), true);
            parity(body.as_bytes(), false);
        }
    }
    parity(br#"{"messages":[{"role":"user","content":"a"}],"abliteration_scale":1,"abliteration_\u0073cale":2}"#, true);
}

fn history(count: usize) -> Result<Vec<u8>, serde_json::Error> {
    let mut messages =
        vec![serde_json::json!({"role":"system","content":"Use the supplied scene packet."})];
    for index in 1..count {
        let role = if index % 2 == 1 { "user" } else { "assistant" };
        messages.push(serde_json::json!({"role":role,"content":"Rain over the city; preserve voice and continuity. ".repeat(8)}));
    }
    serde_json::to_vec(&serde_json::json!({"messages":messages}))
}

fn fixtures() -> Result<Vec<(&'static str, Vec<u8>)>, serde_json::Error> {
    Ok(vec![
        ("small_plain", br#"{"messages":[{"role":"user","content":"Draft a scene with rain."}]}"#.to_vec()),
        ("small_escaped", br#"{"messages":[{"role":"user","content":"Quote: \"; line:\n; tree: \ud83c\udf33; snow: \u96ea"}]}"#.to_vec()),
        ("history_8", history(8)?),
        ("history_32", history(32)?),
        ("history_128", history(128)?),
        ("near_body_limit", serde_json::to_vec(&serde_json::json!({"messages":[{"role":"user","content":"x".repeat(MAX_BODY_BYTES - 768)}]}))?),
    ])
}

#[derive(Serialize)]
struct CaseReport {
    name: &'static str,
    body_bytes: usize,
    pinned_adapter: bool,
    iterations_per_round: usize,
    serde_nanoseconds: Vec<u128>,
    candidate_nanoseconds: Vec<u128>,
    median_time_ratio: f64,
}

#[derive(Serialize)]
struct Report {
    source_revision: &'static str,
    tested_revision: String,
    architecture: &'static str,
    warmup_rounds: usize,
    measured_rounds: usize,
    original_six_geometric_mean_time_ratio: f64,
    original_six_worst_case_time_ratio: f64,
    original_six_qualifies: bool,
    supplemental_worst_case_time_ratio: f64,
    qualifies: bool,
    cases: Vec<CaseReport>,
    supplemental_cases: Vec<CaseReport>,
}

fn timed(body: &[u8], candidate: bool, pinned: bool, iterations: usize) -> u128 {
    let started = Instant::now();
    for _ in 0..iterations {
        let result = canonical(black_box(body), candidate, pinned);
        assert!(result.is_ok());
        drop(black_box(result));
    }
    started.elapsed().as_nanos()
}

#[allow(
    clippy::cast_precision_loss,
    clippy::as_conversions,
    reason = "Timing ratios intentionally approximate integer nanoseconds in binary64."
)]
fn ratio(candidate: u128, baseline: u128) -> f64 {
    candidate as f64 / baseline as f64
}

fn compare_case(name: &'static str, body: &[u8], pinned_adapter: bool) -> CaseReport {
    let iterations = if body.len() > 100_000 { 64 } else { 2048 };
    let mut serde_nanoseconds = Vec::new();
    let mut candidate_nanoseconds = Vec::new();
    for round in 0..10 {
        let mut elapsed = [0; 2];
        for offset in 0..2 {
            let candidate = (round + offset) % 2 == 1;
            elapsed[usize::from(candidate)] = timed(body, candidate, pinned_adapter, iterations);
        }
        if round != 0 {
            serde_nanoseconds.push(elapsed[0]);
            candidate_nanoseconds.push(elapsed[1]);
        }
    }
    let mut baseline = serde_nanoseconds.clone();
    let mut candidate = candidate_nanoseconds.clone();
    baseline.sort_unstable();
    candidate.sort_unstable();
    let median_time_ratio = ratio(candidate[4], baseline[4]);
    CaseReport {
        name,
        body_bytes: body.len(),
        pinned_adapter,
        iterations_per_round: iterations,
        serde_nanoseconds,
        candidate_nanoseconds,
        median_time_ratio,
    }
}

#[test]
#[ignore = "benchmark execution requires the main-only JSON candidate workflow"]
fn ci_comparison() -> Result<(), Box<dyn Error>> {
    assert_eq!(std::env::var("GITHUB_ACTIONS")?.as_str(), "true");
    assert_eq!(std::env::var("GITHUB_REF")?.as_str(), "refs/heads/main");
    let directory = PathBuf::from(std::env::var("BONSAI_JSON_EVIDENCE")?);
    assert!(directory.is_absolute());
    assert!(!directory.to_string_lossy().contains('%'));
    assert!(directory.is_dir());
    fixed_candidate_preserves_unicode_and_rejections();
    fixed_candidate_preserves_schema_history_and_bounds()?;
    fixed_candidate_preserves_serde_scale_lexemes();
    let mut cases = Vec::new();
    for (name, body) in fixtures()? {
        cases.push(compare_case(name, &body, false));
    }
    let geometric_mean_time_ratio = (cases
        .iter()
        .map(|case| case.median_time_ratio.ln())
        .sum::<f64>()
        / 6.0)
        .exp();
    let worst_case_time_ratio = cases
        .iter()
        .map(|case| case.median_time_ratio)
        .fold(0.0, f64::max);
    let original_six_qualifies = geometric_mean_time_ratio <= 0.95 && worst_case_time_ratio <= 1.05;
    let supplemental_cases = [
        ("adapter_scale_default", br#"{"messages":[{"role":"user","content":"Draft a scene with rain."}],"abliteration_scale":1.3}"#.as_slice()),
        ("adapter_scale_decimal", br#"{"messages":[{"role":"user","content":"Draft a scene with rain."}],"abliteration_scale":1.2785641882898677}"#.as_slice()),
        ("adapter_scale_escaped_key", br#"{"messages":[{"role":"user","content":"Draft a scene with rain."}],"abliteration_\u0073cale":1.6447349639294147}"#.as_slice()),
    ].into_iter().map(|(name, body)| compare_case(name, body, true)).collect::<Vec<_>>();
    let supplemental_worst_case_time_ratio = supplemental_cases
        .iter()
        .map(|case| case.median_time_ratio)
        .fold(0.0, f64::max);
    let report = Report {
        source_revision: "ff8f5c8bb75dfc56803321c2f5a92656c7de842f",
        tested_revision: std::env::var("GITHUB_SHA")?,
        architecture: std::env::consts::ARCH,
        warmup_rounds: 1,
        measured_rounds: 9,
        original_six_geometric_mean_time_ratio: geometric_mean_time_ratio,
        original_six_worst_case_time_ratio: worst_case_time_ratio,
        original_six_qualifies,
        supplemental_worst_case_time_ratio,
        qualifies: original_six_qualifies && supplemental_worst_case_time_ratio <= 1.05,
        cases,
        supplemental_cases,
    };
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(directory.join("comparison.json"))?;
    serde_json::to_writer_pretty(file, &report)?;
    Ok(())
}
