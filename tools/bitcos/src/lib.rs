//! Safe scalar reference for the experimental BITCOS ternary representation.
//!
//! This crate preserves the byte layouts and scalar accumulation order of
//! the retired `tools/bitcos/bitcos.cpp` reference. It provides neither a model
//! runtime nor SIMD kernels; performance has not been qualified.
//! The public dot functions additionally reject invalid weight encodings and
//! nonfinite activation scales; the C++ raw dot primitives do not validate all
//! of those inputs. A zero activation scale is valid.
//!
//! PQ2 stores a little-endian FP16 scale followed by 128 two-bit symbols
//! (`0 = -1`, `1 = 0`, `2 = +1`). PTQ stores 26 packed trit bytes followed by
//! that same scale. Compact blocks store the scale, a 16-byte presence bitmap,
//! and the minimum number of sign bytes, with unused high sign bits zero.

#![forbid(unsafe_code)]

use std::error;
use std::fmt;

/// One native PQ2 group: a scale and 128 two-bit ternary symbols.
pub type Pq2Block = [u8; 34];

/// One native PTQ group: packed trits followed by an FP16 scale.
pub type PtqBlock = [u8; 28];

/// Native `Q8_K` activation layout, shared by two 128-weight groups.
///
/// The group sums are retained for layout compatibility but are not used by
/// these scalar kernels. Values cover the entire `i8` range.
#[derive(Clone, Debug, PartialEq)]
#[repr(C)]
pub struct Q8KBlock {
    /// Finite activation scale; zero is accepted.
    pub scale: f32,
    /// Quantized activations, in weight order.
    pub values: [i8; 256],
    /// Native group sums, unused by these scalar kernels.
    pub sums: [i16; 16],
}

const _: () = assert!(size_of::<Q8KBlock>() == 292);
const _: () = assert!(std::mem::offset_of!(Q8KBlock, values) == 4);

/// A rejected encoding, dimension, allocation, or row request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    /// Weight scales must be finite and nonzero FP16 values.
    InvalidWeightScale,
    /// A PQ2 block contains the reserved `+2` symbol.
    NonTernarySymbol,
    /// PTQ bytes do not use the unique encoding emitted by `pack_ptq`.
    NoncanonicalPtq,
    /// `Q8_K` activation scales must be finite.
    NonFiniteActivationScale,
    /// A dot requires a nonempty even number of weight groups and one `Q8_K`
    /// activation block per pair.
    InvalidDotDimensions,
    /// Matrix dimensions must be nonzero, exact, and even in weight groups.
    InvalidMatrixDimensions,
    /// A length or serialized offset exceeds the supported integer range.
    SizeOverflow,
    /// A requested allocation could not be reserved.
    AllocationFailed,
    /// Compact offsets must start at zero and end at the payload length.
    InvalidMatrixIndex,
    /// A row extent is descending, outside the payload, or too short.
    InvalidRowExtent,
    /// A compact block lacks its scale and presence bitmap.
    TruncatedBlockPrefix,
    /// A compact block lacks sign bits required by its presence bitmap.
    TruncatedSigns,
    /// Unused high bits of the last compact sign byte must be zero.
    NoncanonicalSignPadding,
    /// A row contains bytes beyond its declared number of blocks.
    ExtraRowBytes,
    /// The requested row does not exist.
    RowOutOfBounds,
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidWeightScale => "finite nonzero FP16 scale required",
            Self::NonTernarySymbol => "PQ2 +2 symbol is not ternary",
            Self::NoncanonicalPtq => "noncanonical PTQ encoding",
            Self::NonFiniteActivationScale => "finite Q8_K activation scale required",
            Self::InvalidDotDimensions => "Q8_K dot requires nonempty pairs of weight groups",
            Self::InvalidMatrixDimensions => "invalid matrix dimensions",
            Self::SizeOverflow => "matrix size or row index capacity exceeded",
            Self::AllocationFailed => "matrix allocation failed",
            Self::InvalidMatrixIndex => "invalid compact matrix index",
            Self::InvalidRowExtent => "invalid compact row extent",
            Self::TruncatedBlockPrefix => "truncated compact block prefix",
            Self::TruncatedSigns => "truncated compact signs",
            Self::NoncanonicalSignPadding => "noncanonical compact sign padding",
            Self::ExtraRowBytes => "extra compact row bytes",
            Self::RowOutOfBounds => "row out of bounds",
        })
    }
}

