//! Safe reference kernels and automatic baseline NEON packed integer arithmetic.
//!
//! The reference basis is PrismML-Eng/llama.cpp revision
//! `73bfe6cc485554822f80c473a3c0c4f25e80f41b`, `ggml/src/ggml-quants.c`
//! (SHA256 `46dd353f30e7f1c42c7d82d88d91fad64dbe5d6e680b234de25355d9fb482d13`)
//! and `ggml/src/ggml-cpu/quants.c`
//! (SHA256 `b8dd3a7c7da1ff702908d5b71c7c7791eb11e2481f6ce3d0ca91e4705f33bb65`).
//! These reference semantics do not establish full-model bit parity with Prism.
//! PQ2 uses signed `Q8_K` activations for rows divisible by 256, retaining the
//! pinned `Q8_0` fallback for odd numbers of 128-element groups. PTQ1 uses
//! `Q8_0` activations in groups of 32. Neither path expands a matrix to floats.

use bonsai_ternary::{Q8KBlock, half_to_float, unpack_ptq};

use crate::model::Tensor;
use crate::{Result, invalid};

#[cfg(all(
    feature = "experimental-neon",
    target_arch = "aarch64",
    target_os = "windows",
    target_env = "msvc",
    target_feature = "neon"
))]
mod neon;

const F32: u32 = 0;
const F16: u32 = 1;
const Q4_0: u32 = 2;
const Q8_0: u32 = 8;
const BF16: u32 = 30;
const PQ2: u32 = 142;
const PTQ1: u32 = 143;

fn layout(kind: u32) -> Result<(usize, usize)> {
    match kind {
        F32 => Ok((1, 4)),
        F16 | BF16 => Ok((1, 2)),
        Q4_0 => Ok((32, 18)),
        Q8_0 => Ok((32, 34)),
        PQ2 => Ok((128, 34)),
        PTQ1 => Ok((128, 28)),
        _ => Err(invalid(format!("unsupported runtime tensor type {kind}"))),
    }
}

fn dimensions(tensor: &Tensor) -> Result<(usize, usize, usize)> {
    let Some(&columns) = tensor.dimensions.first() else {
        return Err(invalid("tensor requires at least one dimension"));
    };
    let rows = tensor.dimensions[1..]
        .iter()
        .try_fold(1_usize, |count, &dimension| count.checked_mul(dimension))
        .ok_or_else(|| invalid("tensor row count overflow"))?;
    let (group, bytes) = layout(tensor.kind)?;
    if columns == 0 || rows == 0 || !columns.is_multiple_of(group) {
        return Err(invalid(
            "tensor dimensions do not contain complete nonempty blocks",
        ));
    }
    let row_bytes = (columns / group)
        .checked_mul(bytes)
        .ok_or_else(|| invalid("tensor row byte length overflow"))?;
    if row_bytes.checked_mul(rows) != Some(tensor.data.len()) {
        return Err(invalid("tensor data length does not match its dimensions"));
    }
    Ok((columns, rows, row_bytes))
}

fn finite(value: f32) -> Result<f32> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(invalid("nonfinite tensor or activation value"))
    }
}

fn scale(bytes: &[u8]) -> Result<f32> {
    finite(half_to_float(u16::from_le_bytes([bytes[0], bytes[1]])))
}

fn fill_into<T>(output: &mut Vec<T>, fill: impl FnOnce(&mut Vec<T>) -> Result<()>) -> Result<()> {
    output.clear();
    let result = fill(output);
    if result.is_err() {
        output.clear();
    }
    result
}

fn collect_into<T>(
    values: impl ExactSizeIterator<Item = Result<T>>,
    output: &mut Vec<T>,
) -> Result<()> {
    fill_into(output, |output| {
        output.try_reserve_exact(values.len())?;
        for value in values {
            output.push(value?);
        }
        Ok(())
    })
}

fn pq2_values(block: &[u8]) -> [i16; 128] {
    std::array::from_fn(|index| i16::from((block[2 + index / 4] >> (2 * (index % 4))) & 3) - 1)
}

fn ptq1_values(block: &[u8]) -> Result<[i16; 128]> {
    let mut packed = <[u8; 28]>::try_from(block)?;
    // The codec validates a nonzero scale. Scale zero is valid in a native
    // tensor, so validate the trit bytes with a neutral scale, independently.
    packed[26..].copy_from_slice(&0x3c00_u16.to_le_bytes());
    Ok(pq2_values(&unpack_ptq(&packed)?))
}

