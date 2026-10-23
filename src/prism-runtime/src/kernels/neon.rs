//! Experimental baseline NEON integer arithmetic for one native PQ2 block.

use fearless_simd::{Neon, i8x16, i32x4, prelude::*, u8x16, u16x8};

#[inline]
fn decode(packed: u8x16<Neon>) -> [i8x16<Neon>; 4] {
    let a = packed & 3;
    let b = (packed >> 2) & 3;
    let c = (packed >> 4) & 3;
    let d = packed >> 6;
    // Interleave the four digits of each byte into consecutive activation
    // order. Pairing bytes as u16 lanes keeps each adjacent pair together.
    let ab_low: u16x8<Neon> = a.zip_low(b).bitcast();
    let ab_high: u16x8<Neon> = a.zip_high(b).bitcast();
    let cd_low: u16x8<Neon> = c.zip_low(d).bitcast();
    let cd_high: u16x8<Neon> = c.zip_high(d).bitcast();
    [
        ab_low.zip_low(cd_low).bitcast::<i8x16<Neon>>() - 1,
        ab_low.zip_high(cd_low).bitcast::<i8x16<Neon>>() - 1,
        ab_high.zip_low(cd_high).bitcast::<i8x16<Neon>>() - 1,
        ab_high.zip_high(cd_high).bitcast::<i8x16<Neon>>() - 1,
    ]
}

#[inline]
fn dot16(weights: i8x16<Neon>, activations: i8x16<Neon>) -> i32x4<Neon> {
    let (weight_low, weight_high) = weights.widen();
    let (activation_low, activation_high) = activations.widen();
    // Signed widening preserves -128. Every i16 product lies in -256..=254;
    // widening each product before addition prevents narrow accumulation.
    let (low_low, low_high) = (weight_low * activation_low).widen();
    let (high_low, high_high) = (weight_high * activation_high).widen();
    low_low + low_high + high_low + high_high
}

#[inline]
fn scalar(packed: &[u8; 32], activations: &[i8; 128]) -> i32 {
    activations
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let weight = i32::from((packed[index / 4] >> (2 * (index % 4))) & 3) - 1;
            weight * i32::from(*value)
        })
        .sum()
}

/// Computes the exact integer dot of 128 consecutive native PQ2 symbols.
///
/// Codes 0..=3 decode to -1..=2, including the native fourth symbol. Arrays
/// establish the complete extents; safe slice loads require no alignment.
/// The absolute sum is at most 128 * 2 * 128 = 32,768, so all i32 partial
/// sums and the horizontal reduction fit, including activation -128.
#[inline]
pub(super) fn dot(packed: &[u8; 32], activations: &[i8; 128]) -> i32 {
    // This statically selected baseline token performs no runtime detection.
    // The Option branch disappears on the qualified baseline target; retain
    // defined scalar behavior if the dependency cannot provide the token.
    let Some(neon) = fearless_simd::Level::baseline().as_neon() else {
        return scalar(packed, activations);
    };
    let mut total = i32x4::splat(neon, 0);
    for (bytes, values) in packed
        .as_chunks::<16>()
        .0
        .iter()
        .zip(activations.as_chunks::<64>().0)
    {
        let decoded = decode(u8x16::from_slice(neon, bytes));
        for (weights, activations) in decoded.into_iter().zip(values.as_chunks::<16>().0) {
            total += dot16(weights, i8x16::from_slice(neon, activations));
        }
    }
    total.reduce_sum()
}

#[cfg(test)]
mod tests {
    use super::dot;

    type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

    fn reference(packed: &[u8; 32], activations: &[i8; 128]) -> i32 {
        let mut sum = 0;
        for (symbols, values) in packed.iter().zip(activations.as_chunks::<4>().0) {
            for (digit, value) in values.iter().enumerate() {
                let code = (symbols >> (digit * 2)) & 3;
                sum += (i32::from(code) - 1) * i32::from(*value);
            }
        }
        sum
    }

    #[test]
    fn simd_neon_integer_dot_exhaustive_packed_bytes() {
        let mut activations = std::array::from_fn(|index: usize| {
            i8::from_ne_bytes([index.to_le_bytes()[0].wrapping_mul(197).wrapping_add(13)])
        });
        activations[..6].copy_from_slice(&[i8::MIN, -127, -1, 0, 1, i8::MAX]);
        for position in 0..32 {
            for byte in 0..=u8::MAX {
                let mut packed = [0x55; 32];
                packed[position] = byte;
                assert_eq!(dot(&packed, &activations), reference(&packed, &activations));
            }
        }
    }

    #[test]
    fn simd_neon_integer_dot_each_lane_and_extremes() {
        for position in 0..128 {
            for code in 0..=3 {
                let mut packed = [0x55; 32];
                let shift = 2 * (position % 4);
                packed[position / 4] &= !(3 << shift);
                packed[position / 4] |= code << shift;
                for value in [i8::MIN, -127, -1, 0, 1, i8::MAX] {
                    let mut activations = [0; 128];
                    activations[position] = value;
                    let expected = (i32::from(code) - 1) * i32::from(value);
                    assert_eq!(dot(&packed, &activations), expected);
                }
            }
        }
        for byte in 0..=u8::MAX {
            for value in [i8::MIN, -127, -1, 0, 1, i8::MAX] {
                let packed = [byte; 32];
                let activations = [value; 128];
                assert_eq!(dot(&packed, &activations), reference(&packed, &activations));
            }
        }
        assert_eq!(dot(&[0xff; 32], &[i8::MIN; 128]), -32_768);
        assert_eq!(dot(&[0xff; 32], &[i8::MAX; 128]), 32_512);
    }

    #[test]
    fn simd_neon_integer_dot_unaligned_random_blocks() -> TestResult {
        #[repr(align(16))]
        struct Aligned<T>(T);

        let mut packed_storage = Aligned([0; 48]);
        let mut activation_storage = Aligned([0; 144]);
        let mut state = 0x9e37_79b9_u32;
        for _ in 0..4096 {
            let mut next_byte = || {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                state.to_le_bytes()[0]
            };
            for byte in &mut packed_storage.0[1..33] {
                *byte = next_byte();
            }
            for value in &mut activation_storage.0[3..131] {
                *value = i8::from_ne_bytes([next_byte()]);
            }
            let packed = <&[u8; 32]>::try_from(&packed_storage.0[1..33])?;
            let activations = <&[i8; 128]>::try_from(&activation_storage.0[3..131])?;
            assert_eq!(dot(packed, activations), reference(packed, activations));
        }
        Ok(())
    }
}
