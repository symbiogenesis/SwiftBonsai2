//! Opt-in actual-model application acceptance. Ordinary suites never load weights.
#![cfg(windows)]
#![forbid(unsafe_code)]
#![expect(
    clippy::unwrap_used,
    reason = "Acceptance assertions must fail loudly."
)]

use bonsai_broker::{
    launcher::{LaunchConfig, LaunchedService},
    runtime::{RunExit, RunReport},
};
use serde_json::{Value, json};
use std::{
    env, ffi::OsString, fs, future::pending, io, net::TcpListener, path::Path, time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::TcpStream,
    sync::oneshot,
    task::{LocalSet, spawn_local},
    time::timeout,
};

const CASE: &str = "pinned_native_model_streams_recovers_and_restarts_in_required_sandbox";
const PTQ1_ADAPTER_CASE: &str =
    "pinned_native_ptq1_adapter_starts_and_streams_within_device_budget";
const PQ2_ADAPTER_ROW4_CASE: &str =
    "pinned_native_pq2_adapter_row4_starts_and_streams_within_device_budget";
const CASE_BUDGET: Duration = Duration::from_secs(240);
const OPTIONAL_RESERVE: Duration = Duration::from_secs(120);
const WAIT: Duration = Duration::from_secs(600);
const EXTENDED_COMPLETION_CAP: u32 = 4096;
const BOUNDED_HISTORY_REPETITIONS: usize = 128;
const SHORT_COMPLETION_CAP: u32 = 4;
const STALLED_CONSUMER_PAUSE: Duration = Duration::from_secs(1);
const STOP_WAIT: Duration = Duration::from_secs(30);

fn case_budget_setting(setting: Option<&str>) -> Option<Duration> {
    let setting = setting?;
    if setting.is_empty() || !setting.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let seconds = setting.parse::<u64>().ok()?;
    (1..=CASE_BUDGET.as_secs())
        .contains(&seconds)
        .then_some(Duration::from_secs(seconds))
}

fn case_budget() -> Duration {
    // Hosted preparation passes only the allowance left after owner-job setup.
    // Missing/malformed limits must never silently restore a fresh 240 seconds.
    case_budget_setting(
        env::var("BONSAI_NATIVE_CASE_BUDGET_SECONDS")
            .ok()
            .as_deref(),
    )
    .unwrap()
}

struct Workloads {
    extended_history: bool,
    stalled_consumer: bool,
}

struct AdapterProfile {
    name: &'static str,
    case: &'static str,
    repacked: bool,
    row_workers: u8,
    optional_requests: bool,
}

const PTQ1_ADAPTER_PROFILE: AdapterProfile = AdapterProfile {
    name: "ptq1-adapter",
    case: PTQ1_ADAPTER_CASE,
    repacked: true,
    row_workers: 1,
    optional_requests: true,
};
const PQ2_ADAPTER_ROW4_PROFILE: AdapterProfile = AdapterProfile {
    name: "pq2-adapter-row4",
    case: PQ2_ADAPTER_ROW4_CASE,
    repacked: false,
    row_workers: 4,
    optional_requests: false,
};

#[derive(Debug, PartialEq, Eq)]
struct StreamSummary {
    finish_reason: String,
    content_bytes: usize,
    completion_tokens: Option<u64>,
}

fn stream_observation(summary: &StreamSummary, cap: u32) -> Value {
    assert!(summary.content_bytes > 0);
    assert!(["length", "stop"].contains(&summary.finish_reason.as_str()));
    assert!(
        summary
            .completion_tokens
            .is_none_or(|count| count <= u64::from(cap))
    );
    json!({"requestedCompletionCap":cap, "finishReason":summary.finish_reason,
        "visibleContentBytes":summary.content_bytes,
        "reportedCompletionTokens":summary.completion_tokens})
}

fn cleanup_observation(report: &RunReport, expected: RunExit) -> Value {
    assert_eq!(report.exit, expected);
    assert!(matches!(
        expected,
        RunExit::Shutdown | RunExit::AdmissionClosed
    ));
    let exit = if expected == RunExit::Shutdown {
        "shutdown"
    } else {
        "admission-closed"
    };
    // LaunchedService::run returns success only after confirming the owned Job
    // empty. Callers separately verify the frontend listener is released.
    json!({"exit":exit, "completedOperations":report.completed,
        "failedOperations":report.failed, "peakInFlight":report.peak_in_flight,
        "ownedJobEmpty":true, "listenerReleased":true})
}

fn ci_only() {
    assert_eq!(env::var("GITHUB_ACTIONS").as_deref(), Ok("true"));
    assert_eq!(env::var("GITHUB_REF").as_deref(), Ok("refs/heads/main"));
    assert_eq!(env::consts::ARCH, "aarch64");
    let revision = env::var("GITHUB_SHA").unwrap();
    assert_eq!(revision.len(), 40);
    assert!(revision.bytes().all(|byte| byte.is_ascii_hexdigit()));
}

fn configuration(data: &Path, evidence: &Path) -> LaunchConfig {
    // Production PID checks detect any race between releasing this reservation
    // and the child binding it. No existing listener is ever accepted.
    let reservation = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = reservation.local_addr().unwrap().port().to_string();
    drop(reservation);
    let mut args: Vec<OsString> = [
        "--launch-development",
        "--require-native-sandbox",
        "--native-engine",
        "--profile",
        "swift-pq2-assessment",
        "--frontend-port",
        "0",
        "--backend-port",
        &port,
        "--data-directory",
    ]
    .into_iter()
    .map(OsString::from)
    .collect();
    args.push(data.as_os_str().to_owned());
    args.push("--evidence-parent".into());
    args.push(evidence.as_os_str().to_owned());
    LaunchConfig::parse(args).unwrap()
}

async fn start(data: &Path, evidence: &Path) -> LaunchedService {
    timeout(
        WAIT,
        configuration(data, evidence).start(pending::<io::Result<()>>()),
    )
    .await
    .unwrap()
    .unwrap()
}

fn key(owner: &LaunchedService) -> String {
    // Never print credentials, raw HTTP requests, or the protected directory.
    let value = fs::read_to_string(owner.evidence_directory().join("frontend.key")).unwrap();
    value.trim().to_owned()
}

fn request(messages: Value, streaming: bool, limit: u32) -> Value {
    let mut body = json!({"stream": streaming, "max_completion_tokens": limit});
    body["messages"] = messages;
    body
}

fn short_request(messages: Value, streaming: bool) -> Value {
    let mut body = request(messages, streaming, SHORT_COMPLETION_CAP);
    body["enable_thinking"] = json!(false);
    body
}

async fn connect(port: u16, key: &str, body: &Value) -> TcpStream {
    let bytes = serde_json::to_vec(body).unwrap();
    let mut stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let head = format!(
        "POST /v1/chat/completions HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {key}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
        bytes.len()
    );
    stream.write_all(head.as_bytes()).await.unwrap();
    stream.write_all(&bytes).await.unwrap();
    stream
}