impl error::Error for Error {}

/// Converts every IEEE 754 binary16 bit pattern to binary32, preserving zero
/// signs and shifting NaN payload bits exactly as the C++ reference does.
#[must_use]
pub fn half_to_float(value: u16) -> f32 {
    let sign = u32::from(value & 0x8000) << 16;
    let mut mantissa = u32::from(value & 1023);
    let mut exponent = u32::from((value >> 10) & 31);
    if exponent == 0 {
        if mantissa == 0 {
            return f32::from_bits(sign);
        }
        exponent = 113;
        while mantissa & 1024 == 0 {
            mantissa <<= 1;
            exponent -= 1;
        }
        mantissa &= 1023;
    } else if exponent == 31 {
        exponent = 255;
    } else {
        exponent += 112;
    }
    f32::from_bits(sign | (exponent << 23) | (mantissa << 13))
}

const fn check_scale(bytes: &[u8]) -> Result<(), Error> {
    let scale = u16::from_le_bytes([bytes[0], bytes[1]]);
    if matches!(scale, 0 | 0x8000) || scale & 0x7c00 == 0x7c00 {
        return Err(Error::InvalidWeightScale);
    }
    Ok(())
}

const fn code(block: &Pq2Block, index: usize) -> u8 {
    (block[2 + index / 4] >> (2 * (index % 4))) & 3
}

fn check_pq2(block: &Pq2Block) -> Result<(), Error> {
    check_scale(block)?;
    if block[2..].iter().any(|byte| byte & (byte >> 1) & 0x55 != 0) {
        return Err(Error::NonTernarySymbol);
    }
    Ok(())
}

fn check_dot(blocks: usize, activations: &[Q8KBlock]) -> Result<(), Error> {
    if blocks == 0 || !blocks.is_multiple_of(2) || activations.len() != blocks / 2 {
        return Err(Error::InvalidDotDimensions);
    }
    if activations.iter().any(|block| !block.scale.is_finite()) {
        return Err(Error::NonFiniteActivationScale);
    }
    Ok(())
}

/// Packs one PQ2 group into the native PTQ byte layout.
///
/// # Errors
/// Returns an error for a zero or nonfinite weight scale or a reserved PQ2
/// symbol. All valid source bytes match the C++ reference.
pub fn pack_ptq(source: &Pq2Block) -> Result<PtqBlock, Error> {
    check_pq2(source)?;
    let mut result = [0; 28];
    let mut position = 0;
    for width in [16, 8] {
        let start = position / 5;
        for m in 0..width {
            let mut q = 0_u16;
            for n in 0..5 {
                q = q * 3 + u16::from(code(source, position + m + n * width));
            }
            result[start + m] =
                u8::try_from((q * 256).div_ceil(243)).map_err(|_| Error::SizeOverflow)?;
        }
        position += width * 5;
    }
    for h in 0..2 {
        let mut q = 0_u16;
        for n in 0..4 {
            q = q * 3 + u16::from(code(source, 120 + h + n * 2));
        }
        result[24 + h] =
            u8::try_from((q * 3 * 256).div_ceil(243)).map_err(|_| Error::SizeOverflow)?;
    }
    result[26..].copy_from_slice(&source[..2]);
    Ok(result)
}

/// Unpacks one native PTQ group to PQ2.
///
/// Unlike the C++ reference, this boundary rejects alternate encodings that
/// decode to the same ternary values: repacking must reproduce every byte.
///
/// # Errors
/// Returns an error if the stored weight scale is zero or nonfinite, or if
/// the encoding is noncanonical.
pub fn unpack_ptq(source: &PtqBlock) -> Result<Pq2Block, Error> {
    check_scale(&source[26..])?;
    let mut result = [0; 34];
    result[..2].copy_from_slice(&source[26..]);
    let powers = [1_u8, 3, 9, 27, 81];
    let mut position = 0;
    for width in [16, 8] {
        let start = position / 5;
        for (n, power) in powers.iter().enumerate() {
            for m in 0..width {
                let q = source[start + m].wrapping_mul(*power);
                let c = u8::try_from((u16::from(q) * 3) >> 8).map_err(|_| Error::SizeOverflow)?;
                let index = position + n * width + m;
                result[2 + index / 4] |= c << (2 * (index % 4));
            }
        }
        position += width * 5;
    }
    for (n, power) in powers[..4].iter().enumerate() {
        for h in 0..2 {
            let q = source[24 + h].wrapping_mul(*power);
            let c = u8::try_from((u16::from(q) * 3) >> 8).map_err(|_| Error::SizeOverflow)?;
            let index = 120 + n * 2 + h;
            result[2 + index / 4] |= c << (2 * (index % 4));
        }
    }
    if pack_ptq(&result)? != *source {
        return Err(Error::NoncanonicalPtq);
    }
    Ok(result)
}

