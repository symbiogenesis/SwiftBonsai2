//! Regression coverage for bounded response framing and endpoint ownership.
#![expect(
    clippy::unwrap_used,
    reason = "test setup and successful-operation assertions must panic on failure"
)]

use bonsai_broker::{
    AdmittedChat, RequestPolicy,
    response::{
        CLIENT_WRITE_TIMEOUT, MAX_RESPONSE_BYTES, RESPONSE_BUFFER_BYTES, ResponseError,
        send_chat_response,
    },
};
use http::{HeaderValue, Request, Response, Version};
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
    io::{AsyncRead, AsyncWrite, ReadBuf},
    time::{Instant, Sleep, sleep, timeout},
};

const KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn run(test: impl Future<Output = ()>) {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(test);
}

fn admitted(stream: bool) -> AdmittedChat {
    let body =
        format!(r#"{{"messages":[{{"role":"user","content":"A scene."}}],"stream":{stream}}}"#);
    let head = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .version(Version::HTTP_11)
        .header("host", "127.0.0.1:8080")
        .header("authorization", format!("Bearer {KEY}"))
        .header("content-type", "application/json")
        .header("content-length", body.len())
        .body(())
        .unwrap()
        .into_parts()
        .0;
    RequestPolicy::new(8080, KEY)
        .unwrap()
        .check_head(&head)
        .unwrap()
        .prepare(body.as_bytes())
        .unwrap()
        .admit(|_| Ok(100))
        .unwrap()
}

fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(60)
}

#[derive(Default)]
struct ReadState {
    consumed: usize,
    polls: usize,
    largest_buffer: usize,
    dropped: bool,
}

// A decoder fixture: an error after payload means HTTP framing was truncated,
// while an empty successful read means the decoder verified clean message EOF.
struct Backend {
    bytes: Vec<u8>,
    position: usize,
    fragment: usize,
    fail_at_end: bool,
    stall_at_end: bool,
    delay: Option<Duration>,
    timer: Option<Pin<Box<Sleep>>>,
    state: Rc<RefCell<ReadState>>,
}

impl Backend {
    fn new(bytes: impl Into<Vec<u8>>, fragment: usize) -> Self {
        Self {
            bytes: bytes.into(),
            position: 0,
            fragment,
            fail_at_end: false,
            stall_at_end: false,
            delay: None,
            timer: None,
            state: Rc::new(RefCell::new(ReadState::default())),
        }
    }
}

impl AsyncRead for Backend {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        {
            let mut state = self.state.borrow_mut();
            state.polls += 1;
            state.largest_buffer = state.largest_buffer.max(buffer.remaining());
        }
        if let Some(delay) = self.delay {
            let timer = self.timer.get_or_insert_with(|| Box::pin(sleep(delay)));
            if timer.as_mut().poll(cx).is_pending() {
                return Poll::Pending;
            }
            self.timer = None;
        }
        if self.position == self.bytes.len() {
            return if self.fail_at_end {
                Poll::Ready(Err(io::Error::from(io::ErrorKind::UnexpectedEof)))
            } else if self.stall_at_end {
                Poll::Pending
            } else {
                Poll::Ready(Ok(()))
            };
        }
        let count = buffer
            .remaining()
            .min(self.fragment)
            .min(self.bytes.len() - self.position);
        buffer.put_slice(&self.bytes[self.position..self.position + count]);
        self.position += count;
        self.state.borrow_mut().consumed += count;
        Poll::Ready(Ok(()))
    }
}

impl Drop for Backend {
    fn drop(&mut self) {
        self.state.borrow_mut().dropped = true;
    }
}

#[derive(Default)]
struct WriteState {
    bytes: Vec<u8>,
    flushes: Vec<usize>,
    shutdowns: usize,
    dropped: bool,
}