async fn exchange(port: u16, key: &str, body: &Value) -> Vec<u8> {
    timeout(WAIT, async {
        let stream = connect(port, key, body).await;
        let mut bytes = Vec::new();
        stream
            .take(4 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .await
            .unwrap();
        assert!(bytes.len() <= 4 * 1024 * 1024);
        bytes
    })
    .await
    .unwrap()
}

fn response<'a>(bytes: &'a [u8], status: &str) -> (&'a str, &'a [u8]) {
    let end = bytes
        .windows(4)
        .position(|part| part == b"\r\n\r\n")
        .unwrap();
    let head = std::str::from_utf8(&bytes[..end]).unwrap();
    assert_eq!(head.lines().next(), Some(status));
    (head, &bytes[end + 4..])
}

fn json_response(bytes: &[u8], status: &str) -> Value {
    let (head, body) = response(bytes, status);
    let length: usize = head
        .lines()
        .find_map(|line| line.strip_prefix("Content-Length: "))
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(length, body.len());
    serde_json::from_slice(body).unwrap()
}

fn streamed_response(bytes: &[u8]) -> StreamSummary {
    let (head, mut body) = response(bytes, "HTTP/1.1 200 OK");
    assert!(
        head.lines()
            .any(|line| line.eq_ignore_ascii_case("Transfer-Encoding: chunked"))
    );
    let mut payload = Vec::new();
    loop {
        let end = body.windows(2).position(|part| part == b"\r\n").unwrap();
        let length = usize::from_str_radix(std::str::from_utf8(&body[..end]).unwrap(), 16).unwrap();
        body = &body[end + 2..];
        assert!(body.len() >= length + 2);
        payload.extend_from_slice(&body[..length]);
        assert_eq!(&body[length..length + 2], b"\r\n");
        body = &body[length + 2..];
        if length == 0 {
            break;
        }
    }
    assert_eq!(body, b"");
    let payload = std::str::from_utf8(&payload).unwrap();
    let mut terminal = false;
    let mut done = false;
    let mut content = false;
    let mut finish_reason = String::new();
    let mut content_bytes = 0;
    let mut completion_tokens = None;
    for line in payload
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
    {
        assert!(!done, "No event may follow DONE.");
        if line == "[DONE]" {
            assert!(terminal);
            done = true;
            continue;
        }
        let event: Value = serde_json::from_str(line).unwrap();
        assert!(event.get("error").is_none());
        if let Some(tokens) = event["usage"]["completion_tokens"].as_u64() {
            assert!(completion_tokens.replace(tokens).is_none());
        }
        let choices = event["choices"].as_array().unwrap();
        for choice in choices {
            assert!(
                !terminal,
                "No generation event may follow a terminal reason."
            );
            if let Some(text) = choice["delta"]["content"].as_str() {
                content |= !text.is_empty();
                content_bytes += text.len();
            }
            if let Some(reason) = choice["finish_reason"].as_str() {
                assert!(!terminal);
                assert!(["length", "stop"].contains(&reason));
                terminal = true;
                reason.clone_into(&mut finish_reason);
            }
        }
    }
    assert!(terminal && done && content);
    StreamSummary {
        finish_reason,
        content_bytes,
        completion_tokens,
    }
}

fn extended_history(repetitions: usize) -> Value {
    json!([
        {"role":"user", "content":"Keep the following fictional forest notes for the next scene."},
        {"role":"assistant", "content":format!("Forest notes:{}", " tree".repeat(repetitions))},
        {"role":"user", "content":"Name a tree from these notes."}
    ])
}

fn bounded_history_request() -> Value {
    short_request(extended_history(BOUNDED_HISTORY_REPETITIONS), true)
}

async fn extended_acceptance(port: u16, credential: &str) -> Value {
    // Tokenization is deliberately exercised through ordinary public admission.
    // These are fixed fixture sizes, never bytes/characters relabeled as tokens.
    let rejected = exchange(
        port,
        credential,
        &request(extended_history(4500), true, EXTENDED_COMPLETION_CAP),
    )
    .await;
    assert_eq!(
        json_response(&rejected, "HTTP/1.1 413 Content Too Large")["error"]["code"],
        "context_length_exceeded"
    );
    let summary = streamed_response(&exchange(port, credential, &bounded_history_request()).await);
    assert!(
        summary
            .completion_tokens
            .is_none_or(|count| count <= u64::from(SHORT_COMPLETION_CAP))
    );
    // A bounded supplied-history request must leave the same instance usable.
    let reused = short_request(json!([{"role":"user", "content":"Name a bird."}]), true);
    streamed_response(&exchange(port, credential, &reused).await);
    json!({
        "enabled":true, "requested":true, "status":"passed",
        "scope":"bounded-supplied-history-and-same-owner-reuse",
        "rejectedHistoryRepetitions":4500,
        "rejectedRequestedCompletionCap":EXTENDED_COMPLETION_CAP,
        "admittedHistoryRepetitions":BOUNDED_HISTORY_REPETITIONS,
        "requestedCompletionCap":SHORT_COMPLETION_CAP,
        "thinkingEnabled":false,
        "finishReason":summary.finish_reason, "visibleContentBytes":summary.content_bytes,
        "reportedCompletionTokens":summary.completion_tokens,
        "postCompletionRequestPassed":true,
        "exactPromptTokenCountKnown":false, "fullContextQualified":false,
        "fullOutputQualified":false
    })
}

async fn prefix_line(reader: &mut BufReader<TcpStream>, limit: usize) -> String {
    let mut line = String::new();
    let count = reader
        .take(u64::try_from(limit + 1).unwrap())
        .read_line(&mut line)
        .await
        .unwrap();
    assert!(count > 0 && count <= limit && line.ends_with("\r\n"));
    line
}

async fn read_until_content(client: TcpStream) -> (BufReader<TcpStream>, usize) {
    let mut reader = BufReader::new(client);
    let mut header = String::new();
    loop {
        let line = prefix_line(&mut reader, 16 * 1024 - header.len()).await;
        header.push_str(&line);
        if line == "\r\n" {
            break;
        }
    }
    let (head, _) = response(header.as_bytes(), "HTTP/1.1 200 OK");
    for required in [
        "Content-Type: text/event-stream",
        "Transfer-Encoding: chunked",
    ] {
        assert!(head.lines().any(|line| line.eq_ignore_ascii_case(required)));
    }
    let mut payload = Vec::new();
    loop {
        let line = prefix_line(&mut reader, 32).await;
        let length = usize::from_str_radix(line.trim_end(), 16).unwrap();
        assert!(length > 0 && payload.len() + length <= 64 * 1024);
        let first = payload.len();
        payload.resize(first + length, 0);
        reader.read_exact(&mut payload[first..]).await.unwrap();
        let mut ending = [0; 2];
        reader.read_exact(&mut ending).await.unwrap();
        assert_eq!(&ending, b"\r\n");
        // A chunk may split either UTF-8 or an SSE record. Only parse complete
        // lines; incomplete text is retained until another chunk arrives.
        for bytes in payload.split_inclusive(|byte| *byte == b'\n') {
            if bytes.last() != Some(&b'\n') {
                continue;
            }
            let Ok(line) = std::str::from_utf8(bytes) else {
                continue;
            };
            let Some(data) = line.trim_end().strip_prefix("data: ") else {
                continue;
            };
            if let Ok(event) = serde_json::from_str::<Value>(data)
                && let Some(text) = event["choices"].as_array().and_then(|choices| {
                    choices.iter().find_map(|choice| {
                        choice["delta"]["content"]
                            .as_str()
                            .filter(|text| !text.is_empty())
                    })
                })
            {
                return (reader, text.len());
            }
        }
    }
}

