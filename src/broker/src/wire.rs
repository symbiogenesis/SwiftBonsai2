//! Shared HTTP wire mechanics; request and response policy stays with callers.

#![allow(
    clippy::redundant_pub_crate,
    reason = "Keep shared HTTP helpers explicitly crate scoped"
)]

use http::{HeaderMap, HeaderName, HeaderValue};

pub(super) enum LengthError {
    Syntax,
    Overflow,
}

pub(super) fn content_length(bytes: &[u8]) -> Result<usize, LengthError> {
    // Validate the entire field before reporting overflow, preserving the
    // request policy's distinction between invalid framing and excessive size.
    if bytes.is_empty() || !bytes.iter().all(u8::is_ascii_digit) {
        return Err(LengthError::Syntax);
    }
    bytes
        .iter()
        .try_fold(0usize, |value, byte| {
            value.checked_mul(10)?.checked_add(usize::from(byte - b'0'))
        })
        .ok_or(LengthError::Overflow)
}

pub(super) fn head_end(raw: &[u8], previous: usize) -> Option<usize> {
    // A terminator can straddle the previous read by at most three bytes.
    // Each byte is scanned at most four times even with one-byte fragments.
    let start = previous.saturating_sub(3);
    raw[start..]
        .array_windows()
        .position(|bytes| bytes == b"\r\n\r\n")
        .map(|offset| start + offset + 4)
}

pub(super) fn valid_line_endings(raw: &[u8]) -> bool {
    !raw.iter().enumerate().any(|(i, byte)| match byte {
        b'\r' => raw.get(i + 1) != Some(&b'\n'),
        b'\n' => i == 0 || raw[i - 1] != b'\r',
        _ => false,
    })
}

pub(super) fn append_headers(
    target: &mut HeaderMap,
    headers: &[httparse::Header<'_>],
) -> Result<(), ()> {
    for header in headers {
        let name = HeaderName::from_bytes(header.name.as_bytes()).map_err(|_| ())?;
        let value = HeaderValue::from_bytes(header.value).map_err(|_| ())?;
        // insert() would hide duplicate critical headers, including identical CL.
        target.append(name, value);
    }
    Ok(())
}
