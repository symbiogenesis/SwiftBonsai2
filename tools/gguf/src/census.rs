// Keep the private contract explicit if the parent is reorganized later.
#![allow(clippy::redundant_pub_crate)]

use crate::inspect::Inspection;
use crate::{CODEC_COMMIT, PRISM_DIALECT, Result};
use serde_json::{Value, json};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

const BUFFER_BYTES: usize = 65_536;
const BLOCK_BYTES: usize = 34;
const BLOCK_ELEMENTS: u64 = 128;

fn bad(message: impl Into<String>) -> crate::Error {
    crate::bad(format!("GGUF: {}", message.into()))
}

fn add(a: u64, b: u64) -> Result<u64> {
    a.checked_add(b)
        .ok_or_else(|| bad("descriptor/count arithmetic overflow."))
}

fn mul(a: u64, b: u64) -> Result<u64> {
    a.checked_mul(b)
        .ok_or_else(|| bad("descriptor/count arithmetic overflow."))
}

fn sign_bytes(nonzero: u64) -> Result<u64> {
    Ok(add(nonzero, 7)? / 8)
}

fn candidate(
    eligible: bool,
    bitmap: u64,
    signs: u64,
    block_signs: u64,
    scales: u64,
) -> Result<Value> {
    let mut result = json!({"Eligible": eligible, "IncludesFormatOrIndexOverhead": false});
    if eligible {
        result["BitmapBytes"] = json!(bitmap);
        result["UnchangedFp16ScaleBytes"] = json!(scales);
        result["TensorPackedSignBytesLowerBound"] = json!(signs);
        result["Block128BytePaddedSignBytes"] = json!(block_signs);
        result["TensorPackedPayloadBytesLowerBound"] = json!(add(add(bitmap, scales)?, signs)?);
        result["Block128BytePaddedPayloadBytes"] = json!(add(add(bitmap, scales)?, block_signs)?);
    }
    Ok(result)
}

