//! Untimed ownership regression for request bytes during response waits.

use super::*;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

struct Body {
    bytes: Vec<u8>,
    dropped: Rc<Cell<bool>>,
}

impl AsRef<[u8]> for Body {
    fn as_ref(&self) -> &[u8] {
        &self.bytes
    }
}

impl Drop for Body {
    fn drop(&mut self) {
        self.dropped.set(true);
    }
}

struct Stream {
    response: io::Cursor<Vec<u8>>,
    written: Rc<RefCell<Vec<u8>>>,
    body_dropped: Rc<Cell<bool>>,
}

impl AsyncRead for Stream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        output: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        assert!(self.body_dropped.get(), "prompt survived the request write");
        Pin::new(&mut self.response).poll_read(cx, output)
    }
}

impl AsyncWrite for Stream {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        assert!(!self.body_dropped.get());
        self.written.borrow_mut().extend_from_slice(bytes);
        Poll::Ready(Ok(bytes.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        assert!(!self.body_dropped.get());
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

#[test]
fn owned_request_drops_prompt_before_any_response_read_and_preserves_wire_bytes() -> io::Result<()>
{
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()?;
    runtime.block_on(async {
        for streaming in [false, true] {
            let body_dropped = Rc::new(Cell::new(false));
            let written = Rc::new(RefCell::new(Vec::new()));
            let bytes = vec![b'x'; crate::MAX_BODY_BYTES];
            let expected = bytes.clone();
            let body = Body {
                bytes,
                dropped: Rc::clone(&body_dropped),
            };
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: 0\r\n\r\n",
                if streaming {
                    "text/event-stream"
                } else {
                    "application/json"
                }
            );
            let stream = Stream {
                response: io::Cursor::new(response.into_bytes()),
                written: Rc::clone(&written),
                body_dropped: Rc::clone(&body_dropped),
            };
            let policy = BackendPolicy::new(8081, &"a".repeat(64))
                .map_err(|_| io::Error::other("Invalid fixture policy."))?;
            let response = exchange(
                stream,
                body,
                streaming,
                "/v1/chat/completions",
                &policy,
                Instant::now() + Duration::from_secs(60),
            )
            .await;
            assert!(response.is_ok());
            assert!(body_dropped.get());
            let written = written.borrow();
            let head_end = wire::head_end(&written, 0)
                .ok_or_else(|| io::Error::other("Missing fixture head."))?;
            assert_eq!(&written[head_end..], expected);
        }
        Ok(())
    })
}

#[test]
fn header_fast_path_and_fragmented_fallback_preserve_coalesced_payload() -> io::Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()?;
    runtime.block_on(async {
        const PAYLOAD: &[u8] = b"{}coalesced surplus";
        const HEAD: &[u8] =
            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n";
        for capacity in [1, 2, 3, 7, HEAD.len() - 1, HEAD.len(), 8192] {
            let wire = [HEAD, PAYLOAD].concat();
            let mut reader = BufReader::with_capacity(capacity, io::Cursor::new(wire));
            let (metadata, framing) =
                read_head(&mut reader, false, Instant::now() + Duration::from_secs(60))
                    .await
                    .map_err(|_| io::Error::other("Fixture head was rejected."))?;
            assert_eq!(metadata.status(), 200);
            assert!(matches!(framing, Framing::Length(2)));
            let mut payload = Vec::new();
            reader.read_to_end(&mut payload).await?;
            assert_eq!(payload, PAYLOAD);
        }
        Ok(())
    })
}

#[test]
fn borrowed_payloads_exclude_framing_and_interoperate_with_direct_reads() -> io::Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()?;
    runtime.block_on(async {
        for chunked in [false, true] {
            let wire = if chunked {
                b"3\r\nabc\r\n1\r\nd\r\n0\r\n\r\nsurplus".as_slice()
            } else {
                b"abcdsurplus".as_slice()
            };
            let mut body = BackendBody {
                reader: Some(BufReader::new(io::Cursor::new(wire.to_vec()))),
                framing: if chunked {
                    Framing::Size {
                        value: 0,
                        digits: 0,
                        cr: false,
                    }
                } else {
                    Framing::Length(4)
                },
                remaining_budget: MAX_RESPONSE_BYTES,
                metadata_bytes: 0,
                deadline: Instant::now() + Duration::from_secs(60),
                timer: Box::pin(sleep_until(Instant::now() + Duration::from_secs(60))),
            };
            let storage = body
                .reader
                .as_ref()
                .map(|reader| reader.buffer().as_ptr())
                .ok_or_else(|| io::Error::other("Missing fixture reader."))?;
            let available = body.fill_buf().await?;
            assert_eq!(
                available,
                if chunked {
                    b"abc".as_slice()
                } else {
                    b"abcd".as_slice()
                }
            );
            assert_eq!(
                available.as_ptr(),
                storage.wrapping_add(if chunked { 3 } else { 0 })
            );
            body.consume(1);
            let mut byte = [0];
            body.read_exact(&mut byte).await?;
            assert_eq!(&byte, b"b");
            let available = body.fill_buf().await?;
            assert_eq!(
                available,
                if chunked {
                    b"c".as_slice()
                } else {
                    b"cd".as_slice()
                }
            );
            // Defensive clamping must never consume chunk endings or surplus.
            body.consume(usize::MAX);
            if chunked {
                assert_eq!(body.fill_buf().await?, b"d");
                body.consume(1);
            }
            assert_eq!(body.fill_buf().await?, b"");
            assert_eq!(body.remaining_budget, MAX_RESPONSE_BYTES - 4);
            assert_eq!(
                body.reader.as_ref().map(BufReader::buffer),
                Some(b"surplus".as_slice())
            );
        }
        Ok(())
    })
}
