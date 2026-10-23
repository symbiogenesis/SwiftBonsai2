//! Regression coverage for private backend HTTP exchanges and body decoding.
#![expect(
    clippy::unwrap_used,
    reason = "test setup and successful-operation assertions must panic on failure"
)]

use bonsai_broker::{
    AdmittedChat, MAX_HEADER_BYTES, MAX_HEADERS, PendingChat, Rejection, RequestPolicy,
    backend::{BackendError, BackendPolicy, count_and_admit, request_chat},
    response::{MAX_RESPONSE_BYTES, ResponseError, send_chat_response},
};
use http::{Request, Version};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    future::Future,
    io,
    pin::Pin,
    rc::Rc,
    task::{Context, Poll},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, ReadBuf},
    time::{Instant, Sleep, sleep, timeout},
};

// Synthetic, intentionally different public and backend credentials.
const CLIENT_KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const BACKEND_KEY: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const COUNT: &[u8] = br#"{"input_tokens":100,"object":"response.input_tokens"}"#;

fn run(test: impl Future<Output = ()>) {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(test);
}

fn policy() -> BackendPolicy {
    BackendPolicy::new(8081, BACKEND_KEY).unwrap()
}

fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(60)
}

fn pending(stream: bool) -> PendingChat {
    let body = format!(
        r#"{{"messages":[{{"role":"user","content":"A scene.\nKeep rain & snow ☃."}}],"max_completion_tokens":128,"stream":{stream}}}"#
    );
    let head = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .version(Version::HTTP_11)
        .header("host", "127.0.0.1:8080")
        .header("origin", "http://127.0.0.1:8080")
        .header("authorization", format!("Bearer {CLIENT_KEY}"))
        .header("content-type", "application/json")
        .header("content-length", body.len())
        .body(())
        .unwrap()
        .into_parts()
        .0;
    RequestPolicy::new(8080, CLIENT_KEY)
        .unwrap()
        .check_head(&head)
        .unwrap()
        .prepare(body.as_bytes())
        .unwrap()
}

fn admitted(stream: bool) -> AdmittedChat {
    pending(stream).admit(|_| Ok(100)).unwrap()
}

fn length_response(body: &[u8], stream: bool) -> Vec<u8> {
    let mut bytes = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\n\r\n",
        if stream {
            "text/event-stream"
        } else {
            "application/json"
        },
        body.len()
    )
    .into_bytes();
    bytes.extend_from_slice(body);
    bytes
}

fn chunked_response(body: &[u8], stream: bool) -> Vec<u8> {
    let mut bytes = chunked_head(stream);
    bytes.extend_from_slice(body);
    bytes
}

fn chunked_head(stream: bool) -> Vec<u8> {
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nTransfer-Encoding: chunked\r\n\r\n",
        if stream {
            "text/event-stream"
        } else {
            "application/json"
        }
    )
    .into_bytes()
}

#[derive(Default)]
struct State {
    read_polls: usize,
    read_bytes: usize,
    largest_read: usize,
    write_polls: usize,
    written: Vec<u8>,
    flushes: usize,
    dropped: bool,
}

// Owned, non-Send full-duplex fixture. All data and stalls are deterministic;
// no socket, listener, background task or native process is needed.
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent fault-injection switches exercise each I/O failure mode"
)]
struct Stream {
    input: Vec<u8>,
    position: usize,
    fragment: usize,
    stall_after_input: bool,
    read_delay: Option<Duration>,
    read_timer: Option<Pin<Box<Sleep>>>,
    write_fragment: usize,
    write_delay: Option<Duration>,
    write_timer: Option<Pin<Box<Sleep>>>,
    stall_write: bool,
    fail_write: bool,
    zero_write: bool,
    stall_flush: bool,
    fail_flush: bool,
    state: Rc<RefCell<State>>,
}

impl Stream {
    fn new(input: impl Into<Vec<u8>>, fragment: usize) -> Self {
        Self {
            input: input.into(),
            position: 0,
            fragment,
            stall_after_input: false,
            read_delay: None,
            read_timer: None,
            write_fragment: usize::MAX,
            write_delay: None,
            write_timer: None,
            stall_write: false,
            fail_write: false,
            zero_write: false,
            stall_flush: false,
            fail_flush: false,
            state: Rc::new(RefCell::new(State::default())),
        }
    }
}

