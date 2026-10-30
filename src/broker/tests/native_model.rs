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
    "pinned_native_ptq1_adapter_streams_scales_recovers_and_restarts_in_required_sandbox";
const WAIT: Duration = Duration::from_secs(600);
const EXTENDED_COMPLETION_CAP: u32 = 4096;
const STALLED_CONSUMER_PAUSE: Duration = Duration::from_secs(1);
const STOP_WAIT: Duration = Duration::from_secs(30);

struct Workloads {
    extended_history: bool,
    stalled_consumer: bool,
}

#[derive(Debug, PartialEq, Eq)]
struct StreamSummary {
    finish_reason: String,
    content_bytes: usize,
    completion_tokens: Option<u64>,
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
        {"role":"user", "content":"Write a long, detailed story continuing these notes. Develop several scenes, characters and a complete ending."}
    ])
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
    let summary = streamed_response(
        &exchange(
            port,
            credential,
            &request(extended_history(3900), true, EXTENDED_COMPLETION_CAP),
        )
        .await,
    );
    assert!(summary.completion_tokens.is_none_or(|count| count <= 4096));
    // A successful long request must leave the same model instance usable.
    streamed_response(
        &exchange(
            port,
            credential,
            &request(json!([{"role":"user", "content":"Name a bird."}]), true, 16),
        )
        .await,
    );
    json!({
        "enabled":true, "scope":"supplied-history-and-full-output-reservation",
        "rejectedHistoryRepetitions":4500, "admittedHistoryRepetitions":3900,
        "requestedCompletionCap":EXTENDED_COMPLETION_CAP,
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
        "enabled":true, "scope":"explicit-stop-with-nonreading-client",
        "requestedCompletionCap":EXTENDED_COMPLETION_CAP,
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

async fn nonreading_prefix(port: u16, credential: &str) -> (BufReader<TcpStream>, usize) {
    let body = request(
        json!([{"role":"user", "content":"Write a long, detailed story about a cedar forest. Develop several scenes, characters and a complete ending."}]),
        true,
        EXTENDED_COMPLETION_CAP,
    );
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

async fn supplied_history_acceptance(port: u16, credential: &str) {
    let history = json!([
        {"role":"user", "content":"Remember the word cedar."},
        {"role":"assistant", "content":"I will remember cedar.", "reasoning_content":"The supplied word is cedar."},
        {"role":"user", "content":"Write one sentence about cedar."}
    ]);
    streamed_response(&exchange(port, credential, &request(history, true, 32)).await);
}

async fn acceptance(data: &Path, evidence: &Path, workloads: &Workloads) -> Value {
    let owner = start(data, evidence).await;
    let port = owner.port();
    let credential = key(&owner);
    let (stop, stopped) = oneshot::channel();
    let running = spawn_local(owner.run(async { stopped.await.map_err(io::Error::other) }));
    supplied_history_acceptance(port, &credential).await;
    // Exact tokenizer counting must reject before generation, then permit a
    // fresh request in the same owned model instance. This is not an 8K decode.
    let oversized = json!([{"role":"user", "content":" cedar".repeat(9000)}]);
    let rejected = exchange(port, &credential, &request(oversized, false, 32)).await;
    assert_eq!(
        json_response(&rejected, "HTTP/1.1 413 Content Too Large")["error"]["code"],
        "context_length_exceeded"
    );
    let small = request(
        json!([{"role":"user", "content":"Name a tree."}]),
        false,
        16,
    );
    let completed = json_response(
        &exchange(port, &credential, &small).await,
        "HTTP/1.1 200 OK",
    );
    assert!(
        completed["choices"][0]["message"]["content"]
            .as_str()
            .is_some_and(|text| !text.is_empty())
    );
    assert!(
        ["length", "stop"].contains(&completed["choices"][0]["finish_reason"].as_str().unwrap())
    );
    let extended_report = if workloads.extended_history {
        extended_acceptance(port, &credential).await
    } else {
        json!({"enabled":false})
    };
    let stalled = if workloads.stalled_consumer {
        Some(nonreading_prefix(port, &credential).await)
    } else {
        None
    };
    stop.send(()).unwrap();
    let stopped_report = timeout(STOP_WAIT, running).await.unwrap().unwrap().unwrap();
    assert_eq!(stopped_report.exit, RunExit::Shutdown);
    assert!(TcpStream::connect(("127.0.0.1", port)).await.is_err());
    let mut stalled_report = if let Some((client, content_bytes)) = stalled {
        let extra = usize::from(workloads.extended_history);
        let summary =
            stalled_stop_summary(&stopped_report, 2 + 2 * extra, 1 + extra, content_bytes);
        // The client handle stays held through owned shutdown/listener cleanup.
        drop(client);
        summary
    } else {
        json!({"enabled":false})
    };
    let owner = start(data, evidence).await;
    let port = owner.port();
    let credential = key(&owner);
    let running = spawn_local(owner.run(pending::<io::Result<()>>()));
    let long = request(
        json!([{"role":"user", "content":"Write a long story about a cedar forest."}]),
        true,
        4096,
    );
    let client = connect(port, &credential, &long).await;
    drop(timeout(WAIT, read_until_content(client)).await.unwrap());
    assert_eq!(
        timeout(WAIT, running).await.unwrap().unwrap().unwrap().exit,
        RunExit::AdmissionClosed
    );
    assert!(TcpStream::connect(("127.0.0.1", port)).await.is_err());

    // Fresh ownership after a cancelled generation must work without replay.
    let owner = start(data, evidence).await;
    let port = owner.port();
    let credential = key(&owner);
    let (stop, stopped) = oneshot::channel();
    let running = spawn_local(owner.run(async { stopped.await.map_err(io::Error::other) }));
    streamed_response(
        &exchange(
            port,
            &credential,
            &request(
                json!([{"role":"user", "content":"Name a flower."}]),
                true,
                16,
            ),
        )
        .await,
    );
    stop.send(()).unwrap();
    assert_eq!(
        timeout(WAIT, running).await.unwrap().unwrap().unwrap().exit,
        RunExit::Shutdown
    );
    assert!(TcpStream::connect(("127.0.0.1", port)).await.is_err());
    if workloads.stalled_consumer {
        stalled_report["freshRestartAfterStopPassed"] = json!(true);
    }
    json!({"extendedWorkload":extended_report, "stalledConsumerWorkload":stalled_report})
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
    let body = request(extended_history(3900), true, EXTENDED_COMPLETION_CAP);
    assert_eq!(body["max_completion_tokens"], 4096);
    assert_eq!(body["messages"].as_array().unwrap().len(), 3);
    assert_eq!(body["messages"][1]["role"], "assistant");
}

#[test]
#[ignore = "Actual pinned model; explicitly dispatched main-only device CI, never ordinary suites."]
fn pinned_native_model_streams_recovers_and_restarts_in_required_sandbox() {
    ci_only();
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
    let workload_report =
        runtime.block_on(LocalSet::new().run_until(acceptance(data, evidence, &workloads)));
    let report = json!({
        "schemaVersion":1, "case":CASE, "revision":env::var("GITHUB_SHA").unwrap(),
        "runId":env::var("GITHUB_RUN_ID").unwrap(), "runAttempt":env::var("GITHUB_RUN_ATTEMPT").unwrap(),
        "scope":"bounded-actual-model-runner-lifecycle", "passed":true,
        "runtimeLock":serde_json::from_str::<Value>(include_str!("../../../config/prism-inference.lock.json")).unwrap(),
        "modelSha256":"5912bb739217cf25b4283a098c7b893d9baff36bdf2e72d3dbf4cb999d5633d5",
        "requiredNativeSandbox":true, "adapterEnabled":false, "fullContextQualified":false,
        "fullModelNumericalParityQualified":false, "performanceQualified":false,
        "dailyServiceIsolationQualified":false, "defaultRuntimeQualified":false,
        "extendedWorkload":workload_report["extendedWorkload"],
        "stalledConsumerWorkload":workload_report["stalledConsumerWorkload"]
    });
    fs::write(
        evidence.join("acceptance.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
}

fn profile_case(profile: &str, extended: bool, stalled: bool) -> Option<&'static str> {
    match profile {
        "pq2-no-adapter" => Some(CASE),
        "ptq1-adapter" if !extended && !stalled => Some(PTQ1_ADAPTER_CASE),
        _ => None,
    }
}

fn adapter_configuration(data: &Path, evidence: &Path) -> LaunchConfig {
    let reservation = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = reservation.local_addr().unwrap().port().to_string();
    drop(reservation);
    let mut args: Vec<OsString> = [
        "--launch-development",
        "--require-native-sandbox",
        "--native-engine",
        "--profile",
        "swift-pq2-assessment",
        "--use-repacked-ptq1",
        "--use-abliterate-lora",
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

async fn start_adapter(data: &Path, evidence: &Path) -> LaunchedService {
    timeout(
        WAIT,
        adapter_configuration(data, evidence).start(pending::<io::Result<()>>()),
    )
    .await
    .unwrap()
    .unwrap()
}

fn adapter_request(messages: Value, scale: f64, limit: u32) -> Value {
    assert!(scale.is_finite() && (0.0..=2.0).contains(&scale));
    assert!([4, 8].contains(&limit));
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

fn save_adapter_report(evidence: &Path, report: &Value) {
    fs::write(
        evidence.join("acceptance.json"),
        serde_json::to_vec_pretty(report).unwrap(),
    )
    .unwrap();
}

async fn adapter_stream(
    port: u16,
    credential: &str,
    scale: f64,
    label: &str,
    history: bool,
) -> Value {
    let messages = if history {
        json!([
            {"role":"user","content":"Remember the word cedar."},
            {"role":"assistant","content":"Cedar.","reasoning_content":"The supplied word is cedar."},
            {"role":"user","content":"Name that tree in one sentence."}
        ])
    } else {
        json!([{"role":"user","content":"Name a tree."}])
    };
    let cap = if history { 8 } else { 4 };
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

async fn ptq1_adapter_acceptance(data: &Path, evidence: &Path, report: &mut Value) {
    report["phase"] = json!("initial-owner-startup");
    save_adapter_report(evidence, report);
    let owner = start_adapter(data, evidence).await;
    report["verifiedReadinessObserved"] = json!(true);
    report["adapterEnabled"] = json!(true);
    save_adapter_report(evidence, report);
    let port = owner.port();
    let credential = key(&owner);
    let (stop, stopped) = oneshot::channel();
    let running = spawn_local(owner.run(async { stopped.await.map_err(io::Error::other) }));
    let mut streams = Vec::new();
    for scale in [0.0, -0.0, 1.3, 2.0] {
        report["phase"] = json!("supplied-history-scale-stream");
        save_adapter_report(evidence, report);
        streams.push(
            adapter_stream(
                port,
                &credential,
                scale,
                "supplied-history-scale-stream",
                true,
            )
            .await,
        );
        report["streams"] = json!(streams);
        save_adapter_report(evidence, report);
    }
    let mut rejected_scales = Vec::new();
    for invalid in [json!(-0.1), json!(2.000_000_01), Value::Null, json!("1.3")] {
        report["phase"] = json!("invalid-scale-rejection-and-reuse");
        save_adapter_report(evidence, report);
        let mut body = adapter_request(json!([{"role":"user","content":"Name a tree."}]), 1.3, 4);
        body["abliteration_scale"] = invalid.clone();
        let rejection = json_response(
            &exchange(port, &credential, &body).await,
            "HTTP/1.1 400 Bad Request",
        );
        assert!(rejection["error"].is_object());
        rejected_scales.push(invalid);
        streams.push(
            adapter_stream(
                port,
                &credential,
                1.3,
                "same-owner-after-invalid-scale",
                false,
            )
            .await,
        );
        report["invalidScalesRejected"] = json!(rejected_scales);
        report["streams"] = json!(streams);
        save_adapter_report(evidence, report);
    }
    report["phase"] = json!("context-rejection-and-reuse");
    save_adapter_report(evidence, report);
    let oversized = adapter_request(
        json!([{"role":"user","content":" cedar".repeat(9000)}]),
        1.3,
        4,
    );
    assert_eq!(
        json_response(
            &exchange(port, &credential, &oversized).await,
            "HTTP/1.1 413 Content Too Large"
        )["error"]["code"],
        "context_length_exceeded"
    );
    report["contextRejected"] = json!(true);
    streams.push(
        adapter_stream(
            port,
            &credential,
            0.0,
            "same-owner-after-context-rejection",
            false,
        )
        .await,
    );
    report["streams"] = json!(streams);
    report["initialCleanup"] = adapter_owned_stop(stop, running, port).await;
    report["phase"] = json!("fresh-owner-restart");
    save_adapter_report(evidence, report);
    let owner = start_adapter(data, evidence).await;
    let port = owner.port();
    let credential = key(&owner);
    let (stop, stopped) = oneshot::channel();
    let running = spawn_local(owner.run(async { stopped.await.map_err(io::Error::other) }));
    streams.push(adapter_stream(port, &credential, 2.0, "fresh-owner-restart", false).await);
    report["streams"] = json!(streams);
    report["restartCleanup"] = adapter_owned_stop(stop, running, port).await;
    report["phase"] = json!("completed");
    report["passed"] = json!(true);
    save_adapter_report(evidence, report);
}

#[test]
fn native_profile_selection_and_scale_receipts_preserve_bounded_scope() {
    assert_eq!(profile_case("pq2-no-adapter", true, true), Some(CASE));
    assert_eq!(
        profile_case("ptq1-adapter", false, false),
        Some(PTQ1_ADAPTER_CASE)
    );
    for (profile, extended, stalled) in [
        ("ptq1-adapter", true, false),
        ("ptq1-adapter", false, true),
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
    let body = adapter_request(json!([{"role":"user","content":"Name a tree."}]), -0.0, 8);
    assert_eq!(
        body["abliteration_scale"].as_f64().unwrap().to_bits(),
        (-0.0_f64).to_bits()
    );
    assert_eq!(body["max_completion_tokens"], 8);
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
#[ignore = "Actual PTQ1 and approved adapter; explicit bounded main-only device CI, never ordinary suites."]
fn pinned_native_ptq1_adapter_streams_scales_recovers_and_restarts_in_required_sandbox() {
    ci_only();
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
    runtime.block_on(LocalSet::new().run_until(ptq1_adapter_acceptance(
        Path::new(&data),
        evidence,
        &mut report,
    )));
}