fn stalled_stop_summary(
    report: &RunReport,
    completed_before: usize,
    failed_before: usize,
    content_bytes: usize,
) -> Value {
    assert_eq!(report.exit, RunExit::Shutdown);
    // A naturally completed response cannot qualify interruption merely because
    // its client stopped reading. These are broker operation counters, not proof
    // that the native backend was still generating at the instant of Stop.
    assert_eq!(report.completed, completed_before);
    assert_eq!(report.failed, failed_before + 1);
    assert!(content_bytes > 0);
    json!({
        "enabled":true, "requested":true, "status":"interrupted-awaiting-recovery",
        "scope":"explicit-stop-with-nonreading-client",
        "requestedCompletionCap":EXTENDED_COMPLETION_CAP,
        "thinkingEnabled":false,
        "observedVisiblePrefixBytes":content_bytes,
        "nonreadingPauseMilliseconds":STALLED_CONSUMER_PAUSE.as_millis(),
        "stopCompletionBoundSeconds":STOP_WAIT.as_secs(),
        "completedOperationsBeforeStall":completed_before,
        "failedOperationsBeforeStall":failed_before,
        "completedOperationsAtStop":report.completed,
        "failedOperationsAtStop":report.failed,
        "brokerOperationInterrupted":true, "ownedJobStopped":true,
        "backendGeneratingAtStopKnown":false, "socketBackpressureQualified":false,
        "freshRestartAfterStopPassed":false, "performanceQualified":false
    })
}

fn stalled_request() -> Value {
    let mut body = request(
        json!([{"role":"user", "content":"Write a long, detailed story about a cedar forest. Develop several scenes, characters and a complete ending."}]),
        true,
        EXTENDED_COMPLETION_CAP,
    );
    // Preserve a large output reservation without spending the laptop budget
    // generating it. Stop follows the first visible prefix and a fixed pause.
    body["enable_thinking"] = json!(false);
    body
}

async fn nonreading_prefix(port: u16, credential: &str) -> (BufReader<TcpStream>, usize) {
    let body = stalled_request();
    let observed = timeout(WAIT, async {
        let client = connect(port, credential, &body).await;
        read_until_content(client).await
    })
    .await
    .unwrap();
    // Keep the socket and any read-ahead bytes alive without consuming more
    // output. The short pause is a workload condition, not a timing sample.
    tokio::time::sleep(STALLED_CONSUMER_PAUSE).await;
    observed
}

async fn supplied_history_acceptance(port: u16, credential: &str) -> Value {
    let history = json!([
        {"role":"user", "content":"Remember the word cedar."},
        {"role":"assistant", "content":"I will remember cedar.", "reasoning_content":"The supplied word is cedar."},
        {"role":"user", "content":"Write one sentence about cedar."}
    ]);
    stream_observation(
        &streamed_response(&exchange(port, credential, &short_request(history, true)).await),
        SHORT_COMPLETION_CAP,
    )
}

async fn history_and_context_acceptance(
    port: u16,
    credential: &str,
    evidence: &Path,
    report: &mut Value,
) {
    report["phase"] = json!("supplied-history-and-context-rejection-reuse");
    save_acceptance_report(evidence, report);
    report["suppliedHistoryStream"] = supplied_history_acceptance(port, credential).await;
    save_acceptance_report(evidence, report);
    // Exact tokenizer counting must reject before generation, then permit a
    // fresh request in the same owned model instance. This is not an 8K decode.
    let oversized = json!([{"role":"user", "content":" cedar".repeat(9000)}]);
    let rejected = exchange(port, credential, &request(oversized, false, 32)).await;
    assert_eq!(
        json_response(&rejected, "HTTP/1.1 413 Content Too Large")["error"]["code"],
        "context_length_exceeded"
    );
    report["contextRejection"] = json!({"status":413, "code":"context_length_exceeded"});
    save_acceptance_report(evidence, report);
    let small = short_request(json!([{"role":"user", "content":"Name a tree."}]), false);
    let completed = json_response(&exchange(port, credential, &small).await, "HTTP/1.1 200 OK");
    report["sameOwnerAfterContextRejection"] = stream_observation(
        &StreamSummary {
            finish_reason: completed["choices"][0]["finish_reason"]
                .as_str()
                .unwrap()
                .into(),
            content_bytes: completed["choices"][0]["message"]["content"]
                .as_str()
                .unwrap()
                .len(),
            completion_tokens: completed["usage"]["completion_tokens"].as_u64(),
        },
        SHORT_COMPLETION_CAP,
    );
    save_acceptance_report(evidence, report);
}

async fn acceptance(
    data: &Path,
    evidence: &Path,
    workloads: &Workloads,
    budget: Duration,
    report: &mut Value,
) {
    let deadline = tokio::time::Instant::now() + budget;
    report["phase"] = json!("initial-owner-startup");
    save_acceptance_report(evidence, report);
    let owner = start(data, evidence).await;
    let port = owner.port();
    let credential = key(&owner);
    let (stop, stopped) = oneshot::channel();
    let running = spawn_local(owner.run(async { stopped.await.map_err(io::Error::other) }));
    history_and_context_acceptance(port, &credential, evidence, report).await;
    let extended_enabled = optional_workload_enabled(
        workloads.extended_history,
        deadline.saturating_duration_since(tokio::time::Instant::now()),
    );
    let extended_report = if extended_enabled {
        report["phase"] = json!("bounded-history-and-same-owner-reuse");
        save_acceptance_report(evidence, report);
        extended_acceptance(port, &credential).await
    } else {
        skipped_workload(workloads.extended_history)
    };
    report["extendedWorkload"] = extended_report;
    save_acceptance_report(evidence, report);
    let stalled_enabled = optional_workload_enabled(
        workloads.stalled_consumer,
        deadline.saturating_duration_since(tokio::time::Instant::now()),
    );
    let stalled = if stalled_enabled {
        report["phase"] = json!("stalled-consumer-first-content-and-pause");
        save_acceptance_report(evidence, report);
        Some(nonreading_prefix(port, &credential).await)
    } else {
        None
    };
    report["phase"] = json!("initial-owner-stop");
    save_acceptance_report(evidence, report);
    stop.send(()).unwrap();
    let stopped_report = timeout(STOP_WAIT, running).await.unwrap().unwrap().unwrap();
    assert_eq!(stopped_report.exit, RunExit::Shutdown);
    assert!(TcpStream::connect(("127.0.0.1", port)).await.is_err());
    report["initialCleanup"] = cleanup_observation(&stopped_report, RunExit::Shutdown);
    save_acceptance_report(evidence, report);
    let mut stalled_report = if let Some((client, content_bytes)) = stalled {
        let extra = usize::from(extended_enabled);
        let summary =
            stalled_stop_summary(&stopped_report, 2 + 2 * extra, 1 + extra, content_bytes);
        // The client handle stays held through owned shutdown/listener cleanup.
        drop(client);
        summary
    } else {
        skipped_workload(workloads.stalled_consumer)
    };
    report["stalledConsumerWorkload"] = stalled_report.clone();
    fresh_owner_recovery(data, evidence, report).await;
    if stalled_enabled {
        stalled_report["freshRestartAfterStopPassed"] = json!(true);
        stalled_report["status"] = json!("passed");
    }
    report["stalledConsumerWorkload"] = stalled_report;
    report["phase"] = json!("completed");
    report["lifecyclePassed"] = json!(true);
    report["allRequestedWorkloadsPassed"] = json!(
        (!workloads.extended_history || extended_enabled)
            && (!workloads.stalled_consumer || stalled_enabled)
    );
    report["passed"] = report["allRequestedWorkloadsPassed"].clone();
    save_acceptance_report(evidence, report);
}