// Keep streaming totals adjacent to their report fields for auditability.
#[allow(clippy::too_many_lines)]
pub(crate) fn census(file: &mut File, inspection: &Inspection) -> Result<Value> {
    if inspection.report["Dialect"].as_str() != Some(PRISM_DIALECT) {
        return Err(bad(
            "PQ2 census requires the explicitly pinned Prism dialect.",
        ));
    }
    let data_offset = inspection.report["TensorDataOffset"]
        .as_u64()
        .ok_or_else(|| bad("missing tensor data offset."))?;
    // Each byte contributes four codes into byte-wide lanes. Each block has
    // only 128 codes, so no carry can cross a lane while accumulating a block.
    let mut code_counts = [0u32; 256];
    for (value, counts) in code_counts.iter_mut().enumerate() {
        for shift in (0..8).step_by(2) {
            *counts += 1u32 << (((value >> shift) & 3) * 8);
        }
    }
    let mut buffer = vec![0u8; BUFFER_BYTES].into_boxed_slice();
    let mut reports = Vec::new();
    let mut total_codes = [0u64; 4];
    let mut total_bytes = 0;
    let mut total_blocks = 0;
    let mut total_zero_scales = 0;
    let mut total_nonfinite_scales = 0;
    let mut total_signs = 0;
    let mut total_block_signs = 0;
    let mut unchanged_bytes = 0;
    let mut all_eligible = true;
    let mut tensors: Vec<_> = inspection.tensors.iter().collect();
    tensors.sort_by_key(|tensor| tensor.offset);
    for tensor in tensors {
        if tensor.type_id != 142 {
            unchanged_bytes = add(unchanged_bytes, tensor.bytes)?;
            continue;
        }
        file.seek(SeekFrom::Start(add(data_offset, tensor.offset)?))?;
        let blocks = tensor.elements / BLOCK_ELEMENTS;
        let mut remaining = blocks;
        let mut counts = [0u64; 4];
        let mut zero_scales = 0;
        let mut nonfinite_scales = 0;
        let mut block_signs = 0;
        while remaining > 0 {
            let batch = usize::try_from(remaining.min(u64::try_from(BUFFER_BYTES / BLOCK_BYTES)?))?;
            file.read_exact(&mut buffer[..batch * BLOCK_BYTES])?;
            for block in buffer[..batch * BLOCK_BYTES].as_chunks::<BLOCK_BYTES>().0 {
                let scale = u16::from_le_bytes([block[0], block[1]]);
                if scale.trailing_zeros() >= 15 {
                    zero_scales += 1;
                } else if scale & 0x7c00 == 0x7c00 {
                    nonfinite_scales += 1;
                }
                let mut block_counts = 0u32;
                for byte in &block[2..] {
                    block_counts += code_counts[usize::from(*byte)];
                }
                let zeros = u64::from((block_counts >> 8) & 255);
                counts[0] = add(counts[0], u64::from(block_counts & 255))?;
                counts[1] = add(counts[1], zeros)?;
                counts[2] = add(counts[2], u64::from((block_counts >> 16) & 255))?;
                counts[3] = add(counts[3], u64::from(block_counts >> 24))?;
                block_signs = add(block_signs, sign_bytes(BLOCK_ELEMENTS - zeros)?)?;
            }
            remaining -= u64::try_from(batch)?;
        }
        let nonzero = add(add(counts[0], counts[2])?, counts[3])?;
        let eligible = counts[3] == 0 && zero_scales == 0 && nonfinite_scales == 0;
        let signs = sign_bytes(nonzero)?;
        let bitmap = mul(blocks, 16)?;
        let scales = mul(blocks, 2)?;
        reports.push(json!({
            "Name": tensor.name, "TypeId": tensor.type_id, "Blocks": blocks, "Scales": blocks,
            "ScaleBytes": scales, "Elements": tensor.elements, "SerializedBytes": tensor.bytes,
            "CodeMinusOne": counts[0], "CodeZero": counts[1], "CodePlusOne": counts[2], "CodePlusTwo": counts[3],
            "ZeroScaleBlocks": zero_scales, "NonfiniteScaleBlocks": nonfinite_scales, "StrictTernaryCodes": counts[3] == 0,
            "LosslessThreeSymbolEligible": eligible, "BitcosCandidate": candidate(eligible, bitmap, signs, block_signs, scales)?
        }));
        for (total, count) in total_codes.iter_mut().zip(counts) {
            *total = add(*total, count)?;
        }
        total_bytes = add(total_bytes, tensor.bytes)?;
        total_blocks = add(total_blocks, blocks)?;
        total_zero_scales = add(total_zero_scales, zero_scales)?;
        total_nonfinite_scales = add(total_nonfinite_scales, nonfinite_scales)?;
        total_signs = add(total_signs, signs)?;
        total_block_signs = add(total_block_signs, block_signs)?;
        all_eligible &= eligible;
    }
    if reports.is_empty() {
        return Err(bad("PQ2 census found no PQ2_0 tensors."));
    }
    let mut candidate = candidate(
        all_eligible,
        mul(total_blocks, 16)?,
        total_signs,
        total_block_signs,
        mul(total_blocks, 2)?,
    )?;
    if all_eligible {
        let retained = file
            .metadata()?
            .len()
            .checked_sub(total_bytes)
            .ok_or_else(|| bad("descriptor/count arithmetic overflow."))?;
        candidate["WholeFileBytesLowerBoundWithExistingNonPq2Bytes"] = json!(add(
            retained,
            candidate["TensorPackedPayloadBytesLowerBound"]
                .as_u64()
                .ok_or_else(|| bad("missing candidate size."))?
        )?);
        candidate["WholeFileBlock128BytePaddedBytesWithExistingNonPq2Bytes"] = json!(add(
            retained,
            candidate["Block128BytePaddedPayloadBytes"]
                .as_u64()
                .ok_or_else(|| bad("missing candidate size."))?
        )?);
    }
    Ok(json!({
        "CodecCommit": CODEC_COMMIT, "TensorCount": reports.len(), "Tensors": reports,
        "Blocks": total_blocks, "Scales": total_blocks, "ScaleBytes": mul(total_blocks, 2)?,
        "SerializedBytesRead": total_bytes, "UnchangedOtherTensorBytes": unchanged_bytes,
        "CodeMinusOne": total_codes[0], "CodeZero": total_codes[1], "CodePlusOne": total_codes[2], "CodePlusTwo": total_codes[3],
        "ZeroScaleBlocks": total_zero_scales, "NonfiniteScaleBlocks": total_nonfinite_scales,
        "LosslessThreeSymbolEligible": all_eligible, "BitcosCandidate": candidate,
        "PayloadBufferBytes": BUFFER_BYTES, "MaximumBlocksPerRead": BUFFER_BYTES / BLOCK_BYTES,
        "Definition": "PQ2_0 stores little-endian FP16 scale then 32 bytes of low-to-high 2-bit codes; decoded value is (code-1)*scale. Counts are raw codes, never decoded zeros. Eligibility requires no +2 codes and finite nonzero scales. Candidate bytes retain exact scale bytes and exclude format, tensor offsets, indices and alignment overhead. They are serialized-size estimates, not RAM or throughput measurements."
    }))
}
