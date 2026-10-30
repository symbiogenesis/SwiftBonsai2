//! Untimed payload ownership, buffering and framing-boundary regressions.

use super::*;
use std::{
    cell::{Cell, RefCell},
    future::poll_fn,
    rc::Rc,
};

const BUFFER_BYTES: usize = 8192;

struct Read {
    destination: *const u8,
    count: usize,
}

struct Source {
    bytes: Vec<u8>,
    offset: usize,
    fragment: usize,
    fail_at_end: bool,
    stall_at_end: bool,
    reads: Rc<RefCell<Vec<Read>>>,
    dropped: Rc<Cell<bool>>,
}

impl Source {
    fn new(bytes: Vec<u8>, fragment: usize) -> Self {
        Self {
            bytes,
            offset: 0,
            fragment,
            fail_at_end: false,
            stall_at_end: false,
            reads: Rc::default(),
            dropped: Rc::default(),
        }
    }
}

impl AsyncRead for Source {
    fn poll_read(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        output: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let count = (this.bytes.len() - this.offset)
            .min(output.remaining())
            .min(this.fragment);
        this.reads.borrow_mut().push(Read {
            destination: output.filled().as_ptr(),
            count,
        });
        if count == 0 && this.fail_at_end {
            return Poll::Ready(Err(io::ErrorKind::ConnectionReset.into()));
        }
        if count == 0 && this.stall_at_end {
            return Poll::Pending;
        }
        output.put_slice(&this.bytes[this.offset..this.offset + count]);
        this.offset += count;
        Poll::Ready(Ok(()))
    }
}

impl Drop for Source {
    fn drop(&mut self) {
        self.dropped.set(true);
    }
}

fn body(reader: BufReader<Source>, framing: Framing) -> BackendBody<Source> {
    let deadline = Instant::now() + Duration::from_secs(60);
    BackendBody {
        reader: Some(reader),
        framing,
        remaining_budget: MAX_RESPONSE_BYTES,
        metadata_bytes: 0,
        deadline,
        timer: Box::pin(sleep_until(deadline)),
    }
}

fn runtime() -> io::Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
}

#[test]
fn payload_reads_bypass_staging_only_within_the_current_frame() -> io::Result<()> {
    runtime()?.block_on(async {
        for chunked in [false, true] {
            for fragment in [1, 4093, BUFFER_BYTES, usize::MAX] {
                let payload = vec![b'x'; 3 * BUFFER_BYTES + 17];
                let mut wire = payload.clone();
                if chunked {
                    wire.extend_from_slice(b"\r\n0\r\n\r\n");
                }
                wire.extend_from_slice(b"surplus");
                let source = Source::new(wire, fragment);
                let reads = Rc::clone(&source.reads);
                let framing = if chunked {
                    Framing::Data(payload.len())
                } else {
                    Framing::Length(payload.len())
                };
                let mut body = body(BufReader::with_capacity(BUFFER_BYTES, source), framing);
                let mut final_storage = [0u8; BUFFER_BYTES];
                let destination = final_storage.as_ptr();
                let mut received = Vec::new();
                loop {
                    let count = body.read(&mut final_storage).await?;
                    if count == 0 {
                        break;
                    }
                    received.extend_from_slice(&final_storage[..count]);
                }
                assert_eq!(received, payload);
                assert_eq!(body.remaining_budget, MAX_RESPONSE_BYTES - payload.len());
                let reads = reads.borrow();
                let direct_bytes: usize = reads
                    .iter()
                    .filter(|read| read.destination == destination)
                    .map(|read| read.count)
                    .sum();
                assert!(direct_bytes >= payload.len() - BUFFER_BYTES);
                // The 17-byte tail needs buffered framing protection; its
                // coalesced ending and surplus never enter final payload bytes.
                assert!(reads.iter().any(|read| read.destination != destination));
            }
        }
        Ok(())
    })
}

#[test]
fn payload_reads_retain_coalesced_prefixes_and_small_next_chunks() -> io::Result<()> {
    runtime()?.block_on(async {
        let prefix = b"head\r\n\r\n";
        let payload = vec![b'x'; 3 * BUFFER_BYTES + 17];
        let mut wire = prefix.to_vec();
        wire.extend_from_slice(&payload);
        wire.extend_from_slice(b"\r\n4\r\ntail\r\n0\r\n\r\nsurplus");
        let source = Source::new(wire, usize::MAX);
        let reads = Rc::clone(&source.reads);
        let mut reader = BufReader::with_capacity(BUFFER_BYTES, source);
        assert_eq!(&reader.fill_buf().await?[..prefix.len()], prefix);
        reader.consume(prefix.len());
        let mut body = body(reader, Framing::Data(payload.len()));
        let mut final_storage = [0u8; BUFFER_BYTES];
        let destination = final_storage.as_ptr();
        let first = body.read(&mut final_storage).await?;
        assert_eq!(first, BUFFER_BYTES - prefix.len());
        assert_eq!(&final_storage[..first], &payload[..first]);
        assert_eq!(reads.borrow().len(), 1, "buffered prefix reread the source");
        let mut received = final_storage[..first].to_vec();
        loop {
            let count = body.read(&mut final_storage).await?;
            if count == 0 {
                break;
            }
            received.extend_from_slice(&final_storage[..count]);
        }
        let mut expected = payload;
        expected.extend_from_slice(b"tail");
        assert_eq!(received, expected);
        assert!(
            reads
                .borrow()
                .iter()
                .any(|read| read.destination == destination)
        );
        assert_eq!(body.remaining_budget, MAX_RESPONSE_BYTES - expected.len());
        Ok(())
    })
}

