//! One-request HTTP/1.1 reception, without binding a socket or sending a response.
//!
//! The supervisor must bound accepted connections before calling this adapter,
//! supply its total request deadline, and use an owned stream whose Drop closes
//! the connection. No background task or borrowed stream is retained here.

use crate::{AuthorizedHead, MAX_HEADER_BYTES, MAX_HEADERS, PendingChat, Rejection, RequestPolicy};
use http::{HeaderName, HeaderValue, Request, Version};
use std::time::Duration;
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    time::{Instant, timeout_at},
};

pub const HEADER_TIMEOUT: Duration = Duration::from_secs(5);
pub const INTAKE_TIMEOUT: Duration = Duration::from_secs(30);

/// Value-free errors: no raw headers, credential, body or I/O diagnostic escapes.
#[derive(Debug, PartialEq, Eq)]
pub enum ReceiveError {
    Deadline,
    Io,
    Incomplete,
    HeadLimit,
    WireSyntax,
    SurplusBytes,
    Policy(Rejection),
}

/// No Debug implementation: this contains private input and an owned stream.
pub struct ReceivedChat<R> {
    stream: R,
    chat: PendingChat,
}

impl<R> ReceivedChat<R> {
    /// The future response adapter must send one response with Connection: close,
    /// then drop the stream. Never reuse it for another request. Exact token
    /// admission is still required before contacting the generation endpoint.
    pub fn into_parts(self) -> (R, PendingChat) {
        (self.stream, self.chat)
    }
}

/// Receive one request under absolute header/intake/supervisor deadlines.
///
/// Requires a Tokio time driver. Takes ownership so errors and cancellation drop
/// the stream. At most 16 KiB (including line/whitespace) is read before head
/// authorization; this fixed buffer can contain a coalesced body prefix. Only
/// after authorization is a body of at most 1 MiB allocated. Duplicate headers
/// reach RequestPolicy unchanged in multiplicity. No chunked decoding, trailing
/// requests, connection reuse, listener or production token counter is provided.
/// Later surplus bytes are left unread and must be discarded when closing.
pub async fn receive_chat<R: AsyncRead + Unpin + 'static>(
    mut stream: R,
    policy: &RequestPolicy,
    supervisor_deadline: Instant,
) -> Result<ReceivedChat<R>, ReceiveError> {
    let start = Instant::now();
    let intake_deadline = supervisor_deadline.min(start + INTAKE_TIMEOUT);
    let header_deadline = intake_deadline.min(start + HEADER_TIMEOUT);
    ensure_time(header_deadline)?;

    let mut raw = vec![0u8; MAX_HEADER_BYTES].into_boxed_slice();
    let mut used = 0;
    let head_end = loop {
        if used == raw.len() {
            return Err(ReceiveError::HeadLimit);
        }
        let scan_from = used.saturating_sub(3);
        used += read_before(&mut stream, &mut raw[used..], header_deadline).await?;
        // Scan each byte at most four times, even with one-byte fragments.
        if let Some(offset) = raw[scan_from..used]
            .windows(4)
            .position(|bytes| bytes == b"\r\n\r\n")
        {
            break scan_from + offset + 4;
        }
    };
    let authorized = parse_head(policy, &raw[..head_end])?;
    ensure_time(header_deadline)?;

    let length = authorized.length;
    let prefix = &raw[head_end..used];
    if prefix.len() > length {
        return Err(ReceiveError::SurplusBytes);
    }
    let mut body = vec![0u8; length];
    body[..prefix.len()].copy_from_slice(prefix);
    let mut filled = prefix.len();
    drop(raw);
    while filled < length {
        filled += read_before(&mut stream, &mut body[filled..], intake_deadline).await?;
    }
    ensure_time(intake_deadline)?;
    let chat = authorized.prepare(&body).map_err(ReceiveError::Policy)?;
    ensure_time(intake_deadline)?;
    Ok(ReceivedChat { stream, chat })
}

async fn read_before<R: AsyncRead + Unpin>(
    stream: &mut R,
    buffer: &mut [u8],
    deadline: Instant,
) -> Result<usize, ReceiveError> {
    ensure_time(deadline)?;
    let count = timeout_at(deadline, stream.read(buffer))
        .await
        .map_err(|_| ReceiveError::Deadline)?
        .map_err(|_| ReceiveError::Io)?;
    // timeout_at allows an immediately ready future to win after expiry.
    ensure_time(deadline)?;
    if count == 0 {
        return Err(ReceiveError::Incomplete);
    }
    Ok(count)
}

fn ensure_time(deadline: Instant) -> Result<(), ReceiveError> {
    if Instant::now() >= deadline {
        Err(ReceiveError::Deadline)
    } else {
        Ok(())
    }
}

fn parse_head(policy: &RequestPolicy, raw: &[u8]) -> Result<AuthorizedHead, ReceiveError> {
    // httparse permits leading blank lines and bare LF. This adapter does not.
    if raw.starts_with(b"\r\n")
        || raw.iter().enumerate().any(|(i, byte)| match byte {
            b'\r' => raw.get(i + 1) != Some(&b'\n'),
            b'\n' => i == 0 || raw[i - 1] != b'\r',
            _ => false,
        })
    {
        return Err(ReceiveError::WireSyntax);
    }
    let mut headers = [httparse::EMPTY_HEADER; MAX_HEADERS];
    let mut parsed = httparse::Request::new(&mut headers);
    match parsed.parse(raw) {
        Ok(httparse::Status::Complete(end)) if end == raw.len() => {}
        Err(httparse::Error::TooManyHeaders) => return Err(ReceiveError::HeadLimit),
        _ => return Err(ReceiveError::WireSyntax),
    }
    if parsed.version != Some(1) {
        return Err(ReceiveError::WireSyntax);
    }
    // http::Uri can discard a fragment. Check the original target before any
    // normalization so an unsupported wire route cannot become the allowed one.
    if parsed.method != Some("POST") || parsed.path != Some("/v1/chat/completions") {
        return Err(ReceiveError::Policy(Rejection::Route));
    }
    let mut request = Request::builder()
        .method(parsed.method.ok_or(ReceiveError::WireSyntax)?)
        .uri(parsed.path.ok_or(ReceiveError::WireSyntax)?)
        .version(Version::HTTP_11)
        .body(())
        .map_err(|_| ReceiveError::WireSyntax)?;
    for header in parsed.headers {
        let name =
            HeaderName::from_bytes(header.name.as_bytes()).map_err(|_| ReceiveError::WireSyntax)?;
        let value = HeaderValue::from_bytes(header.value).map_err(|_| ReceiveError::WireSyntax)?;
        // insert() would hide duplicate critical headers, including identical CL.
        request.headers_mut().append(name, value);
    }
    policy
        .check_head(&request.into_parts().0)
        .map_err(ReceiveError::Policy)
}