impl AsyncRead for Stream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        {
            let mut state = self.state.borrow_mut();
            state.read_polls += 1;
            state.largest_read = state.largest_read.max(buffer.remaining());
        }
        if let Some(delay) = self.read_delay {
            let timer = self
                .read_timer
                .get_or_insert_with(|| Box::pin(sleep(delay)));
            if timer.as_mut().poll(cx).is_pending() {
                return Poll::Pending;
            }
            self.read_timer = None;
        }
        if self.position == self.input.len() && self.stall_after_input {
            return Poll::Pending;
        }
        let count = buffer
            .remaining()
            .min(self.fragment)
            .min(self.input.len() - self.position);
        buffer.put_slice(&self.input[self.position..self.position + count]);
        self.position += count;
        self.state.borrow_mut().read_bytes += count;
        Poll::Ready(Ok(()))
    }
}

impl AsyncWrite for Stream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.state.borrow_mut().write_polls += 1;
        if self.fail_write {
            return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()));
        }
        if self.zero_write {
            return Poll::Ready(Ok(0));
        }
        if self.stall_write {
            return Poll::Pending;
        }
        if let Some(delay) = self.write_delay {
            let timer = self
                .write_timer
                .get_or_insert_with(|| Box::pin(sleep(delay)));
            if timer.as_mut().poll(cx).is_pending() {
                return Poll::Pending;
            }
            self.write_timer = None;
        }
        let count = self.write_fragment.min(bytes.len());
        self.state
            .borrow_mut()
            .written
            .extend_from_slice(&bytes[..count]);
        Poll::Ready(Ok(count))
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.state.borrow_mut().flushes += 1;
        if self.fail_flush {
            Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()))
        } else if self.stall_flush {
            Poll::Pending
        } else {
            Poll::Ready(Ok(()))
        }
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

fn request_parts<'a>(bytes: &'a [u8], path: &str) -> (BTreeMap<String, String>, &'a [u8]) {
    let split = bytes
        .windows(4)
        .position(|part| part == b"\r\n\r\n")
        .unwrap()
        + 4;
    let mut lines = std::str::from_utf8(&bytes[..split]).unwrap().split("\r\n");
    assert_eq!(lines.next().unwrap(), format!("POST {path} HTTP/1.1"));
    let mut headers = BTreeMap::new();
    for line in lines.filter(|line| !line.is_empty()) {
        let (name, value) = line.split_once(':').unwrap();
        assert!(
            headers
                .insert(name.to_ascii_lowercase(), value.trim().into())
                .is_none()
        );
    }
    assert_eq!(headers["host"], "127.0.0.1:8081");
    assert_eq!(headers["authorization"], format!("Bearer {BACKEND_KEY}"));
    assert_eq!(headers["content-type"], "application/json");
    assert_eq!(headers["connection"], "close");
    assert_eq!(headers["content-length"], (bytes.len() - split).to_string());
    for name in [
        "origin",
        "forwarded",
        "x-forwarded-for",
        "cookie",
        "transfer-encoding",
    ] {
        assert!(!headers.contains_key(name));
    }
    assert!(
        !bytes
            .windows(CLIENT_KEY.len())
            .any(|part| part == CLIENT_KEY.as_bytes())
    );
    (headers, &bytes[split..])
}

#[test]
fn configuration_rejects_invalid_private_backend_authority_or_key() {
    for port in [0, 80, 1023] {
        assert!(matches!(
            BackendPolicy::new(port, BACKEND_KEY),
            Err(BackendError::Configuration)
        ));
    }
    for key in ["", "short", &"G".repeat(64), &"a".repeat(65)] {
        assert!(matches!(
            BackendPolicy::new(8081, key),
            Err(BackendError::Configuration)
        ));
    }
}

#[test]
fn exact_count_and_generation_use_identical_canonical_bytes_and_private_headers() {
    run(async {
        for stream in [false, true] {
            let mut counter = Stream::new(length_response(COUNT, false), 1);
            counter.write_fragment = 2;
            let counter_state = counter.state.clone();
            let chat = count_and_admit(counter, pending(stream), &policy(), deadline())
                .await
                .unwrap();
            assert!(counter_state.borrow().dropped);
            assert_eq!(chat.backend_body(), admitted(stream).backend_body());

            let backend = Stream::new(length_response(b"{}", stream), 3);
            let backend_state = backend.state.clone();
            let response = request_chat(backend, &chat, &policy(), deadline())
                .await
                .unwrap();
            assert!(!backend_state.borrow().dropped);
            drop(response);
            assert!(backend_state.borrow().dropped);

            let count_state = counter_state.borrow();
            let generation_state = backend_state.borrow();
            let (_, count_body) =
                request_parts(&count_state.written, "/v1/chat/completions/input_tokens");
            let (_, generation_body) =
                request_parts(&generation_state.written, "/v1/chat/completions");
            assert_eq!(count_body, chat.backend_body());
            assert_eq!(count_body, generation_body);
        }
    });
}

