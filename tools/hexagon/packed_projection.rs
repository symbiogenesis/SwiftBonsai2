//! Model-free, safe Rust control for a bounded PQ2/Q8_K projection schedule.
//!
//! This runs on the CPU and opens no device. It specifies packed tile boundaries,
//! exact pinned Swift activation/reduction arithmetic, resource reservations and
//! cancellation without publishing a partial projection. A signed HVX/HMX kernel,
//! DMA/session ownership, actual-model identities and logits/state parity remain
//! separate gates. See `packed-projection.md` and `packed-sources.json`.
#![forbid(unsafe_code)]
#![deny(warnings)]

const MAX_COLUMNS: usize = 5120;
const MAX_ROWS: usize = 17408;
const PQ2_BYTES: usize = 34;
const Q8K_BYTES: usize = 292;

mod packed_tiles;
pub use packed_tiles::{TileReceiver, TileSource, TileWireLedger};
mod q2_integer;
pub use q2_integer::{GuardedQ2Inputs, reduce_original_block};

/// Fixed validation, allocation, cancellation or numerical rejection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// Unsupported or inconsistent geometry, tile limit or byte extent.
    Extent,
    /// Requested schedule exceeds the explicitly supplied VTCM allowance.
    Capacity,
    /// Nonfinite input, scale, intermediate or result; no partial output survives.
    Nonfinite,
    /// Scratch allocation failed before committed output publication.
    Allocation,
    /// The external cancellation/deadline check refused a tile.
    Cancelled,
    /// Explicit Hadamard signs have the wrong extent or contain other values.
    Signs,
    /// A versioned packed-tile payload, sequence or terminal receiver was invalid.
    Protocol,
}

/// Bounded N=1 packed schedule; it grants no device or profile admission.
#[derive(Clone, Copy, Debug)]
pub struct Plan {
    columns: usize,
    rows: usize,
    tile_rows: usize,
    tile_q8k: usize,
}

/// Exact storage reservations and logical traffic for the proposed schedule.
///
/// These are derived bounds, not measured DMA, mapping or RPC observations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ledger {
    /// Borrowed, original packed weights; there is no persistent Q2 copy.
    pub packed_weight_bytes: usize,
    /// Serialized Q8_K activations prepared once on CPU.
    pub activation_bytes: usize,
    /// Complete f32 output, held privately until every tile succeeds.
    pub output_bytes: usize,
    /// Maximum one-buffer packed CPU staging allocation.
    pub cpu_weight_stage_bytes: usize,
    /// Proposed device bound: two 64-aligned weight stages, activation tile and sums.
    pub proposed_vtcm_bytes: usize,
    /// Weight bytes consumed once by this complete N=1 schedule.
    pub logical_weight_read_bytes: usize,
    /// Activation records transferred once per row tile by the proposed schedule.
    pub proposed_activation_transfer_bytes: usize,
    /// Completed projection returned to CPU once; no intermediate state transfer.
    pub proposed_output_transfer_bytes: usize,
}

fn aligned(value: usize) -> usize {
    value.div_ceil(64) * 64
}

impl Plan {
    /// Validate a bounded first-projection-or-smaller N=1 schedule.
    ///
    /// Columns must be divisible by 256, at most 5120; rows at most 17408.
    /// Tiles contain at most 64 rows and 20 complete Q8_K groups. The supplied
    /// VTCM allowance must come from independently verified effective allocation;
    /// a capability page size alone is insufficient. This method allocates none.
    pub fn new(
        columns: usize,
        rows: usize,
        tile_rows: usize,
        tile_q8k: usize,
        effective_vtcm_allowance: usize,
    ) -> Result<Self, Error> {
        if columns == 0
            || columns > MAX_COLUMNS
            || !columns.is_multiple_of(256)
            || rows == 0
            || rows > MAX_ROWS
            || !(1..=64).contains(&tile_rows)
            || !(1..=20).contains(&tile_q8k)
        {
            return Err(Error::Extent);
        }
        let plan = Self {
            columns,
            rows,
            tile_rows: tile_rows.min(rows),
            tile_q8k: tile_q8k.min(columns / 256),
        };
        if plan.ledger().proposed_vtcm_bytes > effective_vtcm_allowance {
            return Err(Error::Capacity);
        }
        Ok(plan)
    }

