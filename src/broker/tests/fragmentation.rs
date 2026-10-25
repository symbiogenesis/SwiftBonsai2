#![forbid(unsafe_code)]
//! Whole-pipeline framing regressions with bounded, deterministic in-memory I/O.
#![expect(
    clippy::unwrap_used,
    reason = "test fixtures and successful-operation assertions must fail loudly"
)]

use bonsai_broker::{
    RequestPolicy,
    backend::{BackendError, BackendPolicy},
    response::ResponseError,
    supervisor::{BackendConnector, ServeError, serve_chat},
};
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    io,
    pin::Pin,
    rc::Rc,
    task::{Context, Poll},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    time::Instant,
};

const CLIENT_KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const BACKEND_KEY: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const COUNT: &[u8] = br#"{"input_tokens":100,"object":"response.input_tokens"}"#;
const TEXT: &str = "Keep rain, snow ☃, and the voice.\nNext line.";
const JSON: &[u8] = "{\"choices\":[{\"text\":\"rain ☃\"}]}".as_bytes();
const SSE: &[u8] = "data: {\"text\":\"rain ☃\"}\n\ndata: [DONE]\n\n".as_bytes();

fn run(test: impl Future<Output = ()>) {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(test);
}

fn request(stream: bool) -> Vec<u8> {
    let body = serde_json::to_vec(&serde_json::json!({
        "messages": [{"role": "user", "content": TEXT}],
        "max_completion_tokens": 128,
        "stream": stream,
    }))
    .unwrap();
    let mut bytes = format!(
        "POST /v1/chat/completions HTTP/1.1\r\nHost: 127.0.0.1:8080\r\nAuthorization: Bearer {CLIENT_KEY}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
        body.len()
    )
    .into_bytes();
    bytes.extend(body);
    bytes
}

fn response(body: &[u8], streaming: bool, chunked: bool) -> Vec<u8> {
    let content_type = if streaming {
        "text/event-stream"
    } else {
        "application/json"
    };
    let framing = if chunked {
        "Transfer-Encoding: chunked".into()
    } else {
        format!("Content-Length: {}", body.len())
    };
    let mut bytes = format!("HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\n{framing}\r\n\r\n")
        .into_bytes();
    if chunked {
        // Deliberately split JSON/SSE syntax and UTF-8 code points across chunks.
        for chunk in body.chunks(2) {
            bytes.extend_from_slice(format!("{:X}\r\n", chunk.len()).as_bytes());
            bytes.extend_from_slice(chunk);
            bytes.extend_from_slice(b"\r\n");
        }
        bytes.extend_from_slice(b"0\r\n\r\n");
    } else {
        bytes.extend_from_slice(body);
    }
    bytes
}

#[derive(Default)]
struct State {
    written: Vec<u8>,
    read_polls: usize,
    dropped: bool,
}

// Every stream is < 1 KiB. A split is a forced read boundary; quantum also
// bounds writes. Optional self-wakes model readiness without timers or tasks.
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent read/write readiness and peer-lifetime controls model duplex I/O"
)]
struct Stream {
    bytes: Vec<u8>,
    offset: usize,
    split: usize,
    quantum: usize,
    keep_open: bool,
    cooperative: bool,
    read_ready: bool,
    write_ready: bool,
    close_when: Option<Rc<Cell<bool>>>,
    notify_read: Option<Rc<Cell<bool>>>,
    state: Rc<RefCell<State>>,
}

impl Stream {
    fn new(bytes: Vec<u8>) -> Self {
        assert!(bytes.len() < 1024);
        Self {
            bytes,
            offset: 0,
            split: usize::MAX,
            quantum: usize::MAX,
            keep_open: false,
            cooperative: false,
            read_ready: false,
            write_ready: false,
            close_when: None,
            notify_read: None,
            state: Rc::new(RefCell::new(State::default())),
        }
    }
}

