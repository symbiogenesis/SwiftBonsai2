//! One-request orchestration over owned client and private backend streams.
//!
//! No listener, process, task, retry or connection reuse is created here. The
//! caller must bound concurrency and enforce immutable backend ownership and
//! bypass prevention before this adapter is suitable for daily use.

use crate::{
    Rejection, RequestPolicy,
    backend::{BackendError, BackendPolicy, count_and_admit, request_chat_owned},
    reception::{
        BudgetedChat, ReceiveError, ReceivedChat, discard_rate_body, receive_chat,
        receive_chat_with_budget,
    },
    response::{
        ResponseError, send_chat_response_mode, send_context_rejection, send_rate_rejection,
    },
    service::BurstBudget,
};
use std::{future::poll_fn, pin::pin, task::Poll, time::Duration};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, split},
    time::{Instant, sleep_until},
};

/// Maximum elapsed time for the entire request, including intake and connection.
pub const REQUEST_TIMEOUT: Duration = Duration::from_mins(10);

/// A connector preserving one fixed private backend endpoint and HTTP policy.
///
/// Both calls for one request must use the same endpoint and separate protected
/// credential. The launcher must additionally establish model/runtime/template
/// identity and prevent backend replacement and bypass; fixed TCP metadata and
/// this trait cannot establish those properties or process isolation.
pub trait BackendConnector {
    /// Owned connection whose Drop closes the backend stream.
    type Stream: AsyncRead + AsyncWrite + Unpin + 'static;

    /// Fixed private backend HTTP metadata, shared by counting and generation.
    fn policy(&self) -> &BackendPolicy;

    /// Connect once before the supplied absolute deadline, without retries.
    ///
    /// Dropping the returned future must close any partially established stream
    /// and cancel pending connection work. No detached task may outlive it.
    fn connect(
        &self,
        deadline: Instant,
    ) -> impl Future<Output = Result<Self::Stream, BackendError>>;
}

/// Value-free lifecycle errors; no prompts, credentials or I/O messages escape.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServeError {
    /// The supplied absolute deadline or total request cap has expired.
    Deadline,
    /// The complete client request could not be received and authorized.
    Receive(ReceiveError),
    /// The private connection, count, admission or generation exchange failed.
    Backend(BackendError),
    /// The bounded response could not be forwarded completely.
    Response(ResponseError),
    /// A post-request read returned EOF, an error or any surplus input byte.
    ClientAbandoned,
}

/// Receive, count, admit and serve exactly one client request.
///
/// Requires a Tokio time driver. The caller supplies owned streams whose Drop
/// closes their connections. A single absolute deadline bounds intake, both
/// backend connections, counting, generation and response forwarding; this is
/// further shortened to [`REQUEST_TIMEOUT`] from the start of this call.
///
/// After the complete body arrives, an actual read races the entire backend
/// pipeline. EOF, read failure and any surplus byte cancel the pipeline and drop
/// both endpoints. A client TCP write-half-close is deliberately treated as
/// abandonment: clients must keep their sending side open until the response
/// finishes. Pipelining and additional requests on this connection are rejected.
///
/// Counting and generation use the same canonical allocation and connector.
/// Generation is never connected until exact-count admission succeeds. Failure
/// or cancellation drops all owned streams immediately, including cancellation
/// while connecting or waiting for backend data. Backend generation/slot release
/// after connection closure must be qualified separately. No response is retried; writes
/// may already be partial. Exact context-limit admission rejection sends a fixed
/// 413 JSON error before generation; other failures do not invent error responses.
///
/// # Errors
/// Returns [`ServeError`] for request rejection, backend/response failure,
/// deadline expiry or client abandonment.
#[allow(
    clippy::future_not_send,
    reason = "the supplied connector and streams may be local; no task is spawned"
)]
pub async fn serve_chat<C>(
    client: C,
    policy: &RequestPolicy,
    backend: &impl BackendConnector,
    deadline: Instant,
) -> Result<(), ServeError>
where
    C: AsyncRead + AsyncWrite + Unpin + 'static,
{
    serve_chat_inner(client, policy, backend, deadline, None).await
}