    /// Bounds exclude DMA descriptors, session/runtime storage and mappings;
    /// those remain additional independently admitted allocations.
    #[must_use]
    pub fn ledger(self) -> Ledger {
        let packed_weight_bytes = self.rows * self.columns / 128 * PQ2_BYTES;
        let activation_bytes = self.columns / 256 * Q8K_BYTES;
        let output_bytes = self.rows * 4;
        let stage = self.tile_rows * self.tile_q8k * 2 * PQ2_BYTES;
        Ledger {
            packed_weight_bytes,
            activation_bytes,
            output_bytes,
            cpu_weight_stage_bytes: stage,
            proposed_vtcm_bytes: 2 * aligned(stage)
                + aligned(self.tile_q8k * Q8K_BYTES)
                + aligned(self.tile_rows * 4),
            logical_weight_read_bytes: packed_weight_bytes,
            proposed_activation_transfer_bytes: self.rows.div_ceil(self.tile_rows)
                * activation_bytes,
            proposed_output_transfer_bytes: output_bytes,
        }
    }
}

fn zeroed<T: Default + Clone>(count: usize) -> Result<Vec<T>, Error> {
    let mut output = Vec::new();
    output
        .try_reserve_exact(count)
        .map_err(|_| Error::Allocation)?;
    output.resize(count, T::default());
    Ok(output)
}

fn finite(value: f32) -> Result<f32, Error> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(Error::Nonfinite)
    }
}

fn half(bytes: &[u8]) -> Result<f32, Error> {
    let bits = u16::from_le_bytes([bytes[0], bytes[1]]);
    if bits & 0x7c00 == 0x7c00 {
        return Err(Error::Nonfinite);
    }
    let fraction = u32::from(bits & 1023);
    let exponent = u32::from((bits >> 10) & 31);
    let magnitude = if exponent == 0 {
        fraction as f32 * 2.0_f32.powi(-24)
    } else {
        f32::from_bits(((exponent + 112) << 23) | (fraction << 13))
    };
    Ok(if bits & 0x8000 == 0 {
        magnitude
    } else {
        -magnitude
    })
}

/// Exact serialized pinned Q8_K preparation, including fused rounding magic.
///
/// Input is already transformed by the owning CPU graph. The two-bit PQ2
/// blocks share each 256-element Q8_K scale; Q8_0 is never substituted.
pub fn prepare_q8k(input: &[f32]) -> Result<Vec<u8>, Error> {
    if input.is_empty() || input.len() > MAX_COLUMNS || !input.len().is_multiple_of(256) {
        return Err(Error::Extent);
    }
    let (input_blocks, _) = input.as_chunks::<256>();
    let mut output = zeroed(input_blocks.len() * Q8K_BYTES)?;
    let (output_blocks, _) = output.as_chunks_mut::<Q8K_BYTES>();
    for (values, block) in input_blocks.iter().zip(output_blocks) {
        let mut maximum = 0.0_f32;
        for &value in values {
            finite(value)?;
            if value.abs() > maximum.abs() {
                maximum = value;
            }
        }
        if maximum == 0.0 {
            continue;
        }
        let inverse = finite(-127.0 / maximum)?;
        block[..4].copy_from_slice(&(1.0 / inverse).to_le_bytes());
        for (destination, &value) in block[4..260].iter_mut().zip(values) {
            // This is the pinned ARM64 binary contract, not rounded-product
            // round_ties_even. Native golden controls distinguish the two.
            let rounded = finite(inverse.mul_add(value, 12_582_912.0))?;
            let integer = i32::try_from(rounded.to_bits() & 0x007f_ffff)
                .map_err(|_| Error::Nonfinite)?
                - 0x0040_0000;
            let code = i8::try_from(integer.min(127)).map_err(|_| Error::Nonfinite)?;
            *destination = code.to_ne_bytes()[0];
        }
        for group in 0..16 {
            let sum: i16 = block[4 + group * 16..4 + (group + 1) * 16]
                .iter()
                .map(|&byte| i16::from(i8::from_ne_bytes([byte])))
                .sum();
            block[260 + group * 2..262 + group * 2].copy_from_slice(&sum.to_le_bytes());
        }
    }
    Ok(output)
}