async fn fresh_owner_recovery(data: &Path, evidence: &Path, report: &mut Value) {
    report["phase"] = json!("fresh-owner-abandonment");
    save_acceptance_report(evidence, report);
    let owner = start(data, evidence).await;
    let port = owner.port();
    let credential = key(&owner);
    let running = spawn_local(owner.run(pending::<io::Result<()>>()));
    let long = stalled_request();
    let client = connect(port, &credential, &long).await;
    let (client, content_bytes) = timeout(WAIT, read_until_content(client)).await.unwrap();
    report["freshOwnerAbandonmentPrefix"] = json!({"observedVisiblePrefixBytes":content_bytes});
    save_acceptance_report(evidence, report);
    drop(client);
    let abandoned_report = timeout(WAIT, running).await.unwrap().unwrap().unwrap();
    assert_eq!(abandoned_report.exit, RunExit::AdmissionClosed);
    assert!(TcpStream::connect(("127.0.0.1", port)).await.is_err());
    report["abandonmentCleanup"] = cleanup_observation(&abandoned_report, RunExit::AdmissionClosed);
    save_acceptance_report(evidence, report);

    // Fresh ownership after a cancelled generation must work without replay.
    report["phase"] = json!("fresh-owner-complete-stream-and-cleanup");
    save_acceptance_report(evidence, report);
    let owner = start(data, evidence).await;
    let port = owner.port();
    let credential = key(&owner);
    let (stop, stopped) = oneshot::channel();
    let running = spawn_local(owner.run(async { stopped.await.map_err(io::Error::other) }));
    report["freshOwnerStream"] = stream_observation(
        &streamed_response(
            &exchange(
                port,
                &credential,
                &short_request(json!([{"role":"user", "content":"Name a flower."}]), true),
            )
            .await,
        ),
        SHORT_COMPLETION_CAP,
    );
    save_acceptance_report(evidence, report);
    stop.send(()).unwrap();
    let completed_report = timeout(WAIT, running).await.unwrap().unwrap().unwrap();
    assert_eq!(completed_report.exit, RunExit::Shutdown);
    assert!(TcpStream::connect(("127.0.0.1", port)).await.is_err());
    report["freshOwnerCleanup"] = cleanup_observation(&completed_report, RunExit::Shutdown);
    save_acceptance_report(evidence, report);
}

fn optional_workload_enabled(requested: bool, remaining: Duration) -> bool {
    requested && remaining > OPTIONAL_RESERVE
}

fn skipped_workload(requested: bool) -> Value {
    json!({"enabled":false, "requested":requested,
        "status":if requested {"pending"} else {"not-requested"},
        "reason":if requested {Some("insufficient-cleanup-and-recovery-reserve")} else {None},
        "requiredRemainingBudgetSeconds":OPTIONAL_RESERVE.as_secs()})
}

#[test]
fn native_stalled_consumer_retains_fragmented_prefix_connection_until_dropped() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        timeout(Duration::from_secs(10), async {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let client = TcpStream::connect(listener.local_addr().unwrap()).await.unwrap();
            let (mut peer, _) = listener.accept().await.unwrap();
            peer.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n")
                .await
                .unwrap();
            let payload = "data: {\"choices\":[{\"delta\":{\"content\":\"cèdre\"},\"finish_reason\":null}]}\n\n";
            let split = payload.find('è').unwrap() + 1;
            for part in [&payload.as_bytes()[..split], &payload.as_bytes()[split..]] {
                peer.write_all(format!("{:X}\r\n", part.len()).as_bytes())
                    .await
                    .unwrap();
                peer.write_all(part).await.unwrap();
                peer.write_all(b"\r\n").await.unwrap();
            }
            let observed = read_until_content(client).await;
            assert_eq!(observed.1, "cèdre".len());
            {
                let mut byte = [0];
                let mut reading = std::pin::pin!(peer.read(&mut byte));
                assert!(
                    std::future::poll_fn(|cx| {
                        std::task::Poll::Ready(reading.as_mut().poll(cx).is_pending())
                    })
                    .await
                );
            }
            drop(observed);
            assert_eq!(peer.read(&mut [0]).await.unwrap(), 0);
        })
        .await
        .unwrap();
    });
}

#[test]
fn native_stalled_consumer_rejects_error_or_unframed_stream_headers() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        timeout(Duration::from_secs(10), async {
            for head in [
                "HTTP/1.1 429 Too Many Requests\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n",
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\n\r\n",
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: 20\r\n\r\n",
            ] {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                let client = TcpStream::connect(listener.local_addr().unwrap()).await.unwrap();
                let (mut peer, _) = listener.accept().await.unwrap();
                peer.write_all(head.as_bytes()).await.unwrap();
                let reading = tokio::spawn(read_until_content(client));
                assert!(reading.await.unwrap_err().is_panic());
            }
        })
        .await
        .unwrap();
    });
}

#[test]
fn native_stalled_consumer_never_qualifies_natural_completion_as_interruption() {
    let interrupted = RunReport {
        exit: RunExit::Shutdown,
        completed: 2,
        failed: 2,
        peak_in_flight: 1,
    };
    let summary = stalled_stop_summary(&interrupted, 2, 1, 5);
    assert_eq!(summary["brokerOperationInterrupted"], true);
    assert_eq!(summary["backendGeneratingAtStopKnown"], false);
    assert_eq!(summary["socketBackpressureQualified"], false);
    assert_eq!(summary["performanceQualified"], false);
    for report in [
        RunReport {
            completed: 3,
            failed: 1,
            ..interrupted
        },
        RunReport {
            failed: 1,
            ..interrupted
        },
        RunReport {
            failed: 3,
            ..interrupted
        },
        RunReport {
            exit: RunExit::AdmissionClosed,
            ..interrupted
        },
    ] {
        assert!(std::panic::catch_unwind(|| stalled_stop_summary(&report, 2, 1, 5)).is_err());
    }
    assert!(std::panic::catch_unwind(|| stalled_stop_summary(&interrupted, 2, 1, 0)).is_err());
}

