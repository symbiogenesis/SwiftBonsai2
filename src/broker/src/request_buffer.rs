//! Bounded canonical serialization, including allocation growth before rejection.

#![allow(
    clippy::redundant_pub_crate,
    reason = "Keep the canonical serializer explicitly crate scoped."
)]

use crate::{MAX_BODY_BYTES, Rejection};
use std::io::{self, Write};

#[derive(Default)]
struct Buffer {
    bytes: Vec<u8>,
    exceeded: bool,
}

impl Write for Buffer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > MAX_BODY_BYTES - self.bytes.len() {
            self.exceeded = true;
            return Err(io::Error::other(
                "Canonical request exceeds its byte limit.",
            ));
        }
        let needed = self.bytes.len() + bytes.len();
        if needed > self.bytes.capacity() {
            // Vec's ordinary geometric growth can reserve nearly twice the
            // accepted body size. Cap every requested allocation, including
            // the final write that will be rejected for fixed policy overhead.
            let target = needed
                .max(self.bytes.capacity().saturating_mul(2))
                .clamp(128, MAX_BODY_BYTES);
            self.bytes
                .try_reserve_exact(target - self.bytes.len())
                .map_err(|_| io::Error::other("Could not allocate canonical request."))?;
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(super) fn serialize(value: &impl serde::Serialize) -> Result<Vec<u8>, Rejection> {
    let mut output = Buffer::default();
    serde_json::to_writer(&mut output, value).map_err(|_| {
        if output.exceeded {
            Rejection::BodySize
        } else {
            Rejection::ChatSchema
        }
    })?;
    Ok(output.bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_buffer_caps_growth_and_rejects_without_retaining_excess() -> io::Result<()> {
        let mut output = Buffer::default();
        // Awkward writes make ordinary Vec doubling overshoot the power-of-two
        // body limit on its final growth, rather than landing exactly on it.
        let chunk = [b'x'; 4093];
        for _ in 0..MAX_BODY_BYTES / chunk.len() {
            output.write_all(&chunk)?;
            assert!(output.bytes.capacity() <= MAX_BODY_BYTES);
        }
        output.write_all(&chunk[..MAX_BODY_BYTES % chunk.len()])?;
        assert!(output.bytes.capacity() <= MAX_BODY_BYTES);
        assert_eq!(output.bytes.len(), MAX_BODY_BYTES);
        assert!(output.write_all(b"x").is_err());
        assert!(output.exceeded);
        assert_eq!(output.bytes.len(), MAX_BODY_BYTES);
        Ok(())
    }

    #[test]
    fn canonical_buffer_preserves_json_bytes_and_exact_limit() -> Result<(), serde_json::Error> {
        let text = "雪🌱\n\u{0000}\"\\";
        let expected = serde_json::to_vec(text)?;
        assert_eq!(serialize(&text), Ok(expected));
        assert_eq!(
            serialize(&"x".repeat(MAX_BODY_BYTES - 2)).map(|bytes| bytes.len()),
            Ok(MAX_BODY_BYTES)
        );
        assert_eq!(
            serialize(&"x".repeat(MAX_BODY_BYTES - 1)),
            Err(Rejection::BodySize)
        );
        Ok(())
    }
}
