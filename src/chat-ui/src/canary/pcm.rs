//! Strict bounded chunk framing for the documented local PCM stream.

use super::{Failure, MAX_HEAD, Result};

const MAX_PCM: usize = 24_000 * 2 * 120;
const INVALID: Failure = Failure::Message("The local speech stream was incomplete or invalid.");

pub(super) fn decode(
    read: impl FnMut(&mut [u8]) -> Result<usize>,
    mut check: impl FnMut() -> Result<()>,
    mut sink: impl FnMut(&[u8]) -> Result<()>,
) -> Result<()> {
    let mut reader = Buffered::new(read);
    let mut head = Vec::new();
    while !head.ends_with(b"\r\n\r\n") {
        check()?;
        if head.len() == MAX_HEAD {
            return Err(INVALID);
        }
        head.push(reader.byte()?);
    }
    validate_head(&head)?;
    drop(head);
    let mut total = 0_usize;
    loop {
        let mut line = [0; 10];
        let mut count = 0;
        while !line[..count].ends_with(b"\r\n") {
            check()?;
            if count == line.len() {
                return Err(INVALID);
            }
            line[count] = reader.byte()?;
            count += 1;
        }
        let digits = &line[..count - 2];
        if digits.is_empty() || !digits.iter().all(u8::is_ascii_hexdigit) {
            return Err(INVALID);
        }
        let length = usize::from_str_radix(std::str::from_utf8(digits).map_err(|_| INVALID)?, 16)
            .map_err(|_| INVALID)?;
        if length == 0 {
            let mut end = [0; 2];
            reader.exact(&mut end)?;
            check()?;
            if end != *b"\r\n" || total == 0 || reader.fill()? {
                return Err(INVALID);
            }
            check()?;
            return Ok(());
        }
        total = total
            .checked_add(length)
            .filter(|&value| value <= MAX_PCM)
            .ok_or(INVALID)?;
        if length % 2 != 0 {
            return Err(INVALID);
        }
        let mut remaining = length;
        while remaining != 0 {
            check()?;
            let samples = reader.pcm(remaining, &mut check)?;
            sink(samples)?;
            remaining -= samples.len();
        }
        let mut end = [0; 2];
        reader.exact(&mut end)?;
        check()?;
        if end != *b"\r\n" {
            return Err(INVALID);
        }
    }
}

// Metadata and PCM share the same fixed storage. Byte-at-a-time framing never
// becomes byte-at-a-time socket I/O, and available audio never waits for a full
// buffer. The explicit guard in decode still observes cancellation while data
// is already buffered.
struct Buffered<F> {
    read: F,
    buffer: [u8; 16_384],
    start: usize,
    end: usize,
    sample: [u8; 2],
}

impl<F: FnMut(&mut [u8]) -> Result<usize>> Buffered<F> {
    const fn new(read: F) -> Self {
        Self {
            read,
            buffer: [0; 16_384],
            start: 0,
            end: 0,
            sample: [0; 2],
        }
    }

    fn fill(&mut self) -> Result<bool> {
        if self.start == self.end {
            self.start = 0;
            self.end = (self.read)(&mut self.buffer)?;
            if self.end > self.buffer.len() {
                return Err(INVALID);
            }
        }
        Ok(self.start != self.end)
    }

    fn byte(&mut self) -> Result<u8> {
        if !self.fill()? {
            return Err(INVALID);
        }
        let byte = self.buffer[self.start];
        self.start += 1;
        Ok(byte)
    }

    // Lend only whole PCM16 samples within the current HTTP chunk. Advancing
    // the cursor before returning keeps framing and refill bookkeeping private;
    // the borrow prevents another read until the caller finishes with this span.
    fn pcm(&mut self, maximum: usize, check: &mut impl FnMut() -> Result<()>) -> Result<&[u8]> {
        if maximum < 2 || maximum % 2 != 0 || !self.fill()? {
            return Err(INVALID);
        }
        check()?;
        let count = maximum.min(self.end - self.start) & !1;
        if count != 0 {
            let start = self.start;
            self.start += count;
            return Ok(&self.buffer[start..self.start]);
        }
        // A read can split a single sample. Join only those two bytes, without
        // waiting for any additional payload or its trailing framing to arrive.
        let first = self.byte()?;
        let second = self.byte()?;
        check()?;
        self.sample = [first, second];
        Ok(&self.sample)
    }

    fn exact(&mut self, bytes: &mut [u8]) -> Result<()> {
        for byte in bytes {
            *byte = self.byte()?;
        }
        Ok(())
    }
}

