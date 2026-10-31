//! Exact scalar Philox draws for the pinned Qwen semantic and predictor samplers.
//!
//! Ported from `src/philox.h` in qwentts.cpp revision
//! `5916bf7c79dbb4e66fc059cdc507a79a23a28bb4`, under the MIT license reproduced
//! in `LICENSE.qwen-core` (Copyright 2023-2026 The omnivoice.cpp authors).
//! Integer operations follow Philox4x32-10; the final conversion deliberately
//! preserves the reference's separate f32 rounding, including a possible 1.0.
#![forbid(unsafe_code)]

const MULTIPLIERS: [u32; 2] = [0xd251_1f53, 0xcd9e_8d57];
const KEY_INCREMENTS: [u32; 2] = [0x9e37_79b9, 0xbb67_ae85];

const fn words(value: u64) -> [u32; 2] {
    let [low0, low1, low2, low3, high0, high1, high2, high3] = value.to_le_bytes();
    [
        u32::from_le_bytes([low0, low1, low2, low3]),
        u32::from_le_bytes([high0, high1, high2, high3]),
    ]
}

fn block(mut counter: [u32; 4], mut key: [u32; 2]) -> [u32; 4] {
    for round in 0..10 {
        let (low0, high0) = MULTIPLIERS[0].carrying_mul(counter[0], 0);
        let (low1, high1) = MULTIPLIERS[1].carrying_mul(counter[2], 0);
        counter = [
            high1 ^ counter[1] ^ key[0],
            low1,
            high0 ^ counter[3] ^ key[1],
            low0,
        ];
        if round < 9 {
            key[0] = key[0].wrapping_add(KEY_INCREMENTS[0]);
            key[1] = key[1].wrapping_add(KEY_INCREMENTS[1]);
        }
    }
    counter
}

#[allow(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "The pinned C++ expression intentionally rounds the u32 word to f32 before adding 0.5."
)]
fn uniform_word(word: u32) -> f32 {
    (word as f32 + 0.5) * 2.328_306_4e-10
}

/// Draw the reference's first lane from one independently addressed subsequence.
#[allow(
    clippy::redundant_pub_crate,
    reason = "The sampler helper stays private to the speech crate and its native ABI module."
)]
pub(super) fn uniform(seed: i64, subsequence: i64, counter: u32) -> f32 {
    let key = words(seed.cast_unsigned());
    let [subsequence_low, subsequence_high] = words(subsequence.cast_unsigned());
    uniform_word(block([counter, 0, subsequence_low, subsequence_high], key)[0])
}

#[cfg(test)]
mod tests {
    use super::{block, uniform, uniform_word};

    #[test]
    fn philox_matches_random123_known_answer_blocks() {
        for (counter, key, expected) in [
            (
                [0; 4],
                [0; 2],
                [0x6627_e8d5, 0xe169_c58d, 0xbc57_ac4c, 0x9b00_dbd8],
            ),
            (
                [u32::MAX; 4],
                [u32::MAX; 2],
                [0x408f_276d, 0x41c8_3b0e, 0xa20b_c7c6, 0x6d54_51fd],
            ),
            (
                [0x243f_6a88, 0x85a3_08d3, 0x1319_8a2e, 0x0370_7344],
                [0xa409_3822, 0x299f_31d0],
                [0xd16c_fe09, 0x94fd_cceb, 0x5001_e420, 0x2412_6ea1],
            ),
        ] {
            assert_eq!(block(counter, key), expected);
        }
    }

    #[test]
    fn philox_uniform_preserves_reference_float_rounding_and_endpoints() {
        for (word, expected) in [
            (0, 0x2f00_0000),
            (1, 0x2fc0_0000),
            (0x00ff_ffff, 0x3b80_0000),
            (0x0100_0000, 0x3b80_0000),
            (0x0100_0001, 0x3b80_0000),
            (u32::MAX, 0x3f80_0000),
        ] {
            assert_eq!(uniform_word(word).to_bits(), expected);
        }
    }

    #[test]
    fn philox_scalar_draw_uses_full_signed_seed_subsequence_and_counter_bits() {
        for seed in [i64::MIN, -1, 0, 42, i64::MAX] {
            for subsequence in [i64::MIN, -1, 0, 1, 0xffff_ffff, 0x1_0000_0000, i64::MAX] {
                for counter in [0, 1, u32::MAX] {
                    let draw = uniform(seed, subsequence, counter);
                    assert!(draw.is_finite() && draw > 0.0 && draw <= 1.0);
                }
            }
        }
        assert_ne!(
            uniform(0, 0, 0).to_bits(),
            uniform(i64::MIN, 0, 0).to_bits()
        );
        assert_ne!(
            uniform(0, 0, 0).to_bits(),
            uniform(0, i64::MIN, 0).to_bits()
        );
        assert_ne!(
            uniform(0, 0xffff_ffff, 0).to_bits(),
            uniform(0, 0x1_0000_0000, 0).to_bits()
        );
        assert_ne!(
            uniform(0, 0, 0).to_bits(),
            uniform(0, 0, u32::MAX).to_bits()
        );
    }
}