#[test]
fn fragmented_framed_bodies_finish_without_waiting_for_peer_eof() {
    run(async {
        for fragment in [1, 2, 7, MAX_HEADER_BYTES] {
            for stream in [false, true] {
                let expected = b"{\"text\":\"rain\\n\"}\n\0\xff";
                let mut encoded = Vec::new();
                for part in [
                    &expected[..1],
                    &expected[1..5],
                    &expected[5..expected.len() - 2],
                    &expected[expected.len() - 2..],
                ] {
                    encoded.extend_from_slice(format!("{:X}\r\n", part.len()).as_bytes());
                    encoded.extend_from_slice(part);
                    encoded.extend_from_slice(b"\r\n");
                }
                encoded.extend_from_slice(b"0\r\n\r\n");
                for bytes in [
                    length_response(expected, stream),
                    chunked_response(&encoded, stream),
                ] {
                    let mut backend = Stream::new(bytes, fragment);
                    backend.stall_after_input = true;
                    let state = backend.state.clone();
                    let mut body = request_chat(backend, &admitted(stream), &policy(), deadline())
                        .await
                        .unwrap()
                        .into_body();
                    let start = Instant::now();
                    let mut actual = Vec::new();
                    timeout(Duration::from_millis(1), body.read_to_end(&mut actual))
                        .await
                        .unwrap()
                        .unwrap();
                    assert_eq!(actual, expected);
                    assert_eq!(Instant::now(), start);
                    assert!(!state.borrow().dropped);
                    let mut byte = [0];
                    assert_eq!(body.read(&mut byte).await.unwrap(), 0);
                    drop(body);
                    assert!(state.borrow().dropped);
                }
            }
        }
    });
}

#[test]
fn zero_length_and_zero_chunk_bodies_have_immediate_clean_eof() {
    run(async {
        for bytes in [
            length_response(b"", false),
            chunked_response(b"0\r\n\r\n", false),
        ] {
            let mut backend = Stream::new(bytes, 1);
            backend.stall_after_input = true;
            let mut body = request_chat(backend, &admitted(false), &policy(), deadline())
                .await
                .unwrap()
                .into_body();
            let mut actual = Vec::new();
            assert_eq!(body.read_to_end(&mut actual).await.unwrap(), 0);
            assert!(actual.is_empty());
        }
    });
}

#[test]
fn content_length_does_not_consume_a_later_response_or_wait_for_close() {
    run(async {
        let first = length_response(b"{}", false);
        let mut bytes = first.clone();
        bytes.extend_from_slice(b"HTTP/1.1 200 OK\r\nprivate: later\r\n\r\n");
        let backend = Stream::new(bytes, 1);
        let state = backend.state.clone();
        let mut body = request_chat(backend, &admitted(false), &policy(), deadline())
            .await
            .unwrap()
            .into_body();
        let mut actual = Vec::new();
        body.read_to_end(&mut actual).await.unwrap();
        assert_eq!(actual, b"{}");
        assert_eq!(state.borrow().read_bytes, first.len());
        drop(body);
        assert!(state.borrow().dropped);
    });
}

