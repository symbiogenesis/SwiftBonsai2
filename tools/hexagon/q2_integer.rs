//! CPU control for a conditional scale-one signed Q2 integer-dot adapter.
//!
//! Logical Q2 records and F32 inputs are represented here, not the DSP's repacked
//! tiles or RPC ABI. The signed kernel's internal FP16 inverse/rounding, operation
//! entry, shapes and actual returned bits still require independent qualification.
//! No prepared Q8_1 upload, native submission or performance claim follows.

use super::{Error, PQ2_BYTES, finite, half};

const PADDED_K: usize = 256;
const Q2_BLOCK_BYTES: usize = 18;

/// One original 128-symbol PQ2 block expressed as bounded integer work units.
///
/// Original quantization and scales remain CPU-owned. Two F32 activation limbs
/// use at most 31 real codes per 32 lanes, plus a zero-weight +/-127 guard.
/// Each guard opposes its real-code sum, bounding the complete sum by 1857.
/// This is a CPU arithmetic/layout control, not a supported device adapter.
pub struct GuardedQ2Inputs {
    weights: [u8; 4 * Q2_BLOCK_BYTES],
    activation: [[f32; PADDED_K]; 2],
}

impl GuardedQ2Inputs {
    /// Preserve all four original symbols and split existing Q8_K codes exactly.
    ///
    /// This performs no activation quantization. The caller supplies the original
    /// CPU-prepared 128 codes. Original PQ2 scale bytes are validated but are not
    /// moved into the integer primitive; its four logical Q2 scales are one.
    pub fn new(original_pq2: &[u8], prepared_codes: &[i8]) -> Result<Self, Error> {
        let original_pq2: &[u8; PQ2_BYTES] = original_pq2.try_into().map_err(|_| Error::Extent)?;
        if prepared_codes.len() != 128 {
            return Err(Error::Extent);
        }
        half(original_pq2)?;
        let mut result = Self {
            weights: [0x55; 4 * Q2_BLOCK_BYTES],
            activation: [[0.0; PADDED_K]; 2],
        };
        for block in result.weights.chunks_exact_mut(Q2_BLOCK_BYTES) {
            block[..2].copy_from_slice(&0x3c00_u16.to_le_bytes());
        }
        for (lane, &code) in prepared_codes.iter().enumerate() {
            let destination = lane / 31 * 32 + lane % 31;
            let symbol = (original_pq2[2 + lane / 4] >> (lane % 4 * 2)) & 3;
            let byte = destination / 64 * Q2_BLOCK_BYTES + 2 + destination % 64 / 4;
            let shift = destination % 4 * 2;
            result.weights[byte] = (result.weights[byte] & !(3 << shift)) | symbol << shift;
            let first = i16::from(code) / 2;
            result.activation[0][destination] = f32::from(first);
            result.activation[1][destination] = f32::from(i16::from(code) - first);
        }
        for limb in &mut result.activation {
            for group in limb.chunks_exact_mut(32) {
                let sum: f32 = group[..31].iter().sum();
                group[31] = if sum >= 0.0 { -127.0 } else { 127.0 };
            }
        }
        Ok(result)
    }

    /// Four ordinary logical 64-element Q2_0 records, all with raw FP16 scale one.
    ///
    /// This is not the signed kernel's 32-row/640-byte repacked tile format.
    #[must_use]
    pub const fn logical_q2_blocks(&self) -> &[u8; 72] {
        &self.weights
    }

    /// Two padded F32 rows for a future separately checked N=2 operation.
    ///
    /// The native operation must quantize these guarded values through its actual
    /// Q8_1 path; no interface for injecting prepared DSP tiles is assumed.
    #[must_use]
    pub const fn activation_rows(&self) -> &[[f32; PADDED_K]; 2] {
        &self.activation
    }

    /// Replay intended scale-one Q8_1 integer arithmetic on CPU.
    ///
    /// Every 32-lane maximum is exactly 127; intended scale and inverse are one.
    /// Integer values and bounded sums are exactly representable as FP16. This
    /// does not execute or independently emulate the vendor reciprocal polynomial
    /// or HVX conversion intrinsics; those remain a signed device parity gate.
    pub fn cpu_control_dots(&self) -> Result<[f32; 2], Error> {
        let mut outputs = [0.0; 2];
        for (limb, output) in self.activation.iter().zip(&mut outputs) {
            // Match the signed Q2 structure: two Q8_1 32-code corrections per
            // 64-weight block, then scale-one addition into the F32 accumulator.
            for block_index in 0..4 {
                let block =
                    &self.weights[block_index * Q2_BLOCK_BYTES..(block_index + 1) * Q2_BLOCK_BYTES];
                let mut block_dot = 0.0;
                for half_index in 0..2 {
                    let start = block_index * 64 + half_index * 32;
                    let values = &limb[start..start + 32];
                    let maximum = values
                        .iter()
                        .map(|value| value.abs())
                        .fold(0.0_f32, f32::max);
                    let sum: f32 = values.iter().sum();
                    if maximum != 127.0 || sum.abs() > 1857.0 || sum.fract() != 0.0 {
                        return Err(Error::Protocol);
                    }
                    let mut unsigned_dot = 0.0;
                    for (lane, &value) in values.iter().enumerate() {
                        if value.fract() != 0.0 {
                            return Err(Error::Protocol);
                        }
                        let index = half_index * 32 + lane;
                        let symbol = (block[2 + index / 4] >> (index % 4 * 2)) & 3;
                        unsigned_dot += f32::from(symbol) * value;
                    }
                    block_dot += unsigned_dot - sum;
                }
                *output += block_dot;
            }
        }
        Ok(outputs)
    }
}

