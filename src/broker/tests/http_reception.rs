#![forbid(unsafe_code)]
//! Regression coverage for bounded HTTP request reception and cancellation.
#![expect(
    clippy::unwrap_used,
    reason = "test setup and successful-operation assertions must panic on failure"
)]

use bonsai_broker::{
    MAX_BODY_BYTES, MAX_HEADER_BYTES, MAX_HEADERS, Rejection, RequestPolicy,
    reception::{HEADER_TIMEOUT, INTAKE_TIMEOUT, ReceiveError, receive_chat},
};
use std::{
    cell::Cell,
    pin::Pin,
    rc::Rc,
    task::{Context, Poll},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt, ReadBuf, duplex},
    time::{Instant, sleep, timeout},
};

const KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
// Literal JSON whitespace newlines must never be inspected as HTTP head bytes.
const CHAT: &[u8] = br#"{
    "messages":[{"role":"user","content":"Draft a scene.\nKeep the rain."}],
    "max_completion_tokens":128
}"#;

fn run(test: impl Future<Output = ()>) {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(test);
}

fn policy() -> RequestPolicy {
    RequestPolicy::new(8080, KEY).unwrap()
}
fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(60)
}

fn head(length: usize, extra: &str) -> String {
    format!(
        "POST /v1/chat/completions HTTP/1.1\r\nHost: 127.0.0.1:8080\r\nAuthorization: Bearer {KEY}\r\nContent-Type: application/json\r\nContent-Length: {length}\r\n{extra}\r\n"
    )
}

fn wire(body: &[u8], extra: &str) -> Vec<u8> {
    let mut bytes = head(body.len(), extra).into_bytes();
    bytes.extend_from_slice(body);
    bytes
}

// Records stream ownership and constrains read fragmentation. Can forbid a read
// after the supplied head to prove a rejection does not wait for the body.
struct ProbeRead {
    bytes: Vec<u8>,
    position: usize,
    chunk: usize,
    forbid_tail: bool,
    dropped: Rc<Cell<bool>>,
}

impl ProbeRead {
    fn new(bytes: Vec<u8>, chunk: usize) -> Self {
        Self {
            bytes,
            position: 0,
            chunk,
            forbid_tail: false,
            dropped: Rc::new(Cell::new(false)),
        }
    }
}

impl AsyncRead for ProbeRead {
    fn poll_read(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        assert!(
            !(self.forbid_tail && self.position == self.bytes.len()),
            "unexpected body read"
        );
        let count = buffer
            .remaining()
            .min(self.chunk)
            .min(self.bytes.len() - self.position);
        buffer.put_slice(&self.bytes[self.position..self.position + count]);
        self.position += count;
        Poll::Ready(Ok(()))
    }
}

impl Drop for ProbeRead {
    fn drop(&mut self) {
        self.dropped.set(true);
    }
}

#[expect(
    clippy::future_not_send,
    reason = "owned Rc fixture is polled on the deterministic current-thread runtime"
)]
async fn rejected(bytes: Vec<u8>, expected: ReceiveError) {
    let stream = ProbeRead::new(bytes, usize::MAX);
    let dropped = stream.dropped.clone();
    assert!(matches!(receive_chat(stream, &policy(), deadline()).await, Err(e) if e == expected));
    assert!(dropped.get());
}

#[test]
fn fragmented_request_preserves_text_and_still_requires_token_admission() {
    run(async {
        // One-byte reads split every delimiter. Larger chunks coalesce body/head.
        for chunk in [1, 2, 3, 7, MAX_HEADER_BYTES] {
            let stream = ProbeRead::new(wire(CHAT, ""), chunk);
            let dropped = stream.dropped.clone();
            let received = receive_chat(stream, &policy(), deadline()).await.unwrap();
            assert!(!dropped.get());
            let (stream, pending) = received.into_parts();
            let admitted = pending.admit(|_| Ok(100)).unwrap();
            let body: serde_json::Value = serde_json::from_slice(admitted.backend_body()).unwrap();
            assert_eq!(
                body["messages"][0]["content"],
                "Draft a scene.\nKeep the rain."
            );
            assert_eq!(body["max_completion_tokens"], 128);
            assert_eq!(body["cache_prompt"], false);
            drop(stream);
            assert!(dropped.get());
        }
    });
}

#[test]
fn wire_duplicates_and_framing_cannot_be_normalized_away() {
    run(async {
        for extra in [
            format!("content-length: {}\r\n", CHAT.len()),
            "Content-Length: 1\r\n".into(),
            "hOsT: 127.0.0.1:8080\r\n".into(),
            format!("AUTHORIZATION: Bearer {KEY}\r\n"),
            "Content-Type: application/json\r\n".into(),
            "Origin: http://127.0.0.1:8080\r\norigin: http://127.0.0.1:8080\r\n".into(),
        ] {
            rejected(wire(CHAT, &extra), ReceiveError::Policy(Rejection::Headers)).await;
        }
        for extra in [
            "Transfer-Encoding: chunked\r\n",
            "Expect: 100-continue\r\n",
            "Upgrade: websocket\r\n",
            "Trailer: X-End\r\n",
        ] {
            rejected(wire(CHAT, extra), ReceiveError::Policy(Rejection::Framing)).await;
        }
        rejected(
            wire(CHAT, "X-Forwarded-For: 127.0.0.1\r\n"),
            ReceiveError::Policy(Rejection::Headers),
        )
        .await;
    });
}

