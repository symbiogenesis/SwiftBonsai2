//! One-request HTTP/1.1 reception, without binding a socket or sending a response.
//!
//! The supervisor must bound accepted connections before calling this adapter,
//! supply its total request deadline, and use an owned stream whose Drop closes
//! the connection. No background task or borrowed stream is retained here.

use crate::{
    AuthorizedHead, MAX_HEADER_BYTES, MAX_HEADERS, PendingChat, Rejection, RequestPolicy,
    service::BurstBudget, wire,
};
use http::{Method, Request, Version, uri::PathAndQuery};
use std::time::Duration;
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    time::{Instant, timeout_at},
};

/// Maximum elapsed time for receiving and authorizing the request headers.
pub const HEADER_TIMEOUT: Duration = Duration::from_secs(5);
/// Maximum elapsed time for receiving and validating the complete request.
pub const INTAKE_TIMEOUT: Duration = Duration::from_secs(30);
const CHAT_ROUTE: PathAndQuery = PathAndQuery::from_static("/v1/chat/completions");

/// Value-free errors: no raw headers, credential, body or I/O diagnostic escapes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReceiveError {
    /// The header, intake or supervisor deadline has expired.
    Deadline,
    /// The supplied stream failed during reception.
    Io,
    /// The stream ended before the complete request arrived.
    Incomplete,
    /// The header block exceeded the byte or header-count limit.
    HeadLimit,
    /// The request has unsupported HTTP/1.1 wire syntax.
    WireSyntax,
    /// Bytes beyond the declared body arrived with the headers.
    SurplusBytes,
    /// The parsed request was rejected by the request policy.
    Policy(Rejection),
}

/// No Debug implementation: this contains private input and an owned stream.
pub struct ReceivedChat<R> {
    stream: R,
    chat: PendingChat,
}

pub(crate) enum BudgetedChat<R> {
    Ready(ReceivedChat<R>),
    RateLimited {
        stream: R,
        remaining: usize,
        intake_deadline: Instant,
    },
    Rejected(R, Rejection),
}

struct ReceivedHead<R> {
    stream: R,
    authorized: AuthorizedHead,
    raw: Box<[u8]>,
    used: usize,
    head_end: usize,
    intake_deadline: Instant,
}

impl<R> ReceivedChat<R> {
    /// The response adapter must send one response with Connection: close,
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
/// authorization; this fixed buffer also receives bodies that fit beside the
/// head. A separate body of at most 1 MiB is allocated only after authorization.
/// Duplicate headers reach [`RequestPolicy`] unchanged in multiplicity. No chunked
/// decoding, trailing requests, connection reuse, listener or production token
/// counter is provided.
/// Later surplus bytes are left unread and must be discarded when closing.
///
/// # Errors
/// Returns [`ReceiveError`] for an expired deadline, stream failure, incomplete
/// or oversized headers/body, invalid wire syntax, surplus bytes or policy rejection.
pub async fn receive_chat<R: AsyncRead + Unpin + 'static>(
    stream: R,
    policy: &RequestPolicy,
    supervisor_deadline: Instant,
) -> Result<ReceivedChat<R>, ReceiveError> {
    let head = receive_head(stream, policy, supervisor_deadline).await?;
    let (stream, chat) = receive_body(head).await?;
    Ok(ReceivedChat {
        stream,
        chat: chat.map_err(ReceiveError::Policy)?,
    })
}

pub(crate) async fn receive_chat_with_budget<R: AsyncRead + Unpin + 'static>(
    stream: R,
    policy: &RequestPolicy,
    supervisor_deadline: Instant,
    budget: &BurstBudget,
) -> Result<BudgetedChat<R>, ReceiveError> {
    let head = receive_head(stream, policy, supervisor_deadline).await?;
    if budget.take(Instant::now()).is_err() {
        return Ok(BudgetedChat::RateLimited {
            stream: head.stream,
            remaining: head.authorized.length - (head.used - head.head_end),
            intake_deadline: head.intake_deadline,
        });
    }
    let (stream, chat) = receive_body(head).await?;
    Ok(match chat {
        Ok(chat) => BudgetedChat::Ready(ReceivedChat { stream, chat }),
        Err(rejection) => BudgetedChat::Rejected(stream, rejection),
    })
}