#[test]
fn native_acceptance_requires_framed_terminal_generation() {
    fn framed(payload: &str) -> Vec<u8> {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n{:X}\r\n{payload}\r\n0\r\n\r\n",
            payload.len()
        ).into_bytes()
    }
    let content =
        "data: {\"choices\":[{\"delta\":{\"content\":\"cedar\"},\"finish_reason\":null}]}\n\n";
    let terminal = "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"length\"}]}\n\n";
    let done = "data: [DONE]\n\n";
    streamed_response(&framed(&format!("{content}{terminal}{done}")));
    let mut truncated = framed(&format!("{content}{terminal}{done}"));
    truncated.pop();
    for invalid in [
        framed(&format!("{content}{terminal}")),
        framed(&format!("{content}{done}")),
        framed(&format!("{terminal}{done}")),
        framed(&format!("{content}{terminal}{content}{done}")),
        framed(&format!("{content}{terminal}{done}{terminal}")),
        truncated,
    ] {
        assert!(std::panic::catch_unwind(|| streamed_response(&invalid)).is_err());
    }
}

#[test]
fn native_extended_acceptance_distinguishes_output_usage_from_delta_count() {
    let framed = |payload: &str| {
        format!(
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n{:X}\r\n{payload}\r\n0\r\n\r\n",
            payload.len()
        )
    };
    let content =
        "data: {\"choices\":[{\"delta\":{\"content\":\"cedar\"},\"finish_reason\":null}]}\n\n";
    let terminal = "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n";
    let terminal_usage = "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"completion_tokens\":7}}\n\n";
    let usage = "data: {\"choices\":[],\"usage\":{\"completion_tokens\":7}}\n\n";
    let done = "data: [DONE]\n\n";
    let expected = StreamSummary {
        finish_reason: "stop".into(),
        content_bytes: 5,
        completion_tokens: Some(7),
    };
    for payload in [
        format!("{content}{terminal_usage}{done}"),
        format!("{content}{terminal}{usage}{done}"),
    ] {
        assert_eq!(streamed_response(framed(&payload).as_bytes()), expected);
    }
    assert_eq!(
        streamed_response(framed(&format!("{content}{terminal}{done}")).as_bytes()),
        StreamSummary {
            finish_reason: "stop".into(),
            content_bytes: 5,
            completion_tokens: None,
        }
    );
    let duplicate = framed(&format!("{content}{terminal_usage}{usage}{done}"));
    assert!(std::panic::catch_unwind(|| streamed_response(duplicate.as_bytes())).is_err());
}

#[test]
fn native_lifecycle_observations_separate_generation_usage_and_owned_cleanup() {
    let summary = StreamSummary {
        finish_reason: "length".into(),
        content_bytes: "cèdre".len(),
        completion_tokens: Some(4),
    };
    let observed = stream_observation(&summary, 4);
    assert_eq!(observed["visibleContentBytes"], 6);
    assert_eq!(observed["reportedCompletionTokens"], 4);
    assert_eq!(observed["requestedCompletionCap"], 4);
    assert!(std::panic::catch_unwind(|| stream_observation(&summary, 2)).is_err());
    assert_eq!(
        stream_observation(
            &StreamSummary {
                completion_tokens: None,
                ..summary
            },
            4
        )["reportedCompletionTokens"],
        Value::Null
    );
    let abandoned = RunReport {
        exit: RunExit::AdmissionClosed,
        completed: 0,
        failed: 1,
        peak_in_flight: 1,
    };
    let cleanup = cleanup_observation(&abandoned, RunExit::AdmissionClosed);
    assert_eq!(cleanup["exit"], "admission-closed");
    assert_eq!(cleanup["completedOperations"], 0);
    assert_eq!(cleanup["failedOperations"], 1);
    assert_eq!(cleanup["ownedJobEmpty"], true);
    assert_eq!(cleanup["listenerReleased"], true);
    assert!(
        std::panic::catch_unwind(|| cleanup_observation(&abandoned, RunExit::Shutdown)).is_err()
    );
    let failed = RunReport {
        exit: RunExit::AcceptFailure,
        ..abandoned
    };
    assert!(
        std::panic::catch_unwind(|| cleanup_observation(&failed, RunExit::AcceptFailure)).is_err()
    );
}

#[test]
fn native_bounded_history_and_stall_requests_preserve_distinct_output_limits() {
    let body = bounded_history_request();
    assert_eq!(body["max_completion_tokens"], 4);
    assert_eq!(body["enable_thinking"], false);
    assert_eq!(body["messages"].as_array().unwrap().len(), 3);
    assert_eq!(body["messages"][1]["role"], "assistant");
    assert_eq!(
        body["messages"][1]["content"],
        format!("Forest notes:{}", " tree".repeat(128))
    );
    let stalled = stalled_request();
    assert_eq!(stalled["max_completion_tokens"], 4096);
    assert_eq!(stalled["enable_thinking"], false);
    assert_eq!(STALLED_CONSUMER_PAUSE, Duration::from_secs(1));
    assert_eq!(STOP_WAIT, Duration::from_secs(30));
    assert_eq!(CASE_BUDGET, Duration::from_secs(240));
    let skipped = skipped_workload(true);
    assert_eq!(skipped["status"], "pending");
    assert_eq!(
        skipped["reason"],
        "insufficient-cleanup-and-recovery-reserve"
    );
    assert_eq!(skipped["requiredRemainingBudgetSeconds"], 120);
    assert_eq!(skipped_workload(false)["status"], "not-requested");
    assert!(!optional_workload_enabled(false, CASE_BUDGET));
    assert!(!optional_workload_enabled(true, OPTIONAL_RESERVE));
    assert!(!optional_workload_enabled(true, Duration::ZERO));
    assert!(optional_workload_enabled(
        true,
        OPTIONAL_RESERVE + Duration::from_secs(1)
    ));
    for (extended, stalled) in [(false, false), (true, false), (false, true), (true, true)] {
        assert_eq!(
            profile_case("pq2-no-adapter", extended, stalled),
            Some(CASE)
        );
    }
}