/// Validates a supported tensor without retaining decoded rows or matrices.
///
/// # Errors
/// Rejects unsupported kinds, malformed dimensions and byte extents, nonfinite
/// scalar values or scales, and noncanonical PTQ1 trit encodings.
pub fn validate_tensor(tensor: &Tensor) -> Result<()> {
    dimensions(tensor)?;
    match tensor.kind {
        F32 => {
            for chunk in tensor.data.as_chunks::<4>().0 {
                finite(f32::from_le_bytes(*chunk))?;
            }
        }
        F16 | BF16 => {
            for chunk in tensor.data.as_chunks::<2>().0 {
                let bits = u16::from_le_bytes(*chunk);
                finite(if tensor.kind == F16 {
                    half_to_float(bits)
                } else {
                    f32::from_bits(u32::from(bits) << 16)
                })?;
            }
        }
        PTQ1 => {
            for block in tensor.data.as_chunks::<28>().0 {
                scale(&block[26..])?;
                ptq1_values(block)?;
            }
        }
        _ => {
            let (_, block_bytes) = layout(tensor.kind)?;
            for block in tensor.data.chunks_exact(block_bytes) {
                scale(block)?;
            }
        }
    }
    Ok(())
}

/// Decodes a single contiguous row, including row zero of a vector tensor.
///
/// Only the selected row is expanded; the packed tensor remains immutable.
///
/// # Errors
/// Rejects unsupported kinds, incomplete blocks, invalid PTQ1 encodings,
/// nonfinite values, malformed byte extents, and an out-of-range row.
pub fn row(tensor: &Tensor, index: usize) -> Result<Vec<f32>> {
    let mut output = Vec::new();
    row_into(tensor, index, &mut output)?;
    Ok(output)
}

/// Decodes a single row into reusable caller-owned storage.
///
/// Existing elements are discarded. Capacity is retained, including on error;
/// the output is empty whenever decoding fails. Only the selected row expands.
///
/// # Errors
/// Rejects the same malformed tensors and row indices as [`row`], and returns
/// an error if the output cannot be allocated.
pub fn row_into(tensor: &Tensor, index: usize, output: &mut Vec<f32>) -> Result<()> {
    fill_into(output, |output| row_impl(tensor, index, output))
}

fn row_impl(tensor: &Tensor, index: usize, output: &mut Vec<f32>) -> Result<()> {
    let (columns, rows, row_bytes) = dimensions(tensor)?;
    if index >= rows {
        return Err(invalid("tensor row index out of bounds"));
    }
    let bytes = &tensor.data[index * row_bytes..(index + 1) * row_bytes];
    output.try_reserve_exact(columns)?;
    match tensor.kind {
        F32 => {
            for chunk in bytes.as_chunks::<4>().0 {
                output.push(finite(f32::from_le_bytes(*chunk))?);
            }
        }
        F16 | BF16 => {
            for chunk in bytes.as_chunks::<2>().0 {
                let bits = u16::from_le_bytes(*chunk);
                output.push(finite(if tensor.kind == F16 {
                    half_to_float(bits)
                } else {
                    f32::from_bits(u32::from(bits) << 16)
                })?);
            }
        }
        PQ2 | PTQ1 => {
            let (_, block_bytes) = layout(tensor.kind)?;
            for block in bytes.chunks_exact(block_bytes) {
                let (values, d) = if tensor.kind == PQ2 {
                    (pq2_values(block), scale(block)?)
                } else {
                    (ptq1_values(block)?, scale(&block[26..])?)
                };
                output.extend(values.map(|value| f32::from(value) * d));
            }
        }
        Q4_0 => {
            for block in bytes.as_chunks::<18>().0 {
                let d = scale(block)?;
                for shift in [0, 4] {
                    output.extend(
                        block[2..]
                            .iter()
                            .map(|&byte| f32::from(i16::from((byte >> shift) & 15) - 8) * d),
                    );
                }
            }
        }
        Q8_0 => {
            for block in bytes.as_chunks::<34>().0 {
                let d = scale(block)?;
                output.extend(
                    block[2..]
                        .iter()
                        .map(|&byte| f32::from(i8::from_ne_bytes([byte])) * d),
                );
            }
        }
        _ => return Err(invalid("unsupported row decoder")),
    }
    Ok(())
}

// Bounded casts implement the native float quantizers, with checked finite
// inputs and an explicit integer range check before conversion.
#[allow(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "The finite rounded activation is range checked before conversion to i8."
)]
fn quantized(value: f32) -> Result<i8> {
    if value.is_finite() && (-127.0..=127.0).contains(&value) {
        Ok(value as i8)
    } else {
        Err(invalid(
            "activation quantization exceeded its finite i8 range",
        ))
    }
}

