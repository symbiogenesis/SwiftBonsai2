//! Bounded responses from an already HTTP-decoded backend body.
//!
//! The backend module provides a compatible supplied-stream HTTP decoder. Other
//! callers must bound upstream headers, validate framing, and supply a reader that reports
//! truncated Content-Length/chunked bodies as errors, never clean EOF. Raw socket
//! readers are unsuitable. No upstream headers are forwarded to the client.

use crate::{AdmittedChat, Object, singleton};
use http::{Response, StatusCode};
use serde::de::IgnoredAny;
use std::{future::poll_fn, pin::Pin, task::Poll, time::Duration};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf},
    time::{Instant, timeout_at},
};

/// Largest accepted decoded backend response body in bytes.
pub const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
/// Fixed payload buffer size for bounded JSON reads and SSE frames.
pub const RESPONSE_BUFFER_BYTES: usize = 8 * 1024;
/// Maximum elapsed time for writing and flushing one group of response bytes.
pub const CLIENT_WRITE_TIMEOUT: Duration = Duration::from_secs(5);
const CHUNK_PREFIX_BYTES: usize = 2 * size_of::<usize>() + 2;
const MAX_SSE_READS_PER_GROUP: usize = 32;

// Keep one literal for both formatted JSON heads and compile-time SSE heads.
macro_rules! common_headers {
    () => {
        "Connection: close\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\n"
    };
}
const COMMON_HEADERS: &str = common_headers!();

/// Value-free errors: no upstream header, body, prompt or I/O diagnostic escapes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResponseError {
    /// The supervisor or client-write deadline has expired.
    Deadline,
    /// The backend status, encoding or content type is unsupported.
    BackendMetadata,
    /// The supplied decoded backend body failed while being read.
    BackendIo,
    /// The client stream failed while writing, flushing or shutting down.
    ClientIo,
    /// The decoded backend body exceeds the fixed response cap.
    BodyLimit,
    /// A nonstreaming response is not a UTF-8 JSON object.
    JsonSyntax,
}

/// Send a fixed pre-generation context admission error without private values.
/// The caller must not have written any response bytes or begun generation.
pub(crate) async fn send_context_rejection<W: AsyncWrite + Unpin>(
    mut client: W,
    supervisor_deadline: Instant,
) -> Result<(), ResponseError> {
    const BODY: &[u8] = br#"{"error":{"code":"context_length_exceeded"}}"#;
    let head = format!(
        "HTTP/1.1 413 Content Too Large\r\n{COMMON_HEADERS}Content-Type: application/json\r\nContent-Length: {}\r\n\r\n",
        BODY.len()
    );
    write_group(
        &mut client,
        &[head.as_bytes(), BODY],
        supervisor_deadline,
        true,
    )
    .await
}

/// Reject exact model controls in a completely received, authenticated body.
/// The caller must not have contacted counting or generation endpoints.
pub(crate) async fn send_control_rejection<W: AsyncWrite + Unpin>(
    mut client: W,
    supervisor_deadline: Instant,
) -> Result<(), ResponseError> {
    const BODY: &[u8] = br#"{"error":{"code":"input_control_tokens_rejected"}}"#;
    let head = format!(
        "HTTP/1.1 400 Bad Request\r\n{COMMON_HEADERS}Content-Type: application/json\r\nContent-Length: {}\r\n\r\n",
        BODY.len()
    );
    write_group(
        &mut client,
        &[head.as_bytes(), BODY],
        supervisor_deadline,
        true,
    )
    .await
}

/// Send one fixed authenticated burst denial before counting or generation.
/// The fixed one-token-per-second budget needs at most one second to refill.
pub(crate) async fn send_rate_rejection<W: AsyncWrite + Unpin>(
    mut client: W,
    supervisor_deadline: Instant,
) -> Result<(), ResponseError> {
    const BODY: &[u8] = br#"{"error":{"code":"rate_limit_exceeded"}}"#;
    let head = format!(
        "HTTP/1.1 429 Too Many Requests\r\n{COMMON_HEADERS}Retry-After: 1\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
        BODY.len()
    );
    write_group(
        &mut client,
        &[head.as_bytes(), BODY],
        supervisor_deadline,
        true,
    )
    .await
}