#[test]
fn ambiguous_or_unsupported_response_metadata_and_framing_fail_closed() {
    run(async {
        for (status, headers) in [
            (
                "HTTP/1.1 201 Created",
                "Content-Type: application/json\r\nContent-Length: 2\r\n",
            ),
            (
                "HTTP/1.1 100 Continue",
                "Content-Type: application/json\r\nContent-Length: 2\r\n",
            ),
            (
                "HTTP/1.0 200 OK",
                "Content-Type: application/json\r\nContent-Length: 2\r\n",
            ),
            ("HTTP/1.1 200 OK", "Content-Length: 2\r\n"),
            (
                "HTTP/1.1 200 OK",
                "Content-Type: text/event-stream\r\nContent-Length: 2\r\n",
            ),
            (
                "HTTP/1.1 200 OK",
                "Content-Type: application/json\r\ncontent-type: application/json\r\nContent-Length: 2\r\n",
            ),
            (
                "HTTP/1.1 200 OK",
                "Content-Type: application/json\r\nContent-Encoding: identity\r\nContent-Length: 2\r\n",
            ),
            (
                "HTTP/1.1 200 OK",
                "Content-Type: application/json\r\nTrailer: X-End\r\nContent-Length: 2\r\n",
            ),
            (
                "HTTP/1.1 200 OK",
                "Content-Type: application/json\r\nUpgrade: websocket\r\nContent-Length: 2\r\n",
            ),
            ("HTTP/1.1 200 OK", "Content-Type: application/json\r\n"),
            (
                "HTTP/1.1 200 OK",
                "Content-Type: application/json\r\nContent-Length: 2\r\ncontent-length: 2\r\n",
            ),
            (
                "HTTP/1.1 200 OK",
                "Content-Type: application/json\r\nContent-Length: 2\r\nTransfer-Encoding: chunked\r\n",
            ),
            (
                "HTTP/1.1 200 OK",
                "Content-Type: application/json\r\nTransfer-Encoding: gzip, chunked\r\n",
            ),
            (
                "HTTP/1.1 200 OK",
                "Content-Type: application/json\r\nTransfer-Encoding: chunked\r\ntransfer-encoding: chunked\r\n",
            ),
            (
                "HTTP/1.1 200 OK",
                "Content-Type: application/json\r\nContent-Length: +2\r\n",
            ),
            (
                "HTTP/1.1 200 OK",
                "Content-Type: application/json\r\nContent-Length: 2, 2\r\n",
            ),
            (
                "HTTP/1.1 200 OK",
                "Content-Type: application/json\r\nContent-Length: 99999999999999999999999999999999999999\r\n",
            ),
            (
                "HTTP/1.1 200 OK",
                "Content-Type: application/json\r\nContent-Length: 4194305\r\n",
            ),
        ] {
            let mut backend = Stream::new(format!("{status}\r\n{headers}\r\n").into_bytes(), 1);
            backend.stall_after_input = true;
            let state = backend.state.clone();
            let start = Instant::now();
            assert!(
                request_chat(backend, &admitted(false), &policy(), deadline())
                    .await
                    .is_err()
            );
            assert_eq!(
                Instant::now(),
                start,
                "rejection must not wait for the body"
            );
            assert!(state.borrow().dropped);
        }
    });
}

#[test]
fn raw_response_head_syntax_and_bounds_are_enforced() {
    run(async {
        let good = String::from_utf8(length_response(b"", false)).unwrap();
        for bytes in [
            format!("\r\n{good}"),
            good.replacen("HTTP/1.1 200 OK\r\n", "HTTP/1.1 200 OK\n", 1),
            good.replace("Content-Type:", "Content-Type :"),
            good.replace("Content-Type:", " Content-Type:"),
            good.replace("Content-Length:", "\tContent-Length:"),
            good.replace(
                "Content-Type: application/json",
                "Content-Type: application\rjson",
            ),
        ] {
            assert!(
                request_chat(
                    Stream::new(bytes.into_bytes(), 1),
                    &admitted(false),
                    &policy(),
                    deadline()
                )
                .await
                .is_err()
            );
        }
        let base = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 0\r\nX-Pad: \r\n\r\n";
        let exact = base.replace(
            "X-Pad: ",
            &format!("X-Pad: {}", " ".repeat(MAX_HEADER_BYTES - base.len())),
        );
        assert_eq!(exact.len(), MAX_HEADER_BYTES);
        assert!(
            request_chat(
                Stream::new(exact.clone().into_bytes(), usize::MAX),
                &admitted(false),
                &policy(),
                deadline()
            )
            .await
            .is_ok()
        );
        assert!(matches!(
            request_chat(
                Stream::new(
                    exact.replacen("X-Pad: ", "X-Pad:  ", 1).into_bytes(),
                    usize::MAX
                ),
                &admitted(false),
                &policy(),
                deadline()
            )
            .await,
            Err(BackendError::HeadLimit)
        ));
        let extra = "X-Value: x\r\n".repeat(MAX_HEADERS - 2);
        let exact = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 0\r\n{extra}\r\n"
        );
        assert!(
            request_chat(
                Stream::new(exact.into_bytes(), 7),
                &admitted(false),
                &policy(),
                deadline()
            )
            .await
            .is_ok()
        );
        let excessive = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 0\r\n{extra}X-Value: x\r\n\r\n"
        );
        assert!(matches!(
            request_chat(
                Stream::new(excessive.into_bytes(), 7),
                &admitted(false),
                &policy(),
                deadline()
            )
            .await,
            Err(BackendError::HeadLimit)
        ));
    });
}