fn validate_head(head: &[u8]) -> Result<()> {
    if head.iter().enumerate().any(|(i, &byte)| {
        (byte == b'\n' && (i == 0 || head[i - 1] != b'\r'))
            || (byte == b'\r' && head.get(i + 1) != Some(&b'\n'))
    }) {
        return Err(INVALID);
    }
    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut parsed = httparse::Response::new(&mut headers);
    if parsed.parse(head).ok() != Some(httparse::Status::Complete(head.len()))
        || parsed.version != Some(1)
        || parsed.code != Some(200)
    {
        return Err(INVALID);
    }
    let mut media = false;
    let mut chunked = false;
    let mut close = false;
    for (i, header) in parsed.headers.iter().enumerate() {
        if parsed.headers[..i]
            .iter()
            .any(|other| other.name.eq_ignore_ascii_case(header.name))
            || ["content-length", "content-encoding", "trailer", "upgrade"]
                .iter()
                .any(|name| header.name.eq_ignore_ascii_case(name))
            || header.value.iter().any(|&b| b < 32 || b == 127)
        {
            return Err(INVALID);
        }
        if header.name.eq_ignore_ascii_case("content-type") {
            media = header.value.eq_ignore_ascii_case(b"audio/pcm");
        } else if header.name.eq_ignore_ascii_case("transfer-encoding") {
            chunked = header.value.eq_ignore_ascii_case(b"chunked");
        } else if header.name.eq_ignore_ascii_case("connection") {
            close = header.value.eq_ignore_ascii_case(b"close");
        }
    }
    if media && chunked && close {
        Ok(())
    } else {
        Err(INVALID)
    }
}

#[cfg(test)]
mod tests {
    use super::{Buffered, Failure, INVALID, MAX_PCM, decode};
    use std::{cell::Cell, io::Read};

    const HEAD: &[u8] = b"HTTP/1.1 200 OK\r\nContent-Type: audio/pcm\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n";

    #[test]
    fn pcm_span_lends_ready_samples_without_consuming_chunk_framing() -> Result<(), Failure> {
        let mut source = b"\x01\x80\r\n".as_slice();
        let reads = Cell::new(0);
        let mut reader = Buffered::new(|bytes: &mut [u8]| {
            reads.set(reads.get() + 1);
            source.read(bytes).map_err(|_| Failure::Cancelled)
        });
        let buffer = reader.buffer.as_ptr();
        let span = reader.pcm(2, &mut || Ok(()))?;
        assert_eq!(span, [1, 128]);
        assert_eq!(span.as_ptr(), buffer);
        assert_eq!(reads.get(), 1);
        let mut ending = [0; 2];
        reader.exact(&mut ending)?;
        assert_eq!(ending, *b"\r\n");
        assert_eq!(reads.get(), 1);
        assert_eq!(reader.pcm(2, &mut || Ok(())), Err(INVALID));
        assert_eq!(reads.get(), 2);
        Ok(())
    }

    #[test]
    fn pcm_split_sample_observes_cancellation_before_refill() {
        let reads = Cell::new(0);
        let mut reader = Buffered::new(|bytes: &mut [u8]| {
            reads.set(reads.get() + 1);
            bytes[0] = 1;
            Ok(1)
        });
        assert_eq!(
            reader.pcm(2, &mut || Err(Failure::Cancelled)),
            Err(Failure::Cancelled)
        );
        assert_eq!(reads.get(), 1);
    }

    #[test]
    fn pcm_stream_requires_terminal_chunk_and_rejects_ambiguous_framing() {
        for (body, valid) in [
            (b"4\r\n\x01\x00\x02\x00\r\n0\r\n\r\n".as_slice(), true),
            (b"4\r\n\x01\x00\x02\x00\r\n".as_slice(), false),
            (b"3\r\nabc\r\n0\r\n\r\n".as_slice(), false),
            (b"0\r\n\r\n".as_slice(), false),
            (b"2\r\nxx\r\n0\r\n\r\nextra".as_slice(), false),
            (b"2;ext=x\r\nxx\r\n0\r\n\r\n".as_slice(), false),
            (b"2\r\nxx\r\n0\r\nX: y\r\n\r\n".as_slice(), false),
            (b"ffffffff\r\n".as_slice(), false),
        ] {
            let bytes = [HEAD, body].concat();
            let mut reader = bytes.as_slice();
            let mut pcm = Vec::new();
            let result = decode(
                |data| reader.read(data).map_err(|_| Failure::Cancelled),
                || Ok(()),
                |chunk| {
                    pcm.extend_from_slice(chunk);
                    Ok(())
                },
            );
            assert_eq!(result.is_ok(), valid);
            if valid {
                assert_eq!(pcm, [1, 0, 2, 0]);
            }
        }
        let malformed = [
            b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n".as_slice(),
            &HEAD[17..],
            b"2\r\nxx\r\n0\r\n\r\n",
        ]
        .concat();
        let mut reader = malformed.as_slice();
        assert!(
            decode(
                |data| reader.read(data).map_err(|_| Failure::Cancelled),
                || Ok(()),
                |_| Ok(())
            )
            .is_err()
        );
    }