/// Finish receiving only the already bounded, rejected body after write-half
/// shutdown. This avoids resetting a write-before-read TCP client on close.
pub(crate) async fn discard_rate_body<R: AsyncRead + Unpin>(
    stream: &mut R,
    mut remaining: usize,
    intake_deadline: Instant,
) -> Result<(), ReceiveError> {
    let mut scratch = [0u8; 4096];
    while remaining != 0 {
        let limit = remaining.min(scratch.len());
        match read_before(stream, &mut scratch[..limit], intake_deadline).await {
            Ok(count) => remaining -= count,
            // A client which read the denial/FIN may close without sending its
            // body. No accepted request or backend work depends on those bytes.
            Err(ReceiveError::Incomplete) => break,
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

async fn receive_head<R: AsyncRead + Unpin>(
    mut stream: R,
    policy: &RequestPolicy,
    supervisor_deadline: Instant,
) -> Result<ReceivedHead<R>, ReceiveError> {
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
        let previous = used;
        used += read_before(&mut stream, &mut raw[used..], header_deadline).await?;
        if let Some(end) = wire::head_end(&raw[..used], previous) {
            break end;
        }
    };
    let authorized = parse_head(policy, &raw[..head_end])?;
    ensure_time(header_deadline)?;

    let length = authorized.length;
    let prefix_length = used - head_end;
    if prefix_length > length {
        return Err(ReceiveError::SurplusBytes);
    }
    Ok(ReceivedHead {
        stream,
        authorized,
        raw,
        used,
        head_end,
        intake_deadline,
    })
}

async fn receive_body<R: AsyncRead + Unpin>(
    head: ReceivedHead<R>,
) -> Result<(R, Result<PendingChat, Rejection>), ReceiveError> {
    let ReceivedHead {
        mut stream,
        authorized,
        mut raw,
        mut used,
        head_end,
        intake_deadline,
    } = head;
    let length = authorized.length;
    let prefix_length = used - head_end;
    let chat = if length <= raw.len() - head_end {
        // Reuse the head buffer for small bodies, even when fragmented across
        // reads. Its coalesced prefix is already in place, so no body allocation
        // or prefix copy is needed. Never read beyond the declared body.
        let body_end = head_end + length;
        while used < body_end {
            used += read_before(&mut stream, &mut raw[used..body_end], intake_deadline).await?;
        }
        ensure_time(intake_deadline)?;
        authorized.prepare(&raw[head_end..body_end])
    } else {
        let mut body = vec![0u8; length];
        body[..prefix_length].copy_from_slice(&raw[head_end..used]);
        let mut filled = prefix_length;
        drop(raw);
        while filled < length {
            filled += read_before(&mut stream, &mut body[filled..], intake_deadline).await?;
        }
        ensure_time(intake_deadline)?;
        authorized.prepare(&body)
    };
    ensure_time(intake_deadline)?;
    Ok((stream, chat))
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
    if raw.starts_with(b"\r\n") || !wire::valid_line_endings(raw) {
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
    if parsed.method != Some("POST") || parsed.path != Some(CHAT_ROUTE.as_str()) {
        return Err(ReceiveError::Policy(Rejection::Route));
    }
    let mut request = Request::builder()
        .method(Method::POST)
        .uri(CHAT_ROUTE)
        .version(Version::HTTP_11)
        .body(())
        .map_err(|_| ReceiveError::WireSyntax)?;
    wire::append_headers(request.headers_mut(), parsed.headers)
        .map_err(|()| ReceiveError::WireSyntax)?;
    policy
        .check_head(&request.into_parts().0)
        .map_err(ReceiveError::Policy)
}
