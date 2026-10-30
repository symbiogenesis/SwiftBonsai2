//! Untimed authorized request-storage, framing and cancellation regressions.

use super::*;
use crate::MAX_BODY_BYTES;
use std::{
    cell::RefCell,
    future::poll_fn,
    io,
    pin::Pin,
    rc::Rc,
    task::{Context, Poll},
};
use tokio::io::ReadBuf;

const KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const CHAT: &[u8] = br#"{"messages":[{"role":"user","content":"Keep the rain."}]}"#;

struct Read {
    position: usize,
    destination: *const u8,
    initialized: usize,
}

#[derive(Default)]
struct State {
    consumed: usize,
    reads: Vec<Read>,
    dropped: bool,
}

struct Source {
    bytes: Vec<u8>,
    fragment: usize,
    stall_at_end: bool,
    state: Rc<RefCell<State>>,
}

impl Source {
    fn new(bytes: Vec<u8>, fragment: usize) -> Self {
        Self {
            bytes,
            fragment,
            stall_at_end: false,
            state: Rc::default(),
        }
    }
}

impl AsyncRead for Source {
    fn poll_read(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        output: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let mut state = self.state.borrow_mut();
        let position = state.consumed;
        state.reads.push(Read {
            position,
            destination: output.filled().as_ptr(),
            initialized: output.initialized().len(),
        });
        if position == self.bytes.len() && self.stall_at_end {
            return Poll::Pending;
        }
        let count = output
            .remaining()
            .min(self.fragment)
            .min(self.bytes.len() - position);
        output.put_slice(&self.bytes[position..position + count]);
        state.consumed += count;
        Poll::Ready(Ok(()))
    }
}

impl Drop for Source {
    fn drop(&mut self) {
        self.state.borrow_mut().dropped = true;
    }
}

fn head(length: usize) -> String {
    format!(
        "POST /v1/chat/completions HTTP/1.1\r\nHost: 127.0.0.1:8080\r\nAuthorization: Bearer {KEY}\r\nContent-Type: application/json\r\nContent-Length: {length}\r\n\r\n"
    )
}

fn wire(length: usize) -> Vec<u8> {
    let mut bytes = head(length).into_bytes();
    bytes.extend_from_slice(CHAT);
    bytes.resize(bytes.len() + length - CHAT.len(), b' ');
    bytes
}

fn runtime() -> io::Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
}

fn policy() -> io::Result<RequestPolicy> {
    RequestPolicy::new(8080, KEY).map_err(|_| io::Error::other("Invalid fixture policy."))
}

fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(60)
}

#[test]
fn authorized_body_storage_reuses_header_capacity_and_keeps_only_received_payload() -> io::Result<()>
{
    let mut raw = vec![b'h'; MAX_HEADER_BYTES].into_boxed_slice();
    let allocation = raw.as_ptr();
    let prefix = "rain\n雪🌱".as_bytes();
    let end = 333;
    let used = end + prefix.len();
    raw[end..used].copy_from_slice(prefix);
    // This body outgrows the space beside its head but fits the same allocation
    // after compaction. No second allocation or whole-body initialization.
    let length = MAX_HEADER_BYTES - end + 1;
    let bytes = body_buffer(raw, end, used, length)
        .map_err(|_| io::Error::other("Fixture body storage was rejected."))?;
    assert_eq!(bytes.as_ptr(), allocation);
    assert_eq!(bytes, prefix);
    assert_eq!(bytes.capacity(), MAX_HEADER_BYTES);
    // Capacity overflow is deterministic and returns a value-free error.
    assert_eq!(
        body_buffer(vec![0; 16].into_boxed_slice(), 8, 16, usize::MAX),
        Err(ReceiveError::Storage)
    );
    Ok(())
}

#[test]
fn large_request_reads_final_spare_capacity_without_initializing_or_overreading() -> io::Result<()>
{
    runtime()?.block_on(async {
        for length in [
            MAX_HEADER_BYTES - head(MAX_HEADER_BYTES).len() + 1,
            MAX_HEADER_BYTES + 1,
            MAX_BODY_BYTES,
        ] {
            for fragment in [
                head(length).len(),
                head(length).len() + 11,
                4093,
                MAX_HEADER_BYTES,
            ] {
                let mut bytes = wire(length);
                let request_length = bytes.len();
                bytes.extend_from_slice(b"GET /admin HTTP/1.1\r\n\r\n");
                let received = receive_chat(Source::new(bytes, fragment), &policy()?, deadline())
                    .await
                    .map_err(|_| io::Error::other("Fixture request was rejected."))?;
                let (source, pending) = received.into_parts();
                let admitted = pending
                    .admit(|_| Ok(100))
                    .map_err(|_| io::Error::other("Fixture token admission failed."))?;
                let canonical: serde_json::Value = serde_json::from_slice(admitted.backend_body())?;
                assert_eq!(canonical["messages"][0]["content"], "Keep the rain.");
                let state = source.state.borrow();
                assert_eq!(state.consumed, request_length);
                assert!(state.reads.len() > 1);
                assert_eq!(state.reads[0].initialized, MAX_HEADER_BYTES);
                let first = &state.reads[1];
                for read in &state.reads[1..] {
                    assert_eq!(read.initialized, 0);
                    assert_eq!(
                        read.destination,
                        first
                            .destination
                            .wrapping_add(read.position - first.position)
                    );
                }
            }
        }
        Ok(())
    })
}

#[test]
fn large_request_storage_requires_complete_body_and_releases_on_cancel() -> io::Result<()> {
    runtime()?.block_on(async {
        let length = MAX_HEADER_BYTES + 1;
        let mut truncated = wire(length);
        truncated.pop();
        let source = Source::new(truncated.clone(), 4093);
        let state = Rc::clone(&source.state);
        assert!(matches!(
            receive_chat(source, &policy()?, deadline()).await,
            Err(ReceiveError::Incomplete)
        ));
        assert!(state.borrow().dropped);

        let mut stalled = Source::new(truncated.clone(), 4093);
        stalled.stall_at_end = true;
        assert!(matches!(
            receive_chat(stalled, &policy()?, deadline()).await,
            Err(ReceiveError::Deadline)
        ));

        let mut cancelled = Source::new(truncated, 4093);
        cancelled.stall_at_end = true;
        let state = Rc::clone(&cancelled.state);
        let policy = policy()?;
        let mut operation = Box::pin(receive_chat(cancelled, &policy, deadline()));
        poll_fn(|cx| {
            assert!(operation.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
        assert!(!state.borrow().dropped);
        drop(operation);
        assert!(state.borrow().dropped);
        Ok(())
    })
}