#[test]
#[ignore = "Actual pinned model; explicitly dispatched main-only device CI, never ordinary suites."]
fn pinned_native_model_streams_recovers_and_restarts_in_required_sandbox() {
    ci_only();
    let budget = case_budget();
    let data = env::var_os("BONSAI_NATIVE_DATA").unwrap();
    let evidence = env::var_os("BONSAI_NATIVE_EVIDENCE").unwrap();
    let data = Path::new(&data);
    let evidence = Path::new(&evidence);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let enabled = |name: &str| {
        let setting = env::var(name);
        assert!(matches!(
            setting.as_deref(),
            Ok("true" | "false") | Err(env::VarError::NotPresent)
        ));
        setting.as_deref() == Ok("true")
    };
    let workloads = Workloads {
        extended_history: enabled("BONSAI_NATIVE_EXTENDED"),
        stalled_consumer: enabled("BONSAI_NATIVE_STALLED_CONSUMER"),
    };
    assert_eq!(
        profile_case(
            "pq2-no-adapter",
            workloads.extended_history,
            workloads.stalled_consumer
        ),
        Some(CASE)
    );
    let mut report = json!({
        "schemaVersion":1, "case":CASE, "revision":env::var("GITHUB_SHA").unwrap(),
        "runId":env::var("GITHUB_RUN_ID").unwrap(), "runAttempt":env::var("GITHUB_RUN_ATTEMPT").unwrap(),
        "scope":"bounded-actual-model-runner-lifecycle", "passed":false,
        "lifecyclePassed":false, "allRequestedWorkloadsPassed":false,
        "aggregateModelCaseBudgetSeconds":budget.as_secs(), "maximumModelCaseBudgetSeconds":CASE_BUDGET.as_secs(),
        "wholeJobMaximumSeconds":300,
        "boundedHistoryRequested":workloads.extended_history,
        "stalledConsumerRequested":workloads.stalled_consumer,
        "runtimeLock":serde_json::from_str::<Value>(include_str!("../../../config/prism-inference.lock.json")).unwrap(),
        "modelSha256":"5912bb739217cf25b4283a098c7b893d9baff36bdf2e72d3dbf4cb999d5633d5",
        "requiredNativeSandbox":true, "adapterEnabled":false, "fullContextQualified":false,
        "fullModelNumericalParityQualified":false, "performanceQualified":false,
        "dailyServiceIsolationQualified":false, "defaultRuntimeQualified":false,
        "extendedWorkload":{"enabled":false, "requested":workloads.extended_history, "status":"pending"},
        "stalledConsumerWorkload":{"enabled":false, "requested":workloads.stalled_consumer, "status":"pending"}
    });
    runtime.block_on(LocalSet::new().run_until(async {
        timeout(
            budget,
            acceptance(data, evidence, &workloads, budget, &mut report),
        )
        .await
        .unwrap();
    }));
    // Cleanup and fresh-owner recovery finish before a skipped requested gate
    // fails the CI case. A successful core lifecycle cannot qualify that gate.
    assert_eq!(
        report["passed"], true,
        "A requested workload remains pending."
    );
}

fn profile_case(profile: &str, extended: bool, stalled: bool) -> Option<&'static str> {
    if profile != "pq2-no-adapter" && (extended || stalled) {
        return None;
    }
    match profile {
        "pq2-no-adapter" => Some(CASE),
        name => adapter_profile(name).map(|selected| selected.case),
    }
}

fn adapter_profile(name: &str) -> Option<&'static AdapterProfile> {
    match name {
        "ptq1-adapter" => Some(&PTQ1_ADAPTER_PROFILE),
        "pq2-adapter-row4" => Some(&PQ2_ADAPTER_ROW4_PROFILE),
        _ => None,
    }
}

fn adapter_arguments(profile: &AdapterProfile, port: &str) -> Vec<OsString> {
    let mut args: Vec<OsString> = [
        "--launch-development",
        "--require-native-sandbox",
        "--native-engine",
        "--profile",
        "swift-pq2-assessment",
        "--use-abliterate-lora",
        "--frontend-port",
        "0",
        "--backend-port",
        port,
    ]
    .into_iter()
    .map(OsString::from)
    .collect();
    if profile.repacked {
        args.push("--use-repacked-ptq1".into());
    }
    if profile.row_workers == 4 {
        args.extend(["--native-row-workers", "4"].map(OsString::from));
    }
    args
}

fn adapter_configuration(profile: &AdapterProfile, data: &Path, evidence: &Path) -> LaunchConfig {
    let reservation = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = reservation.local_addr().unwrap().port().to_string();
    drop(reservation);
    let mut args = adapter_arguments(profile, &port);
    args.push("--data-directory".into());
    args.push(data.as_os_str().to_owned());
    args.push("--evidence-parent".into());
    args.push(evidence.as_os_str().to_owned());
    LaunchConfig::parse(args).unwrap()
}

async fn start_adapter(profile: &AdapterProfile, data: &Path, evidence: &Path) -> LaunchedService {
    timeout(
        WAIT,
        adapter_configuration(profile, data, evidence).start(pending::<io::Result<()>>()),
    )
    .await
    .unwrap()
    .unwrap()
}

fn adapter_request(messages: Value, scale: f64, limit: u32) -> Value {
    assert!(scale.is_finite() && (0.0..=2.0).contains(&scale));
    assert!([2, 4].contains(&limit));
    let mut body = request(messages, true, limit);
    body["enable_thinking"] = json!(false);
    body["abliteration_scale"] = json!(scale);
    body
}

fn adapter_summary(label: &str, scale: f64, cap: u32, summary: &StreamSummary) -> Value {
    assert!(summary.content_bytes > 0);
    assert!(["length", "stop"].contains(&summary.finish_reason.as_str()));
    assert!(
        summary
            .completion_tokens
            .is_none_or(|tokens| tokens <= u64::from(cap))
    );
    json!({"phase":label, "requestedScale":scale,
        "requestedScaleF64Bits":format!("{:016x}", scale.to_bits()),
        "requestedCompletionCap":cap, "finishReason":summary.finish_reason,
        "visibleContentBytes":summary.content_bytes,
        "reportedCompletionTokens":summary.completion_tokens})
}

fn save_acceptance_report(evidence: &Path, report: &Value) {
    fs::write(
        evidence.join("acceptance.json"),
        serde_json::to_vec_pretty(report).unwrap(),
    )
    .unwrap();
}

async fn adapter_stream(port: u16, credential: &str, scale: f64, label: &str, cap: u32) -> Value {
    let messages = json!([{"role":"user","content":"Name a tree."}]);
    adapter_summary(
        label,
        scale,
        cap,
        &streamed_response(
            &exchange(port, credential, &adapter_request(messages, scale, cap)).await,
        ),
    )
}

async fn adapter_owned_stop(
    stop: oneshot::Sender<()>,
    running: tokio::task::JoinHandle<Result<RunReport, bonsai_broker::launcher::LaunchError>>,
    port: u16,
) -> Value {
    stop.send(()).unwrap();
    let report = timeout(STOP_WAIT, running).await.unwrap().unwrap().unwrap();
    assert_eq!(report.exit, RunExit::Shutdown);
    assert!(TcpStream::connect(("127.0.0.1", port)).await.is_err());
    json!({"exit":"shutdown", "completedOperations":report.completed,
        "failedOperations":report.failed, "ownedJobEmpty":true, "listenerReleased":true})
}

