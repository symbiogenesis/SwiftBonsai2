//! Narrow HTTP exchange over an owned, already connected private backend stream.
//!
//! No socket, process, task, retry, redirect or connection reuse is provided.
//! The supervisor must supply streams to the same immutable, owned backend for
//! counting and generation, prevent bypass, and ensure Drop closes each stream.

use crate::{
    AdmittedChat, MAX_HEADER_BYTES, MAX_HEADERS, Object, PendingChat, Rejection, configuration,
    response::{MAX_RESPONSE_BYTES, validate_metadata},
    singleton, wire,
};
use http::{Response, Version};
use serde::Deserialize;
use std::{
    io,
    pin::Pin,
    task::{Context, Poll, ready},
    time::Duration,
};
use tokio::{
    io::{
        AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt,
        BufReader, ReadBuf,
    },
    time::{Instant, Sleep, sleep_until, timeout_at},
};

/// Maximum time allowed for writing one backend request.
pub const WRITE_TIMEOUT: Duration = Duration::from_secs(5);
/// Largest accepted token-counter response body in bytes.
pub const MAX_COUNT_BYTES: usize = 4096;
/// Largest accepted chunk-size line in bytes, including its CRLF.
pub const MAX_CHUNK_LINE_BYTES: usize = 32;
/// Largest accumulated chunk framing metadata in bytes for one response.
pub const MAX_CHUNK_METADATA_BYTES: usize = 64 * 1024;

/// Contains no private header, body, credential or native I/O diagnostic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackendError {
    /// The private backend port or credential format is invalid.
    Configuration,
    /// The absolute exchange or write deadline has expired.
    Deadline,
    /// The supplied stream failed while exchanging HTTP headers or request data.
    Io,
    /// The backend response exceeded the header count or byte limit.
    HeadLimit,
    /// The backend response has invalid HTTP/1.1 header syntax.
    WireSyntax,
    /// The response status, content type or encoding is unsupported.
    Metadata,
    /// The response uses unsupported or ambiguous HTTP body framing.
    Framing,
    /// The body is oversized, truncated or fails while being read.
    Body,
    /// The token-counter response does not match the bounded strict schema.
    CountSchema,
    /// The existing request policy rejected the counted prompt.
    Admission(Rejection),
}

/// Private backend connection metadata. No Debug/Serialize implementation.
/// The caller must use a separate random backend key and enforce its protection.
pub struct BackendPolicy {
    authority: String,
    authorization: String,
}

impl BackendPolicy {
    /// Configure the private backend endpoint and its separate credential.
    ///
    /// # Errors
    /// Returns [`BackendError::Configuration`] for a privileged port or invalid key.
    pub fn new(port: u16, key: &str) -> Result<Self, BackendError> {
        if port < 1024 || !configuration::valid_key(key.as_bytes()) {
            return Err(BackendError::Configuration);
        }
        Ok(Self {
            authority: format!("127.0.0.1:{port}"),
            authorization: format!("Bearer {key}"),
        })
    }
}

