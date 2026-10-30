//! Opt-in actual-model application acceptance. Ordinary suites never load weights.
#![cfg(windows)]
#![forbid(unsafe_code)]
#![expect(
    clippy::unwrap_used,
    reason = "Acceptance assertions must fail loudly."
)]

use bonsai_broker::{
    launcher::{LaunchConfig, LaunchedService},
    runtime::RunExit,
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
const WAIT: Duration = Duration::from_secs(600);
const EXTENDED_COMPLETION_CAP: u32 = 4096;

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

async fn abandon_after_content(client: TcpStream) {
    let mut reader = BufReader::new(client);
    let mut header_bytes = 0;
    loop {
        let mut line = String::new();
        assert_ne!(reader.read_line(&mut line).await.unwrap(), 0);
        header_bytes += line.len();
        assert!(header_bytes < 16 * 1024);
        if line == "\r\n" {
            break;
        }
    }
    let mut payload = Vec::new();
    loop {
        let mut line = String::new();
        assert_ne!(reader.read_line(&mut line).await.unwrap(), 0);
        assert!(line.len() <= 32);
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
                && event["choices"].as_array().is_some_and(|choices| {
                    choices.iter().any(|choice| {
                        choice["delta"]["content"]
                            .as_str()
                            .is_some_and(|text| !text.is_empty())
                    })
                })
            {
                return;
            }
        }
    }
}

async fn acceptance(data: &Path, evidence: &Path, extended: bool) -> Value {
    let owner = start(data, evidence).await;
    let port = owner.port();
    let credential = key(&owner);
    let (stop, stopped) = oneshot::channel();
    let running = spawn_local(owner.run(async { stopped.await.map_err(io::Error::other) }));

    let history = json!([
        {"role":"user", "content":"Remember the word cedar."},
        {"role":"assistant", "content":"I will remember cedar.", "reasoning_content":"The supplied word is cedar."},
        {"role":"user", "content":"Write one sentence about cedar."}
    ]);
    streamed_response(&exchange(port, &credential, &request(history, true, 32)).await);

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
    let extended_report = if extended {
        extended_acceptance(port, &credential).await
    } else {
        json!({"enabled":false})
    };
    stop.send(()).unwrap();
    assert_eq!(
        timeout(WAIT, running).await.unwrap().unwrap().unwrap().exit,
        RunExit::Shutdown
    );
    assert!(TcpStream::connect(("127.0.0.1", port)).await.is_err());

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
    timeout(WAIT, abandon_after_content(client)).await.unwrap();
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
    extended_report
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
    let setting = env::var("BONSAI_NATIVE_EXTENDED");
    assert!(matches!(
        setting.as_deref(),
        Ok("true" | "false") | Err(env::VarError::NotPresent)
    ));
    let extended = setting.as_deref() == Ok("true");
    let extended_report =
        runtime.block_on(LocalSet::new().run_until(acceptance(data, evidence, extended)));
    let report = json!({
        "schemaVersion":1, "case":CASE, "revision":env::var("GITHUB_SHA").unwrap(),
        "runId":env::var("GITHUB_RUN_ID").unwrap(), "runAttempt":env::var("GITHUB_RUN_ATTEMPT").unwrap(),
        "scope":"bounded-actual-model-runner-lifecycle", "passed":true,
        "runtimeLock":serde_json::from_str::<Value>(include_str!("../../../config/prism-inference.lock.json")).unwrap(),
        "modelSha256":"5912bb739217cf25b4283a098c7b893d9baff36bdf2e72d3dbf4cb999d5633d5",
        "requiredNativeSandbox":true, "adapterEnabled":false, "fullContextQualified":false,
        "fullModelNumericalParityQualified":false, "performanceQualified":false,
        "dailyServiceIsolationQualified":false, "defaultRuntimeQualified":false,
        "extendedWorkload":extended_report
    });
    fs::write(
        evidence.join("acceptance.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
}