impl AsyncRead for Stream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        output: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        this.state.borrow_mut().read_polls += 1;
        if this.offset == this.bytes.len() && this.keep_open {
            return if this.close_when.as_ref().is_some_and(|flag| flag.get()) {
                Poll::Ready(Ok(()))
            } else {
                Poll::Pending
            };
        }
        if let Some(flag) = this.notify_read.take() {
            flag.set(true);
            cx.waker().wake_by_ref();
            return Poll::Pending;
        }
        if this.cooperative && !this.read_ready {
            this.read_ready = true;
            cx.waker().wake_by_ref();
            return Poll::Pending;
        }
        this.read_ready = false;
        let end = if this.offset < this.split {
            this.split.min(this.bytes.len())
        } else {
            this.bytes.len()
        };
        let count = (end - this.offset)
            .min(this.quantum)
            .min(output.remaining());
        output.put_slice(&this.bytes[this.offset..this.offset + count]);
        this.offset += count;
        Poll::Ready(Ok(()))
    }
}

impl AsyncWrite for Stream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        if this.cooperative && !this.write_ready {
            this.write_ready = true;
            cx.waker().wake_by_ref();
            return Poll::Pending;
        }
        this.write_ready = false;
        let count = bytes.len().min(this.quantum);
        this.state
            .borrow_mut()
            .written
            .extend_from_slice(&bytes[..count]);
        Poll::Ready(Ok(count))
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        self.state.borrow_mut().dropped = true;
    }
}

struct Connector {
    policy: BackendPolicy,
    streams: RefCell<VecDeque<Stream>>,
    calls: Cell<usize>,
}

impl BackendConnector for Connector {
    type Stream = Stream;

    fn policy(&self) -> &BackendPolicy {
        &self.policy
    }

    #[expect(
        clippy::future_not_send,
        reason = "all fixture state belongs to the deterministic current-thread runtime"
    )]
    fn connect(&self, _: Instant) -> impl Future<Output = Result<Stream, BackendError>> {
        self.calls.set(self.calls.get() + 1);
        std::future::ready(
            self.streams
                .borrow_mut()
                .pop_front()
                .ok_or(BackendError::Io),
        )
    }
}

struct Exchange {
    client: Stream,
    backend: Connector,
    states: [Rc<RefCell<State>>; 3],
}

impl Exchange {
    fn new(streaming: bool, chunked: bool) -> Self {
        Self::with_replies(
            streaming,
            response(COUNT, false, chunked),
            response(if streaming { SSE } else { JSON }, streaming, chunked),
        )
    }

    fn with_replies(streaming: bool, counter: Vec<u8>, generation: Vec<u8>) -> Self {
        let mut client = Stream::new(request(streaming));
        client.keep_open = true;
        let counter = Stream::new(counter);
        let generation = Stream::new(generation);
        let states = [
            client.state.clone(),
            counter.state.clone(),
            generation.state.clone(),
        ];
        Self {
            client,
            backend: Connector {
                policy: BackendPolicy::new(8081, BACKEND_KEY).unwrap(),
                streams: RefCell::new([counter, generation].into()),
                calls: Cell::new(0),
            },
            states,
        }
    }

    fn split(&mut self, endpoint: usize, boundary: usize) {
        if endpoint == 0 {
            self.client.split = boundary;
        } else {
            self.backend.streams.borrow_mut()[endpoint - 1].split = boundary;
        }
    }

    #[expect(
        clippy::future_not_send,
        reason = "owned Rc observations are confined to the deterministic local runtime"
    )]
    async fn finish(self) -> Outcome {
        let start = Instant::now();
        let result = serve_chat(
            self.client,
            &RequestPolicy::new(8080, CLIENT_KEY).unwrap(),
            &self.backend,
            start + Duration::from_secs(1),
        )
        .await;
        // No I/O schedule needs elapsed time, sockets, or background work.
        assert_eq!(Instant::now(), start);
        let calls = self.backend.calls.get();
        drop(self.backend);
        assert!(self.states.iter().all(|state| state.borrow().dropped));
        Outcome {
            result,
            calls,
            states: self.states,
        }
    }
}

struct Outcome {
    result: Result<(), ServeError>,
    calls: usize,
    states: [Rc<RefCell<State>>; 3],
}