#[test]
fn payload_direct_reads_reject_truncation_and_io_failure_stickily() -> io::Result<()> {
    runtime()?.block_on(async {
        for chunked in [false, true] {
            for fail_at_end in [false, true] {
                let mut source = Source::new(b"ready prefix".to_vec(), usize::MAX);
                source.fail_at_end = fail_at_end;
                let dropped = Rc::clone(&source.dropped);
                let framing = if chunked {
                    Framing::Data(2 * BUFFER_BYTES)
                } else {
                    Framing::Length(2 * BUFFER_BYTES)
                };
                let mut body = body(BufReader::with_capacity(BUFFER_BYTES, source), framing);
                let mut final_storage = [0u8; BUFFER_BYTES];
                let count = body.read(&mut final_storage).await?;
                assert_eq!(&final_storage[..count], b"ready prefix");
                let error = body
                    .read(&mut final_storage)
                    .await
                    .err()
                    .map(|error| error.kind());
                assert_eq!(
                    error,
                    Some(if fail_at_end {
                        io::ErrorKind::Other
                    } else {
                        io::ErrorKind::UnexpectedEof
                    })
                );
                assert!(dropped.get());
                assert_eq!(
                    body.read(&mut final_storage)
                        .await
                        .err()
                        .map(|error| error.kind()),
                    Some(io::ErrorKind::InvalidData)
                );
            }
        }
        Ok(())
    })
}

#[test]
fn payload_empty_reads_leave_framing_and_source_untouched() -> io::Result<()> {
    runtime()?.block_on(async {
        let source = Source::new(b"x".to_vec(), usize::MAX);
        let reads = Rc::clone(&source.reads);
        let mut body = body(
            BufReader::with_capacity(BUFFER_BYTES, source),
            Framing::Length(1),
        );
        poll_fn(|cx| Pin::new(&mut body).poll_read(cx, &mut ReadBuf::new(&mut []))).await?;
        assert!(reads.borrow().is_empty());
        assert_eq!(body.remaining_budget, MAX_RESPONSE_BYTES);
        let mut output = [0u8; 1];
        assert_eq!(body.read(&mut output).await?, 1);
        assert_eq!(output, [b'x']);
        Ok(())
    })
}

#[test]
fn payload_direct_reads_preserve_a_prefilled_destination() -> io::Result<()> {
    runtime()?.block_on(async {
        let source = Source::new(vec![b'x'; BUFFER_BYTES], usize::MAX);
        let mut body = body(
            BufReader::with_capacity(BUFFER_BYTES, source),
            Framing::Length(BUFFER_BYTES),
        );
        let mut storage = [b'p'; BUFFER_BYTES + 1];
        let mut output = ReadBuf::new(&mut storage);
        output.advance(1);
        poll_fn(|cx| Pin::new(&mut body).poll_read(cx, &mut output)).await?;
        assert_eq!(output.filled().len(), BUFFER_BYTES + 1);
        assert_eq!(storage[0], b'p');
        assert!(storage[1..].iter().all(|byte| *byte == b'x'));
        assert_eq!(body.remaining_budget, MAX_RESPONSE_BYTES - BUFFER_BYTES);
        Ok(())
    })
}

#[test]
fn payload_pending_direct_reads_keep_the_absolute_deadline() -> io::Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()?;
    runtime.block_on(async {
        for chunked in [false, true] {
            let mut source = Source::new(b"ready prefix".to_vec(), usize::MAX);
            source.stall_at_end = true;
            let dropped = Rc::clone(&source.dropped);
            let framing = if chunked {
                Framing::Data(2 * BUFFER_BYTES)
            } else {
                Framing::Length(2 * BUFFER_BYTES)
            };
            let mut body = body(BufReader::with_capacity(BUFFER_BYTES, source), framing);
            let mut storage = [0u8; BUFFER_BYTES];
            let count = body.read(&mut storage).await?;
            assert_eq!(&storage[..count], b"ready prefix");
            assert_eq!(
                body.read(&mut storage)
                    .await
                    .err()
                    .map(|error| error.kind()),
                Some(io::ErrorKind::TimedOut)
            );
            assert!(dropped.get());
        }
        Ok(())
    })
}
