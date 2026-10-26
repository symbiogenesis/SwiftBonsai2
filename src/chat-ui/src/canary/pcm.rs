//! Strict bounded chunk framing for the documented local PCM stream.

use super::{Failure, MAX_HEAD, Result};

const MAX_PCM: usize = 24_000 * 2 * 120;
const INVALID: Failure = Failure::Message("The local speech stream was incomplete or invalid.");

pub(super) fn decode(
    mut read: impl FnMut(&mut [u8]) -> Result<usize>,
    mut sink: impl FnMut(&[u8]) -> Result<()>,
) -> Result<()> {
    let mut head = Vec::new();
    while !head.ends_with(b"\r\n\r\n") {
        if head.len() == MAX_HEAD {
            return Err(INVALID);
        }
        let mut byte = [0];
        exact(&mut read, &mut byte)?;
        head.push(byte[0]);
    }
    validate_head(&head)?;
    let mut total = 0_usize;
    let mut buffer = [0; 16_384];
    loop {
        let mut line = Vec::new();
        while !line.ends_with(b"\r\n") {
            if line.len() == 10 {
                return Err(INVALID);
            }
            let mut byte = [0];
            exact(&mut read, &mut byte)?;
            line.push(byte[0]);
        }
        let digits = &line[..line.len() - 2];
        if digits.is_empty() || !digits.iter().all(u8::is_ascii_hexdigit) {
            return Err(INVALID);
        }
        let length = usize::from_str_radix(std::str::from_utf8(digits).map_err(|_| INVALID)?, 16)
            .map_err(|_| INVALID)?;
        if length == 0 {
            let mut end = [0; 2];
            exact(&mut read, &mut end)?;
            if end != *b"\r\n" || total == 0 || read(&mut [0])? != 0 {
                return Err(INVALID);
            }
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
            let count = remaining.min(buffer.len());
            exact(&mut read, &mut buffer[..count])?;
            sink(&buffer[..count])?;
            remaining -= count;
        }
        let mut end = [0; 2];
        exact(&mut read, &mut end)?;
        if end != *b"\r\n" {
            return Err(INVALID);
        }
    }
}

fn exact(read: &mut impl FnMut(&mut [u8]) -> Result<usize>, mut bytes: &mut [u8]) -> Result<()> {
    while !bytes.is_empty() {
        let count = read(bytes)?;
        if count == 0 || count > bytes.len() {
            return Err(INVALID);
        }
        bytes = &mut bytes[count..];
    }
    Ok(())
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
    use super::{Failure, decode};
    use std::io::Read;

    const HEAD: &[u8] = b"HTTP/1.1 200 OK\r\nContent-Type: audio/pcm\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n";

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
            |_| {
                delivered = true;
                Err(Failure::Cancelled)
            },
        );
        assert!(delivered);
        assert_eq!(result, Err(Failure::Cancelled));
    }
}