fn q8k_into(input: &[f32], output: &mut Vec<Q8KBlock>) -> Result<()> {
    if !input.len().is_multiple_of(256) {
        return Err(invalid(
            "Q8_K activations require a multiple of 256 columns",
        ));
    }
    collect_into(
        input.as_chunks::<256>().0.iter().map(|chunk| {
            let mut maximum = 0.0_f32;
            for &value in chunk {
                finite(value)?;
                if value.abs() > maximum.abs() {
                    maximum = value;
                }
            }
            let mut block = Q8KBlock {
                scale: 0.0,
                values: [0; 256],
                sums: [0; 16],
            };
            if maximum == 0.0 {
                return Ok(block);
            }
            let inverse_scale = -127.0 / maximum;
            if !inverse_scale.is_finite() {
                return Err(invalid("Q8_K activation scale overflow"));
            }
            block.scale = 1.0 / inverse_scale;
            for (target, &value) in block.values.iter_mut().zip(chunk) {
                // nearest_int in the pinned C reference is ties-to-even.
                *target = quantized((value * inverse_scale).round_ties_even().min(127.0))?;
            }
            for (sum, values) in block.sums.iter_mut().zip(block.values.as_chunks::<16>().0) {
                *sum = values.iter().map(|&value| i16::from(value)).sum();
            }
            Ok(block)
        }),
        output,
    )
}

struct Q8Block {
    scale: f32,
    values: [i8; 32],
}

fn q8_into(input: &[f32], output: &mut Vec<Q8Block>) -> Result<()> {
    if !input.len().is_multiple_of(32) {
        return Err(invalid("Q8_0 activations require a multiple of 32 columns"));
    }
    collect_into(
        input.as_chunks::<32>().0.iter().map(|chunk| {
            let mut maximum = 0.0_f32;
            for &value in chunk {
                maximum = maximum.max(finite(value)?.abs());
            }
            let d = maximum / 127.0;
            let inverse_scale = if d == 0.0 { 0.0 } else { 1.0 / d };
            let mut block = Q8Block {
                scale: round_f16(d),
                values: [0; 32],
            };
            finite(block.scale)?;
            for (target, &value) in block.values.iter_mut().zip(chunk) {
                *target = quantized((value * inverse_scale).round())?;
            }
            Ok(block)
        }),
        output,
    )
}

#[allow(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "Bounded dot sums and power-of-two transform lengths are exactly representable."
)]
const fn integer_float(value: i32) -> f32 {
    // Packed dot sums and power-of-two Hadamard lengths are exactly
    // representable in f32 for the supported dimensions.
    value as f32
}

fn dot_integer(weights: &[i16], activations: &[i8]) -> f32 {
    integer_float(
        weights
            .iter()
            .zip(activations)
            .map(|(&weight, &activation)| i32::from(weight) * i32::from(activation))
            .sum(),
    )
}

#[allow(
    clippy::suboptimal_flops,
    reason = "Preserve scalar reference multiply-then-add rounding."
)]
fn dot_pq2(bytes: &[u8], input: &[Q8KBlock]) -> Result<f32> {
    let mut output = 0.0;
    for (index, block) in bytes.as_chunks::<34>().0.iter().enumerate() {
        let activation = &input[index / 2];
        let start = (index % 2) * 128;
        output += (scale(block)? * activation.scale)
            * dot_integer(&pq2_values(block), &activation.values[start..start + 128]);
    }
    finite(output)
}

#[cfg(all(
    feature = "experimental-neon",
    target_arch = "aarch64",
    target_os = "windows",
    target_env = "msvc",
    target_feature = "neon"
))]
#[allow(
    clippy::suboptimal_flops,
    reason = "Preserve scalar reference multiply-then-add rounding."
)]
fn dot_pq2_neon(bytes: &[u8], input: &[Q8KBlock]) -> Result<f32> {
    let mut output = 0.0;
    for (index, block) in bytes.as_chunks::<34>().0.iter().enumerate() {
        let activation = &input[index / 2];
        let start = (index % 2) * 128;
        output += (scale(block)? * activation.scale)
            * integer_float(neon::dot(
                block[2..].try_into()?,
                activation.values[start..start + 128].try_into()?,
            ));
    }
    finite(output)
}

#[allow(
    clippy::suboptimal_flops,
    reason = "Preserve scalar reference multiply-then-add rounding."
)]
fn dot_q8_groups(kind: u32, bytes: &[u8], input: &[Q8Block]) -> Result<f32> {
    let (_, block_bytes) = layout(kind)?;
    let mut output = 0.0;
    for (index, block) in bytes.chunks_exact(block_bytes).enumerate() {
        let (weights, d) = if kind == PQ2 {
            (pq2_values(block), scale(block)?)
        } else {
            (ptq1_values(block)?, scale(&block[26..])?)
        };
        let mut sum = 0.0;
        for (values, activation) in weights
            .as_chunks::<32>()
            .0
            .iter()
            .zip(&input[index * 4..index * 4 + 4])
        {
            sum += activation.scale * dot_integer(values, &activation.values);
        }
        output += d * sum;
    }
    finite(output)
}