#[expect(
    clippy::struct_excessive_bools,
    reason = "independent fault-injection switches exercise each I/O failure mode"
)]
struct Client {
    state: Rc<RefCell<WriteState>>,
    fragment: usize,
    zero_write: bool,
    fail_write: bool,
    fail_flush: bool,
    stall_flush: bool,
    stall_shutdown: bool,
    body_write_delay: Option<Duration>,
    timer: Option<Pin<Box<Sleep>>>,
}

impl Client {
    fn new(fragment: usize) -> Self {
        Self {
            state: Rc::new(RefCell::new(WriteState::default())),
            fragment,
            zero_write: false,
            fail_write: false,
            fail_flush: false,
            stall_flush: false,
            stall_shutdown: false,
            body_write_delay: None,
            timer: None,
        }
    }
}

impl AsyncWrite for Client {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        if self.fail_write {
            return Poll::Ready(Err(io::Error::from(io::ErrorKind::BrokenPipe)));
        }
        if self.zero_write {
            return Poll::Ready(Ok(0));
        }
        if let Some(delay) = self.body_write_delay {
            let in_body = self
                .state
                .borrow()
                .bytes
                .windows(4)
                .any(|part| part == b"\r\n\r\n");
            if in_body {
                let timer = self.timer.get_or_insert_with(|| Box::pin(sleep(delay)));
                if timer.as_mut().poll(cx).is_pending() {
                    return Poll::Pending;
                }
                self.timer = None;
            }
        }
        let count = self.fragment.min(bytes.len());
        self.state
            .borrow_mut()
            .bytes
            .extend_from_slice(&bytes[..count]);
        Poll::Ready(Ok(count))
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        if self.fail_flush {
            return Poll::Ready(Err(io::Error::from(io::ErrorKind::BrokenPipe)));
        }
        if self.stall_flush {
            return Poll::Pending;
        }
        let mut state = self.state.borrow_mut();
        let count = state.bytes.len();
        state.flushes.push(count);
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.state.borrow_mut().shutdowns += 1;
        if self.stall_shutdown {
            Poll::Pending
        } else {
            Poll::Ready(Ok(()))
        }
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        self.state.borrow_mut().dropped = true;
    }
}

fn response(body: Backend, stream: bool) -> Response<Backend> {
    Response::builder()
        .status(200)
        .header(
            "content-type",
            if stream {
                "text/event-stream"
            } else {
                "application/json"
            },
        )
        .body(body)
        .unwrap()
}

fn wire_parts(bytes: &[u8]) -> (BTreeMap<String, String>, &[u8], usize) {
    let split = bytes
        .windows(4)
        .position(|part| part == b"\r\n\r\n")
        .unwrap()
        + 4;
    let head = std::str::from_utf8(&bytes[..split]).unwrap();
    let mut lines = head.split("\r\n");
    assert_eq!(lines.next(), Some("HTTP/1.1 200 OK"));
    let mut headers = BTreeMap::new();
    for line in lines.filter(|line| !line.is_empty()) {
        let (name, value) = line.split_once(':').unwrap();
        assert!(
            headers
                .insert(name.to_ascii_lowercase(), value.trim().to_string())
                .is_none()
        );
    }
    (headers, &bytes[split..], split)
}

fn assert_headers(headers: &BTreeMap<String, String>, stream: bool, length: usize) {
    assert_eq!(
        headers.len(),
        5,
        "only freshly generated headers are permitted"
    );
    assert_eq!(headers["connection"], "close");
    assert_eq!(headers["cache-control"], "no-store");
    assert_eq!(headers["x-content-type-options"], "nosniff");
    if stream {
        assert_eq!(headers["content-type"], "text/event-stream");
        assert_eq!(headers["transfer-encoding"], "chunked");
    } else {
        assert_eq!(headers["content-type"], "application/json");
        assert_eq!(headers["content-length"], length.to_string());
    }
}