fn pq2_sum(weights: &Pq2Block, activations: &[i8]) -> i16 {
    activations
        .iter()
        .enumerate()
        .fold(0, |sum, (index, value)| {
            sum + (i16::from(code(weights, index)) - 1) * i16::from(*value)
        })
}

fn scale_product(bytes: &[u8], activation: &Q8KBlock) -> f32 {
    half_to_float(u16::from_le_bytes([bytes[0], bytes[1]])) * activation.scale
}

/// Computes the scalar PQ2 by `Q8_K` dot product in C++ accumulation order.
///
/// Each weight group has its own FP16 scale. One `Q8_K` block supplies the first
/// and second 128-element activation halves for two consecutive groups.
///
/// # Errors
/// Returns an error for mismatched or empty dimensions, invalid weight scales,
/// reserved PQ2 symbols, or nonfinite activation scales.
#[allow(
    clippy::suboptimal_flops,
    reason = "Preserve reference multiply-then-add rounding; no fused operation."
)]
pub fn dot_pq2_scalar(weights: &[Pq2Block], activations: &[Q8KBlock]) -> Result<f32, Error> {
    check_dot(weights.len(), activations)?;
    let mut total = 0.0;
    for (index, block) in weights.iter().enumerate() {
        check_pq2(block)?;
        let activation = &activations[index / 2];
        let start = (index % 2) * 128;
        let sum = pq2_sum(block, &activation.values[start..start + 128]);
        total += scale_product(block, activation) * f32::from(sum);
    }
    Ok(total)
}

/// Computes the scalar native PTQ decoding by `Q8_K` dot product.
///
/// This is the experimental comparator using `Q8_K` inputs, not Prism's released
/// PTQ by `Q8_0` kernel. Float operations follow the C++ reference order.
///
/// # Errors
/// Returns an error for mismatched or empty dimensions, invalid weight scales,
/// noncanonical PTQ encodings, or nonfinite activation scales.
#[allow(
    clippy::suboptimal_flops,
    reason = "Preserve reference multiply-then-add rounding; no fused operation."
)]
pub fn dot_ptq_scalar_q8k(weights: &[PtqBlock], activations: &[Q8KBlock]) -> Result<f32, Error> {
    check_dot(weights.len(), activations)?;
    let mut total = 0.0;
    for (index, block) in weights.iter().enumerate() {
        let decoded = unpack_ptq(block)?;
        let activation = &activations[index / 2];
        let start = (index % 2) * 128;
        let sum = pq2_sum(&decoded, &activation.values[start..start + 128]);
        total += scale_product(&block[26..], activation) * f32::from(sum);
    }
    Ok(total)
}

/// An immutable compact matrix with validated row offsets and exact payload.
///
/// Rows contain an even number of groups. Each group stores only its nonzero
/// signs, with no persistent PQ2-sized staging allocation. Offsets are byte
/// offsets into the payload, including a final sentinel.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Matrix {
    blocks: usize,
    offsets: Vec<u32>,
    payload: Vec<u8>,
    serialized: u64,
}

fn allocated_zeroes<T: Clone + Default>(length: usize) -> Result<Vec<T>, Error> {
    let mut result = Vec::new();
    result
        .try_reserve_exact(length)
        .map_err(|_| Error::AllocationFailed)?;
    result.resize(length, T::default());
    Ok(result)
}