#[cfg(all(
    feature = "experimental-neon",
    target_arch = "aarch64",
    target_os = "windows",
    target_env = "msvc",
    target_feature = "neon"
))]
#[allow(
    clippy::suboptimal_flops,
    reason = "Preserve scalar reference multiply-then-add rounding."
)]
fn dot_q8_groups_neon(kind: u32, bytes: &[u8], input: &[Q8Block]) -> Result<f32> {
    let (_, block_bytes) = layout(kind)?;
    let mut output = 0.0;
    for (index, block) in bytes.chunks_exact(block_bytes).enumerate() {
        let activations = <&[Q8Block; 4]>::try_from(&input[index * 4..index * 4 + 4])?;
        let values = activations.each_ref().map(|activation| &activation.values);
        let (sums, d) = if kind == PQ2 {
            (
                neon::dot_groups(block[2..].try_into()?, values),
                scale(block)?,
            )
        } else {
            (
                neon::dot_ptq1_groups(block.try_into()?, values)?,
                scale(&block[26..])?,
            )
        };
        let mut sum = 0.0;
        for (integer, activation) in sums.into_iter().zip(activations) {
            sum += activation.scale * integer_float(integer);
        }
        output += d * sum;
    }
    finite(output)
}

#[allow(
    clippy::suboptimal_flops,
    reason = "Preserve scalar reference multiply-then-add rounding."
)]
fn dot_legacy(kind: u32, bytes: &[u8], input: &[Q8Block]) -> Result<f32> {
    let (_, block_bytes) = layout(kind)?;
    let mut output = 0.0;
    for (block, activation) in bytes.chunks_exact(block_bytes).zip(input) {
        let values: [i16; 32] = std::array::from_fn(|index| {
            if kind == Q4_0 {
                i16::from((block[2 + index % 16] >> (4 * (index / 16))) & 15) - 8
            } else {
                i16::from(i8::from_ne_bytes([block[2 + index]]))
            }
        });
        output += (scale(block)? * activation.scale) * dot_integer(&values, &activation.values);
    }
    finite(output)
}

#[allow(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "The scalar reference rounds the accumulated binary64 sum to binary32."
)]
fn float_dot(weights: &[f32], input: &[f32]) -> f32 {
    // The non-SIMD ggml reference sums rounded f32 products in f64.
    weights
        .iter()
        .zip(input)
        .map(|(&a, &b)| f64::from(a * b))
        .sum::<f64>() as f32
}

/// Reusable activation and decoded-row storage for [`matvec_into`].
///
/// Construct with [`Default::default`]. Storage grows fallibly to accommodate
/// each call and retains its capacity across tensors, activation formats and
/// errors. Scratch contains no tensor-sized decoded matrix or model state.
pub struct MatvecScratch {
    q8k: Vec<Q8KBlock>,
    q8: Vec<Q8Block>,
    activations: Vec<f32>,
    row: Vec<f32>,
    requested_backend: MatvecBackend,
    selected_backend: MatvecBackend,
    #[cfg(test)]
    preparations: usize,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum ActivationContract {
    Q8K,
    Q8,
    F32,
    F16,
    BF16,
}

impl ActivationContract {
    const fn for_tensor(tensor: &Tensor, columns: usize) -> Self {
        match tensor.kind {
            PQ2 if columns.is_multiple_of(256) => Self::Q8K,
            PQ2 | PTQ1 | Q4_0 | Q8_0 => Self::Q8,
            F16 => Self::F16,
            BF16 => Self::BF16,
            _ => Self::F32,
        }
    }
}

/// One scoped activation preparation shared by compatible matrix operations.
///
/// Created by [`MatvecScratch::prepare`]. The guard borrows both its exact input
/// and scratch until dropped, preventing input mutation or scratch reuse between
/// sibling projections. No pointer/content cache survives the scope. Tensor
/// weights must already use the same input basis, signs and permutation.
pub struct PreparedMatvec<'a> {
    input: &'a [f32],
    contract: ActivationContract,
    scratch: &'a mut MatvecScratch,
}

/// Packed matrix kernel selection, independent of model and activation precision.
///
/// Automatic selection uses the compiled baseline NEON implementation on native
/// Windows ARM64 MSVC for complete PQ2 and PTQ1 blocks. Other targets and formats
/// use scalar. Select [`Self::Scalar`] to retain the reference implementation.
/// No optional CPU extension is enabled or inferred by this selection.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum MatvecBackend {
    /// Selects the best available packed kernel for the build, target and format.
    #[default]
    Auto,
    /// Scalar reference implementation and unconditional fallback.
    Scalar,
    /// Explicit baseline NEON request, requiring the `experimental-neon` feature.
    ///
    /// Exact synthetic parity does not qualify whole-model performance or parity.
    ExperimentalNeon,
}