// Return decoded payload plus end offsets for complete nonterminal chunks. The
// offsets allow flush verification without prescribing backend read boundaries.
fn chunks(bytes: &[u8], terminal: bool) -> (Vec<u8>, Vec<usize>) {
    let mut at = 0;
    let mut decoded = Vec::new();
    let mut ends = Vec::new();
    while at < bytes.len() {
        let line_end = bytes[at..]
            .windows(2)
            .position(|part| part == b"\r\n")
            .unwrap()
            + at;
        let count =
            usize::from_str_radix(std::str::from_utf8(&bytes[at..line_end]).unwrap(), 16).unwrap();
        at = line_end + 2;
        if count == 0 {
            assert!(
                terminal,
                "failure must not produce successful terminal framing"
            );
            assert_eq!(&bytes[at..], b"\r\n");
            return (decoded, ends);
        }
        assert!(count <= RESPONSE_BUFFER_BYTES);
        decoded.extend_from_slice(&bytes[at..at + count]);
        at += count;
        assert_eq!(&bytes[at..at + 2], b"\r\n");
        at += 2;
        ends.push(at);
    }
    assert!(
        !terminal,
        "clean completion requires a terminating zero chunk"
    );
    (decoded, ends)
}

fn sized_json(length: usize) -> Vec<u8> {
    let mut body = br#"{"text":""#.to_vec();
    body.resize(length - 2, b'x');
    body.extend_from_slice(b"\"}");
    assert_eq!(body.len(), length);
    body
}

fn assert_closed(client: &Rc<RefCell<WriteState>>, backend: &Rc<RefCell<ReadState>>) {
    assert!(client.borrow().dropped);
    assert!(backend.borrow().dropped);
}

#[test]
fn json_is_validated_then_sent_unchanged_with_fresh_headers_and_partial_writes() {
    run(async {
        let body = b" \r\n{\"duplicate\":1,\"duplicate\":2,\"text\":\"rain\\n\"}\t ".to_vec();
        let backend = Backend::new(body.clone(), 3);
        let backend_state = backend.state.clone();
        let client = Client::new(2);
        let client_state = client.state.clone();
        let mut backend = response(backend, false);
        for (name, value) in [
            ("set-cookie", "private=secret"),
            ("authorization", "Bearer backend-secret"),
            ("server", "private-backend"),
            ("location", "http://private.invalid/"),
            ("connection", "x-private"),
            ("x-private", "secret"),
            ("cache-control", "public"),
            // Already decoded bodies do not inherit upstream wire framing.
            ("content-length", "0"),
            ("transfer-encoding", "chunked"),
        ] {
            backend
                .headers_mut()
                .insert(name, HeaderValue::from_static(value));
        }
        Box::pin(send_chat_response(
            client,
            backend,
            &admitted(false),
            deadline(),
        ))
        .await
        .unwrap();
        assert_closed(&client_state, &backend_state);
        let state = client_state.borrow();
        let (headers, actual, _) = wire_parts(&state.bytes);
        assert_headers(&headers, false, body.len());
        assert_eq!(actual, body);
        assert_eq!(state.shutdowns, 1);
    });
}