#[test]
fn truncated_content_length_is_a_sticky_error_instead_of_clean_eof() {
    run(async {
        for fragment in [1, usize::MAX] {
            let backend = Stream::new(
                b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 3\r\n\r\n{}"
                    .to_vec(),
                fragment,
            );
            let state = backend.state.clone();
            let mut body = request_chat(backend, &admitted(false), &policy(), deadline())
                .await
                .unwrap()
                .into_body();
            let mut actual = Vec::new();
            assert!(body.read_to_end(&mut actual).await.is_err());
            assert_eq!(actual, b"{}");
            assert!(body.read(&mut [0]).await.is_err());
            drop(body);
            assert!(state.borrow().dropped);
        }
    });
}

#[test]
fn malformed_or_truncated_chunks_never_become_clean_eof() {
    run(async {
        for fragment in [1, usize::MAX] {
            for payload in [
                b"".as_slice(),
                b"1",
                b"1\r",
                b"1\nX\r\n0\r\n\r\n",
                b"g\r\n",
                b"+1\r\n",
                b" 1\r\n",
                b"1;extension=x\r\na\r\n0\r\n\r\n",
                b"2\r\na",
                b"1\r\na",
                b"1\r\na\r",
                b"1\r\naXX0\r\n\r\n",
                b"1\r\na\r\n",
                b"0\r\n",
                b"0\r\n\r",
                b"0\r\nX-Trailer: secret\r\n\r\n",
                b"FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF\r\n",
            ] {
                let backend = Stream::new(chunked_response(payload, false), fragment);
                let state = backend.state.clone();
                let mut body = request_chat(backend, &admitted(false), &policy(), deadline())
                    .await
                    .unwrap()
                    .into_body();
                assert!(body.read_to_end(&mut Vec::new()).await.is_err());
                assert!(
                    state.borrow().dropped,
                    "framing error closes the stream immediately"
                );
                assert!(body.read(&mut [0]).await.is_err());
                drop(body);
                assert!(state.borrow().dropped);
            }
        }
    });
}

#[test]
fn decoded_body_and_chunk_metadata_have_separate_finite_limits() {
    run(async {
        for length in [MAX_RESPONSE_BYTES, MAX_RESPONSE_BYTES + 1] {
            let mut payload = format!("{length:X}\r\n").into_bytes();
            payload.resize(payload.len() + length, b'x');
            payload.extend_from_slice(b"\r\n0\r\n\r\n");
            let backend = Stream::new(chunked_response(&payload, true), usize::MAX);
            let mut body = request_chat(backend, &admitted(true), &policy(), deadline())
                .await
                .unwrap()
                .into_body();
            let mut actual = Vec::new();
            let result = body.read_to_end(&mut actual).await;
            if length == MAX_RESPONSE_BYTES {
                result.unwrap();
                assert_eq!(actual.len(), length);
            } else {
                assert!(result.is_err());
                assert!(actual.len() <= MAX_RESPONSE_BYTES);
            }
        }
        // Every data byte is legal, but enough tiny chunks exhaust metadata.
        let mut payload = b"1\r\nx\r\n".repeat(14_000);
        payload.extend_from_slice(b"0\r\n\r\n");
        let mut body = request_chat(
            Stream::new(chunked_response(&payload, true), usize::MAX),
            &admitted(true),
            &policy(),
            deadline(),
        )
        .await
        .unwrap()
        .into_body();
        let mut actual = Vec::new();
        assert!(body.read_to_end(&mut actual).await.is_err());
        assert!(actual.len() < 14_000);
    });
}