type Pq2RowKernel = fn(&[u8], &[Q8KBlock]) -> Result<f32>;
type Q8GroupRowKernel = fn(u32, &[u8], &[Q8Block]) -> Result<f32>;

impl Default for MatvecScratch {
    fn default() -> Self {
        Self {
            q8k: Vec::new(),
            q8: Vec::new(),
            activations: Vec::new(),
            row: Vec::new(),
            requested_backend: MatvecBackend::Auto,
            selected_backend: MatvecBackend::Scalar,
            #[cfg(test)]
            preparations: 0,
        }
    }
}

impl MatvecScratch {
    /// Prepares one exact activation vector for compatible sibling projections.
    ///
    /// Quantization or floating activation rounding occurs once. Compatibility
    /// requires equal input width and activation precision/quantizer; in
    /// particular, `Q8_K` and `Q8_0` preparations cannot be interchanged. The
    /// caller must establish identical transform basis, signs and permutation.
    /// Scratch retains reusable format buffers and one decoded row, without
    /// a decoded matrix or per-sibling activation copies.
    ///
    /// # Errors
    /// Rejects malformed tensor dimensions, unsupported kinds, mismatched or
    /// nonfinite input, activation precision overflow, quantization errors, and
    /// failed scratch allocation.
    pub fn prepare<'a>(
        &'a mut self,
        tensor: &Tensor,
        input: &'a [f32],
    ) -> Result<PreparedMatvec<'a>> {
        self.selected_backend = MatvecBackend::Scalar;
        let (columns, _, _) = dimensions(tensor)?;
        if input.len() != columns || input.iter().any(|value| !value.is_finite()) {
            return Err(invalid(
                "matvec requires a matching finite activation vector",
            ));
        }
        let contract = ActivationContract::for_tensor(tensor, columns);
        match contract {
            ActivationContract::Q8K => q8k_into(input, &mut self.q8k)?,
            ActivationContract::Q8 => q8_into(input, &mut self.q8)?,
            _ => collect_into(
                input.iter().map(|&value| {
                    finite(match contract {
                        ActivationContract::F16 => round_f16(value),
                        ActivationContract::BF16 => round_bf16(value),
                        _ => value,
                    })
                }),
                &mut self.activations,
            )?,
        }
        #[cfg(test)]
        {
            self.preparations += 1;
        }
        Ok(PreparedMatvec {
            input,
            contract,
            scratch: self,
        })
    }

    #[cfg(test)]
    pub(crate) const fn preparation_count(&self) -> usize {
        self.preparations
    }

    /// Creates reusable storage with an explicit kernel request.
    ///
    /// Unsupported requests fall back to scalar at each matrix operation. The
    /// default constructor requests automatic selection. Disabling default
    /// features omits NEON unless `experimental-neon` is explicitly enabled.
    #[must_use]
    pub fn with_backend(backend: MatvecBackend) -> Self {
        Self {
            requested_backend: backend,
            ..Self::default()
        }
    }

    /// Configured dispatch policy, preserved across preparation and failures.
    ///
    /// Individual operations may select scalar fallback. Use
    /// [`Self::selected_backend`] for the last operation's actual selection.
    #[must_use]
    pub const fn requested_backend(&self) -> MatvecBackend {
        self.requested_backend
    }

    /// Returns the backend selected for the most recent matrix operation.
    ///
    /// Initially scalar. Selection resets for every call, including failed calls;
    /// this records dispatch rather than successful completion or qualification.
    #[must_use]
    pub const fn selected_backend(&self) -> MatvecBackend {
        self.selected_backend
    }

    const fn select_pq2(&self) -> (MatvecBackend, Pq2RowKernel) {
        #[cfg(all(
            feature = "experimental-neon",
            target_arch = "aarch64",
            target_os = "windows",
            target_env = "msvc",
            target_feature = "neon"
        ))]
        if matches!(
            self.requested_backend,
            MatvecBackend::Auto | MatvecBackend::ExperimentalNeon
        ) {
            return (MatvecBackend::ExperimentalNeon, dot_pq2_neon);
        }
        // Read the request on scalar-only builds as well; no request can bypass
        // the build/target contract or enable an optional instruction set.
        let _ = self.requested_backend;
        (MatvecBackend::Scalar, dot_pq2)
    }

    const fn select_q8_groups(&self) -> (MatvecBackend, Q8GroupRowKernel) {
        #[cfg(all(
            feature = "experimental-neon",
            target_arch = "aarch64",
            target_os = "windows",
            target_env = "msvc",
            target_feature = "neon"
        ))]
        if matches!(
            self.requested_backend,
            MatvecBackend::Auto | MatvecBackend::ExperimentalNeon
        ) {
            return (MatvecBackend::ExperimentalNeon, dot_q8_groups_neon);
        }
        let _ = self.requested_backend;
        (MatvecBackend::Scalar, dot_q8_groups)
    }
}