#[test]
fn backend_status_media_type_and_compression_fail_before_any_io() {
    run(async {
        for stream in [false, true] {
            for case in 0..11 {
                let backend = Backend::new(b"{}".to_vec(), usize::MAX);
                let backend_state = backend.state.clone();
                let client = Client::new(usize::MAX);
                let client_state = client.state.clone();
                let mut backend = response(backend, stream);
                match case {
                    0 => *backend.status_mut() = http::StatusCode::CREATED,
                    1 => *backend.status_mut() = http::StatusCode::NO_CONTENT,
                    2 => *backend.status_mut() = http::StatusCode::INTERNAL_SERVER_ERROR,
                    3 => {
                        backend.headers_mut().remove("content-type");
                    }
                    4 => {
                        backend
                            .headers_mut()
                            .append("content-type", HeaderValue::from_static("application/json"));
                    }
                    5 => {
                        backend
                            .headers_mut()
                            .insert("content-type", HeaderValue::from_static("text/plain"));
                    }
                    6 => {
                        backend.headers_mut().insert(
                            "content-type",
                            HeaderValue::from_static(if stream {
                                "application/json"
                            } else {
                                "text/event-stream"
                            }),
                        );
                    }
                    7 => {
                        backend.headers_mut().insert(
                            "content-type",
                            HeaderValue::from_static("application/json; charset=latin1"),
                        );
                    }
                    8 => {
                        backend
                            .headers_mut()
                            .insert("content-encoding", HeaderValue::from_static("identity"));
                    }
                    9 => {
                        backend
                            .headers_mut()
                            .insert("content-encoding", HeaderValue::from_static(""));
                    }
                    10 => {
                        backend.headers_mut().insert(
                            "content-type",
                            HeaderValue::from_static("application/json, application/json"),
                        );
                    }
                    _ => unreachable!(),
                }
                assert!(matches!(
                    Box::pin(send_chat_response(
                        client,
                        backend,
                        &admitted(stream),
                        deadline()
                    ))
                    .await,
                    Err(ResponseError::BackendMetadata)
                ));
                assert_closed(&client_state, &backend_state);
                assert_eq!(client_state.borrow().bytes, [0_u8; 0]);
                assert_eq!(client_state.borrow().shutdowns, 0);
                assert_eq!(backend_state.borrow().polls, 0);
            }
        }
    });
}

#[test]
fn validated_media_types_accept_case_and_optional_utf8_charset() {
    run(async {
        for (stream, media) in [
            (false, "APPLICATION/JSON"),
            (false, "Application/Json; Charset=UTF-8"),
            (true, "TEXT/EVENT-STREAM"),
            (true, "Text/Event-Stream; Charset=UTF-8"),
        ] {
            let client = Client::new(usize::MAX);
            let state = client.state.clone();
            let mut backend = response(Backend::new(b"{}".to_vec(), usize::MAX), stream);
            backend
                .headers_mut()
                .insert("content-type", HeaderValue::from_static(media));
            Box::pin(send_chat_response(
                client,
                backend,
                &admitted(stream),
                deadline(),
            ))
            .await
            .unwrap();
            let state = state.borrow();
            let (headers, _, _) = wire_parts(&state.bytes);
            assert_headers(&headers, stream, 2);
        }
    });
}

#[test]
fn json_requires_one_complete_object_and_never_sends_invalid_bytes() {
    run(async {
        for body in [
            b"".as_slice(),
            b"[]",
            b"null",
            b"true",
            b"1",
            b"\"text\"",
            b"{",
            b"{\"x\":1,}",
            b"{}{}",
            b"{} trailing",
            b"{\"x\":\"\xff\"}",
        ] {
            let backend = Backend::new(body.to_vec(), 1);
            let backend_state = backend.state.clone();
            let client = Client::new(usize::MAX);
            let client_state = client.state.clone();
            assert!(matches!(
                Box::pin(send_chat_response(
                    client,
                    response(backend, false),
                    &admitted(false),
                    deadline()
                ))
                .await,
                Err(ResponseError::JsonSyntax)
            ));
            assert_closed(&client_state, &backend_state);
            assert_eq!(client_state.borrow().bytes, [0_u8; 0]);
            assert_eq!(client_state.borrow().shutdowns, 0);
        }
    });
}

