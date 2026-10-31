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

struct VectoredClient {
    bytes: Vec<u8>,
    maximum_write: usize,
    payload_pointers: Vec<*const u8>,
    zero: bool,
    shutdown: bool,
}

impl VectoredClient {
    fn new(maximum_write: usize) -> Self {
        Self {
            bytes: Vec::new(),
            maximum_write,
            payload_pointers: Vec::new(),
            zero: false,
            shutdown: false,
        }
    }
}

impl AsyncWrite for VectoredClient {
    fn poll_write(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        let count = self.maximum_write.min(bytes.len());
        self.bytes.extend_from_slice(&bytes[..count]);
        Poll::Ready(Ok(count))
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
        buffers: &[IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        if self.zero {
            return Poll::Ready(Ok(0));
        }
        let mut count = 0;
        for buffer in buffers {
            if buffer.len() >= RESPONSE_BUFFER_BYTES {
                self.payload_pointers.push(buffer.as_ptr());
            }
            let take = buffer.len().min(self.maximum_write - count);
            self.bytes.extend_from_slice(&buffer[..take]);
            count += take;
            if count == self.maximum_write {
                break;
            }
        }
        Poll::Ready(Ok(count))
    }

    fn is_write_vectored(&self) -> bool {
        true
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.shutdown = true;
        Poll::Ready(Ok(()))
    }
}

#[test]
fn borrowed_sse_spans_use_original_storage_and_finish_partial_vectored_writes() -> io::Result<()> {
    runtime()?.block_on(async {
        let payload = vec![b'x'; 2 * RESPONSE_BUFFER_BYTES + 7];
        for maximum_write in [1, 17, usize::MAX] {
            let mut reader = tokio::io::BufReader::with_capacity(
                RESPONSE_BUFFER_BYTES,
                io::Cursor::new(payload.clone()),
            );
            let pointer = reader.buffer().as_ptr();
            let mut client = VectoredClient::new(maximum_write);
            write_buffered_sse_response(&mut client, &mut reader, deadline())
                .await
                .map_err(|_| io::Error::other("Fixture SSE was rejected."))?;
            let mut expected = SSE_HEAD.as_bytes().to_vec();
            for chunk in payload.chunks(RESPONSE_BUFFER_BYTES) {
                expected.extend_from_slice(format!("{:X}\r\n", chunk.len()).as_bytes());
                expected.extend_from_slice(chunk);
                expected.extend_from_slice(b"\r\n");
            }
            expected.extend_from_slice(b"0\r\n\r\n");
            assert_eq!(client.bytes, expected);
            assert!(client.payload_pointers.contains(&pointer));
            assert!(client.shutdown);
        }
        Ok(())
    })
}

#[test]
fn borrowed_sse_does_not_complete_after_read_or_vectored_write_failure() -> io::Result<()> {
    runtime()?.block_on(async {
        let mut source = Source::new(vec![b'x'; RESPONSE_BUFFER_BYTES + 7], RESPONSE_BUFFER_BYTES);
        source.fail_at_end = true;
        let mut reader = tokio::io::BufReader::with_capacity(RESPONSE_BUFFER_BYTES, source);
        let mut client = VectoredClient::new(usize::MAX);
        assert_eq!(
            write_buffered_sse_response(&mut client, &mut reader, deadline()).await,
            Err(ResponseError::BackendIo)
        );
        assert!(!client.shutdown);
        assert!(!client.bytes.ends_with(b"0\r\n\r\n"));

        let mut reader = tokio::io::BufReader::with_capacity(
            RESPONSE_BUFFER_BYTES,
            io::Cursor::new(vec![b'x'; RESPONSE_BUFFER_BYTES]),
        );
        let mut client = VectoredClient::new(usize::MAX);
        client.zero = true;
        assert_eq!(
            write_buffered_sse_response(&mut client, &mut reader, deadline()).await,
            Err(ResponseError::ClientIo)
        );
        assert!(!client.shutdown);
        Ok(())
    })
}

#[test]
fn borrowed_sse_preserves_exact_cap_probe_and_deadline_failure() -> io::Result<()> {
    runtime()?.block_on(async {
        for length in [MAX_RESPONSE_BYTES, MAX_RESPONSE_BYTES + 1] {
            let mut reader = tokio::io::BufReader::with_capacity(
                RESPONSE_BUFFER_BYTES,
                io::Cursor::new(vec![b'x'; length]),
            );
            let mut client = VectoredClient::new(usize::MAX);
            let result = write_buffered_sse_response(&mut client, &mut reader, deadline()).await;
            if length == MAX_RESPONSE_BYTES {
                assert_eq!(result, Ok(()));
                assert!(client.shutdown);
                assert!(client.bytes.ends_with(b"0\r\n\r\n"));
            } else {
                assert_eq!(result, Err(ResponseError::BodyLimit));
                assert!(!client.shutdown);
                assert!(!client.bytes.ends_with(b"0\r\n\r\n"));
            }
        }
        let mut source = Source::new(vec![b'x'; RESPONSE_BUFFER_BYTES], RESPONSE_BUFFER_BYTES);
        source.stall_at_end = true;
        let mut reader = tokio::io::BufReader::with_capacity(RESPONSE_BUFFER_BYTES, source);
        let mut client = VectoredClient::new(usize::MAX);
        assert_eq!(
            write_buffered_sse_response(&mut client, &mut reader, deadline()).await,
            Err(ResponseError::Deadline)
        );
        assert!(!client.shutdown);
        assert!(!client.bytes.ends_with(b"0\r\n\r\n"));
        Ok(())
    })
}
