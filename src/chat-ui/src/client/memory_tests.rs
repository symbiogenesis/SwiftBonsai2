//! Untimed request ownership checks with inert HTTP/SSE streams.

use super::*;
use std::{cell::Cell, rc::Rc, sync::mpsc};

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
    written: Vec<u8>,
    dropped: Rc<Cell<bool>>,
    flushed: bool,
    fail_after: Option<usize>,
    fail_flush: bool,
}

impl Read for Stream {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        assert!(self.flushed, "response read before flushing the request");
        assert!(self.dropped.get(), "request survived until a response read");
        self.response.read(bytes)
    }
}

impl Write for Stream {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        assert!(
            !self.dropped.get(),
            "request dropped before the write finished"
        );
        if self
            .fail_after
            .is_some_and(|limit| self.written.len() >= limit)
        {
            return Err(io::Error::other("Synthetic write failure"));
        }
        let count = bytes.len().min(17);
        self.written.extend_from_slice(&bytes[..count]);
        Ok(count)
    }

    fn flush(&mut self) -> io::Result<()> {
        assert!(!self.dropped.get(), "request dropped before flush");
        if self.fail_flush {
            return Err(io::Error::other("Synthetic flush failure"));
        }
        self.flushed = true;
        Ok(())
    }
}

#[test]
fn owned_request_releases_bytes_before_response_reads_and_preserves_partial_writes() -> Result<()> {
    let payload = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"Ready\"},\"finish_reason\":\"stop\"}]}\n\n",
        "data: [DONE]\n\n"
    );
    for chunked in [false, true] {
        let response = if chunked {
            format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n{:X}\r\n{payload}\r\n0\r\n\r\n",
                payload.len()
            )
        } else {
            format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\n\r\n{payload}",
                payload.len()
            )
        };
        let dropped = Rc::new(Cell::new(false));
        let bytes = vec![b'x'; MAX_REQUEST];
        let mut expected = format!(
            "POST /v1/chat/completions HTTP/1.1\r\nHost: 127.0.0.1:8080\r\nAuthorization: Bearer inert-test-key\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            bytes.len()
        ).into_bytes();
        expected.extend_from_slice(&bytes);
        let request = Body {
            bytes,
            dropped: Rc::clone(&dropped),
        };
        let mut connection = Stream {
            response: io::Cursor::new(response.into_bytes()),
            written: Vec::new(),
            dropped: Rc::clone(&dropped),
            flushed: false,
            fail_after: None,
            fail_flush: false,
        };
        let (_commands, incoming) = mpsc::channel();
        let (events, outgoing) = event_channel();
        let (answer, completion) = exchange(
            &mut connection,
            8080,
            "inert-test-key",
            request,
            &events,
            &incoming,
            Instant::now() + DEADLINE,
        )?;
        assert!(dropped.get());
        assert_eq!(connection.written, expected);
        assert_eq!(answer.content.as_ref(), "Ready");
        assert_eq!(completion, Completion::Finished);
        assert!(matches!(outgoing.try_recv(), Ok(Event::Text(text)) if text == "Ready"));
    }
    Ok(())
}

#[test]
fn owned_request_releases_bytes_on_partial_write_and_flush_failure() {
    for (fail_after, fail_flush) in [(Some(0), false), (Some(256), false), (None, true)] {
        let dropped = Rc::new(Cell::new(false));
        let request = Body {
            bytes: vec![b'x'; 1024],
            dropped: Rc::clone(&dropped),
        };
        let mut connection = Stream {
            response: io::Cursor::new(Vec::new()),
            written: Vec::new(),
            dropped: Rc::clone(&dropped),
            flushed: false,
            fail_after,
            fail_flush,
        };
        let (_commands, incoming) = mpsc::channel();
        let (events, outgoing) = event_channel();
        assert!(
            exchange(
                &mut connection,
                8080,
                "inert-test-key",
                request,
                &events,
                &incoming,
                Instant::now() + DEADLINE,
            )
            .is_err()
        );
        assert!(dropped.get());
        assert!(!connection.flushed);
        assert!(outgoing.try_recv().is_err());
    }
}