async fn bounded_adapter_acceptance(
    profile: &AdapterProfile,
    data: &Path,
    evidence: &Path,
    budget: Duration,
    report: &mut Value,
) {
    let deadline = tokio::time::Instant::now() + budget;
    report["phase"] = json!("initial-owner-startup");
    save_acceptance_report(evidence, report);
    let owner = start_adapter(profile, data, evidence).await;
    report["verifiedReadinessObserved"] = json!(true);
    report["adapterEnabled"] = json!(true);
    report["phase"] = json!("required-scale-1.3-stream");
    save_acceptance_report(evidence, report);
    let port = owner.port();
    let credential = key(&owner);
    let (stop, stopped) = oneshot::channel();
    let running = spawn_local(owner.run(async { stopped.await.map_err(io::Error::other) }));
    let mut streams =
        vec![adapter_stream(port, &credential, 1.3, "required-scale-1.3-stream", 4).await];
    report["streams"] = json!(streams);
    save_acceptance_report(evidence, report);
    // Optional requests share this one aggregate budget; no matrix or restart
    // can continue in another job. Preserve a conservative teardown reserve.
    if profile.optional_requests
        && deadline.saturating_duration_since(tokio::time::Instant::now()) > OPTIONAL_RESERVE
    {
        report["phase"] = json!("optional-scale-zero");
        save_acceptance_report(evidence, report);
        streams.push(adapter_stream(port, &credential, 0.0, "optional-scale-zero", 2).await);
        report["streams"] = json!(streams);
        save_acceptance_report(evidence, report);
        if deadline.saturating_duration_since(tokio::time::Instant::now()) > OPTIONAL_RESERVE {
            report["phase"] = json!("optional-invalid-scale-and-reuse");
            save_acceptance_report(evidence, report);
            let mut body =
                adapter_request(json!([{"role":"user","content":"Name a tree."}]), 1.3, 2);
            body["abliteration_scale"] = json!(2.000_000_01);
            let rejected = json_response(
                &exchange(port, &credential, &body).await,
                "HTTP/1.1 400 Bad Request",
            );
            assert!(rejected["error"].is_object());
            report["invalidScaleRejected"] = json!(true);
            save_acceptance_report(evidence, report);
            streams.push(
                adapter_stream(port, &credential, 1.3, "same-owner-after-invalid-scale", 2).await,
            );
            report["streams"] = json!(streams);
            report["sameOwnerAfterInvalidScalePassed"] = json!(true);
            save_acceptance_report(evidence, report);
        }
    }
    report["phase"] = json!("owned-cleanup");
    save_acceptance_report(evidence, report);
    report["initialCleanup"] = adapter_owned_stop(stop, running, port).await;
    report["phase"] = json!("completed");
    report["passed"] = json!(true);
    save_acceptance_report(evidence, report);
}

#[test]
fn native_profile_selection_and_scale_receipts_preserve_bounded_scope() {
    assert_eq!(profile_case("pq2-no-adapter", false, false), Some(CASE));
    assert_eq!(profile_case("pq2-no-adapter", true, true), Some(CASE));
    assert_eq!(
        profile_case("ptq1-adapter", false, false),
        Some(PTQ1_ADAPTER_CASE)
    );
    for (profile, extended, stalled) in [
        ("ptq1-adapter", true, false),
        ("ptq1-adapter", false, true),
        ("pq2-adapter-row4", true, false),
        ("pq2-adapter-row4", false, true),
        ("unknown", false, false),
    ] {
        assert_eq!(profile_case(profile, extended, stalled), None);
    }
    let summary = adapter_summary(
        "synthetic",
        -0.0,
        4,
        &StreamSummary {
            finish_reason: "length".into(),
            content_bytes: 5,
            completion_tokens: Some(4),
        },
    );
    assert_eq!(summary["requestedScaleF64Bits"], "8000000000000000");
    let body = adapter_request(json!([{"role":"user","content":"Name a tree."}]), -0.0, 2);
    assert_eq!(
        body["abliteration_scale"].as_f64().unwrap().to_bits(),
        (-0.0_f64).to_bits()
    );
    assert_eq!(body["max_completion_tokens"], 2);
    assert_eq!(body["enable_thinking"], false);
    assert!(std::panic::catch_unwind(|| adapter_request(json!([]), f64::NAN, 4)).is_err());
    assert!(std::panic::catch_unwind(|| adapter_request(json!([]), 1.3, 4096)).is_err());
    assert!(
        std::panic::catch_unwind(|| adapter_summary(
            "synthetic",
            1.3,
            4,
            &StreamSummary {
                finish_reason: "length".into(),
                content_bytes: 5,
                completion_tokens: Some(5),
            }
        ))
        .is_err()
    );
}

#[test]
fn native_row4_profile_is_explicit_original_pq2_and_one_stream_only() {
    assert_eq!(
        profile_case("pq2-adapter-row4", false, false),
        Some(PQ2_ADAPTER_ROW4_CASE)
    );
    assert!(adapter_profile("pq2-no-adapter").is_none());
    assert!(adapter_profile("pq2-adapter").is_none());
    for profile in [&PTQ1_ADAPTER_PROFILE, &PQ2_ADAPTER_ROW4_PROFILE] {
        assert_eq!(adapter_profile(profile.name).unwrap().case, profile.case);
        let args = adapter_arguments(profile, "8081");
        for required in [
            "--native-engine",
            "--require-native-sandbox",
            "--use-abliterate-lora",
        ] {
            assert_eq!(args.iter().filter(|arg| *arg == required).count(), 1);
        }
        assert!(
            args.windows(2)
                .any(|pair| pair[0] == "--profile" && pair[1] == "swift-pq2-assessment")
        );
        assert_eq!(
            args.iter().any(|arg| arg == "--use-repacked-ptq1"),
            profile.repacked
        );
        assert_eq!(
            args.iter()
                .filter(|arg| *arg == "--native-row-workers")
                .count(),
            usize::from(profile.row_workers == 4)
        );
        if profile.row_workers == 4 {
            assert!(
                args.windows(2)
                    .any(|pair| pair[0] == "--native-row-workers" && pair[1] == "4")
            );
            assert!(!profile.repacked);
            assert!(!profile.optional_requests);
        } else {
            assert_eq!(profile.row_workers, 1);
            assert!(profile.optional_requests);
        }
    }
    let body = adapter_request(json!([{"role":"user","content":"Name a tree."}]), 1.3, 4);
    assert_eq!(body["abliteration_scale"], 1.3);
    assert_eq!(body["max_completion_tokens"], 4);
    assert_eq!(body["enable_thinking"], false);
    assert_eq!(CASE_BUDGET, Duration::from_secs(240));
}