/// CPU-owned signed, normalized 1024-element Sylvester Walsh-Hadamard input.
///
/// Explicit signs must be supplied from the exact model's independently bound
/// arrays. This API validates values/extent, not their artifact provenance.
/// Stage boundaries and normalization match the pinned graph. Failures return
/// no transformed vector. No recurrent basis or adapter operation is moved.
pub fn transform_input(input: &[f32], signs: &[i8]) -> Result<Vec<f32>, Error> {
    if input.is_empty() || input.len() > MAX_COLUMNS || !input.len().is_multiple_of(1024) {
        return Err(Error::Extent);
    }
    if signs.len() != input.len() || signs.iter().any(|&sign| sign != -1 && sign != 1) {
        return Err(Error::Signs);
    }
    let mut output = zeroed(input.len())?;
    for ((destination, &value), &sign) in output.iter_mut().zip(input).zip(signs) {
        *destination = finite(value)? * f32::from(sign);
    }
    for block in output.as_chunks_mut::<1024>().0 {
        let mut width = 1;
        while width < 1024 {
            for butterfly in block.chunks_exact_mut(width * 2) {
                for index in 0..width {
                    let left = butterfly[index];
                    let right = butterfly[index + width];
                    butterfly[index] = left + right;
                    butterfly[index + width] = left - right;
                }
            }
            width *= 2;
        }
        for value in block {
            *value = finite(*value * (1.0 / 32.0))?;
        }
    }
    Ok(output)
}

fn packed_sum(packed: &[u8], activation: &[u8]) -> i32 {
    let mut sum = 0_i32;
    for (&byte, group) in packed.iter().zip(activation.as_chunks::<4>().0) {
        for (lane, &value) in group.iter().enumerate() {
            let weight = i32::from((byte >> (lane * 2)) & 3) - 1;
            sum += weight * i32::from(i8::from_ne_bytes([value]));
        }
    }
    sum
}

/// Execute the bounded tile control on CPU with exact per-PQ2-block FMA order.
///
/// `continue_work` is checked before preparation, each tile and final commit;
/// the caller owns the aggregate deadline. On every error `output` is empty.
/// A private result survives K-tile boundaries so reductions are never restarted
/// or combined out of order. Original bytes are staged without a Q2 conversion.
pub fn tiled_projection(
    plan: Plan,
    weights: &[u8],
    input: &[f32],
    output: &mut Vec<f32>,
    mut continue_work: impl FnMut() -> bool,
) -> Result<(), Error> {
    output.clear();
    if weights.len() != plan.ledger().packed_weight_bytes || input.len() != plan.columns {
        return Err(Error::Extent);
    }
    if !continue_work() {
        return Err(Error::Cancelled);
    }
    let activation = prepare_q8k(input)?;
    project_prepared(plan, weights, &activation, output, continue_work)
}