/// Multiplies packed rows by one activation vector in the stored tensor basis.
///
/// Floating tensors use their native activation precision. PQ2 uses `Q8_K`
/// for rows divisible by 256, otherwise the pinned `Q8_0` fallback. PTQ1 and
/// the supported legacy Q kinds use `Q8_0`. Activations are quantized once per
/// call and reused for every row. No matrix-sized decoded copy exists.
///
/// # Errors
/// Rejects malformed tensors, unsupported kinds, nonfinite values, invalid PTQ1
/// encodings, input dimension mismatches, and incomplete activation groups.
pub fn matvec(tensor: &Tensor, input: &[f32]) -> Result<Vec<f32>> {
    let mut output = Vec::new();
    matvec_into(tensor, input, &mut output, &mut MatvecScratch::default())?;
    Ok(output)
}

/// Multiplies packed rows into reusable output and activation storage.
///
/// Numerical operations follow [`matvec`]. Existing output elements are
/// discarded, and the output is empty on error. Output and scratch capacities
/// are retained for subsequent calls; any required growth is fallible.
///
/// # Errors
/// Rejects the same invalid tensors and activations as [`matvec`], and returns
/// an error if output or scratch storage cannot be allocated.
pub fn matvec_into(
    tensor: &Tensor,
    input: &[f32],
    output: &mut Vec<f32>,
    scratch: &mut MatvecScratch,
) -> Result<()> {
    output.clear();
    scratch.prepare(tensor, input)?.matvec_into(tensor, output)
}

impl PreparedMatvec<'_> {
    /// Checks whether a tensor uses this preparation's width and activation contract.
    ///
    /// This cannot establish the caller's transform basis, signs or permutation.
    ///
    /// # Errors
    /// Rejects unsupported tensor kinds and malformed dimensions or byte extents.
    pub fn is_compatible(&self, tensor: &Tensor) -> Result<bool> {
        let (columns, _, _) = dimensions(tensor)?;
        Ok(columns == self.input.len()
            && ActivationContract::for_tensor(tensor, columns) == self.contract)
    }

    /// Multiplies compatible packed rows using the already prepared activation.
    ///
    /// Arithmetic and backend dispatch match [`matvec_into`]. Existing output
    /// elements are discarded and output is empty on error. All growth is
    /// fallible; this operation does not transform or quantize activations again.
    ///
    /// # Errors
    /// Rejects incompatible activation contracts, malformed tensors, nonfinite
    /// results, invalid PTQ1 encodings and failed output or row allocation.
    pub fn matvec_into(&mut self, tensor: &Tensor, output: &mut Vec<f32>) -> Result<()> {
        self.scratch.selected_backend = MatvecBackend::Scalar;
        fill_into(output, |output| self.multiply(tensor, output))
    }

    fn multiply(&mut self, tensor: &Tensor, output: &mut Vec<f32>) -> Result<()> {
        let (columns, rows, row_bytes) = dimensions(tensor)?;
        if columns != self.input.len()
            || ActivationContract::for_tensor(tensor, columns) != self.contract
        {
            return Err(invalid("incompatible prepared matvec activation contract"));
        }
        let scratch = &mut *self.scratch;
        output.try_reserve_exact(rows)?;
        match tensor.kind {
            PQ2 if columns.is_multiple_of(256) => {
                let (backend, dot) = scratch.select_pq2();
                scratch.selected_backend = backend;
                for bytes in tensor.data.chunks_exact(row_bytes) {
                    output.push(dot(bytes, &scratch.q8k)?);
                }
            }
            PQ2 | PTQ1 => {
                let (backend, dot) = scratch.select_q8_groups();
                scratch.selected_backend = backend;
                for bytes in tensor.data.chunks_exact(row_bytes) {
                    output.push(dot(tensor.kind, bytes, &scratch.q8)?);
                }
            }
            Q4_0 | Q8_0 => {
                for bytes in tensor.data.chunks_exact(row_bytes) {
                    output.push(dot_legacy(tensor.kind, bytes, &scratch.q8)?);
                }
            }
            _ => {
                for (index, _) in tensor.data.chunks_exact(row_bytes).enumerate() {
                    row_into(tensor, index, &mut scratch.row)?;
                    output.push(finite(float_dot(&scratch.row, &scratch.activations))?);
                }
            }
        }
        Ok(())
    }
}