/// Send one HTTP/1.1 response, then drop both owned endpoints.
///
/// Requires a Tokio time driver and endpoints whose Drop closes their connection.
/// The lifetime bounds discourage borrowed endpoints but cannot prove Drop
/// behavior. The backend body MUST already be HTTP-decoded with truncated framing
/// reported as an error. Status must be 200 and the singleton Content-Type must
/// match the admitted request's stream setting. Content-Encoding is unsupported.
///
/// JSON is buffered within a fixed cap and checked for object syntax before any
/// write. SSE bytes are streamed unchanged with chunked HTTP framing; clean EOF
/// finishes the HTTP body but does not prove successful generation or valid SSE.
/// No `[DONE]` event is invented. Model text still requires safe client rendering.
///
/// All errors/cancellation drop both endpoints without graceful shutdown. Never
/// retry or append another response after an error: writes may be partial. The
/// future supervisor must monitor real client disconnects and cancel this future
/// while it awaits backend data; [`AsyncWrite`] alone cannot detect that disconnect.
///
/// # Errors
/// Returns [`ResponseError`] for an expired deadline, invalid response metadata,
/// body limit, JSON syntax failure or backend/client I/O failure. Client writes
/// may already be partial; callers must close the endpoints without retrying.
pub async fn send_chat_response<W, R>(
    client: W,
    backend: Response<R>,
    chat: &AdmittedChat,
    supervisor_deadline: Instant,
) -> Result<(), ResponseError>
where
    W: AsyncWrite + Unpin + 'static,
    R: AsyncRead + Unpin + 'static,
{
    send_chat_response_mode(
        client,
        backend,
        chat.streams_response(),
        supervisor_deadline,
    )
    .await
}

pub(crate) async fn send_chat_response_mode<W, R>(
    mut client: W,
    backend: Response<R>,
    streaming: bool,
    supervisor_deadline: Instant,
) -> Result<(), ResponseError>
where
    W: AsyncWrite + Unpin + 'static,
    R: AsyncRead + Unpin + 'static,
{
    ensure_time(supervisor_deadline)?;
    validate_metadata(&backend, streaming)?;
    let mut body = backend.into_body();
    if streaming {
        write_sse_response(&mut client, &mut body, supervisor_deadline).await
    } else {
        write_json_response(&mut client, &mut body, supervisor_deadline).await
    }
}

async fn write_sse_response<W: AsyncWrite + Unpin, R: AsyncRead + Unpin>(
    client: &mut W,
    body: &mut R,
    supervisor_deadline: Instant,
) -> Result<(), ResponseError> {
    let head = concat!(
        "HTTP/1.1 200 OK\r\n",
        common_headers!(),
        "Content-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n"
    );
    write_group(client, &[head.as_bytes()], supervisor_deadline, false).await?;
    // Read into the final wire frame: no per-chunk String allocation or
    // separate writes for the size, payload and CRLF.
    let mut buffer = [0u8; CHUNK_PREFIX_BYTES + RESPONSE_BUFFER_BYTES + 2];
    let mut sent = 0;
    loop {
        // At the cap, probe one byte to distinguish exact-limit EOF from
        // surplus without forwarding any byte beyond the limit.
        let bound = RESPONSE_BUFFER_BYTES.min(MAX_RESPONSE_BYTES - sent + 1);
        let (count, status) = read_sse_group(
            body,
            &mut buffer[CHUNK_PREFIX_BYTES..CHUNK_PREFIX_BYTES + bound],
            supervisor_deadline,
        )
        .await?;
        if count == 0 {
            break;
        }
        if count > MAX_RESPONSE_BYTES - sent {
            return Err(ResponseError::BodyLimit);
        }
        buffer[CHUNK_PREFIX_BYTES - 2..CHUNK_PREFIX_BYTES].copy_from_slice(b"\r\n");
        let mut first = CHUNK_PREFIX_BYTES - 2;
        let mut size = count;
        while size != 0 {
            first -= 1;
            buffer[first] = b"0123456789ABCDEF"[size & 0xf];
            size >>= 4;
        }
        let end = CHUNK_PREFIX_BYTES + count;
        buffer[end..end + 2].copy_from_slice(b"\r\n");
        write_group(
            client,
            &[&buffer[first..end + 2]],
            supervisor_deadline,
            false,
        )
        .await?;
        sent += count;
        match status {
            SseReadStatus::Open => {}
            SseReadStatus::Complete => break,
            SseReadStatus::Failed => return Err(ResponseError::BackendIo),
        }
    }
    // Never emit the terminator on backend error, byte limit or deadline.
    write_group(client, &[b"0\r\n\r\n"], supervisor_deadline, true).await
}