// Kept private: production callers cannot supply unchecked prepared records or
// change preparation modes. The native-capture regression uses this same loop.
fn project_prepared(
    plan: Plan,
    weights: &[u8],
    activation: &[u8],
    output: &mut Vec<f32>,
    mut continue_work: impl FnMut() -> bool,
) -> Result<(), Error> {
    let mut pending = zeroed::<f32>(plan.rows)?;
    let mut stage = zeroed::<u8>(plan.ledger().cpu_weight_stage_bytes)?;
    let row_bytes = plan.columns / 128 * PQ2_BYTES;
    let groups = plan.columns / 256;
    for row_start in (0..plan.rows).step_by(plan.tile_rows) {
        let tile_rows = plan.tile_rows.min(plan.rows - row_start);
        for group_start in (0..groups).step_by(plan.tile_q8k) {
            if !continue_work() {
                return Err(Error::Cancelled);
            }
            let tile_groups = plan.tile_q8k.min(groups - group_start);
            let stage_row = tile_groups * 2 * PQ2_BYTES;
            for row in 0..tile_rows {
                let source = (row_start + row) * row_bytes + group_start * 2 * PQ2_BYTES;
                stage[row * stage_row..(row + 1) * stage_row]
                    .copy_from_slice(&weights[source..source + stage_row]);
            }
            for row in 0..tile_rows {
                for block_index in 0..tile_groups * 2 {
                    let start = row * stage_row + block_index * PQ2_BYTES;
                    let block = &stage[start..start + PQ2_BYTES];
                    let group = group_start + block_index / 2;
                    let q8 = &activation[group * Q8K_BYTES..(group + 1) * Q8K_BYTES];
                    let scale = f32::from_le_bytes(q8[..4].try_into().map_err(|_| Error::Extent)?);
                    let start = 4 + block_index % 2 * 128;
                    let integer = packed_sum(&block[2..], &q8[start..start + 128]);
                    let product_scale = half(block)? * scale;
                    pending[row_start + row] =
                        finite(product_scale.mul_add(integer as f32, pending[row_start + row]))?;
                }
            }
        }
    }
    if !continue_work() {
        return Err(Error::Cancelled);
    }
    *output = pending;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complete_q8k_records_match_retained_native_boundary_capture() -> Result<(), Error> {
        let expected = include_bytes!("fixtures/swift-q8k-parity.bin");
        for (case, word) in [0xbe01_0f7e, 0x3e01_0f7e].into_iter().enumerate() {
            let mut input = [0.0; 256];
            input[0] = f32::from_bits(0x3e65_3d88);
            input[1] = f32::from_bits(word);
            let prepared = prepare_q8k(&input)?;
            assert_eq!(
                &prepared,
                &expected[case * Q8K_BYTES..(case + 1) * Q8K_BYTES]
            );
            let rounded_product = (-127.0 / input[0] * input[1]).round_ties_even() as i8;
            assert_ne!(i8::from_ne_bytes([prepared[5]]), rounded_product);
        }
        Ok(())
    }

    #[test]
    fn zero_ties_signed_extrema_and_nonfinite_preparation() -> Result<(), Error> {
        assert_eq!(prepare_q8k(&[-0.0; 256])?, [0; Q8K_BYTES]);
        let mut input = [0.0; 256];
        input[0] = -127.0;
        input[1] = 127.0;
        let prepared = prepare_q8k(&input)?;
        assert_eq!(&prepared[..6], &[0, 0, 128, 63, 129, 127]);
        for value in [
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::from_bits(1),
        ] {
            assert_eq!(prepare_q8k(&[value; 256]), Err(Error::Nonfinite));
        }
        assert_eq!(prepare_q8k(&[0.0; 384]), Err(Error::Extent));
        Ok(())
    }

    #[test]
    fn all_packed_bytes_and_full_i8_range_match_expanded_integer_oracle() {
        for byte in 0..=u8::MAX {
            for value in [i8::MIN, -127, -1, 0, 1, 127] {
                let packed = [byte; 32];
                let input = [value.to_ne_bytes()[0]; 128];
                let expected: i32 = (0..128)
                    .map(|index| {
                        (i32::from((byte >> (2 * (index % 4))) & 3) - 1) * i32::from(value)
                    })
                    .sum();
                assert_eq!(packed_sum(&packed, &input), expected);
            }
        }
    }

    #[test]
    fn captured_native_fma_result_and_tile_boundary_carry() -> Result<(), Error> {
        // Independent pinned DLL type-142 result: c1f90b01; nonfused c1f90b02.
        // Prepared bytes are exactly the independently captured input, not
        // another quantizer or a Rust-calculated expected floating result.
        let mut native_weights = [0xaa; 68];
        native_weights[..2].copy_from_slice(&0x3c01_u16.to_le_bytes());
        native_weights[34..36].copy_from_slice(&0x3787_u16.to_le_bytes());
        let mut native_activation = [0; Q8K_BYTES];
        native_activation[..4].copy_from_slice(&0x3da0_0001_u32.to_le_bytes());
        native_activation[4..132].fill(3);
        native_activation[132..260].fill((-13_i8).to_ne_bytes()[0]);
        for group in 0..16 {
            let sum = if group < 8 { 48_i16 } else { -208_i16 };
            native_activation[260 + group * 2..262 + group * 2].copy_from_slice(&sum.to_le_bytes());
        }
        let mut captured_output = Vec::new();
        project_prepared(
            Plan::new(256, 1, 1, 1, 1 << 20)?,
            &native_weights,
            &native_activation,
            &mut captured_output,
            || true,
        )?;
        assert_eq!(captured_output[0].to_bits(), 0xc1f9_0b01);

        let columns = 5120;
        let rows = 7;
        let mut weights = vec![0; rows * columns / 128 * PQ2_BYTES];
        for (index, block) in weights.chunks_exact_mut(PQ2_BYTES).enumerate() {
            block[..2].copy_from_slice(&[0x3c01_u16, 0xb555, 0, 0x8000][index % 4].to_le_bytes());
            for (offset, byte) in block[2..].iter_mut().enumerate() {
                *byte = (index.wrapping_mul(31).wrapping_add(offset * 77) % 256) as u8;
            }
        }
        let input: Vec<_> = (0..columns)
            .map(|index| ((index * 71 % 253) as f32 - 126.0) * 0.03125)
            .collect();
        let reference = expanded_reference(&weights, &input)?;
        for row_tile in [1, 3, 4, 64] {
            for column_tile in [1, 3, 7, 20] {
                let plan = Plan::new(columns, rows, row_tile, column_tile, 1 << 20)?;
                // Deliberately unaligned packed storage.
                let mut unaligned = vec![0xff];
                unaligned.extend_from_slice(&weights);
                let mut output = vec![f32::NAN];
                tiled_projection(plan, &unaligned[1..], &input, &mut output, || true)?;
                assert_eq!(
                    output.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
                    reference
                );
            }
        }
        Ok(())
    }

    fn expanded_reference(weights: &[u8], input: &[f32]) -> Result<Vec<u32>, Error> {
        let activation = prepare_q8k(input)?;
        let mut output = Vec::new();
        for row in weights.chunks_exact(input.len() / 128 * PQ2_BYTES) {
            let mut sum = 0.0_f32;
            for (index, block) in row.chunks_exact(PQ2_BYTES).enumerate() {
                let q8 = &activation[index / 2 * Q8K_BYTES..(index / 2 + 1) * Q8K_BYTES];
                // Independently expand one whole block, rather than traversing
                // packed bytes in the staged fused integer implementation.
                let expanded: Vec<_> = (0..128)
                    .map(|lane| i32::from((block[2 + lane / 4] >> (lane % 4 * 2)) & 3) - 1)
                    .collect();
                let integer: i32 = expanded
                    .iter()
                    .enumerate()
                    .map(|(lane, &weight)| {
                        weight * i32::from(i8::from_ne_bytes([q8[4 + index % 2 * 128 + lane]]))
                    })
                    .sum();
                let scale = f32::from_le_bytes(q8[..4].try_into().map_err(|_| Error::Extent)?);
                sum = (half(block)? * scale).mul_add(integer as f32, sum);
            }
            output.push(sum.to_bits());
        }
        Ok(output)
    }

    #[test]
    fn errors_cancelled_tiles_and_final_commit_leave_no_partial_projection() -> Result<(), Error> {
        let plan = Plan::new(512, 3, 2, 1, 1 << 20)?;
        let weights = vec![0; plan.ledger().packed_weight_bytes];
        for cancellation_call in 0..=5 {
            let mut calls = 0;
            let mut output = vec![42.0];
            let result = tiled_projection(plan, &weights, &[1.0; 512], &mut output, || {
                let keep = calls != cancellation_call;
                calls += 1;
                keep
            });
            assert_eq!(result, Err(Error::Cancelled));
            assert!(output.is_empty());
        }
        let mut output = vec![42.0];
        let mut bad = weights;
        let last = bad.len() - PQ2_BYTES;
        bad[last..last + 2].copy_from_slice(&0x7c00_u16.to_le_bytes());
        assert_eq!(
            tiled_projection(plan, &bad, &[0.0; 512], &mut output, || true),
            Err(Error::Nonfinite)
        );
        assert!(output.is_empty());
        assert_eq!(
            tiled_projection(
                plan,
                &bad[..bad.len() - 1],
                &[1.0; 512],
                &mut output,
                || true
            ),
            Err(Error::Extent)
        );
        assert!(output.is_empty());
        Ok(())
    }

    #[test]
    fn signed_hadamard_axes_validation_and_independent_impulse_control() -> Result<(), Error> {
        let mut input = vec![0.0; 5120];
        input[0] = 32.0;
        input[1024] = -64.0;
        // Nonzero coordinate binds interior butterfly/lane order independently
        // of the implementation: H[i,j] = (-1)^popcount(i & j) / sqrt(1024).
        input[2048 + 37] = 96.0;
        let mut signs = vec![1; 5120];
        signs[0] = -1;
        signs[2048 + 37] = -1;
        let transformed = transform_input(&input, &signs)?;
        assert!(transformed[..1024].iter().all(|&value| value == -1.0));
        assert!(transformed[1024..2048].iter().all(|&value| value == -2.0));
        for (index, &value) in transformed[2048..3072].iter().enumerate() {
            let odd = (index & 37).count_ones() % 2 == 1;
            assert_eq!(value, if odd { 3.0 } else { -3.0 });
        }
        assert!(transformed[3072..].iter().all(|&value| value == 0.0));
        signs[17] = 0;
        assert_eq!(transform_input(&input, &signs), Err(Error::Signs));
        assert_eq!(transform_input(&input, &signs[..1024]), Err(Error::Signs));
        assert_eq!(
            transform_input(&input[..256], &signs[..256]),
            Err(Error::Extent)
        );
        assert_eq!(
            half(&0x8000_u16.to_le_bytes())?.to_bits(),
            (-0.0_f32).to_bits()
        );
        assert_eq!(half(&1_u16.to_le_bytes())?, 2.0_f32.powi(-24));
        assert_eq!(half(&0x7bff_u16.to_le_bytes())?, 65504.0);
        Ok(())
    }

    #[test]
    fn first_projection_ledger_is_bounded_without_whole_model_expansion() -> Result<(), Error> {
        let plan = Plan::new(5120, 17408, 32, 4, 1 << 20)?;
        assert_eq!(
            plan.ledger(),
            Ledger {
                packed_weight_bytes: 23_674_880,
                activation_bytes: 5840,
                output_bytes: 69_632,
                cpu_weight_stage_bytes: 8704,
                proposed_vtcm_bytes: 18_752,
                logical_weight_read_bytes: 23_674_880,
                proposed_activation_transfer_bytes: 3_176_960,
                proposed_output_transfer_bytes: 69_632,
            }
        );
        assert!(matches!(
            Plan::new(5120, 17408, 32, 4, 18_751),
            Err(Error::Capacity)
        ));
        for (columns, rows, tile_rows, groups) in [
            (384, 1, 1, 1),
            (5120, 17409, 1, 1),
            (256, 1, 65, 1),
            (256, 1, 1, 21),
            (0, 1, 1, 1),
        ] {
            assert!(matches!(
                Plan::new(columns, rows, tile_rows, groups, 1 << 20),
                Err(Error::Extent)
            ));
        }
        Ok(())
    }
}