#[test]
fn exact_count_schema_rejects_missing_duplicate_unknown_or_noninteger_fields() {
    run(async {
        for bytes in [
            b"".as_slice(), b"[]", b"null", b"{}",
            br#"{"input_tokens":100}"#,
            br#"{"object":"response.input_tokens"}"#,
            br#"{"input_tokens":100,"object":"other"}"#,
            br#"{"input_tokens":100,"object":"response.input_tokens","extra":1}"#,
            br#"{"input_tokens":100,"input_tokens":100,"object":"response.input_tokens"}"#,
            br#"{"input_tokens":100,"object":"response.input_tokens","object":"response.input_tokens"}"#,
            br#"{"input_tokens":-1,"object":"response.input_tokens"}"#,
            br#"{"input_tokens":1.0,"object":"response.input_tokens"}"#,
            br#"{"input_tokens":"100","object":"response.input_tokens"}"#,
            br#"{"input_tokens":null,"object":"response.input_tokens"}"#,
            br#"{"input_tokens":100,"object":{"response.input_tokens":null}}"#,
            br#"{"input_tokens":999999999999999999999999999999,"object":"response.input_tokens"}"#,
            br#"{"input_tokens":100,"object":"response.input_tokens"}{}"#,
        ] {
            let backend = Stream::new(length_response(bytes, false), 3);
            let state = backend.state.clone();
            assert!(matches!(count_and_admit(backend, pending(false), &policy(), deadline()).await, Err(BackendError::CountSchema)));
            assert!(state.borrow().dropped);
        }
    });
}

