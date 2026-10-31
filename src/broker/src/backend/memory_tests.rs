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
