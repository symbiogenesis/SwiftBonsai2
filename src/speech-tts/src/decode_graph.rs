//! Safe decode-workspace selection for the pinned native Qwen engine.
//!
//! The CPU worker finishes and synchronizes each compute before selecting its
//! next workspace. GGML still owns graph allocation and release; this module
//! decides which completed window can be reused and when others must be freed.
#![forbid(unsafe_code)]

/// Select the zero-based 256-token window without signed addition overflow.
#[allow(
    clippy::redundant_pub_crate,
    reason = "The policy is private to the speech crate and its native ABI module."
)]
pub(super) fn window_index(padded_tokens: i32, windows: usize) -> Option<usize> {
    let tokens = usize::try_from(padded_tokens).ok().filter(|&n| n != 0)?;
    let index = (tokens - 1) / 256;
    (index < windows).then_some(index)
}

/// Evict completed unselected windows before a missing or different-width build.
#[allow(
    clippy::redundant_pub_crate,
    reason = "The policy is private to the speech crate and its native ABI module."
)]
pub(super) const fn rebuild(has_context: bool, cached_width: i32, requested_width: i32) -> bool {
    !has_context || cached_width != requested_width
}

#[cfg(test)]
mod tests {
    use super::{rebuild, window_index};

    #[test]
    fn selection_preserves_every_window_boundary_and_rejects_invalid_extents() {
        for window in 0..16 {
            let start = window * 256 + 1;
            let end = (window + 1) * 256;
            for tokens in start..=end {
                assert_eq!(window_index(tokens, 16), usize::try_from(window).ok());
            }
        }
        for tokens in [i32::MIN, -1, 0, 4097, i32::MAX] {
            assert_eq!(window_index(tokens, 16), None);
        }
        assert_eq!(window_index(256, 0), None);
        assert_eq!(window_index(256, 1), Some(0));
        assert_eq!(window_index(257, 1), None);
    }

    #[test]
    fn reuse_requires_a_live_context_with_the_requested_batch_width() {
        for requested_width in 1..=16 {
            assert!(!rebuild(true, requested_width, requested_width));
            assert!(rebuild(false, requested_width, requested_width));
            assert!(rebuild(false, 0, requested_width));
            for cached_width in 1..=16 {
                assert_eq!(
                    rebuild(true, cached_width, requested_width),
                    cached_width != requested_width,
                );
            }
        }
    }

    #[test]
    fn fresh_requests_and_failed_replacements_have_no_stale_selection_state() {
        for tokens in [256, 4096, 256, 1024, 1024, 256] {
            assert_eq!(
                window_index(tokens, 16),
                usize::try_from(tokens / 256 - 1).ok()
            );
            // Allocation failure leaves no context; retry must rebuild even
            // if a native slot still records the previous width.
            assert!(rebuild(false, 2, 2));
            assert!(!rebuild(true, 2, 2));
        }
    }
}