enum SseReadStatus {
    Open,
    Complete,
    Failed,
}

// Wait only for the first payload. Coalesce subsequent immediately-ready reads
// within the existing wire buffer, with a finite poll count and no delay timer.
// If read-ahead discovers an error, deliver the valid prefix before reporting it;
// the caller must omit successful terminal framing and close both endpoints.
async fn read_sse_group<R: AsyncRead + Unpin>(
    body: &mut R,
    buffer: &mut [u8],
    deadline: Instant,
) -> Result<(usize, SseReadStatus), ResponseError> {
    let mut count = read_before(body, buffer, deadline).await?;
    if count == 0 {
        return Ok((0, SseReadStatus::Complete));
    }
    for _ in 1..MAX_SSE_READS_PER_GROUP {
        if count == buffer.len() {
            break;
        }
        ensure_time(deadline)?;
        let ready = poll_fn(|cx| {
            let mut output = ReadBuf::new(&mut buffer[count..]);
            Poll::Ready(match Pin::new(&mut *body).poll_read(cx, &mut output) {
                Poll::Pending => None,
                Poll::Ready(result) => Some(result.map(|()| output.filled().len())),
            })
        })
        .await;
        ensure_time(deadline)?;
        match ready {
            None => break,
            Some(Ok(0)) => return Ok((count, SseReadStatus::Complete)),
            Some(Ok(read)) => count += read,
            Some(Err(_)) => return Ok((count, SseReadStatus::Failed)),
        }
    }
    Ok((count, SseReadStatus::Open))
}

async fn write_json_response<W: AsyncWrite + Unpin, R: AsyncRead + Unpin>(
    client: &mut W,
    body: &mut R,
    supervisor_deadline: Instant,
) -> Result<(), ResponseError> {
    let bytes = collect_json(body, supervisor_deadline).await?;
    let head = format!(
        "HTTP/1.1 200 OK\r\n{COMMON_HEADERS}Content-Type: application/json\r\nContent-Length: {}\r\n\r\n",
        bytes.len()
    );
    write_group(
        client,
        &[head.as_bytes(), &bytes],
        supervisor_deadline,
        true,
    )
    .await
}

pub(crate) fn validate_metadata<R>(
    backend: &Response<R>,
    streaming: bool,
) -> Result<(), ResponseError> {
    if backend.status() != StatusCode::OK || backend.headers().contains_key("content-encoding") {
        return Err(ResponseError::BackendMetadata);
    }
    let content_type = singleton(backend.headers(), "content-type")
        .map_err(|_| ResponseError::BackendMetadata)?
        .ok_or(ResponseError::BackendMetadata)?;
    let expected: &[u8] = if streaming {
        b"text/event-stream"
    } else {
        b"application/json"
    };
    let Some(media) = content_type.get(..expected.len()) else {
        return Err(ResponseError::BackendMetadata);
    };
    let parameters = &content_type[expected.len()..];
    if !media.eq_ignore_ascii_case(expected)
        || !(parameters.is_empty() || parameters.eq_ignore_ascii_case(b"; charset=utf-8"))
    {
        return Err(ResponseError::BackendMetadata);
    }
    Ok(())
}