#[test]
#[ignore = "Actual PTQ1 and approved adapter; explicit bounded main-only device CI, never ordinary suites."]
fn pinned_native_ptq1_adapter_starts_and_streams_within_device_budget() {
    ci_only();
    let budget = case_budget();
    assert_eq!(
        profile_case(
            &env::var("BONSAI_NATIVE_PROFILE").unwrap(),
            env::var("BONSAI_NATIVE_EXTENDED").as_deref() == Ok("true"),
            env::var("BONSAI_NATIVE_STALLED_CONSUMER").as_deref() == Ok("true")
        ),
        Some(PTQ1_ADAPTER_CASE)
    );
    let data = env::var_os("BONSAI_NATIVE_DATA").unwrap();
    let evidence = env::var_os("BONSAI_NATIVE_EVIDENCE").unwrap();
    let evidence = Path::new(&evidence);
    let mut report = json!({"schemaVersion":1, "case":PTQ1_ADAPTER_CASE,
        "revision":env::var("GITHUB_SHA").unwrap(), "runId":env::var("GITHUB_RUN_ID").unwrap(),
        "runAttempt":env::var("GITHUB_RUN_ATTEMPT").unwrap(), "scope":"bounded-actual-model-ptq1-adapter-lifecycle",
        "profile":"ptq1-adapter", "passed":false, "requiredNativeSandbox":true,
        "aggregateModelCaseBudgetSeconds":budget.as_secs(), "maximumModelCaseBudgetSeconds":CASE_BUDGET.as_secs(),
        "wholeJobMaximumSeconds":300,
        "sameOwnerAfterInvalidScalePassed":false, "invalidScaleRejected":false,
        "unrunIndependentGates":["signed-zero-and-scale-2", "context-rejection-and-reuse", "fresh-owner-restart",
            "supplied-history", "extended-history", "stalled-consumer", "full-context-and-output"],
        "runtimeLock":serde_json::from_str::<Value>(include_str!("../../../config/prism-inference.lock.json")).unwrap(),
        "modelExportReceipt":serde_json::from_str::<Value>(include_str!("../../../config/swift.ptq1-export.json")).unwrap(),
        "adapterLock":serde_json::from_str::<Value>(include_str!("../../../config/swift-lora.lock.json")).unwrap(),
        "adapterRequested":true, "adapterEnabled":false, "verifiedReadinessObserved":false, "completeStateParityQualified":false, "authoringBasisQualified":false,
        "fullContextQualified":false, "fullOutputQualified":false, "performanceQualified":false,
        "qualityQualified":false, "dailyServiceIsolationQualified":false, "defaultRuntimeQualified":false});
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(LocalSet::new().run_until(async {
        timeout(
            budget,
            bounded_adapter_acceptance(
                &PTQ1_ADAPTER_PROFILE,
                Path::new(&data),
                evidence,
                budget,
                &mut report,
            ),
        )
        .await
        .unwrap();
    }));
}

#[test]
#[ignore = "Actual original PQ2 and approved adapter with four row workers; explicit bounded main-only device CI."]
fn pinned_native_pq2_adapter_row4_starts_and_streams_within_device_budget() {
    ci_only();
    let budget = case_budget();
    assert_eq!(
        profile_case(
            &env::var("BONSAI_NATIVE_PROFILE").unwrap(),
            env::var("BONSAI_NATIVE_EXTENDED").as_deref() == Ok("true"),
            env::var("BONSAI_NATIVE_STALLED_CONSUMER").as_deref() == Ok("true")
        ),
        Some(PQ2_ADAPTER_ROW4_CASE)
    );
    let runtime_lock =
        serde_json::from_str::<Value>(include_str!("../../../config/prism-inference.lock.json"))
            .unwrap();
    assert_eq!(runtime_lock["version"], "0.1.9");
    let data = env::var_os("BONSAI_NATIVE_DATA").unwrap();
    let evidence = env::var_os("BONSAI_NATIVE_EVIDENCE").unwrap();
    let evidence = Path::new(&evidence);
    let mut report = json!({"schemaVersion":1, "case":PQ2_ADAPTER_ROW4_CASE,
        "revision":env::var("GITHUB_SHA").unwrap(), "runId":env::var("GITHUB_RUN_ID").unwrap(),
        "runAttempt":env::var("GITHUB_RUN_ATTEMPT").unwrap(), "scope":"bounded-actual-model-pq2-adapter-row4-lifecycle",
        "profile":PQ2_ADAPTER_ROW4_PROFILE.name, "passed":false, "requiredNativeSandbox":true,
        "aggregateModelCaseBudgetSeconds":budget.as_secs(), "maximumModelCaseBudgetSeconds":CASE_BUDGET.as_secs(),
        "wholeJobMaximumSeconds":300,
        "configuredNativeRowWorkers":4, "rowWorkerCountIndependentlyObserved":false,
        "requiredStreams":1, "requestedAdapterScale":1.3, "requestedCompletionCap":4,
        "weightRepresentation":"PQ2_0", "runtimeLock":runtime_lock,
        "modelIdentity":serde_json::from_str::<Value>(include_str!("../../../config/swift-assessment.lock.json")).unwrap()["model"],
        "adapterLock":serde_json::from_str::<Value>(include_str!("../../../config/swift-lora.lock.json")).unwrap(),
        "adapterRequested":true, "adapterEnabled":false, "verifiedReadinessObserved":false,
        "sameOwnerAfterInvalidScalePassed":false, "invalidScaleRejected":false,
        "unrunIndependentGates":["multi-scale-and-signed-zero", "invalid-scale-and-reuse", "context-rejection-and-reuse",
            "fresh-owner-restart", "supplied-history", "extended-history", "stalled-consumer", "full-context-and-output",
            "performance-and-TPS", "creative-quality", "daily-service-isolation"],
        "completeStateParityQualified":false, "authoringBasisQualified":false,
        "fullContextQualified":false, "fullOutputQualified":false, "performanceQualified":false,
        "qualityQualified":false, "dailyServiceIsolationQualified":false, "defaultRuntimeQualified":false});
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(LocalSet::new().run_until(async {
        timeout(
            budget,
            bounded_adapter_acceptance(
                &PQ2_ADAPTER_ROW4_PROFILE,
                Path::new(&data),
                evidence,
                budget,
                &mut report,
            ),
        )
        .await
        .unwrap();
    }));
}

#[test]
fn native_case_budget_rejects_missing_unbounded_and_malformed_allowances() {
    for invalid in [
        None,
        Some(""),
        Some("0"),
        Some("241"),
        Some("3600"),
        Some("-1"),
        Some("+120"),
        Some("120.0"),
        Some(" 120"),
        Some("120\n"),
        Some("18446744073709551616"),
    ] {
        assert_eq!(case_budget_setting(invalid), None);
    }
    for seconds in [1, 30, 120, 121, 239, 240] {
        let budget = case_budget_setting(Some(&seconds.to_string())).unwrap();
        assert_eq!(budget, Duration::from_secs(seconds));
        // Setup can reduce the allowance below the optional-workload reserve;
        // neither workload may use the original maximum to reopen admission.
        assert_eq!(optional_workload_enabled(true, budget), seconds > 120);
    }
}

#[test]
fn native_case_budget_cancels_owned_tasks_before_followup() {
    struct Owned(std::rc::Rc<std::cell::Cell<bool>>);
    impl Drop for Owned {
        fn drop(&mut self) {
            self.0.set(true);
        }
    }
    let dropped = std::rc::Rc::new(std::cell::Cell::new(false));
    let followed = std::rc::Rc::new(std::cell::Cell::new(false));
    let owner = Owned(dropped.clone());
    let observed = followed.clone();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap();
    runtime.block_on(LocalSet::new().run_until(async {
        let result = timeout(CASE_BUDGET, async {
            spawn_local(async move {
                let _owner = owner;
                pending::<()>().await;
            });
            tokio::task::yield_now().await;
            pending::<()>().await;
            observed.set(true);
        })
        .await;
        assert!(result.is_err());
    }));
    assert!(dropped.get());
    assert!(!followed.get());
}