#[test]
fn raw_routes_are_checked_before_uri_normalization() {
    run(async {
        for target in [
            "/v1/chat/completions#suffix",
            "/v1/chat/completions?x=1",
            "http://127.0.0.1:8080/v1/chat/completions",
            "/v1/chat/%63ompletions",
            "//v1/chat/completions",
        ] {
            let bytes = String::from_utf8(wire(CHAT, ""))
                .unwrap()
                .replacen("/v1/chat/completions", target, 1)
                .into_bytes();
            rejected(bytes, ReceiveError::Policy(Rejection::Route)).await;
        }
    });
}

#[test]
fn raw_syntax_rejects_liberal_line_endings_and_folded_headers() {
    run(async {
        let good = head(CHAT.len(), "");
        for malformed in [
            format!("\r\n{good}"),
            good.replacen("HTTP/1.1\r\n", "HTTP/1.1\n", 1),
            good.replace("Host:", "Host :"),
            good.replace("Host:", " Host:"),
            good.replace("Content-Type:", "\tContent-Type:"),
            good.replace("POST ", "POST  "),
            good.replace("HTTP/1.1", "HTTP/1.0"),
            head(CHAT.len(), "X-Bad: a\rb\r\n"),
        ] {
            rejected(malformed.into_bytes(), ReceiveError::WireSyntax).await;
        }
        // No valid terminator: EOF is incomplete, never normalized into a head.
        rejected(
            good.replace("\r\n", "\n").into_bytes(),
            ReceiveError::Incomplete,
        )
        .await;
    });
}

#[test]
fn raw_head_limits_include_line_whitespace_and_every_header() {
    run(async {
        let base = head(CHAT.len(), "X-Pad: \r\n");
        let padding = " ".repeat(MAX_HEADER_BYTES - base.len());
        let exact = head(CHAT.len(), &format!("X-Pad: {padding}\r\n"));
        assert_eq!(exact.len(), MAX_HEADER_BYTES);
        let mut bytes = exact.clone().into_bytes();
        bytes.extend_from_slice(CHAT);
        assert!(
            receive_chat(ProbeRead::new(bytes, usize::MAX), &policy(), deadline())
                .await
                .is_ok()
        );
        rejected(
            exact.replacen("X-Pad: ", "X-Pad:  ", 1).into_bytes(),
            ReceiveError::HeadLimit,
        )
        .await;
        // Repeated innocuous names still count separately, not as one map key.
        let extra = "X-Value: x\r\n".repeat(MAX_HEADERS - 4);
        assert!(
            receive_chat(
                ProbeRead::new(wire(CHAT, &extra), usize::MAX),
                &policy(),
                deadline()
            )
            .await
            .is_ok()
        );
        rejected(
            wire(CHAT, &(extra + "X-Value: x\r\n")),
            ReceiveError::HeadLimit,
        )
        .await;
        rejected(vec![b'X'; MAX_HEADER_BYTES], ReceiveError::HeadLimit).await;
    });
}

#[test]
fn rejected_head_never_requests_the_body() {
    run(async {
        for (bytes, rejection) in [
            (head(MAX_BODY_BYTES + 1, ""), Rejection::BodySize),
            (
                head(CHAT.len(), "").replace(KEY, &"a".repeat(64)),
                Rejection::Unauthorized,
            ),
            (
                head(CHAT.len(), "").replace("127.0.0.1:8080", "localhost:8080"),
                Rejection::Host,
            ),
            (head(CHAT.len(), "Origin: null\r\n"), Rejection::Origin),
        ] {
            let mut stream = ProbeRead::new(bytes.into_bytes(), usize::MAX);
            stream.forbid_tail = true;
            let dropped = stream.dropped.clone();
            assert!(
                matches!(receive_chat(stream, &policy(), deadline()).await, Err(ReceiveError::Policy(e)) if e == rejection)
            );
            assert!(dropped.get());
        }
    });
}

#[test]
fn incomplete_and_surplus_bodies_close_the_stream() {
    run(async {
        rejected(Vec::new(), ReceiveError::Incomplete).await;
        rejected(b"POST /v1/".to_vec(), ReceiveError::Incomplete).await;
        let mut short = wire(CHAT, "");
        short.pop();
        rejected(short, ReceiveError::Incomplete).await;
        let mut surplus = wire(CHAT, "");
        surplus.extend_from_slice(b"GET /admin HTTP/1.1\r\n\r\n");
        rejected(surplus, ReceiveError::SurplusBytes).await;
    });
}