async fn collect_json<R: AsyncRead + Unpin>(
    body: &mut R,
    deadline: Instant,
) -> Result<Vec<u8>, ResponseError> {
    // Read into the final allocation's spare capacity without staging a copy or
    // initializing bytes that the reader will immediately overwrite. Reserve
    // explicitly so read_buf never supplies Vec's unbounded growth policy.
    let mut bytes = Vec::new();
    loop {
        if bytes.len() == MAX_RESPONSE_BYTES {
            // Keep the exact-limit overflow probe outside the accepted storage.
            if read_before(body, &mut [0u8; 1], deadline).await? != 0 {
                return Err(ResponseError::BodyLimit);
            }
            break;
        }
        ensure_time(deadline)?;
        if bytes.len() == bytes.capacity() {
            let target = bytes
                .capacity()
                .saturating_mul(2)
                .clamp(RESPONSE_BUFFER_BYTES, MAX_RESPONSE_BYTES);
            bytes
                .try_reserve_exact(target - bytes.len())
                .map_err(|_| ResponseError::BodyLimit)?;
        }
        let bound = RESPONSE_BUFFER_BYTES.min(MAX_RESPONSE_BYTES - bytes.len());
        let limit = u64::try_from(bound).map_err(|_| ResponseError::BodyLimit)?;
        let mut bounded = (&mut *body).take(limit);
        let count = timeout_at(deadline, bounded.read_buf(&mut bytes))
            .await
            .map_err(|_| ResponseError::Deadline)?
            .map_err(|_| ResponseError::BackendIo)?;
        ensure_time(deadline)?;
        if count == 0 {
            break;
        }
    }
    // Check UTF-8 explicitly: IgnoredAny need not materialize string values.
    std::str::from_utf8(&bytes).map_err(|_| ResponseError::JsonSyntax)?;
    serde_json::from_slice::<Object<IgnoredAny>>(&bytes).map_err(|_| ResponseError::JsonSyntax)?;
    // Parsing is bounded synchronous work, not a preemptive CPU deadline.
    ensure_time(deadline)?;
    Ok(bytes)
}

async fn read_before<R: AsyncRead + Unpin>(
    body: &mut R,
    buffer: &mut [u8],
    deadline: Instant,
) -> Result<usize, ResponseError> {
    ensure_time(deadline)?;
    let count = timeout_at(deadline, body.read(buffer))
        .await
        .map_err(|_| ResponseError::Deadline)?
        .map_err(|_| ResponseError::BackendIo)?;
    ensure_time(deadline)?;
    Ok(count)
}

async fn write_group<W: AsyncWrite + Unpin>(
    client: &mut W,
    pieces: &[&[u8]],
    supervisor_deadline: Instant,
    shutdown: bool,
) -> Result<(), ResponseError> {
    // One deadline covers the whole group, including all partial writes, flush
    // and (on success only) shutdown. No helper restarts it between pieces.
    let deadline = supervisor_deadline.min(Instant::now() + CLIENT_WRITE_TIMEOUT);
    ensure_time(deadline)?;
    timeout_at(deadline, async {
        for piece in pieces {
            client.write_all(piece).await?;
        }
        client.flush().await?;
        if shutdown {
            client.shutdown().await?;
        }
        Ok::<_, std::io::Error>(())
    })
    .await
    .map_err(|_| ResponseError::Deadline)?
    .map_err(|_| ResponseError::ClientIo)?;
    // timeout_at allows an immediately ready operation to win after expiry.
    ensure_time(deadline)
}

fn ensure_time(deadline: Instant) -> Result<(), ResponseError> {
    if Instant::now() >= deadline {
        Err(ResponseError::Deadline)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod memory_tests;
