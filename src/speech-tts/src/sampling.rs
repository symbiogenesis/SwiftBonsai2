//! Exact masking, repetition and greedy selection from the pinned Qwen sampler.
//!
//! Ported from `src/sampling.h` in qwentts.cpp revision
//! `5916bf7c79dbb4e66fc059cdc507a79a23a28bb4`, under the MIT license reproduced
//! in `LICENSE.qwen-core` (Copyright 2023-2026 The omnivoice.cpp authors).
//! Immutable independently captured observations retain the native control;
//! `build_overlay.rs` guards the downloaded source by normalized length and
//! SHA-256. The remaining probability pipeline stays native. Native storage
//! supplies the reusable seen workspace; this policy performs no allocation.
//! Original header SHA-256:
//! `9812a8b428efffcfe3d832409f3eae092cb3be90bfeefbf141f75df0320e2241`.
#![forbid(unsafe_code)]

/// Select the first maximum using the pinned `std::max_element` comparisons.
///
/// Equal values retain the earlier token, a leading NaN remains selected, and
/// later NaNs are ignored. Empty input returns zero without accessing storage.
#[allow(
    clippy::redundant_pub_crate,
    reason = "Sampler policy is private to the speech crate and its native ABI module."
)]
pub(super) fn greedy_argmax(logits: &[f32]) -> usize {
    let mut best = 0;
    for (index, &candidate) in logits.iter().enumerate().skip(1) {
        if logits[best] < candidate {
            best = index;
        }
    }
    best
}

/// Mask the clipped half-open interval, preserving the optional kept token.
#[allow(
    clippy::redundant_pub_crate,
    reason = "Sampler policy is private to the speech crate and its native ABI module."
)]
pub(super) fn suppress(logits: &mut [f32], low: i32, high: i32, keep: i32) {
    let low = usize::try_from(low.max(0)).unwrap_or(usize::MAX);
    let high = usize::try_from(high).unwrap_or(0).min(logits.len());
    let keep = usize::try_from(keep).ok();
    for (index, logit) in logits.iter_mut().enumerate().take(high).skip(low) {
        if Some(index) != keep {
            *logit = f32::NEG_INFINITY;
        }
    }
}

/// Apply the pinned sign-sensitive penalty once per valid history token.
///
/// The caller supplies one workspace byte per logit. A smaller workspace is
/// rejected before mutation; the audited native wrapper always supplies the
/// complete vocabulary extent.
#[allow(
    clippy::redundant_pub_crate,
    clippy::float_cmp,
    reason = "Sampler policy is private to the speech crate and its native ABI module."
)]
pub(super) fn repetition_penalty(
    logits: &mut [f32],
    history: &[i32],
    penalty: f32,
    seen: &mut [u8],
) -> bool {
    if penalty == 1.0 || history.is_empty() {
        return true;
    }
    let Some(seen) = seen.get_mut(..logits.len()) else {
        return false;
    };
    seen.fill(0);
    for &token in history {
        let Ok(token) = usize::try_from(token) else {
            continue;
        };
        let Some(logit) = logits.get_mut(token) else {
            continue;
        };
        if seen[token] == 0 {
            seen[token] = 1;
            // The explicit comparison preserves -0 and NaN behavior of the
            // pinned C++ ternary; signbit and abs-based rewrites differ.
            *logit = if *logit < 0.0 {
                *logit * penalty
            } else {
                *logit / penalty
            };
        }
    }
    true
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::float_cmp,
        reason = "Exact zero, infinity and integral results describe the pinned sampler contract."
    )]
    use super::{greedy_argmax, repetition_penalty, suppress};

    #[test]
    fn greedy_selection_retains_first_ties_and_pinned_nan_ordering() {
        let nan = f32::from_bits(0x7fc0_1234);
        for (logits, expected) in [
            (vec![], 0),
            (vec![nan], 0),
            (vec![nan, f32::INFINITY], 0),
            (vec![-0.0, 0.0], 0),
            (vec![0.0, -0.0], 0),
            (vec![1.0, nan, 2.0, 2.0], 2),
            (vec![f32::NEG_INFINITY; 3], 0),
            (
                vec![f32::NEG_INFINITY, f32::INFINITY, nan, f32::INFINITY],
                1,
            ),
        ] {
            let original: Vec<_> = logits.iter().map(|value| value.to_bits()).collect();
            assert_eq!(greedy_argmax(&logits), expected);
            assert_eq!(
                logits
                    .iter()
                    .map(|value| value.to_bits())
                    .collect::<Vec<_>>(),
                original
            );
        }
    }

    #[test]
    fn suppression_clips_ranges_and_preserves_kept_bits() {
        let original = [0.0, -0.0, f32::from_bits(0x7fc0_1234), f32::INFINITY];
        let mut logits = original;
        suppress(&mut logits, i32::MIN, i32::MAX, 2);
        assert_eq!(logits[2].to_bits(), original[2].to_bits());
        assert_eq!(logits[0], f32::NEG_INFINITY);
        assert_eq!(logits[1], f32::NEG_INFINITY);
        assert_eq!(logits[3], f32::NEG_INFINITY);
        for (low, high) in [(3, 1), (0, -1), (5, 8), (0, 0)] {
            let mut logits = original;
            suppress(&mut logits, low, high, -1);
            assert_eq!(logits.map(f32::to_bits), original.map(f32::to_bits));
        }
        suppress(&mut [], i32::MIN, i32::MAX, -1);
    }

    #[test]
    fn repetition_ignores_invalid_and_duplicate_tokens_and_reuses_workspace() {
        let mut logits = [-4.0, 4.0, -0.0, f32::NEG_INFINITY];
        let mut seen = [255; 4];
        assert!(repetition_penalty(
            &mut logits,
            &[-1, 0, 1, 1, 0, 2, 3, 4, i32::MAX],
            2.0,
            &mut seen,
        ));
        assert_eq!(
            logits.map(f32::to_bits),
            [-8.0, 2.0, -0.0, f32::NEG_INFINITY].map(f32::to_bits)
        );
        assert!(repetition_penalty(&mut logits, &[0, 0, 1], 0.5, &mut seen));
        assert_eq!(logits[..2], [-4.0, 4.0]);
    }

    #[test]
    fn no_op_and_rejected_workspace_preserve_logits() {
        let original = [f32::from_bits(0x7fc0_1234), -0.0];
        let mut logits = original;
        assert!(repetition_penalty(&mut logits, &[0, 1], 1.0, &mut []));
        assert!(repetition_penalty(&mut logits, &[], 2.0, &mut []));
        assert!(!repetition_penalty(&mut logits, &[0, 1], 2.0, &mut [0]));
        assert_eq!(logits.map(f32::to_bits), original.map(f32::to_bits));
    }
}