#[test]
fn exact_count_applies_checked_context_admission_and_bounded_collection() {
    run(async {
        let zero = br#"{"input_tokens":0,"object":"response.input_tokens"}"#;
        assert!(matches!(
            count_and_admit(
                Stream::new(length_response(zero, false), 1),
                pending(false),
                &policy(),
                deadline()
            )
            .await,
            Err(BackendError::Admission(Rejection::TokenCountUnavailable))
        ));
        for count in [8001usize, usize::MAX] {
            let bytes = format!(r#"{{"input_tokens":{count},"object":"response.input_tokens"}}"#);
            let backend = Stream::new(length_response(bytes.as_bytes(), false), 7);
            assert!(matches!(
                count_and_admit(backend, pending(false), &policy(), deadline()).await,
                Err(BackendError::Admission(Rejection::ContextLimit))
            ));
        }
        for length in [4096, 4097] {
            let mut bytes = COUNT.to_vec();
            bytes.resize(length, b' ');
            let backend = Stream::new(length_response(&bytes, false), usize::MAX);
            let state = backend.state.clone();
            let result = count_and_admit(backend, pending(false), &policy(), deadline()).await;
            if length == 4096 {
                assert!(result.is_ok());
            } else {
                assert!(result.is_err());
            }
            assert!(state.borrow().dropped);
        }
    });
}

#[test]
fn request_write_errors_and_flush_failure_drop_the_owned_stream() {
    run(async {
        for case in 0..3 {
            let mut backend = Stream::new(length_response(b"{}", false), 1);
            backend.fail_write = case == 0;
            backend.zero_write = case == 1;
            backend.fail_flush = case == 2;
            let state = backend.state.clone();
            assert!(matches!(
                request_chat(backend, &admitted(false), &policy(), deadline()).await,
                Err(BackendError::Io)
            ));
            assert!(state.borrow().dropped);
            assert_eq!(state.borrow().read_polls, 0);
        }
    });
}

#[test]
fn request_writes_and_flush_share_one_absolute_five_second_budget() {
    run(async {
        for case in 0..3 {
            let mut backend = Stream::new(length_response(b"{}", false), 1);
            backend.stall_write = case == 0;
            backend.stall_flush = case == 1;
            if case == 2 {
                backend.write_fragment = 1;
                backend.write_delay = Some(Duration::from_secs(1));
            }
            let state = backend.state.clone();
            let start = Instant::now();
            assert!(matches!(
                request_chat(backend, &admitted(false), &policy(), deadline()).await,
                Err(BackendError::Deadline)
            ));
            assert_eq!(Instant::now() - start, Duration::from_secs(5));
            assert!(state.borrow().dropped);
            assert_eq!(state.borrow().read_polls, 0);
        }
    });
}

#[test]
fn supervisor_deadline_bounds_slow_head_and_body_without_reset_on_progress() {
    run(async {
        for slow_drip in [false, true] {
            let mut backend = Stream::new(
                if slow_drip {
                    length_response(b"{}", false)
                } else {
                    Vec::new()
                },
                1,
            );
            backend.stall_after_input = true;
            backend.read_delay = slow_drip.then_some(Duration::from_secs(1));
            let state = backend.state.clone();
            let start = Instant::now();
            assert!(matches!(
                request_chat(
                    backend,
                    &admitted(false),
                    &policy(),
                    start + Duration::from_secs(2)
                )
                .await,
                Err(BackendError::Deadline)
            ));
            assert_eq!(Instant::now() - start, Duration::from_secs(2));
            assert!(state.borrow().dropped);
        }
        let mut backend = Stream::new(
            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 1\r\n\r\n"
                .to_vec(),
            usize::MAX,
        );
        backend.stall_after_input = true;
        let state = backend.state.clone();
        let start = Instant::now();
        let mut body = request_chat(
            backend,
            &admitted(false),
            &policy(),
            start + Duration::from_secs(2),
        )
        .await
        .unwrap()
        .into_body();
        assert!(body.read_to_end(&mut Vec::new()).await.is_err());
        assert_eq!(Instant::now() - start, Duration::from_secs(2));
        assert!(body.read(&mut [0]).await.is_err());
        drop(body);
        assert!(state.borrow().dropped);
    });
}

#[test]
fn expiry_rejects_ready_input_and_operations_becoming_ready_at_the_deadline() {
    run(async {
        for counter in [false, true] {
            let backend = Stream::new(length_response(COUNT, false), usize::MAX);
            let state = backend.state.clone();
            if counter {
                assert!(matches!(
                    count_and_admit(backend, pending(false), &policy(), Instant::now()).await,
                    Err(BackendError::Deadline)
                ));
            } else {
                assert!(matches!(
                    request_chat(backend, &admitted(false), &policy(), Instant::now()).await,
                    Err(BackendError::Deadline)
                ));
            }
            assert!(state.borrow().dropped);
            assert_eq!(state.borrow().read_polls, 0);
            assert_eq!(state.borrow().write_polls, 0);
        }
        for delayed_write in [false, true] {
            let mut backend = Stream::new(length_response(b"{}", false), usize::MAX);
            if delayed_write {
                backend.write_delay = Some(Duration::from_secs(2));
            } else {
                backend.read_delay = Some(Duration::from_secs(2));
            }
            let state = backend.state.clone();
            let start = Instant::now();
            assert!(matches!(
                request_chat(
                    backend,
                    &admitted(false),
                    &policy(),
                    start + Duration::from_secs(2)
                )
                .await,
                Err(BackendError::Deadline)
            ));
            assert_eq!(Instant::now() - start, Duration::from_secs(2));
            assert!(state.borrow().dropped);
        }
    });
}

#[test]
fn cancelling_request_or_body_collection_drops_the_owned_connection() {
    run(async {
        for blocked_write in [false, true] {
            let mut backend = Stream::new(Vec::new(), 1);
            backend.stall_write = blocked_write;
            backend.stall_after_input = true;
            let state = backend.state.clone();
            assert!(
                timeout(
                    Duration::from_millis(1),
                    request_chat(backend, &admitted(false), &policy(), deadline())
                )
                .await
                .is_err()
            );
            assert!(state.borrow().dropped);
        }
        let mut backend = Stream::new(
            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 1\r\n\r\n"
                .to_vec(),
            usize::MAX,
        );
        backend.stall_after_input = true;
        let state = backend.state.clone();
        let mut body = request_chat(backend, &admitted(false), &policy(), deadline())
            .await
            .unwrap()
            .into_body();
        assert!(
            timeout(Duration::from_millis(1), async move {
                body.read_to_end(&mut Vec::new()).await
            })
            .await
            .is_err()
        );
        assert!(state.borrow().dropped);
    });
}

#[test]
fn truncated_wire_stream_cannot_produce_successful_downstream_terminal_framing() {
    run(async {
        let backend = Stream::new(chunked_response(b"3\r\nabc\r\n", true), 1);
        let backend_state = backend.state.clone();
        let chat = admitted(true);
        let response = request_chat(backend, &chat, &policy(), deadline())
            .await
            .unwrap();
        let client = Stream::new(Vec::new(), usize::MAX);
        let client_state = client.state.clone();
        assert!(matches!(
            Box::pin(send_chat_response(client, response, &chat, deadline())).await,
            Err(ResponseError::BackendIo)
        ));
        assert!(backend_state.borrow().dropped);
        assert!(client_state.borrow().dropped);
        let state = client_state.borrow();
        let split = state
            .written
            .windows(4)
            .position(|part| part == b"\r\n\r\n")
            .unwrap()
            + 4;
        assert!(!state.written[split..].ends_with(b"0\r\n\r\n"));
        assert!(!state.written.windows(6).any(|part| part == b"[DONE]"));
    });
}