fn parts(wire: &[u8]) -> (&[u8], &[u8]) {
    let split = wire
        .windows(4)
        .position(|bytes| bytes == b"\r\n\r\n")
        .unwrap()
        + 4;
    wire.split_at(split)
}

fn assert_success(outcome: &Outcome, streaming: bool) {
    assert_eq!(outcome.result, Ok(()));
    assert_eq!(outcome.calls, 2);
    let counter = outcome.states[1].borrow();
    let generation = outcome.states[2].borrow();
    let (count_head, count_body) = parts(&counter.written);
    let (generation_head, generation_body) = parts(&generation.written);
    assert!(count_head.starts_with(b"POST /v1/chat/completions/input_tokens HTTP/1.1\r\n"));
    assert!(generation_head.starts_with(b"POST /v1/chat/completions HTTP/1.1\r\n"));
    assert_eq!(count_body, generation_body);
    for head in [count_head, generation_head] {
        let head = std::str::from_utf8(head).unwrap();
        assert!(head.contains(&format!("Authorization: Bearer {BACKEND_KEY}\r\n")));
        assert!(!head.contains(CLIENT_KEY));
    }
    let forwarded: serde_json::Value = serde_json::from_slice(generation_body).unwrap();
    assert_eq!(forwarded["messages"][0]["content"], TEXT);
    assert_eq!(forwarded["cache_prompt"], false);
    assert_eq!(forwarded["stream"], streaming);
    let client = outcome.states[0].borrow();
    let (head, mut body) = parts(&client.written);
    assert!(head.starts_with(b"HTTP/1.1 200 OK\r\n"));
    if streaming {
        let mut decoded = Vec::new();
        loop {
            let end = body.windows(2).position(|bytes| bytes == b"\r\n").unwrap();
            let count =
                usize::from_str_radix(std::str::from_utf8(&body[..end]).unwrap(), 16).unwrap();
            body = &body[end + 2..];
            if count == 0 {
                assert_eq!(body, b"\r\n");
                break;
            }
            decoded.extend_from_slice(&body[..count]);
            assert_eq!(&body[count..count + 2], b"\r\n");
            body = &body[count + 2..];
        }
        assert_eq!(decoded, SSE);
    } else {
        assert_eq!(body, JSON);
    }
}

#[test]
fn every_single_wire_split_preserves_the_complete_count_and_generation_pipeline() {
    run(async {
        // Vary one endpoint at a time: linear coverage of every boundary instead
        // of a redundant Cartesian product. Both framing modes get controls.
        for streaming in [false, true] {
            for chunked in [false, true] {
                let lengths = [
                    request(streaming).len(),
                    response(COUNT, false, chunked).len(),
                    response(if streaming { SSE } else { JSON }, streaming, chunked).len(),
                ];
                for (endpoint, length) in lengths.into_iter().enumerate() {
                    for boundary in 1..length {
                        let mut exchange = Exchange::new(streaming, chunked);
                        exchange.split(endpoint, boundary);
                        assert_success(&exchange.finish().await, streaming);
                    }
                }
            }
        }
    });
}

#[test]
fn cooperative_one_byte_reads_and_writes_preserve_utf8_and_terminal_framing() {
    run(async {
        for streaming in [false, true] {
            let mut exchange = Exchange::new(streaming, true);
            exchange.client.quantum = 1;
            exchange.client.cooperative = true;
            for stream in exchange.backend.streams.borrow_mut().iter_mut() {
                stream.quantum = 1;
                stream.cooperative = true;
            }
            assert_success(&exchange.finish().await, streaming);
        }
    });
}

