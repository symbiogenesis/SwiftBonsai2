//! Serialize bounded JSON without first retaining an oversized encoded document.

#![allow(
    clippy::redundant_pub_crate,
    reason = "Share the bounded encoder only within the native application."
)]

use std::io::{self, Write};

pub(crate) fn to_vec(
    value: &(impl serde::Serialize + ?Sized),
    limit: usize,
) -> Result<Vec<u8>, serde_json::Error> {
    let mut writer = BoundedWriter {
        bytes: Vec::new(),
        limit,
    };
    serde_json::to_writer(&mut writer, value)?;
    Ok(writer.bytes)
}

struct BoundedWriter {
    bytes: Vec<u8>,
    limit: usize,
}

impl Write for BoundedWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let required = self
            .bytes
            .len()
            .checked_add(bytes.len())
            .filter(|required| *required <= self.limit)
            .ok_or_else(|| io::Error::from(io::ErrorKind::FileTooLarge))?;
        if required > self.bytes.capacity() {
            // Retain amortized growth, but never request capacity past the
            // encoded-byte limit, including expansion of escaped characters.
            let capacity = self
                .bytes
                .capacity()
                .saturating_mul(2)
                .max(1024)
                .max(required)
                .min(self.limit);
            self.bytes
                .try_reserve_exact(capacity - self.bytes.len())
                .map_err(|_| io::Error::from(io::ErrorKind::OutOfMemory))?;
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_output_preserves_bytes_and_enforces_exact_encoded_limits()
    -> Result<(), serde_json::Error> {
        for text in ["", "Plain text", "雨 🌱", "\0\n\t\\\""] {
            let value = serde_json::json!({"text": text, "values": [true, false, null]});
            let expected = serde_json::to_vec(&value)?;
            let exact = to_vec(&value, expected.len())?;
            assert_eq!(exact, expected);
            assert!(exact.capacity() <= expected.len());
            assert_eq!(to_vec(&value, expected.len() + 1)?, expected);
            for limit in [0, expected.len() - 1] {
                assert_eq!(
                    to_vec(&value, limit)
                        .err()
                        .and_then(|error| error.io_error_kind()),
                    Some(io::ErrorKind::FileTooLarge)
                );
            }
        }
        Ok(())
    }

    #[test]
    fn rejected_json_stops_visiting_values_and_preserves_bounded_storage() -> io::Result<()> {
        use std::sync::atomic::{AtomicUsize, Ordering};

        struct Observed<'a>(&'a AtomicUsize);
        impl serde::Serialize for Observed<'_> {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                self.0.fetch_add(1, Ordering::Relaxed);
                serializer.serialize_str("\0\0\0\0\0\0\0\0")
            }
        }
        let visited = AtomicUsize::new(0);
        let values: [_; 100] = std::array::from_fn(|_| Observed(&visited));
        assert_eq!(
            to_vec(&values.as_slice(), 128)
                .err()
                .and_then(|error| error.io_error_kind()),
            Some(io::ErrorKind::FileTooLarge)
        );
        assert!(visited.load(Ordering::Relaxed) < values.len());

        let mut writer = BoundedWriter {
            bytes: Vec::new(),
            limit: 1024,
        };
        for _ in 0..100 {
            writer.write_all(b"0123456789")?;
            assert!(writer.bytes.capacity() <= writer.limit);
        }
        assert_eq!(
            writer.write_all(&[0; 25]).err().map(|error| error.kind()),
            Some(io::ErrorKind::FileTooLarge)
        );
        assert_eq!(writer.bytes.len(), 1000);
        assert!(writer.bytes.capacity() <= writer.limit);
        Ok(())
    }
}
