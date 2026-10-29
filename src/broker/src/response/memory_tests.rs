//! Untimed final-storage and framing regressions for buffered JSON responses.

use super::*;
use std::{cell::RefCell, io, rc::Rc, task::Context};

struct Read {
    destination: *const u8,
    offset: usize,
    remaining: usize,
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
    fail_at_end: bool,
    stall_at_end: bool,
    state: Rc<RefCell<State>>,
}

impl Source {
    fn new(bytes: Vec<u8>, fragment: usize) -> Self {
        Self {
            bytes,
            fragment,
            fail_at_end: false,
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
        let offset = state.consumed;
        state.reads.push(Read {
            destination: output.filled().as_ptr(),
            offset,
            remaining: output.remaining(),
            initialized: output.initialized().len(),
        });
        if offset == self.bytes.len() {
            return if self.fail_at_end {
                Poll::Ready(Err(io::ErrorKind::UnexpectedEof.into()))
            } else if self.stall_at_end {
                Poll::Pending
            } else {
                Poll::Ready(Ok(()))
            };
        }
        let count = output
            .remaining()
            .min(self.fragment)
            .min(self.bytes.len() - offset);
        output.put_slice(&self.bytes[offset..offset + count]);
        state.consumed += count;
        Poll::Ready(Ok(()))
    }
}

impl Drop for Source {
    fn drop(&mut self) {
        self.state.borrow_mut().dropped = true;
    }
}

fn json(length: usize) -> Vec<u8> {
    let mut bytes = vec![b' '; length];
    bytes[..2].copy_from_slice(b"{}");
    bytes
}

fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(60)
}

fn runtime() -> io::Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
}

#[test]
fn json_partial_reads_fill_final_storage_without_initializing_the_tail() -> io::Result<()> {
    runtime()?.block_on(async {
        let expected = json(RESPONSE_BUFFER_BYTES - 3);
        let mut source = Source::new(expected.clone(), 17);
        let bytes = collect_json(&mut source, deadline())
            .await
            .map_err(|_| io::Error::other("Fixture JSON was rejected."))?;
        assert_eq!(bytes, expected);
        assert!(bytes.capacity() <= MAX_RESPONSE_BYTES);
        let state = source.state.borrow();
        assert!(state.reads.len() > 2);
        for read in &state.reads {
            assert_eq!(read.destination, bytes.as_ptr().wrapping_add(read.offset));
            assert_eq!(read.initialized, 0);
            assert!(read.remaining <= RESPONSE_BUFFER_BYTES);
        }
        Ok(())
    })
}

#[test]
fn json_storage_growth_preserves_bytes_and_probes_the_exact_limit_once() -> io::Result<()> {
    runtime()?.block_on(async {
        for length in [
            RESPONSE_BUFFER_BYTES,
            RESPONSE_BUFFER_BYTES + 1,
            MAX_RESPONSE_BYTES,
        ] {
            let expected = json(length);
            let mut source = Source::new(expected.clone(), 4093);
            let bytes = collect_json(&mut source, deadline())
                .await
                .map_err(|_| io::Error::other("Fixture JSON was rejected."))?;
            assert_eq!(bytes, expected);
            assert!(bytes.capacity() <= MAX_RESPONSE_BYTES);
            let state = source.state.borrow();
            assert_eq!(state.consumed, length);
            assert!(state.reads.iter().all(|read| {
                read.remaining <= RESPONSE_BUFFER_BYTES
                    && read.remaining <= MAX_RESPONSE_BYTES - read.offset + 1
            }));
            if length == MAX_RESPONSE_BYTES {
                let probes: Vec<_> = state
                    .reads
                    .iter()
                    .filter(|read| read.offset == MAX_RESPONSE_BYTES)
                    .collect();
                assert_eq!(probes.len(), 1);
                assert_eq!(probes[0].remaining, 1);
            }
        }
        let mut source = Source::new(json(MAX_RESPONSE_BYTES + 100), 4093);
        assert_eq!(
            collect_json(&mut source, deadline()).await,
            Err(ResponseError::BodyLimit)
        );
        assert_eq!(source.state.borrow().consumed, MAX_RESPONSE_BYTES + 1);
        Ok(())
    })
}

#[test]
fn json_final_storage_requires_complete_framing_and_obeys_cancellation() -> io::Result<()> {
    runtime()?.block_on(async {
        let mut failed = Source::new(json(RESPONSE_BUFFER_BYTES + 1), 17);
        failed.fail_at_end = true;
        assert_eq!(
            collect_json(&mut failed, deadline()).await,
            Err(ResponseError::BackendIo)
        );
        let mut stalled = Source::new(json(RESPONSE_BUFFER_BYTES + 1), 17);
        stalled.stall_at_end = true;
        assert_eq!(
            collect_json(&mut stalled, deadline()).await,
            Err(ResponseError::Deadline)
        );
        let mut cancelled = Source::new(json(RESPONSE_BUFFER_BYTES + 1), 17);
        cancelled.stall_at_end = true;
        let state = Rc::clone(&cancelled.state);
        let mut operation = Box::pin(async move { collect_json(&mut cancelled, deadline()).await });
        poll_fn(|cx| {
            assert!(operation.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
        assert_eq!(state.borrow().consumed, RESPONSE_BUFFER_BYTES + 1);
        assert!(!state.borrow().dropped);
        drop(operation);
        assert!(state.borrow().dropped);
        Ok(())
    })
}