#[test]
fn exact_four_mib_json_and_sse_bodies_succeed() {
    run(async {
        assert_eq!(MAX_RESPONSE_BYTES, 4 * 1024 * 1024);
        for stream in [false, true] {
            let body = sized_json(MAX_RESPONSE_BYTES);
            let backend = Backend::new(body.clone(), usize::MAX);
            let backend_state = backend.state.clone();
            let client = Client::new(usize::MAX);
            let client_state = client.state.clone();
            Box::pin(send_chat_response(
                client,
                response(backend, stream),
                &admitted(stream),
                deadline(),
            ))
            .await
            .unwrap();
            assert_closed(&client_state, &backend_state);
            let state = client_state.borrow();
            let (headers, actual, _) = wire_parts(&state.bytes);
            assert_headers(&headers, stream, body.len());
            let actual = if stream {
                chunks(actual, true).0
            } else {
                actual.to_vec()
            };
            assert_eq!(actual, body);
            assert_eq!(backend_state.borrow().consumed, MAX_RESPONSE_BYTES);
        }
    });
}

#[test]
fn cap_plus_one_probe_rejects_json_and_sse_without_forwarding_excess() {
    run(async {
        for stream in [false, true] {
            let backend = Backend::new(
                sized_json(MAX_RESPONSE_BYTES + RESPONSE_BUFFER_BYTES),
                usize::MAX,
            );
            let backend_state = backend.state.clone();
            let client = Client::new(usize::MAX);
            let client_state = client.state.clone();
            assert!(matches!(
                Box::pin(send_chat_response(
                    client,
                    response(backend, stream),
                    &admitted(stream),
                    deadline()
                ))
                .await,
                Err(ResponseError::BodyLimit)
            ));
            assert_closed(&client_state, &backend_state);
            assert_eq!(backend_state.borrow().consumed, MAX_RESPONSE_BYTES + 1);
            let state = client_state.borrow();
            assert_eq!(state.shutdowns, 0);
            if stream {
                let (_, body, _) = wire_parts(&state.bytes);
                assert!(chunks(body, false).0.len() <= MAX_RESPONSE_BYTES);
            } else {
                assert_eq!(state.bytes, [0_u8; 0]);
            }
        }
    });
}

#[test]
fn fragmented_sse_is_flushed_in_bounded_chunks_without_inventing_events() {
    run(async {
        assert_eq!(RESPONSE_BUFFER_BYTES, 8192);
        for fragment in [
            1,
            15,
            16,
            255,
            256,
            RESPONSE_BUFFER_BYTES,
            RESPONSE_BUFFER_BYTES * 2,
        ] {
            let mut body = b": comment\r\ndata: {\"delta\":\"rain\"}\n\n".to_vec();
            body.extend(std::iter::repeat_n(b'x', RESPONSE_BUFFER_BYTES * 2 + 17));
            body.extend_from_slice(b"\n\ndata: \xff\n\n");
            let backend = Backend::new(body.clone(), fragment);
            let backend_state = backend.state.clone();
            let client = Client::new(3);
            let client_state = client.state.clone();
            Box::pin(send_chat_response(
                client,
                response(backend, true),
                &admitted(true),
                deadline(),
            ))
            .await
            .unwrap();
            let state = client_state.borrow();
            let (headers, actual, head_length) = wire_parts(&state.bytes);
            assert_headers(&headers, true, body.len());
            let (decoded, chunk_ends) = chunks(actual, true);
            assert_eq!(decoded, body);
            assert_eq!(state.flushes.first(), Some(&head_length));
            for end in chunk_ends {
                assert!(
                    state.flushes.contains(&(head_length + end)),
                    "each complete chunk must be flushed"
                );
            }
            assert!(backend_state.borrow().largest_buffer <= RESPONSE_BUFFER_BYTES);
            assert_eq!(state.shutdowns, 1);
            assert_closed(&client_state, &backend_state);
        }
    });
}