/// Count the exact canonical body, then apply the existing context admission.
///
/// Ownership closes the supplied connection on failure, completion or cancellation.
///
/// # Errors
/// Returns [`BackendError`] for an expired deadline, stream failure, invalid
/// HTTP response, token-count schema mismatch or rejected context admission.
pub async fn count_and_admit<S>(
    stream: S,
    pending: PendingChat,
    policy: &BackendPolicy,
    deadline: Instant,
) -> Result<AdmittedChat, BackendError>
where
    S: AsyncRead + AsyncWrite + Unpin + 'static,
{
    let response = exchange(
        stream,
        &pending.body,
        false,
        "/v1/chat/completions/input_tokens",
        policy,
        deadline,
    )
    .await?;
    // Read directly into the final allocation, keeping one byte for the
    // oversized-body probe without allowing unbounded read_to_end growth.
    let limit = u64::try_from(MAX_COUNT_BYTES + 1).map_err(|_| BackendError::CountSchema)?;
    let mut body = response.into_body().take(limit);
    let mut bytes = Vec::with_capacity(MAX_COUNT_BYTES + 1);
    body.read_to_end(&mut bytes).await.map_err(|error| {
        if error.kind() == io::ErrorKind::TimedOut {
            BackendError::Deadline
        } else {
            BackendError::Body
        }
    })?;
    if bytes.len() > MAX_COUNT_BYTES {
        return Err(BackendError::CountSchema);
    }
    let Object(count): Object<Count> =
        serde_json::from_slice(&bytes).map_err(|_| BackendError::CountSchema)?;
    ensure_time(deadline)?;
    if count.object != "response.input_tokens" {
        return Err(BackendError::CountSchema);
    }
    // PendingChat checks positivity and retains the canonical allocation unchanged.
    let admitted = pending
        .admit(|_| Ok(count.input_tokens))
        .map_err(BackendError::Admission)?;
    ensure_time(deadline)?;
    Ok(admitted)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Count {
    input_tokens: usize,
    object: String,
}

/// Send the admitted bytes unchanged to the sole generation route.
///
/// The returned owned decoder is suitable for [`crate::response::send_chat_response`], including errors
/// on incomplete framing. It enforces the same absolute body deadline itself.
///
/// # Errors
/// Returns [`BackendError`] for an expired deadline, stream failure, invalid
/// HTTP response metadata or unsupported body framing.
pub async fn request_chat<S>(
    stream: S,
    chat: &AdmittedChat,
    policy: &BackendPolicy,
    deadline: Instant,
) -> Result<Response<BackendBody<S>>, BackendError>
where
    S: AsyncRead + AsyncWrite + Unpin + 'static,
{
    exchange(
        stream,
        chat.backend_body(),
        chat.streams_response(),
        "/v1/chat/completions",
        policy,
        deadline,
    )
    .await
}

async fn exchange<S>(
    mut stream: S,
    body: &[u8],
    streaming: bool,
    route: &str,
    policy: &BackendPolicy,
    deadline: Instant,
) -> Result<Response<BackendBody<S>>, BackendError>
where
    S: AsyncRead + AsyncWrite + Unpin + 'static,
{
    let write_deadline = deadline.min(Instant::now() + WRITE_TIMEOUT);
    ensure_time(write_deadline)?;
    let head = format!(
        "POST {route} HTTP/1.1\r\nHost: {}\r\nAuthorization: {}\r\nContent-Type: application/json\r\nAccept-Encoding: identity\r\nConnection: close\r\nContent-Length: {}\r\n\r\n",
        policy.authority,
        policy.authorization,
        body.len()
    );
    timeout_at(write_deadline, async {
        stream.write_all(head.as_bytes()).await?;
        stream.write_all(body).await?;
        stream.flush().await
    })
    .await
    .map_err(|_| BackendError::Deadline)?
    .map_err(|_| BackendError::Io)?;
    ensure_time(write_deadline)?;

    let mut reader = BufReader::with_capacity(8192, stream);
    let mut raw = Vec::with_capacity(MAX_HEADER_BYTES);
    loop {
        ensure_time(deadline)?;
        if raw.len() == MAX_HEADER_BYTES {
            return Err(BackendError::HeadLimit);
        }
        let available = timeout_at(deadline, reader.fill_buf())
            .await
            .map_err(|_| BackendError::Deadline)?
            .map_err(|_| BackendError::Io)?;
        ensure_time(deadline)?;
        if available.is_empty() {
            return Err(BackendError::WireSyntax);
        }
        let count = available.len().min(MAX_HEADER_BYTES - raw.len());
        let previous = raw.len();
        raw.extend_from_slice(&available[..count]);
        if let Some(offset) = raw[previous.saturating_sub(3)..]
            .array_windows()
            .position(|bytes| bytes == b"\r\n\r\n")
        {
            let end = previous.saturating_sub(3) + offset + 4;
            // Leave coalesced body bytes in the owned reader, not the head.
            reader.consume(end - previous);
            raw.truncate(end);
            break;
        }
        reader.consume(count);
    }
    let (metadata, framing) = parse_head(&raw, streaming)?;
    ensure_time(deadline)?;
    Ok(metadata.map(|()| BackendBody {
        reader: Some(reader),
        framing,
        remaining_budget: MAX_RESPONSE_BYTES,
        metadata_bytes: 0,
        deadline,
        timer: Box::pin(sleep_until(deadline)),
    }))
}

fn parse_head(raw: &[u8], streaming: bool) -> Result<(Response<()>, Framing), BackendError> {
    if !raw.starts_with(b"HTTP/1.1 ") || !wire::valid_line_endings(raw) {
        return Err(BackendError::WireSyntax);
    }
    let mut headers = [httparse::EMPTY_HEADER; MAX_HEADERS];
    let mut parsed = httparse::Response::new(&mut headers);
    match parsed.parse(raw) {
        Ok(httparse::Status::Complete(end)) if end == raw.len() => {}
        Err(httparse::Error::TooManyHeaders) => return Err(BackendError::HeadLimit),
        _ => return Err(BackendError::WireSyntax),
    }
    if parsed.version != Some(1) || parsed.code != Some(200) {
        return Err(BackendError::Metadata);
    }
    let mut response = Response::builder()
        .version(Version::HTTP_11)
        .status(200)
        .body(())
        .map_err(|_| BackendError::Metadata)?;
    wire::append_headers(response.headers_mut(), parsed.headers)
        .map_err(|()| BackendError::WireSyntax)?;
    validate_metadata(&response, streaming).map_err(|_| BackendError::Metadata)?;
    let headers = response.headers();
    if headers.contains_key("trailer") || headers.contains_key("upgrade") {
        return Err(BackendError::Framing);
    }
    if let Some(connection) = singleton(headers, "connection").map_err(|_| BackendError::Framing)?
        && !connection.eq_ignore_ascii_case(b"close")
        && !connection.eq_ignore_ascii_case(b"keep-alive")
    {
        return Err(BackendError::Framing);
    }
    let length = singleton(headers, "content-length").map_err(|_| BackendError::Framing)?;
    let transfer = singleton(headers, "transfer-encoding").map_err(|_| BackendError::Framing)?;
    let framing = match (length, transfer) {
        (Some(bytes), None) if !bytes.is_empty() && bytes.iter().all(u8::is_ascii_digit) => {
            let length = bytes
                .iter()
                .try_fold(0usize, |n, b| {
                    n.checked_mul(10)?.checked_add(usize::from(b - b'0'))
                })
                .ok_or(BackendError::Framing)?;
            if length > MAX_RESPONSE_BYTES {
                return Err(BackendError::Body);
            }
            Framing::Length(length)
        }
        (None, Some(bytes)) if bytes.eq_ignore_ascii_case(b"chunked") => Framing::Size {
            value: 0,
            digits: 0,
            cr: false,
        },
        _ => return Err(BackendError::Framing),
    };
    Ok((response, framing))
}

enum Framing {
    Length(usize),
    Size {
        value: usize,
        digits: usize,
        cr: bool,
    },
    Data(usize),
    DataCr,
    DataLf,
    EndCr,
    EndLf,
    Complete,
    Failed,
}

/// A bounded owned HTTP body decoder; raw socket EOF is never framed completion.
///
/// Errors are sticky and close the underlying stream immediately. Framed EOF
/// does not wait for socket EOF; surplus is discarded on Drop, never reused.
/// Chunk extensions/trailers are deliberately unsupported for the pinned server.
pub struct BackendBody<S> {
    reader: Option<BufReader<S>>,
    framing: Framing,
    remaining_budget: usize,
    metadata_bytes: usize,
    deadline: Instant,
    timer: Pin<Box<Sleep>>,
}

impl<S> BackendBody<S> {
    fn fail<T>(&mut self, kind: io::ErrorKind) -> Poll<io::Result<T>> {
        self.framing = Framing::Failed;
        self.reader = None;
        Poll::Ready(Err(io::Error::new(kind, "backend body rejected")))
    }
}

impl<S: AsyncRead + Unpin> BackendBody<S> {
    // Polonius lets success lend the reader's buffer while disjoint failure
    // branches tear down its owner through fail().
    fn poll_available(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<&[u8]>> {
        let Some(reader) = self.reader.as_mut() else {
            return self.fail(io::ErrorKind::InvalidData);
        };
        let Ok(available) = ready!(Pin::new(reader).poll_fill_buf(cx)) else {
            return self.fail(io::ErrorKind::Other);
        };
        // A ready read may cross the deadline after the state-machine check.
        if Instant::now() >= self.deadline {
            return self.fail(io::ErrorKind::TimedOut);
        }
        if available.is_empty() {
            return self.fail(io::ErrorKind::UnexpectedEof);
        }
        Poll::Ready(Ok(available))
    }
}

impl<S: AsyncRead + Unpin> AsyncRead for BackendBody<S> {
    // Keep the complete framing state machine visible in one audit boundary.
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        output: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if output.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        // Bound immediately-ready metadata work so cancellation can be polled.
        for _ in 0..1024 {
            if matches!(this.framing, Framing::Failed) {
                return this.fail(io::ErrorKind::InvalidData);
            }
            if Instant::now() >= this.deadline || this.timer.as_mut().poll(cx).is_ready() {
                return this.fail(io::ErrorKind::TimedOut);
            }
            match this.framing {
                Framing::Length(0) | Framing::Complete => return Poll::Ready(Ok(())),
                Framing::Length(remaining) | Framing::Data(remaining) => {
                    let available = ready!(this.poll_available(cx))?;
                    let count = available.len().min(remaining).min(output.remaining());
                    output.put_slice(&available[..count]);
                    let Some(reader) = this.reader.as_mut() else {
                        return this.fail(io::ErrorKind::InvalidData);
                    };
                    Pin::new(reader).consume(count);
                    this.remaining_budget -= count;
                    this.framing = match this.framing {
                        Framing::Length(_) => Framing::Length(remaining - count),
                        _ if count == remaining => Framing::DataCr,
                        _ => Framing::Data(remaining - count),
                    };
                    return Poll::Ready(Ok(()));
                }
                _ => {}
            }
            let byte = ready!(this.poll_available(cx))?[0];
            let Some(reader) = this.reader.as_mut() else {
                return this.fail(io::ErrorKind::InvalidData);
            };
            Pin::new(reader).consume(1);
            this.metadata_bytes += 1;
            if this.metadata_bytes > MAX_CHUNK_METADATA_BYTES {
                return this.fail(io::ErrorKind::InvalidData);
            }
            this.framing = match this.framing {
                Framing::Size {
                    value,
                    digits: _,
                    cr: true,
                } if byte == b'\n' => {
                    if value > this.remaining_budget {
                        return this.fail(io::ErrorKind::InvalidData);
                    }
                    if value == 0 {
                        Framing::EndCr
                    } else {
                        Framing::Data(value)
                    }
                }
                Framing::Size {
                    value,
                    digits,
                    cr: false,
                } if byte == b'\r' && digits > 0 => Framing::Size {
                    value,
                    digits,
                    cr: true,
                },
                Framing::Size {
                    value,
                    digits,
                    cr: false,
                } if byte.is_ascii_hexdigit() && digits + 3 <= MAX_CHUNK_LINE_BYTES => {
                    let digit = match byte {
                        b'0'..=b'9' => byte - b'0',
                        b'a'..=b'f' => byte - b'a' + 10,
                        _ => byte - b'A' + 10,
                    };
                    let Some(value) = value
                        .checked_mul(16)
                        .and_then(|n| n.checked_add(usize::from(digit)))
                    else {
                        return this.fail(io::ErrorKind::InvalidData);
                    };
                    Framing::Size {
                        value,
                        digits: digits + 1,
                        cr: false,
                    }
                }
                Framing::DataCr if byte == b'\r' => Framing::DataLf,
                Framing::DataLf if byte == b'\n' => Framing::Size {
                    value: 0,
                    digits: 0,
                    cr: false,
                },
                Framing::EndCr if byte == b'\r' => Framing::EndLf,
                Framing::EndLf if byte == b'\n' => Framing::Complete,
                _ => return this.fail(io::ErrorKind::InvalidData),
            };
        }
        cx.waker().wake_by_ref();
        Poll::Pending
    }
}

fn ensure_time(deadline: Instant) -> Result<(), BackendError> {
    if Instant::now() >= deadline {
        Err(BackendError::Deadline)
    } else {
        Ok(())
    }
}
