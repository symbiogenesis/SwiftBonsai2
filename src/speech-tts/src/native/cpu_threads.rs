//! Safe Rust CPU-thread policy for the private native Qwen boundary.
#![forbid(unsafe_code)]

use std::{
    ffi::{c_int, c_uint},
    io,
    sync::atomic::{AtomicI32, Ordering},
};

/// One process-wide explicit override, read by native backend initialization.
pub(super) struct CpuThreads(AtomicI32);

impl CpuThreads {
    /// Retain the pinned native fallback until the first explicit selection.
    pub(super) const fn new() -> Self {
        Self(AtomicI32::new(0))
    }

    /// Select a bounded override without modifying it on rejected input.
    pub(super) fn set(&self, threads: usize) -> io::Result<()> {
        let threads = c_int::try_from(threads)
            .ok()
            .filter(|count| (1..=256).contains(count))
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Invalid speech CPU thread count.",
                )
            })?;
        self.0.store(threads, Ordering::Relaxed);
        Ok(())
    }

    /// Apply the original override or logical/2, minimum-one native heuristic.
    pub(super) fn get(&self, logical_threads: c_uint) -> c_int {
        let selected = self.0.load(Ordering::Relaxed);
        if selected > 0 {
            return selected;
        }
        // Match the pinned Windows C++ unsigned-to-int cast, including its
        // two's-complement interpretation, before the signed division by two.
        let logical_threads = logical_threads.cast_signed();
        (logical_threads / 2).max(1)
    }
}

#[cfg(test)]
mod tests {
    use super::CpuThreads;
    use std::io;

    #[test]
    fn native_thread_fallback_preserves_zero_odd_and_signed_cast_boundaries() {
        let selection = CpuThreads::new();
        for (logical, expected) in [
            (0, 1),
            (1, 1),
            (2, 1),
            (3, 1),
            (4, 2),
            (17, 8),
            (18, 9),
            (256, 128),
            (0x7fff_ffff, 1_073_741_823),
            (0x8000_0000, 1),
            (u32::MAX, 1),
        ] {
            assert_eq!(selection.get(logical), expected);
        }
    }

    #[test]
    fn native_thread_override_is_bounded_and_rejected_updates_preserve_selection() -> io::Result<()>
    {
        let selection = CpuThreads::new();
        for count in 1..=256 {
            selection.set(count)?;
            for logical in [0, 1, 18, u32::MAX] {
                assert_eq!(usize::try_from(selection.get(logical)).ok(), Some(count));
            }
        }
        for count in [0, 257, usize::MAX] {
            assert_eq!(
                selection.set(count).err().map(|error| error.kind()),
                Some(io::ErrorKind::InvalidInput)
            );
            assert_eq!(selection.get(18), 256);
        }
        Ok(())
    }
}
