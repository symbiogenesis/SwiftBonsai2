//! Proposed application-owned packed kernel wire format, replayed on CPU only.
//!
//! This is not Qualcomm's RPC ABI and cannot be submitted to a bundled kernel.
//! A separately reviewed/signed kernel must implement this exact protocol before
//! native integration. No device buffer or asynchronous ownership is represented.

use super::{Error, PQ2_BYTES, Plan, Q8K_BYTES, finite, half, packed_sum, prepare_q8k, zeroed};

const HEADER_BYTES: usize = 64;
const MAGIC: &[u8; 8] = b"PQ2Q8K01";

/// Bounded host wire staging and logical traffic, excluding all vendor framing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TileWireLedger {
    /// Count of complete row/column work units in one projection.
    pub tile_count: usize,
    /// Maximum single serialized payload; one host stage is sufficient.
    pub maximum_payload_bytes: usize,
    /// Complete 64-byte application headers transferred for one projection.
    pub header_transfer_bytes: usize,
    /// Complete original weight, prepared activation and header payload traffic.
    pub total_payload_transfer_bytes: usize,
}

impl Plan {
    /// Application wire bounds; session, mapping, DMA and RPC costs are additional.
    #[must_use]
    pub fn wire_ledger(self) -> TileWireLedger {
        let tile_count =
            self.rows.div_ceil(self.tile_rows) * (self.columns / 256).div_ceil(self.tile_q8k);
        let header_transfer_bytes = tile_count * HEADER_BYTES;
        let ledger = self.ledger();
        TileWireLedger {
            tile_count,
            maximum_payload_bytes: HEADER_BYTES
                + ledger.cpu_weight_stage_bytes
                + self.tile_q8k * Q8K_BYTES,
            header_transfer_bytes,
            total_payload_transfer_bytes: ledger.logical_weight_read_bytes
                + ledger.proposed_activation_transfer_bytes
                + header_transfer_bytes,
        }
    }
}

#[derive(Clone, Copy)]
struct Tile {
    row: usize,
    group: usize,
    rows: usize,
    groups: usize,
}

fn tile(plan: Plan, sequence: usize) -> Option<Tile> {
    if sequence >= plan.wire_ledger().tile_count {
        return None;
    }
    let columns = (plan.columns / 256).div_ceil(plan.tile_q8k);
    let row = sequence / columns * plan.tile_rows;
    let group = sequence % columns * plan.tile_q8k;
    Some(Tile {
        row,
        group,
        rows: plan.tile_rows.min(plan.rows - row),
        groups: plan.tile_q8k.min(plan.columns / 256 - group),
    })
}

fn header(plan: Plan, sequence: usize, tile: Tile) -> Result<[u8; HEADER_BYTES], Error> {
    let mut output = [0; HEADER_BYTES];
    output[..8].copy_from_slice(MAGIC);
    // All integers, including bit fields, are fixed-width little endian.
    let flags = usize::from(tile.group == 0)
        | (usize::from(tile.group + tile.groups == plan.columns / 256) << 1)
        | (usize::from(sequence + 1 == plan.wire_ledger().tile_count) << 2);
    let fields = [
        1,
        HEADER_BYTES,
        plan.columns,
        plan.rows,
        plan.tile_rows,
        plan.tile_q8k,
        tile.row,
        tile.group,
        tile.rows,
        tile.groups,
        sequence,
        flags,
        tile.rows * tile.groups * 2 * PQ2_BYTES,
        tile.groups * Q8K_BYTES,
    ];
    for (destination, value) in output[8..].chunks_exact_mut(4).zip(fields) {
        destination.copy_from_slice(
            &u32::try_from(value)
                .map_err(|_| Error::Extent)?
                .to_le_bytes(),
        );
    }
    Ok(output)
}

/// Bounded original-weight/prepared-activation payload source, without device I/O.
///
/// Inputs are already in the CPU graph's original transform basis. Sign arrays,
/// model and runtime identities must be independently bound by the trusted caller;
/// this format's structural checks provide no authenticity or profile admission.
pub struct TileSource<'a> {
    plan: Plan,
    weights: &'a [u8],
    activation: Vec<u8>,
    sequence: usize,
    failed: bool,
}

