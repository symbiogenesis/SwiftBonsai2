//! Bounded responses from an already HTTP-decoded backend body.
//!
//! The backend module provides a compatible supplied-stream HTTP decoder. Other
//! callers must bound upstream headers, validate framing, and supply a reader that reports
//! truncated Content-Length/chunked bodies as errors, never clean EOF. Raw socket
//! readers are unsuitable. No upstream headers are forwarded to the client.

use crate::{AdmittedChat, Object, singleton};
use http::{Response, StatusCode};
use serde::de::IgnoredAny;
use std::time::Duration;
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    time::{Instant, timeout_at},
};

pub const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
pub const RESPONSE_BUFFER_BYTES: usize = 8 * 1024;
pub const CLIENT_WRITE_TIMEOUT: Duration = Duration::from_secs(5);

const COMMON_HEADERS: &str =
    "Connection: close\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\n";

/// Value-free errors: no upstream header, body, prompt or I/O diagnostic escapes.
#[derive(Debug, PartialEq, Eq)]
pub enum ResponseError {
    Deadline,
    BackendMetadata,
    BackendIo,
    ClientIo,
    BodyLimit,
    JsonSyntax,
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
/// No [DONE] event is invented. Model text still requires safe client rendering.
///
/// All errors/cancellation drop both endpoints without graceful shutdown. Never
/// retry or append another response after an error: writes may be partial. The
/// future supervisor must monitor real client disconnects and cancel this future
/// while it awaits backend data; AsyncWrite alone cannot detect that disconnect.
pub async fn send_chat_response<W, R>(
    mut client: W,
    backend: Response<R>,
    chat: &AdmittedChat,
    supervisor_deadline: Instant,
) -> Result<(), ResponseError>
where
    W: AsyncWrite + Unpin + 'static,
    R: AsyncRead + Unpin + 'static,
{
    ensure_time(supervisor_deadline)?;
    validate_metadata(&backend, chat.streams_response())?;
    let mut body = backend.into_body();
    if chat.streams_response() {
        let head = format!(
            "HTTP/1.1 200 OK\r\n{COMMON_HEADERS}Content-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n"
        );
        write_group(&mut client, &[head.as_bytes()], supervisor_deadline, false).await?;
        let mut buffer = [0u8; RESPONSE_BUFFER_BYTES];
        let mut sent = 0;
        loop {
            // At the cap, probe one byte to distinguish exact-limit EOF from
            // surplus without forwarding any byte beyond the limit.
            let bound = buffer.len().min(MAX_RESPONSE_BYTES - sent + 1);
            let count = read_before(&mut body, &mut buffer[..bound], supervisor_deadline).await?;
            if count == 0 {
                break;
            }
            if count > MAX_RESPONSE_BYTES - sent {
                return Err(ResponseError::BodyLimit);
            }
            let prefix = format!("{count:X}\r\n");
            write_group(
                &mut client,
                &[prefix.as_bytes(), &buffer[..count], b"\r\n"],
                supervisor_deadline,
                false,
            )
            .await?;
            sent += count;
        }
        // Never emit the terminator on backend error, byte limit or deadline.
        write_group(&mut client, &[b"0\r\n\r\n"], supervisor_deadline, true).await
    } else {
        let bytes = collect_json(&mut body, supervisor_deadline).await?;
        let head = format!(
            "HTTP/1.1 200 OK\r\n{COMMON_HEADERS}Content-Type: application/json\r\nContent-Length: {}\r\n\r\n",
            bytes.len()
        );
        write_group(
            &mut client,
            &[head.as_bytes(), &bytes],
            supervisor_deadline,
            true,
        )
        .await
    }
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
    let mut bytes = Vec::with_capacity(MAX_RESPONSE_BYTES);
    let mut buffer = [0u8; RESPONSE_BUFFER_BYTES];
    loop {
        let bound = buffer.len().min(MAX_RESPONSE_BYTES - bytes.len() + 1);
        let count = read_before(body, &mut buffer[..bound], deadline).await?;
        if count == 0 {
            break;
        }
        if count > MAX_RESPONSE_BYTES - bytes.len() {
            return Err(ResponseError::BodyLimit);
        }
        bytes.extend_from_slice(&buffer[..count]);
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