#[allow(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "The converted binary16 sign and magnitude occupy at most sixteen bits."
)]
pub(crate) fn half_bits(value: f32) -> u16 {
    // Port of ggml_compute_fp32_to_fp16; the shift/add path rounds ties to
    // even via native f32 arithmetic and includes subnormals and signed zero.
    let mut base = (value.abs() * f32::from_bits(0x7780_0000)) * f32::from_bits(0x0880_0000);
    let bits = value.to_bits();
    let doubled = bits.wrapping_add(bits);
    let exponent_bias = (doubled & 0xff00_0000).max(0x7100_0000);
    base += f32::from_bits((exponent_bias >> 1) + 0x0780_0000);
    let rounded = base.to_bits();
    let magnitude = if doubled > 0xff00_0000 {
        0x7e00
    } else {
        ((rounded >> 13) & 0x7c00) + (rounded & 0x0fff)
    };
    ((bits >> 16) & 0x8000 | magnitude) as u16
}

/// Rounds to IEEE binary16, returning its exactly expanded binary32 value.
/// Signed zero, infinities, subnormals, and ties-to-even are preserved.
#[must_use]
pub fn round_f16(value: f32) -> f32 {
    half_to_float(half_bits(value))
}

const fn round_bf16(value: f32) -> f32 {
    let bits = value.to_bits();
    if value.is_nan() {
        f32::from_bits((bits & 0xffff_0000) | 0x0040_0000)
    } else {
        f32::from_bits(bits.wrapping_add(0x7fff + ((bits >> 16) & 1)) & 0xffff_0000)
    }
}

/// Applies the `SiLU` activation to one value.
#[must_use]
pub fn silu(value: f32) -> f32 {
    value / (1.0 + (-value).exp())
}

/// Normalizes a nonempty vector using the ggml scalar RMS accumulation order.
///
/// # Errors
/// Rejects unequal or empty vectors, nonfinite inputs, negative epsilon, and
/// a normalization whose scale is not finite and positive.
pub fn rms_norm(input: &[f32], weight: &[f32], epsilon: f32) -> Result<Vec<f32>> {
    let mut output = Vec::new();
    rms_norm_into(input, weight, epsilon, &mut output)?;
    Ok(output)
}

/// Normalizes a vector into reusable caller-owned storage.
///
/// Numerical operations follow [`rms_norm`]. Existing elements are discarded,
/// capacity is retained, and the output is empty on every error.
///
/// # Errors
/// Rejects the same invalid vectors and epsilon as [`rms_norm`], and returns
/// an error if the output cannot be allocated.
pub fn rms_norm_into(
    input: &[f32],
    weight: &[f32],
    epsilon: f32,
    output: &mut Vec<f32>,
) -> Result<()> {
    fill_into(output, |output| {
        rms_norm_impl(input, weight, epsilon, output)
    })
}

#[allow(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "The scalar normalization contract rounds the binary64 mean to binary32."
)]
fn rms_norm_impl(input: &[f32], weight: &[f32], epsilon: f32, output: &mut Vec<f32>) -> Result<()> {
    if input.is_empty() || input.len() != weight.len() || !epsilon.is_finite() || epsilon < 0.0 {
        return Err(invalid(
            "RMS normalization requires equal nonempty vectors and nonnegative epsilon",
        ));
    }
    let count = u32::try_from(input.len())?;
    let mut sum = 0.0_f64;
    for (&value, &w) in input.iter().zip(weight) {
        finite(value)?;
        finite(w)?;
        sum += f64::from(value * value);
    }
    let mean = (sum / f64::from(count)) as f32;
    let scale = 1.0 / (mean + epsilon).sqrt();
    if !scale.is_finite() || scale <= 0.0 {
        return Err(invalid("RMS normalization produced an invalid scale"));
    }
    collect_into(
        input
            .iter()
            .zip(weight)
            .map(|(&value, &w)| finite(value * scale * w)),
        output,
    )
}

/// Applies a normalized Sylvester Hadamard transform to one contiguous block.
///
/// # Errors
/// Rejects zero or non-power-of-two block lengths and nonfinite input/output.
pub fn hadamard(values: &mut [f32]) -> Result<()> {
    if !values.len().is_power_of_two() || values.iter().any(|value| !value.is_finite()) {
        return Err(invalid("Hadamard requires a finite power-of-two block"));
    }
    let count = u32::try_from(values.len())?;
    let mut half = 1;
    while half < values.len() {
        for block in values.chunks_exact_mut(half * 2) {
            for index in 0..half {
                let left = block[index];
                let right = block[index + half];
                block[index] = left + right;
                block[index + half] = left - right;
            }
        }
        half *= 2;
    }
    let normalization = 1.0 / integer_float(i32::try_from(count)?).sqrt();
    for value in values {
        *value = finite(*value * normalization)?;
    }
    Ok(())
}