impl<'a> TileSource<'a> {
    /// Validate geometry and prepare pinned Q8_K exactly once before transfer.
    pub fn new(plan: Plan, weights: &'a [u8], input: &[f32]) -> Result<Self, Error> {
        if weights.len() != plan.ledger().packed_weight_bytes || input.len() != plan.columns {
            return Err(Error::Extent);
        }
        Ok(Self {
            plan,
            weights,
            activation: prepare_q8k(input)?,
            sequence: 0,
            failed: false,
        })
    }

    /// Replace one caller-owned staging vector with the next complete payload.
    ///
    /// `false` means every work unit was produced. Cancellation or malformed
    /// scales terminally close this source and leave staging empty. The caller
    /// must finish device consumption before reusing staging in a future transport.
    /// This synchronous encoder provides no asynchronous lifetime guarantee.
    pub fn next_payload(
        &mut self,
        staging: &mut Vec<u8>,
        continue_work: impl FnOnce() -> bool,
    ) -> Result<bool, Error> {
        staging.clear();
        let result = self.encode(staging, continue_work);
        if result.is_err() {
            staging.clear();
            self.failed = true;
        }
        result
    }

    fn encode(
        &mut self,
        staging: &mut Vec<u8>,
        continue_work: impl FnOnce() -> bool,
    ) -> Result<bool, Error> {
        if self.failed {
            return Err(Error::Protocol);
        }
        if !continue_work() {
            return Err(Error::Cancelled);
        }
        let Some(tile) = tile(self.plan, self.sequence) else {
            return Ok(false);
        };
        let header = header(self.plan, self.sequence, tile)?;
        let row_bytes = tile.groups * 2 * PQ2_BYTES;
        let extent = HEADER_BYTES + tile.rows * row_bytes + tile.groups * Q8K_BYTES;
        staging
            .try_reserve_exact(extent)
            .map_err(|_| Error::Allocation)?;
        staging.extend_from_slice(&header);
        for row in tile.row..tile.row + tile.rows {
            let start = (row * self.plan.columns / 256 + tile.group) * 2 * PQ2_BYTES;
            let packed = &self.weights[start..start + row_bytes];
            let (blocks, []) = packed.as_chunks::<PQ2_BYTES>() else {
                return Err(Error::Protocol);
            };
            for block in blocks {
                half(block)?;
            }
            staging.extend_from_slice(packed);
        }
        staging.extend_from_slice(
            &self.activation[tile.group * Q8K_BYTES..(tile.group + tile.groups) * Q8K_BYTES],
        );
        self.sequence += 1;
        Ok(true)
    }
}

/// Strict ordered CPU replay of the proposed signed-kernel payload contract.
///
/// Every received work unit is consumed once. Changed geometry, version, flags,
/// sequence, extents, malformed arithmetic or cancellation terminally discards
/// private results. Only `finish` exposes a complete projection. This is neither
/// a transport adapter nor proof of DMA, HVX/HMX, device completion or teardown.
pub struct TileReceiver {
    plan: Plan,
    sequence: usize,
    pending: Vec<f32>,
    activation: Vec<u8>,
    failed: bool,
}

impl TileReceiver {
    /// Allocate private output and one prepared-activation identity copy.
    ///
    /// The CPU replay retains at most 5840 activation bytes to reject changed
    /// records across row tiles. This is an additional host allocation; it does
    /// not assume or qualify device activation residency.
    pub fn new(plan: Plan) -> Result<Self, Error> {
        Ok(Self {
            plan,
            sequence: 0,
            pending: zeroed(plan.rows)?,
            activation: zeroed(plan.ledger().activation_bytes)?,
            failed: false,
        })
    }

    /// Validate and synchronously replay one exact next tile with ordered FMA.
    pub fn accept(
        &mut self,
        payload: &[u8],
        mut continue_work: impl FnMut() -> bool,
    ) -> Result<(), Error> {
        let result = self.replay(payload, &mut continue_work);
        if result.is_err() {
            self.pending.clear();
            self.failed = true;
        }
        result
    }