#[allow(
    clippy::future_not_send,
    reason = "the supplied connector and streams may be local; no task is spawned"
)]
pub(crate) async fn serve_chat_with_budget<C>(
    client: C,
    policy: &RequestPolicy,
    backend: &impl BackendConnector,
    deadline: Instant,
    budget: &BurstBudget,
) -> Result<(), ServeError>
where
    C: AsyncRead + AsyncWrite + Unpin + 'static,
{
    serve_chat_inner(client, policy, backend, deadline, Some(budget)).await
}

#[allow(
    clippy::future_not_send,
    reason = "the supplied connector and streams may be local; no task is spawned"
)]
async fn serve_chat_inner<C>(
    client: C,
    policy: &RequestPolicy,
    backend: &impl BackendConnector,
    deadline: Instant,
    budget: Option<&BurstBudget>,
) -> Result<(), ServeError>
where
    C: AsyncRead + AsyncWrite + Unpin + 'static,
{
    let deadline = deadline.min(Instant::now() + REQUEST_TIMEOUT);
    ensure_time(deadline)?;
    let Some(received) = receive_admitted(client, policy, deadline, budget).await? else {
        return Ok(());
    };
    let (client, pending) = received.into_parts();
    let (mut input, output) = split(client);
    let mut surplus = [0u8; 1];
    let mut abandoned = pin!(input.read(&mut surplus));
    let mut pipeline = Box::pin(async move {
        let counter = backend
            .connect(deadline)
            .await
            .map_err(ServeError::Backend)?;
        let chat = match count_and_admit(counter, pending, backend.policy(), deadline).await {
            Ok(chat) => chat,
            Err(error) => {
                if error == BackendError::Admission(Rejection::ContextLimit) {
                    // No generation connection or client response has started.
                    // Never use this path for a failure after response forwarding.
                    send_context_rejection(output, deadline)
                        .await
                        .map_err(ServeError::Response)?;
                }
                return Err(ServeError::Backend(error));
            }
        };
        let generation = backend
            .connect(deadline)
            .await
            .map_err(ServeError::Backend)?;
        let streaming = chat.streams_response();
        let response = request_chat_owned(generation, chat, backend.policy(), deadline)
            .await
            .map_err(ServeError::Backend)?;
        send_chat_response_mode(output, response, streaming, deadline)
            .await
            .map_err(ServeError::Response)
    });
    let mut timer = pin!(sleep_until(deadline));
    let result = poll_fn(|cx| {
        // Explicit time checks prevent immediately-ready futures from winning
        // after expiry. Abandonment wins ties over successful pipeline progress.
        if Instant::now() >= deadline || timer.as_mut().poll(cx).is_ready() {
            return Poll::Ready(Err(ServeError::Deadline));
        }
        if abandoned.as_mut().poll(cx).is_ready() {
            return Poll::Ready(Err(ServeError::ClientAbandoned));
        }
        pipeline.as_mut().poll(cx)
    })
    .await;
    ensure_time(deadline)?;
    result
}

#[allow(
    clippy::future_not_send,
    reason = "the supplied client may be local; no task is spawned"
)]
async fn receive_admitted<C>(
    client: C,
    policy: &RequestPolicy,
    deadline: Instant,
    budget: Option<&BurstBudget>,
) -> Result<Option<ReceivedChat<C>>, ServeError>
where
    C: AsyncRead + AsyncWrite + Unpin + 'static,
{
    if let Some(budget) = budget {
        match receive_chat_with_budget(client, policy, deadline, budget)
            .await
            .map_err(ServeError::Receive)?
        {
            BudgetedChat::Ready(received) => Ok(Some(received)),
            BudgetedChat::RateLimited {
                mut stream,
                remaining,
                intake_deadline,
            } => {
                send_rate_rejection(&mut stream, intake_deadline)
                    .await
                    .map_err(ServeError::Response)?;
                discard_rate_body(&mut stream, remaining, intake_deadline)
                    .await
                    .map_err(ServeError::Receive)?;
                Ok(None)
            }
            BudgetedChat::Rejected(_client, rejection) => {
                Err(ServeError::Receive(ReceiveError::Policy(rejection)))
            }
        }
    } else {
        receive_chat(client, policy, deadline)
            .await
            .map(Some)
            .map_err(ServeError::Receive)
    }
}

fn ensure_time(deadline: Instant) -> Result<(), ServeError> {
    if Instant::now() >= deadline {
        Err(ServeError::Deadline)
    } else {
        Ok(())
    }
}