#[test]
fn decoded_backend_truncation_is_an_error_even_after_valid_payload() {
    run(async {
        for stream in [false, true] {
            let mut backend = Backend::new(b"{}".to_vec(), 1);
            backend.fail_at_end = true;
            let backend_state = backend.state.clone();
            let client = Client::new(usize::MAX);
            let client_state = client.state.clone();
            assert!(matches!(
                Box::pin(send_chat_response(
                    client,
                    response(backend, stream),
                    &admitted(stream),
                    deadline()
                ))
                .await,
                Err(ResponseError::BackendIo)
            ));
            assert_closed(&client_state, &backend_state);
            let state = client_state.borrow();
            assert_eq!(state.shutdowns, 0);
            if stream {
                let (_, body, _) = wire_parts(&state.bytes);
                assert_eq!(chunks(body, false).0, b"{}");
            } else {
                assert_eq!(state.bytes, [0_u8; 0]);
            }
        }
    });
}

#[test]
fn zero_writes_write_errors_and_flush_errors_drop_both_streams() {
    run(async {
        for failure in 0..3 {
            let backend = Backend::new(b"{}".to_vec(), usize::MAX);
            let backend_state = backend.state.clone();
            let mut client = Client::new(usize::MAX);
            client.zero_write = failure == 0;
            client.fail_write = failure == 1;
            client.fail_flush = failure == 2;
            let client_state = client.state.clone();
            assert!(matches!(
                Box::pin(send_chat_response(
                    client,
                    response(backend, true),
                    &admitted(true),
                    deadline()
                ))
                .await,
                Err(ResponseError::ClientIo)
            ));
            assert_closed(&client_state, &backend_state);
            assert_eq!(client_state.borrow().shutdowns, 0);
        }
    });
}

#[test]
fn stalled_header_flush_uses_client_or_supervisor_deadline() {
    run(async {
        assert_eq!(CLIENT_WRITE_TIMEOUT, Duration::from_secs(5));
        for bound in [CLIENT_WRITE_TIMEOUT, Duration::from_secs(2)] {
            let backend = Backend::new(b"{}".to_vec(), usize::MAX);
            let backend_state = backend.state.clone();
            let mut client = Client::new(usize::MAX);
            client.stall_flush = true;
            let client_state = client.state.clone();
            let start = Instant::now();
            assert!(matches!(
                Box::pin(send_chat_response(
                    client,
                    response(backend, true),
                    &admitted(true),
                    start + bound
                ))
                .await,
                Err(ResponseError::Deadline)
            ));
            assert_eq!(Instant::now() - start, bound);
            assert_closed(&client_state, &backend_state);
            assert_eq!(client_state.borrow().shutdowns, 0);
            assert_eq!(backend_state.borrow().polls, 0);
        }
    });
}

#[test]
fn partial_chunk_writes_share_one_deadline_instead_of_resetting_it() {
    run(async {
        let backend = Backend::new(b"data: rain\n\n".to_vec(), usize::MAX);
        let backend_state = backend.state.clone();
        let mut client = Client::new(1);
        client.body_write_delay = Some(Duration::from_secs(1));
        let client_state = client.state.clone();
        let start = Instant::now();
        assert!(matches!(
            Box::pin(send_chat_response(
                client,
                response(backend, true),
                &admitted(true),
                deadline()
            ))
            .await,
            Err(ResponseError::Deadline)
        ));
        assert_eq!(Instant::now() - start, CLIENT_WRITE_TIMEOUT);
        assert_closed(&client_state, &backend_state);
        assert_eq!(client_state.borrow().shutdowns, 0);
    });
}

#[test]
fn stalled_shutdown_is_bounded_and_releases_both_streams() {
    run(async {
        let backend = Backend::new(b"{}".to_vec(), usize::MAX);
        let backend_state = backend.state.clone();
        let mut client = Client::new(usize::MAX);
        client.stall_shutdown = true;
        let client_state = client.state.clone();
        let start = Instant::now();
        assert!(matches!(
            Box::pin(send_chat_response(
                client,
                response(backend, true),
                &admitted(true),
                deadline()
            ))
            .await,
            Err(ResponseError::Deadline)
        ));
        assert_eq!(Instant::now() - start, CLIENT_WRITE_TIMEOUT);
        assert_closed(&client_state, &backend_state);
        assert!(client_state.borrow().shutdowns > 0);
    });
}