    fn replay(
        &mut self,
        payload: &[u8],
        continue_work: &mut impl FnMut() -> bool,
    ) -> Result<(), Error> {
        if self.failed {
            return Err(Error::Protocol);
        }
        if !continue_work() {
            return Err(Error::Cancelled);
        }
        let tile = tile(self.plan, self.sequence).ok_or(Error::Protocol)?;
        let expected = header(self.plan, self.sequence, tile)?;
        let weight_bytes = tile.rows * tile.groups * 2 * PQ2_BYTES;
        if payload.len() != HEADER_BYTES + weight_bytes + tile.groups * Q8K_BYTES
            || payload[..HEADER_BYTES] != expected
        {
            return Err(Error::Protocol);
        }
        let weights = &payload[HEADER_BYTES..HEADER_BYTES + weight_bytes];
        let activations = &payload[HEADER_BYTES + weight_bytes..];
        let (weights, []) = weights.as_chunks::<PQ2_BYTES>() else {
            return Err(Error::Protocol);
        };
        let (activation_blocks, []) = activations.as_chunks::<Q8K_BYTES>() else {
            return Err(Error::Protocol);
        };
        validate_activations(activation_blocks)?;
        let recorded =
            &mut self.activation[tile.group * Q8K_BYTES..(tile.group + tile.groups) * Q8K_BYTES];
        if tile.row == 0 {
            recorded.copy_from_slice(activations);
        } else if recorded != activations {
            return Err(Error::Protocol);
        }
        // Validate the complete tile before applying its first partial row sum.
        for block in weights {
            half(block)?;
        }
        for row in 0..tile.rows {
            if !continue_work() {
                return Err(Error::Cancelled);
            }
            for block_index in 0..tile.groups * 2 {
                let block = &weights[row * tile.groups * 2 + block_index];
                let q8 = &activation_blocks[block_index / 2];
                let &[s0, s1, s2, s3, ..] = q8;
                let scale = f32::from_le_bytes([s0, s1, s2, s3]);
                let start = 4 + block_index % 2 * 128;
                let integer = packed_sum(&block[2..], &q8[start..start + 128]);
                let sum = &mut self.pending[tile.row + row];
                *sum = finite((half(block)? * scale).mul_add(integer as f32, *sum))?;
            }
        }
        self.sequence += 1;
        Ok(())
    }

    /// Publish only after every ordered tile and the caller's final deadline check.
    ///
    /// The receiver is consumed, including on failure. Caller-owned prior output
    /// capacity remains allocated after `clear` and must be charged separately.
    pub fn finish(
        self,
        output: &mut Vec<f32>,
        continue_work: impl FnOnce() -> bool,
    ) -> Result<(), Error> {
        output.clear();
        if self.failed || self.sequence != self.plan.wire_ledger().tile_count {
            return Err(Error::Protocol);
        }
        if !continue_work() {
            return Err(Error::Cancelled);
        }
        *output = self.pending;
        Ok(())
    }
}