#[test]
fn every_truncated_count_prefix_prevents_generation_and_client_output() {
    run(async {
        for chunked in [false, true] {
            let reply = response(COUNT, false, chunked);
            for end in 0..reply.len() {
                let outcome = Exchange::with_replies(
                    false,
                    reply[..end].to_vec(),
                    response(JSON, false, chunked),
                )
                .finish()
                .await;
                let expected = if end < parts(&reply).0.len() {
                    BackendError::WireSyntax
                } else {
                    BackendError::Body
                };
                assert_eq!(outcome.result, Err(ServeError::Backend(expected)));
                assert_eq!(outcome.calls, 1);
                assert_eq!(outcome.states[0].borrow().written, [0_u8; 0]);
                assert_eq!(outcome.states[2].borrow().written, [0_u8; 0]);
                assert_eq!(outcome.states[2].borrow().read_polls, 0);
            }
            assert_success(&Exchange::new(false, chunked).finish().await, false);
        }
    });
}

#[test]
fn every_truncated_json_reply_is_rejected_before_any_downstream_write() {
    run(async {
        for chunked in [false, true] {
            let reply = response(JSON, false, chunked);
            for end in 0..reply.len() {
                let outcome = Exchange::with_replies(
                    false,
                    response(COUNT, false, chunked),
                    reply[..end].to_vec(),
                )
                .finish()
                .await;
                let expected = if end < parts(&reply).0.len() {
                    ServeError::Backend(BackendError::WireSyntax)
                } else {
                    ServeError::Response(ResponseError::BackendIo)
                };
                assert_eq!(outcome.result, Err(expected));
                assert_eq!(outcome.calls, 2);
                assert_eq!(outcome.states[0].borrow().written, [0_u8; 0]);
            }
            assert_success(&Exchange::new(false, chunked).finish().await, false);
        }
    });
}

#[test]
fn every_truncated_sse_reply_omits_successful_downstream_terminal_framing() {
    run(async {
        for chunked in [false, true] {
            let reply = response(SSE, true, chunked);
            for end in 0..reply.len() {
                let outcome = Exchange::with_replies(
                    true,
                    response(COUNT, false, chunked),
                    reply[..end].to_vec(),
                )
                .finish()
                .await;
                let head_complete = end >= parts(&reply).0.len();
                let expected = if head_complete {
                    ServeError::Response(ResponseError::BackendIo)
                } else {
                    ServeError::Backend(BackendError::WireSyntax)
                };
                assert_eq!(outcome.result, Err(expected));
                assert_eq!(outcome.calls, 2);
                let client = outcome.states[0].borrow();
                if head_complete {
                    assert!(!parts(&client.written).1.ends_with(b"0\r\n\r\n"));
                } else {
                    assert_eq!(client.written, [0_u8; 0]);
                }
            }
            assert_success(&Exchange::new(true, chunked).finish().await, true);
        }
        // Retain the raw three-byte, one-byte-read regression as well as the
        // complete SSE prefix sweep: an input without [DONE] cannot invent it.
        let exchange = Exchange::with_replies(
            true,
            response(COUNT, false, false),
            b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n".to_vec(),
        );
        exchange.backend.streams.borrow_mut()[1].quantum = 1;
        let outcome = exchange.finish().await;
        assert_eq!(
            outcome.result,
            Err(ServeError::Response(ResponseError::BackendIo))
        );
        assert_eq!(outcome.calls, 2);
        let client = outcome.states[0].borrow();
        assert!(!parts(&client.written).1.ends_with(b"0\r\n\r\n"));
        assert!(!client.written.windows(6).any(|part| part == b"[DONE]"));
    });
}

#[test]
fn abandonment_wins_when_generation_bytes_and_disconnect_become_ready_together() {
    run(async {
        let mut exchange = Exchange::new(false, true);
        let abandoned = Rc::new(Cell::new(false));
        exchange.client.close_when = Some(abandoned.clone());
        exchange.backend.streams.borrow_mut()[1].notify_read = Some(abandoned);
        let outcome = exchange.finish().await;
        assert_eq!(outcome.result, Err(ServeError::ClientAbandoned));
        assert_eq!(outcome.calls, 2);
        assert_eq!(outcome.states[0].borrow().written, [0_u8; 0]);
        assert_ne!(outcome.states[2].borrow().written, [0_u8; 0]);
        assert_eq!(outcome.states[2].borrow().read_polls, 1);
    });
}
