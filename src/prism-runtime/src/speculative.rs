//! Checked scheduling bounds for multi-token prediction.

/// Whether catch-up rows and sequence anchors fit the first decoder batch.
///
/// The addition uses a widened integer so valid `i32` inputs never overflow.
/// A nonpositive batch or a negative row/anchor count is rejected.
#[must_use]
pub fn mtp_first_decode_fits(batch: i32, catchup_rows: i32, anchors: i32) -> bool {
    batch > 0
        && catchup_rows >= 0
        && anchors >= 0
        && i64::from(catchup_rows) + i64::from(anchors) <= i64::from(batch)
}

#[cfg(test)]
mod tests {
    use super::mtp_first_decode_fits;

    #[test]
    fn first_decode_boundaries_and_widened_sum() {
        assert!(mtp_first_decode_fits(32, 31, 1));
        assert!(!mtp_first_decode_fits(32, 32, 1));
        assert!(!mtp_first_decode_fits(0, 0, 0));
        assert!(!mtp_first_decode_fits(32, -1, 1));
        assert!(!mtp_first_decode_fits(32, 0, -1));
        assert!(mtp_first_decode_fits(i32::MAX, i32::MAX, 0));
        assert!(!mtp_first_decode_fits(i32::MAX, i32::MAX, i32::MAX));
    }
}