fn compact_block(row: &[u8]) -> Result<(&[u8], &[u8]), Error> {
    let prefix = row.get(..18).ok_or(Error::TruncatedBlockPrefix)?;
    check_scale(prefix)?;
    let nonzero = prefix[2..]
        .iter()
        .map(|byte| byte.count_ones())
        .sum::<u32>();
    let count = 18 + usize::try_from(nonzero.div_ceil(8)).map_err(|_| Error::SizeOverflow)?;
    let block = row.get(..count).ok_or(Error::TruncatedSigns)?;
    if nonzero % 8 != 0 && block[count - 1] >> (nonzero % 8) != 0 {
        return Err(Error::NoncanonicalSignPadding);
    }
    Ok((block, &row[count..]))
}

fn serialized_size(offsets: usize, payload: usize) -> Result<u64, Error> {
    let index_bytes = u64::try_from(offsets)
        .map_err(|_| Error::SizeOverflow)?
        .checked_mul(4)
        .ok_or(Error::SizeOverflow)?;
    u64::try_from(payload)
        .map_err(|_| Error::SizeOverflow)?
        .checked_add(index_bytes)
        .ok_or(Error::SizeOverflow)
}

impl Matrix {
    /// Packs exactly `rows * blocks_per_row` source groups.
    ///
    /// The payload allocation is sized from validated presence counts before
    /// construction; no full matrix of decoded weights is retained.
    ///
    /// # Errors
    /// Returns an error for invalid dimensions or PQ2 blocks, index capacity
    /// overflow, or a failed allocation. The conservative C++ source-capacity
    /// limit of `u32::MAX / 34` groups is preserved.
    pub fn pack(source: &[Pq2Block], rows: usize, blocks_per_row: usize) -> Result<Self, Error> {
        if rows == 0
            || blocks_per_row == 0
            || !blocks_per_row.is_multiple_of(2)
            || rows.checked_mul(blocks_per_row) != Some(source.len())
        {
            return Err(Error::InvalidMatrixDimensions);
        }
        let capacity = usize::try_from(u32::MAX / 34).map_err(|_| Error::SizeOverflow)?;
        if source.len() > capacity {
            return Err(Error::SizeOverflow);
        }
        let mut bytes = 0_usize;
        for block in source {
            check_pq2(block)?;
            let nonzero = (0..128).filter(|index| code(block, *index) != 1).count();
            bytes = bytes
                .checked_add(18 + nonzero.div_ceil(8))
                .ok_or(Error::SizeOverflow)?;
        }
        let mut payload = allocated_zeroes::<u8>(bytes)?;
        let offset_count = rows.checked_add(1).ok_or(Error::SizeOverflow)?;
        let mut offsets = allocated_zeroes::<u32>(offset_count)?;
        let mut cursor = 0;
        for (row, row_blocks) in source.chunks_exact(blocks_per_row).enumerate() {
            offsets[row] = u32::try_from(cursor).map_err(|_| Error::SizeOverflow)?;
            for source_block in row_blocks {
                let mut block = [0_u8; 34];
                block[..2].copy_from_slice(&source_block[..2]);
                let mut rank = 0_usize;
                for index in 0..128 {
                    let c = code(source_block, index);
                    if c == 1 {
                        continue;
                    }
                    block[2 + index / 8] |= 1 << (index % 8);
                    if c == 2 {
                        block[18 + rank / 8] |= 1 << (rank % 8);
                    }
                    rank += 1;
                }
                let count = 18 + rank.div_ceil(8);
                payload[cursor..cursor + count].copy_from_slice(&block[..count]);
                cursor += count;
            }
        }
        offsets[rows] = u32::try_from(cursor).map_err(|_| Error::SizeOverflow)?;
        let serialized = serialized_size(offset_count, bytes)?;
        Ok(Self {
            blocks: blocks_per_row,
            offsets,
            payload,
            serialized,
        })
    }