fn validate_activations(blocks: &[[u8; Q8K_BYTES]]) -> Result<(), Error> {
    for block in blocks {
        let &[s0, s1, s2, s3, ..] = block;
        finite(f32::from_le_bytes([s0, s1, s2, s3]))?;
        for group in 0..16 {
            let sum: i16 = block[4 + group * 16..4 + (group + 1) * 16]
                .iter()
                .map(|&byte| i16::from(byte.cast_signed()))
                .sum();
            if block[260 + group * 2..262 + group * 2] != sum.to_le_bytes() {
                return Err(Error::Protocol);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn weights(plan: Plan) -> Vec<u8> {
        let mut weights = vec![0; plan.ledger().packed_weight_bytes];
        for (index, block) in weights.chunks_exact_mut(PQ2_BYTES).enumerate() {
            block[..2].copy_from_slice(&[0x3c01_u16, 0xb555, 0, 0x8000][index % 4].to_le_bytes());
            for (offset, byte) in block[2..].iter_mut().enumerate() {
                *byte = (index.wrapping_mul(31).wrapping_add(offset * 77) % 256) as u8;
            }
        }
        weights
    }

    #[test]
    fn wire_replay_preserves_original_bytes_partial_tiles_and_reduction_carry() -> Result<(), Error>
    {
        let input: Vec<_> = (0..5120)
            .map(|index| ((index * 71 % 253) as f32 - 126.0) * 0.03125)
            .collect();
        for (rows, tile_rows, tile_groups) in [(7, 3, 7), (2, 64, 20), (1, 1, 1)] {
            let plan = Plan::new(5120, rows, tile_rows, tile_groups, 1 << 20)?;
            let weights = weights(plan);
            let mut reference = Vec::new();
            super::super::tiled_projection(plan, &weights, &input, &mut reference, || true)?;
            let mut source = TileSource::new(plan, &weights, &input)?;
            let mut receiver = TileReceiver::new(plan)?;
            let mut staging = Vec::new();
            let mut count = 0;
            let mut traffic = 0;
            while source.next_payload(&mut staging, || true)? {
                assert!(staging.len() <= plan.wire_ledger().maximum_payload_bytes);
                receiver.accept(&staging, || true)?;
                count += 1;
                traffic += staging.len();
            }
            assert!(staging.is_empty());
            assert_eq!(count, plan.wire_ledger().tile_count);
            assert_eq!(traffic, plan.wire_ledger().total_payload_transfer_bytes);
            let mut output = vec![42.0];
            receiver.finish(&mut output, || true)?;
            assert_eq!(
                output.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
                reference.iter().map(|v| v.to_bits()).collect::<Vec<_>>()
            );
        }
        let plan = Plan::new(5120, 17408, 32, 4, 1 << 20)?;
        assert_eq!(
            plan.wire_ledger(),
            TileWireLedger {
                tile_count: 2720,
                maximum_payload_bytes: 9936,
                header_transfer_bytes: 174080,
                total_payload_transfer_bytes: 27025920,
            }
        );
        Ok(())
    }

    #[test]
    fn native_capture_fma_is_preserved_by_payload_replay() -> Result<(), Error> {
        let plan = Plan::new(256, 1, 1, 1, 4096)?;
        let mut weights = [0xaa; 68];
        weights[..2].copy_from_slice(&0x3c01_u16.to_le_bytes());
        weights[34..36].copy_from_slice(&0x3787_u16.to_le_bytes());
        let mut activation = vec![0; Q8K_BYTES];
        activation[..4].copy_from_slice(&0x3da0_0001_u32.to_le_bytes());
        activation[4..132].fill(3);
        activation[132..260].fill((-13_i8).to_ne_bytes()[0]);
        for group in 0..16 {
            let sum = if group < 8 { 48_i16 } else { -208_i16 };
            activation[260 + group * 2..262 + group * 2].copy_from_slice(&sum.to_le_bytes());
        }
        let mut source = TileSource {
            plan,
            weights: &weights,
            activation,
            sequence: 0,
            failed: false,
        };
        let mut payload = Vec::new();
        assert!(source.next_payload(&mut payload, || true)?);
        let mut receiver = TileReceiver::new(plan)?;
        receiver.accept(&payload, || true)?;
        let mut output = Vec::new();
        receiver.finish(&mut output, || true)?;
        assert_eq!(output[0].to_bits(), 0xc1f9_0b01);
        Ok(())
    }

    #[test]
    fn changed_headers_truncation_reordering_duplicate_and_late_errors_close_receiver()
    -> Result<(), Error> {
        let plan = Plan::new(512, 3, 2, 1, 1 << 20)?;
        let weights = weights(plan);
        let mut source = TileSource::new(plan, &weights, &[1.0; 512])?;
        let mut first = Vec::new();
        let mut second = Vec::new();
        let mut third = Vec::new();
        assert!(source.next_payload(&mut first, || true)?);
        assert!(source.next_payload(&mut second, || true)?);
        assert!(source.next_payload(&mut third, || true)?);
        for offset in 0..HEADER_BYTES {
            let mut corrupted = first.clone();
            corrupted[offset] ^= 0x80;
            let mut receiver = TileReceiver::new(plan)?;
            assert_eq!(receiver.accept(&corrupted, || true), Err(Error::Protocol));
            assert_eq!(receiver.accept(&first, || true), Err(Error::Protocol));
        }
        let mut extended = first.clone();
        extended.push(0);
        for payload in [&first[..first.len() - 1], &first[..3], &extended, &second] {
            let mut receiver = TileReceiver::new(plan)?;
            assert_eq!(receiver.accept(payload, || true), Err(Error::Protocol));
            let mut output = vec![42.0];
            assert_eq!(receiver.finish(&mut output, || true), Err(Error::Protocol));
            assert!(output.is_empty());
        }
        let mut receiver = TileReceiver::new(plan)?;
        receiver.accept(&first, || true)?;
        assert_eq!(receiver.accept(&first, || true), Err(Error::Protocol));
        let mut bad_scale = second.clone();
        bad_scale[HEADER_BYTES + 3 * PQ2_BYTES..HEADER_BYTES + 3 * PQ2_BYTES + 2]
            .copy_from_slice(&0x7c00_u16.to_le_bytes());
        let mut bad_sum = second.clone();
        let activation_start = HEADER_BYTES + 4 * PQ2_BYTES;
        bad_sum[activation_start + 260] ^= 1;
        let mut nonfinite_activation = second.clone();
        nonfinite_activation[activation_start..activation_start + 4]
            .copy_from_slice(&f32::INFINITY.to_le_bytes());
        for (payload, error) in [
            (bad_scale, Error::Nonfinite),
            (bad_sum, Error::Protocol),
            (nonfinite_activation, Error::Nonfinite),
        ] {
            let mut receiver = TileReceiver::new(plan)?;
            receiver.accept(&first, || true)?;
            assert_eq!(receiver.accept(&payload, || true), Err(error));
            let mut output = vec![42.0];
            assert_eq!(receiver.finish(&mut output, || true), Err(Error::Protocol));
            assert!(output.is_empty());
        }
        // Alter two codes without altering their valid recorded sum. Repeated
        // activation identity, not just sum validation, must reject this tile.
        let activation_start = HEADER_BYTES + 2 * PQ2_BYTES;
        third[activation_start + 4] = third[activation_start + 4].wrapping_add(1);
        third[activation_start + 5] = third[activation_start + 5].wrapping_sub(1);
        let mut receiver = TileReceiver::new(plan)?;
        receiver.accept(&first, || true)?;
        receiver.accept(&second, || true)?;
        assert_eq!(receiver.accept(&third, || true), Err(Error::Protocol));
        Ok(())
    }

    #[test]
    fn cancellation_and_incomplete_finish_never_publish_partial_rows() -> Result<(), Error> {
        let plan = Plan::new(256, 2, 2, 1, 1 << 20)?;
        let weights = weights(plan);
        let mut source = TileSource::new(plan, &weights, &[1.0; 256])?;
        let mut staging = vec![42];
        assert_eq!(
            source.next_payload(&mut staging, || false),
            Err(Error::Cancelled)
        );
        assert!(staging.is_empty());
        assert_eq!(
            source.next_payload(&mut staging, || true),
            Err(Error::Protocol)
        );
        let mut source = TileSource::new(plan, &weights, &[1.0; 256])?;
        assert!(source.next_payload(&mut staging, || true)?);
        for stop in 0..3 {
            let mut calls = 0;
            let mut receiver = TileReceiver::new(plan)?;
            assert_eq!(
                receiver.accept(&staging, || {
                    calls += 1;
                    calls != stop + 1
                }),
                Err(Error::Cancelled)
            );
            let mut output = vec![42.0];
            assert_eq!(receiver.finish(&mut output, || true), Err(Error::Protocol));
            assert!(output.is_empty());
        }
        let mut receiver = TileReceiver::new(plan)?;
        receiver.accept(&staging, || true)?;
        let mut output = vec![42.0];
        assert_eq!(
            receiver.finish(&mut output, || false),
            Err(Error::Cancelled)
        );
        assert!(output.is_empty());
        let receiver = TileReceiver::new(plan)?;
        assert_eq!(receiver.finish(&mut output, || true), Err(Error::Protocol));
        assert!(output.is_empty());
        Ok(())
    }
}