#[test]
fn backend_reads_use_one_absolute_supervisor_deadline() {
    run(async {
        for drip in [false, true] {
            let mut backend = Backend::new(b"{}".to_vec(), 1);
            if drip {
                backend.delay = Some(Duration::from_secs(1));
            } else {
                backend.bytes.clear();
                backend.stall_at_end = true;
            }
            let backend_state = backend.state.clone();
            let client = Client::new(usize::MAX);
            let client_state = client.state.clone();
            let start = Instant::now();
            assert!(matches!(
                Box::pin(send_chat_response(
                    client,
                    response(backend, false),
                    &admitted(false),
                    start + Duration::from_secs(2)
                ))
                .await,
                Err(ResponseError::Deadline)
            ));
            assert_eq!(Instant::now() - start, Duration::from_secs(2));
            assert_closed(&client_state, &backend_state);
            assert_eq!(client_state.borrow().bytes, [0_u8; 0]);
            assert_eq!(client_state.borrow().shutdowns, 0);
        }
    });
}

#[test]
fn expired_deadline_rejects_immediately_ready_input_before_any_io() {
    run(async {
        for stream in [false, true] {
            let backend = Backend::new(b"{}".to_vec(), usize::MAX);
            let backend_state = backend.state.clone();
            let client = Client::new(usize::MAX);
            let client_state = client.state.clone();
            assert!(matches!(
                Box::pin(send_chat_response(
                    client,
                    response(backend, stream),
                    &admitted(stream),
                    Instant::now()
                ))
                .await,
                Err(ResponseError::Deadline)
            ));
            assert_closed(&client_state, &backend_state);
            assert_eq!(backend_state.borrow().polls, 0);
            assert_eq!(client_state.borrow().bytes, [0_u8; 0]);
            assert_eq!(client_state.borrow().shutdowns, 0);
        }
    });
}

#[test]
fn a_backend_read_becoming_ready_at_expiry_still_fails_closed() {
    run(async {
        let mut backend = Backend::new(b"{}".to_vec(), usize::MAX);
        backend.delay = Some(Duration::from_secs(2));
        let backend_state = backend.state.clone();
        let client = Client::new(usize::MAX);
        let client_state = client.state.clone();
        let start = Instant::now();
        assert!(matches!(
            Box::pin(send_chat_response(
                client,
                response(backend, false),
                &admitted(false),
                start + Duration::from_secs(2)
            ))
            .await,
            Err(ResponseError::Deadline)
        ));
        assert_eq!(Instant::now() - start, Duration::from_secs(2));
        assert_closed(&client_state, &backend_state);
        assert_eq!(client_state.borrow().bytes, [0_u8; 0]);
    });
}

#[test]
fn cancellation_during_backend_read_or_client_flush_drops_owned_streams() {
    run(async {
        for blocked_flush in [false, true] {
            let mut backend = Backend::new(Vec::new(), usize::MAX);
            backend.stall_at_end = true;
            let backend_state = backend.state.clone();
            let mut client = Client::new(usize::MAX);
            client.stall_flush = blocked_flush;
            let client_state = client.state.clone();
            let start = Instant::now();
            assert!(
                timeout(
                    Duration::from_millis(1),
                    Box::pin(send_chat_response(
                        client,
                        response(backend, true),
                        &admitted(true),
                        deadline()
                    )),
                )
                .await
                .is_err()
            );
            assert!(Instant::now() - start < CLIENT_WRITE_TIMEOUT);
            assert_closed(&client_state, &backend_state);
            assert_eq!(client_state.borrow().shutdowns, 0);
            let state = client_state.borrow();
            let (_, body, _) = wire_parts(&state.bytes);
            assert_eq!(body, [0_u8; 0]);
        }
    });
}