/// Replaces finite logits by their normalized, stable exponential weights.
///
/// Negative infinity is accepted as an attention mask. All-masked rows fail.
///
/// # Errors
/// Rejects empty vectors, NaNs, positive infinity, and all-masked rows.
#[allow(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "The scalar softmax contract rounds the binary64 inverse sum to binary32."
)]
pub fn softmax(values: &mut [f32]) -> Result<()> {
    let maximum = values.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    if !maximum.is_finite()
        || values
            .iter()
            .any(|value| value.is_nan() || *value == f32::INFINITY)
    {
        return Err(invalid(
            "softmax requires at least one finite logit and no NaN or positive infinity",
        ));
    }
    let mut total = 0.0_f64;
    for value in &mut *values {
        *value = (*value - maximum).exp();
        total += f64::from(*value);
    }
    let inverse_sum = (1.0 / total) as f32;
    for value in values {
        *value *= inverse_sum;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{MatvecScratch, Tensor, matvec_into, q8_into, q8k_into};

    #[test]
    fn prepared_siblings_quantize_once_and_retry_with_fresh_contents() -> crate::Result<()> {
        let mut block = [0x55; 34];
        block[..2].copy_from_slice(&0x3c00_u16.to_le_bytes());
        block[2] = 0xaa;
        let tensor = Tensor {
            name: "prepared".to_owned(),
            dimensions: vec![256, 2],
            kind: super::PQ2,
            data: Arc::from(block.repeat(4)),
        };
        let mut scratch = MatvecScratch::default();
        let mut output = Vec::new();
        let mut input = [127.0; 256];
        {
            let mut prepared = scratch.prepare(&tensor, &input)?;
            for _ in 0..4 {
                prepared.matvec_into(&tensor, &mut output)?;
                assert_eq!(prepared.scratch.preparation_count(), 1);
            }
        }
        let expected = output.clone();
        input.fill(-127.0);
        scratch
            .prepare(&tensor, &input)?
            .matvec_into(&tensor, &mut output)?;
        assert_eq!(scratch.preparation_count(), 2);
        assert!(
            expected
                .iter()
                .zip(output)
                .all(|(a, b)| (-a).to_bits() == b.to_bits())
        );
        Ok(())
    }

    #[test]
    fn matvec_scratch_retains_quantized_and_floating_storage() -> crate::Result<()> {
        let mut scratch = MatvecScratch::default();
        q8k_into(&[127.0; 512], &mut scratch.q8k)?;
        let wide_pointer = scratch.q8k.as_ptr();
        q8k_into(&[0.0; 256], &mut scratch.q8k)?;
        assert_eq!(scratch.q8k.len(), 1);
        assert_eq!(scratch.q8k.as_ptr(), wide_pointer);
        assert_eq!(scratch.q8k[0].values, [0; 256]);
        q8_into(&[127.0; 128], &mut scratch.q8)?;
        let legacy_pointer = scratch.q8.as_ptr();
        q8_into(&[0.0; 32], &mut scratch.q8)?;
        assert_eq!(scratch.q8.len(), 1);
        assert_eq!(scratch.q8.as_ptr(), legacy_pointer);
        assert_eq!(scratch.q8[0].values, [0; 32]);
        let mut tensor = Tensor {
            name: "scratch".to_owned(),
            dimensions: vec![2, 2],
            kind: super::F32,
            data: Arc::from(
                [1.0_f32, 2.0, 3.0, 4.0]
                    .into_iter()
                    .flat_map(f32::to_le_bytes)
                    .collect::<Vec<_>>(),
            ),
        };
        let mut output = Vec::new();
        matvec_into(&tensor, &[1.0; 2], &mut output, &mut scratch)?;
        let activation_pointer = scratch.activations.as_ptr();
        let row_pointer = scratch.row.as_ptr();
        tensor.dimensions = vec![1];
        tensor.data = Arc::from(2.0_f32.to_le_bytes().as_slice());
        matvec_into(&tensor, &[3.0], &mut output, &mut scratch)?;
        assert_eq!(scratch.activations.len(), 1);
        assert_eq!(scratch.row.len(), 1);
        assert_eq!(scratch.activations.as_ptr(), activation_pointer);
        assert_eq!(scratch.row.as_ptr(), row_pointer);
        assert_eq!(output, [6.0]);
        Ok(())
    }
}