    /// Loads compact bytes after validating every row, scale, and sign extent.
    ///
    /// # Errors
    /// Returns an error for invalid group counts, offsets, truncated blocks,
    /// weight scales, unused sign bits, extra row bytes, or integer overflow.
    pub fn load(blocks_per_row: usize, offsets: Vec<u32>, payload: Vec<u8>) -> Result<Self, Error> {
        let payload_length = u32::try_from(payload.len()).map_err(|_| Error::SizeOverflow)?;
        if blocks_per_row == 0
            || !blocks_per_row.is_multiple_of(2)
            || offsets.len() < 2
            || offsets.first() != Some(&0)
            || offsets.last() != Some(&payload_length)
        {
            return Err(Error::InvalidMatrixIndex);
        }
        for &[start, end] in offsets.array_windows() {
            let start = usize::try_from(start).map_err(|_| Error::SizeOverflow)?;
            let end = usize::try_from(end).map_err(|_| Error::SizeOverflow)?;
            let mut row = payload.get(start..end).ok_or(Error::InvalidRowExtent)?;
            if blocks_per_row > row.len() / 18 {
                return Err(Error::InvalidRowExtent);
            }
            for _ in 0..blocks_per_row {
                (_, row) = compact_block(row)?;
            }
            if !row.is_empty() {
                return Err(Error::ExtraRowBytes);
            }
        }
        let serialized = serialized_size(offsets.len(), payload.len())?;
        Ok(Self {
            blocks: blocks_per_row,
            offsets,
            payload,
            serialized,
        })
    }

    /// Returns the number of independently indexed rows.
    #[must_use]
    pub const fn rows(&self) -> usize {
        self.offsets.len() - 1
    }

    /// Returns the number of 128-weight groups in each row.
    #[must_use]
    pub const fn blocks_per_row(&self) -> usize {
        self.blocks
    }

    /// Returns the immutable compact byte payload.
    #[must_use]
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }

    /// Returns byte offsets and the final payload-length sentinel.
    #[must_use]
    pub fn offsets(&self) -> &[u32] {
        &self.offsets
    }

    /// Returns the payload bytes plus four bytes per row offset and sentinel.
    #[must_use]
    pub const fn serialized_bytes(&self) -> u64 {
        self.serialized
    }

    fn row(&self, row: usize) -> Result<&[u8], Error> {
        if row >= self.rows() {
            return Err(Error::RowOutOfBounds);
        }
        let start = usize::try_from(self.offsets[row]).map_err(|_| Error::SizeOverflow)?;
        let end = usize::try_from(self.offsets[row + 1]).map_err(|_| Error::SizeOverflow)?;
        self.payload.get(start..end).ok_or(Error::InvalidRowExtent)
    }

    /// Decodes one row to native PQ2 blocks, primarily for correctness checks.
    ///
    /// # Errors
    /// Returns an error for an out-of-range row or a failed allocation.
    pub fn unpack_row(&self, row: usize) -> Result<Vec<Pq2Block>, Error> {
        let mut source = self.row(row)?;
        let mut result = Vec::new();
        result
            .try_reserve_exact(self.blocks)
            .map_err(|_| Error::AllocationFailed)?;
        for _ in 0..self.blocks {
            let (compact, remainder) = compact_block(source)?;
            source = remainder;
            let mut block = [0; 34];
            block[..2].copy_from_slice(&compact[..2]);
            let mut rank = 0;
            for index in 0..128 {
                let mut c = 1;
                if compact[2 + index / 8] & (1 << (index % 8)) != 0 {
                    c = ((compact[18 + rank / 8] >> (rank % 8)) & 1) * 2;
                    rank += 1;
                }
                block[2 + index / 4] |= c << (2 * (index % 4));
            }
            result.push(block);
        }
        Ok(result)
    }

    /// Computes one compact row by `Q8_K` dot product without unpacking the row.
    ///
    /// # Errors
    /// Returns an error for an out-of-range row, mismatched activation count,
    /// or nonfinite activation scales.
    #[allow(
        clippy::suboptimal_flops,
        reason = "Preserve reference multiply-then-add rounding; no fused operation."
    )]
    pub fn dot_scalar(&self, row: usize, activations: &[Q8KBlock]) -> Result<f32, Error> {
        let mut source = self.row(row)?;
        check_dot(self.blocks, activations)?;
        let mut total = 0.0;
        for index in 0..self.blocks {
            let (block, remainder) = compact_block(source)?;
            source = remainder;
            let activation = &activations[index / 2];
            let start = (index % 2) * 128;
            let mut sum = 0_i16;
            let mut rank = 0;
            for (position, value) in activation.values[start..start + 128].iter().enumerate() {
                if block[2 + position / 8] & (1 << (position % 8)) != 0 {
                    let weight = if block[18 + rank / 8] & (1 << (rank % 8)) != 0 {
                        1
                    } else {
                        -1
                    };
                    rank += 1;
                    sum += weight * i16::from(*value);
                }
            }
            total += scale_product(block, activation) * f32::from(sum);
        }
        Ok(total)
    }
}