#[test]
fn body_buffer_boundary_preserves_framing_with_fragmented_reads() {
    run(async {
        // Both lengths have the same Content-Length digit count. Exercise an
        // exact fit in the head buffer and the separate-body allocation path.
        let capacity = MAX_HEADER_BYTES - head(MAX_HEADER_BYTES, "").len();
        for length in [capacity, capacity + 1] {
            let mut body = CHAT.to_vec();
            body.resize(length, b' ');
            let mut bytes = wire(&body, "");
            let request_length = bytes.len();
            assert_eq!(request_length, MAX_HEADER_BYTES + length - capacity);
            bytes.extend_from_slice(b"GET /admin HTTP/1.1\r\n\r\n");
            // Split after the head, with and without a coalesced body prefix.
            for chunk in [head(length, "").len(), head(length, "").len() + 11] {
                let stream = ProbeRead::new(bytes.clone(), chunk);
                let received = receive_chat(stream, &policy(), deadline()).await.unwrap();
                let (stream, pending) = received.into_parts();
                assert_eq!(stream.position, request_length);
                let admitted = pending.admit(|_| Ok(100)).unwrap();
                let forwarded: serde_json::Value =
                    serde_json::from_slice(admitted.backend_body()).unwrap();
                assert_eq!(
                    forwarded["messages"][0]["content"],
                    "Draft a scene.\nKeep the rain."
                );
            }
        }
    });
}

#[test]
fn body_limit_and_schema_are_enforced_after_fragmented_reception() {
    run(async {
        let prefix = br#"{"messages":[{"role":"user","content":""#;
        let suffix = br#""}]}"#;
        let mut body = prefix.to_vec();
        body.extend(std::iter::repeat_n(
            b'x',
            MAX_BODY_BYTES - prefix.len() - suffix.len(),
        ));
        body.extend_from_slice(suffix);
        assert_eq!(body.len(), MAX_BODY_BYTES);
        assert!(
            receive_chat(ProbeRead::new(wire(&body, ""), 4096), &policy(), deadline())
                .await
                .is_ok()
        );
        rejected(
            wire(br#"{"messages":[],"stream":true,"stream":false}"#, ""),
            ReceiveError::Policy(Rejection::ChatSchema),
        )
        .await;
    });
}

#[test]
fn complete_body_does_not_wait_for_peer_eof() {
    run(async {
        let (mut client, server) = duplex(4096);
        client.write_all(&wire(CHAT, "")).await.unwrap();
        let received = timeout(
            Duration::from_millis(10),
            receive_chat(server, &policy(), deadline()),
        )
        .await
        .unwrap()
        .unwrap();
        drop(received);
        let mut byte = [0];
        assert_eq!(client.read(&mut byte).await.unwrap(), 0);
    });
}

#[test]
fn stalled_reads_obey_header_intake_and_supervisor_deadlines() {
    run(async {
        for (prefix, supervisor, expected) in [
            (Vec::new(), Duration::from_secs(60), HEADER_TIMEOUT),
            (
                head(CHAT.len(), "").into_bytes(),
                Duration::from_secs(60),
                INTAKE_TIMEOUT,
            ),
            (
                head(CHAT.len(), "").into_bytes(),
                Duration::from_secs(2),
                Duration::from_secs(2),
            ),
        ] {
            let (mut client, server) = duplex(4096);
            client.write_all(&prefix).await.unwrap();
            let start = Instant::now();
            assert!(matches!(
                receive_chat(server, &policy(), start + supervisor).await,
                Err(ReceiveError::Deadline)
            ));
            assert!(Instant::now() - start >= expected);
            assert!(Instant::now() - start < expected + Duration::from_secs(1));
            let mut byte = [0];
            assert_eq!(client.read(&mut byte).await.unwrap(), 0);
        }
    });
}

#[test]
fn slow_drip_does_not_reset_the_header_deadline() {
    run(async {
        let (mut client, server) = duplex(4096);
        let writer = tokio::spawn(async move {
            for byte in wire(CHAT, "") {
                if client.write_all(&[byte]).await.is_err() {
                    break;
                }
                sleep(Duration::from_secs(1)).await;
            }
        });
        let start = Instant::now();
        assert!(matches!(
            receive_chat(server, &policy(), deadline()).await,
            Err(ReceiveError::Deadline)
        ));
        assert!(Instant::now() - start < HEADER_TIMEOUT + Duration::from_secs(1));
        writer.await.unwrap();
    });
}

#[test]
fn expired_supervisor_deadline_rejects_even_immediately_ready_input() {
    run(async {
        let stream = ProbeRead::new(wire(CHAT, ""), usize::MAX);
        let dropped = stream.dropped.clone();
        assert!(matches!(
            receive_chat(stream, &policy(), Instant::now()).await,
            Err(ReceiveError::Deadline)
        ));
        assert!(dropped.get());
    });
}

#[test]
fn cancelling_reception_drops_the_owned_connection() {
    run(async {
        let (mut client, server) = duplex(4096);
        // Prove cancellation releases the connection before its own 5s timeout.
        let start = Instant::now();
        assert!(
            timeout(
                Duration::from_millis(1),
                receive_chat(server, &policy(), deadline())
            )
            .await
            .is_err()
        );
        assert!(Instant::now() - start < HEADER_TIMEOUT);
        let mut byte = [0];
        assert_eq!(client.read(&mut byte).await.unwrap(), 0);
    });
}