/// Apply the original per-128 PQ2 ordered CPU FMA to exact returned integer dots.
///
/// The original Q8_K scale is reused without requantization. Fractional,
/// nonfinite or out-of-range primitive results reject before changing a caller's
/// accumulator. Integer-looking wrong device results still require independent
/// parity evidence; these checks cannot authenticate device execution.
pub fn reduce_original_block(
    original_pq2: &[u8],
    original_q8k_scale: f32,
    previous: f32,
    limb_dots: [f32; 2],
) -> Result<f32, Error> {
    let original_pq2: &[u8; PQ2_BYTES] = original_pq2.try_into().map_err(|_| Error::Extent)?;
    finite(original_q8k_scale)?;
    finite(previous)?;
    for value in limb_dots {
        if !value.is_finite() || value.abs() > 16_384.0 || value.fract() != 0.0 {
            return Err(Error::Protocol);
        }
    }
    let integer = limb_dots[0] + limb_dots[1];
    if integer.abs() > 32_768.0 {
        return Err(Error::Protocol);
    }
    // Original integer-dot -> f32 conversion produces positive zero. A device
    // may return negative floating zero even for the same mathematical integer.
    let integer = if integer == 0.0 { 0.0 } else { integer };
    finite((half(original_pq2)? * original_q8k_scale).mul_add(integer, previous))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_symbol_and_full_i8_range_preserves_integer_dots_and_half_sum_bounds()
    -> Result<(), Error> {
        for packed in 0..=u8::MAX {
            let mut original = [packed; 34];
            original[..2].copy_from_slice(&0x3c00_u16.to_le_bytes());
            for code in i8::MIN..=i8::MAX {
                let codes = [code; 128];
                let inputs = GuardedQ2Inputs::new(&original, &codes)?;
                let dots = inputs.cpu_control_dots()?;
                let expected = (0..128)
                    .map(|lane| (i32::from((packed >> (lane % 4 * 2)) & 3) - 1) * i32::from(code))
                    .sum::<i32>();
                assert_eq!(dots[0] + dots[1], expected as f32);
                for (lane, &code) in codes.iter().enumerate() {
                    let destination = lane / 31 * 32 + lane % 31;
                    let byte = destination / 64 * Q2_BLOCK_BYTES + 2 + destination % 64 / 4;
                    assert_eq!(
                        (inputs.weights[byte] >> (destination % 4 * 2)) & 3,
                        (packed >> (lane % 4 * 2)) & 3,
                    );
                    assert_eq!(
                        inputs.activation[0][destination] + inputs.activation[1][destination],
                        f32::from(code)
                    );
                }
                for block in inputs.logical_q2_blocks().chunks_exact(18) {
                    assert_eq!(&block[..2], &0x3c00_u16.to_le_bytes());
                }
            }
        }
        for seed in 0..256 {
            let mut original = [0; 34];
            original[..2].copy_from_slice(&0x3c00_u16.to_le_bytes());
            for (index, byte) in original[2..].iter_mut().enumerate() {
                *byte = ((index * 37 + seed) % 256) as u8;
            }
            let codes: Vec<_> = (0..128)
                .map(|lane| i8::from_ne_bytes([((lane * 53 + seed) % 256) as u8]))
                .collect();
            let inputs = GuardedQ2Inputs::new(&original, &codes)?;
            let dots = inputs.cpu_control_dots()?;
            let expected: i32 = codes
                .iter()
                .enumerate()
                .map(|(lane, &code)| {
                    (i32::from((original[2 + lane / 4] >> (lane % 4 * 2)) & 3) - 1)
                        * i32::from(code)
                })
                .sum();
            assert_eq!(dots[0] + dots[1], expected as f32);
        }
        Ok(())
    }

    #[test]
    fn retained_original_fma_control_rejects_full_projection_nonfused_reduction()
    -> Result<(), Error> {
        let mut first = [0xaa; 34];
        first[..2].copy_from_slice(&0x3c01_u16.to_le_bytes());
        let mut second = [0xaa; 34];
        second[..2].copy_from_slice(&0x3787_u16.to_le_bytes());
        let first_dots = GuardedQ2Inputs::new(&first, &[3; 128])?.cpu_control_dots()?;
        let second_dots = GuardedQ2Inputs::new(&second, &[-13; 128])?.cpu_control_dots()?;
        let scale = f32::from_bits(0x3da0_0001);
        let prior = reduce_original_block(&first, scale, 0.0, first_dots)?;
        let result = reduce_original_block(&second, scale, prior, second_dots)?;
        assert_eq!(result.to_bits(), 0xc1f9_0b01);
        let nonfused = (half(&first)? * scale) * (first_dots[0] + first_dots[1])
            + (half(&second)? * scale) * (second_dots[0] + second_dots[1]);
        assert_eq!(nonfused.to_bits(), 0xc1f9_0b02);
        assert_eq!(
            reduce_original_block(&first, 1.0, -0.0, [-0.0, -0.0])?.to_bits(),
            0.0_f32.to_bits(),
        );
        for invalid in [f32::NAN, f32::INFINITY, 0.5, 16_385.0] {
            assert_eq!(
                reduce_original_block(&first, scale, 0.0, [invalid, 0.0]),
                Err(Error::Protocol)
            );
        }
        let oversized = [0; PQ2_BYTES + 1];
        for malformed in [&first[..0], &first[..PQ2_BYTES - 1], &oversized[..]] {
            assert_eq!(
                GuardedQ2Inputs::new(malformed, &[0; 128]).err(),
                Some(Error::Extent)
            );
            assert_eq!(
                reduce_original_block(malformed, scale, 0.0, [0.0, 0.0]),
                Err(Error::Extent)
            );
        }
        Ok(())
    }
}