    #[test]
    fn pcm_stream_delivers_before_completion_and_propagates_sink_cancellation() {
        let bytes = [HEAD, b"2\r\nxx\r\n"].concat();
        let mut reader = bytes.as_slice();
        let mut delivered = false;
        let result = decode(
            |data| reader.read(data).map_err(|_| Failure::Cancelled),
            || Ok(()),
            |_| {
                delivered = true;
                Err(Failure::Cancelled)
            },
        );
        assert!(delivered);
        assert_eq!(result, Err(Failure::Cancelled));
    }

    #[test]
    fn pcm_framing_reuses_one_buffer_without_byte_sized_source_reads() -> Result<(), Failure> {
        let bytes = [HEAD, b"2\r\nxx\r\n2\r\nyy\r\n0\r\n\r\n"].concat();
        let mut reader = bytes.as_slice();
        let mut reads = 0;
        let mut pcm = Vec::new();
        decode(
            |data| {
                assert_eq!(data.len(), 16_384);
                reads += 1;
                reader.read(data).map_err(|_| Failure::Cancelled)
            },
            || Ok(()),
            |chunk| {
                pcm.extend_from_slice(chunk);
                Ok(())
            },
        )?;
        assert_eq!(pcm, b"xxyy");
        assert_eq!(reads, 2, "one source read plus the required EOF check");
        Ok(())
    }

    #[test]
    fn pcm_ready_prefix_arrives_before_remaining_chunk_or_framing() -> Result<(), Failure> {
        let bytes = [HEAD, b"8\r\n\x01\x00\x02\x00\x03\x00\x04\x00\r\n0\r\n\r\n"].concat();
        let prefix = HEAD.len() + 3 + 4;
        let mut offset = 0;
        let delivered = Cell::new(false);
        let mut pcm = Vec::new();
        decode(
            |data| {
                let limit = if offset < prefix {
                    prefix
                } else {
                    assert!(
                        delivered.get(),
                        "ready samples must precede another source read"
                    );
                    bytes.len()
                };
                let count = data.len().min(limit - offset);
                data[..count].copy_from_slice(&bytes[offset..offset + count]);
                offset += count;
                Ok(count)
            },
            || Ok(()),
            |chunk| {
                assert_eq!(chunk.len() % 2, 0);
                delivered.set(true);
                pcm.extend_from_slice(chunk);
                Ok(())
            },
        )?;
        assert_eq!(pcm, [1, 0, 2, 0, 3, 0, 4, 0]);
        Ok(())
    }

    #[test]
    fn pcm_fragmentation_preserves_complete_samples_and_terminal_validation() -> Result<(), Failure>
    {
        let bytes = [
            HEAD,
            b"6\r\n\x01\x80\x02\x00\xff\x7f\r\n2\r\n\x00\x80\r\n0\r\n\r\n",
        ]
        .concat();
        for fragment in [1, 2, 3, 7, 16_384] {
            let mut reader = bytes.as_slice();
            let mut pcm = Vec::new();
            decode(
                |data| {
                    let count = fragment.min(data.len());
                    reader
                        .read(&mut data[..count])
                        .map_err(|_| Failure::Cancelled)
                },
                || Ok(()),
                |chunk| {
                    assert_ne!(chunk.len(), 0);
                    assert_eq!(chunk.len() % 2, 0);
                    pcm.extend_from_slice(chunk);
                    Ok(())
                },
            )?;
            assert_eq!(pcm, [1, 128, 2, 0, 255, 127, 0, 128]);
        }
        Ok(())
    }

    #[test]
    fn pcm_buffered_data_still_observes_cancellation_and_rejects_truncation() {
        let bytes = [HEAD, b"2\r\nxx\r\n2\r\nyy\r\n0\r\n\r\n"].concat();
        let mut reader = bytes.as_slice();
        let cancelled = Cell::new(false);
        let mut delivered = 0;
        let result = decode(
            |data| reader.read(data).map_err(|_| Failure::Cancelled),
            || {
                if cancelled.get() {
                    Err(Failure::Cancelled)
                } else {
                    Ok(())
                }
            },
            |_| {
                delivered += 1;
                cancelled.set(true);
                Ok(())
            },
        );
        assert_eq!(result, Err(Failure::Cancelled));
        assert_eq!(delivered, 1);
        for body in [
            b"4\r\nxx".as_slice(),
            b"4\r\nxxx".as_slice(),
            b"2\r\nxxX\n0\r\n\r\n".as_slice(),
            b"2\r\nxx\r\n0\r\n\r\nextra".as_slice(),
            format!("{:x}\r\n", MAX_PCM + 2).as_bytes(),
        ] {
            let bytes = [HEAD, body].concat();
            let mut reader = bytes.as_slice();
            assert_eq!(
                decode(
                    |data| reader.read(data).map_err(|_| Failure::Cancelled),
                    || Ok(()),
                    |_| Ok(()),
                ),
                Err(INVALID)
            );
        }
    }
}
