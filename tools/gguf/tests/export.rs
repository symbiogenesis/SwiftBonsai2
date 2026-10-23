//! Synthetic export checks with independent expected native wire bytes.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bonsai_gguf::{CODEC_COMMIT, PRISM_DIALECT, Result, export_ptq1, inspect};
use sha2::{Digest, Sha256};

struct Fixture {
    bytes: Vec<u8>,
    data_offset: usize,
    type_positions: [usize; 3],
    large_blocks: usize,
    last_offset: usize,
}

fn u32_bytes(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend(value.to_le_bytes());
}

fn u64_bytes(bytes: &mut Vec<u8>, value: u64) {
    bytes.extend(value.to_le_bytes());
}

fn text(bytes: &mut Vec<u8>, value: &str) -> Result<()> {
    u64_bytes(bytes, u64::try_from(value.len())?);
    bytes.extend(value.as_bytes());
    Ok(())
}

fn string_metadata(bytes: &mut Vec<u8>, key: &str, value: &str) -> Result<()> {
    text(bytes, key)?;
    u32_bytes(bytes, 8);
    text(bytes, value)
}

fn tensor(
    bytes: &mut Vec<u8>,
    name: &str,
    elements: u64,
    type_id: u32,
    offset: usize,
) -> Result<usize> {
    text(bytes, name)?;
    u32_bytes(bytes, 1);
    u64_bytes(bytes, elements);
    let position = bytes.len();
    u32_bytes(bytes, type_id);
    u64_bytes(bytes, u64::try_from(offset)?);
    Ok(position)
}

const fn aligned(value: usize) -> usize {
    value.div_ceil(32) * 32
}

fn fixture(large_blocks: usize, file_type_tag: u32) -> Result<Fixture> {
    let mut bytes = Vec::new();
    u32_bytes(&mut bytes, 0x4655_4747);
    u32_bytes(&mut bytes, 3);
    u64_bytes(&mut bytes, 3);
    u64_bytes(&mut bytes, 4);
    string_metadata(&mut bytes, "general.architecture", "synthetic")?;
    text(&mut bytes, "general.file_type")?;
    u32_bytes(&mut bytes, file_type_tag);
    u32_bytes(&mut bytes, 141);
    string_metadata(&mut bytes, "tokenizer.chat_template", "{{ unchanged }}")?;
    string_metadata(
        &mut bytes,
        "prism.hadamard.transform",
        "unchanged-transform",
    )?;
    let source_pq_offset = 64;
    let last_offset = aligned(source_pq_offset + large_blocks * 34) + 32;
    // Descriptor order deliberately differs from physical order.
    let last_position = tensor(&mut bytes, "last.pq", 128, 142, last_offset)?;
    let other_position = tensor(&mut bytes, "first.bf16", 2, 30, 0)?;
    let large_position = tensor(
        &mut bytes,
        "large.pq",
        u64::try_from(large_blocks)? * 128,
        142,
        source_pq_offset,
    )?;
    let data_offset = aligned(bytes.len());
    bytes.resize(data_offset, 0);
    // Nonzero source gaps and trailing padding must be normalized in output.
    bytes.resize(data_offset + last_offset + 64, 0xef);
    bytes[data_offset..data_offset + 4].copy_from_slice(&[1, 2, 3, 4]);
    let mut block = [0xaa; 34];
    block[..2].copy_from_slice(&0x3c00_u16.to_le_bytes());
    for index in 0..large_blocks {
        let position = data_offset + source_pq_offset + index * 34;
        bytes[position..position + 34].copy_from_slice(&block);
    }
    block[..2].copy_from_slice(&0xbc00_u16.to_le_bytes());
    bytes[data_offset + last_offset..data_offset + last_offset + 34].copy_from_slice(&block);
    Ok(Fixture {
        bytes,
        data_offset,
        type_positions: [last_position, other_position, large_position],
        large_blocks,
        last_offset,
    })
}

fn scratch() -> Result<PathBuf> {
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let path = std::env::temp_dir().join(format!(
        "SwiftBonsai-GgufExport-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir(&path)?;
    Ok(path)
}

fn sha(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .into_iter()
        .flat_map(|byte| [byte >> 4, byte & 0x0f])
        .map(|nibble| char::from(b"0123456789abcdef"[usize::from(nibble)]))
        .collect()
}

fn convert(source: &Path, output: &Path, bytes: &[u8]) -> Result<serde_json::Value> {
    export_ptq1(
        source,
        output,
        u64::try_from(bytes.len())?,
        &sha(bytes),
        Duration::from_secs(30),
    )
}

#[test]
fn streamed_export_matches_independent_wire_bytes_across_batch_boundary() -> Result<()> {
    let scratch = scratch()?;
    let source = scratch.join("source.gguf");
    let output = scratch.join("export.gguf");
    let fixture = fixture(1928, 4)?;
    fs::write(&source, &fixture.bytes)?;
    let report = convert(&source, &output, &fixture.bytes)?;
    let converted = fs::read(&output)?;
    let last_offset = aligned(32 + fixture.large_blocks * 28);
    let mut expected = fixture.bytes[..fixture.data_offset].to_vec();
    let file_type_key = b"general.file_type";
    let file_type_position = expected
        .windows(file_type_key.len())
        .position(|bytes| bytes == file_type_key)
        .ok_or("missing fixture file_type key")?
        + file_type_key.len()
        + 4;
    expected[file_type_position..file_type_position + 4].copy_from_slice(&143_u32.to_le_bytes());
    for (position, type_id, offset) in [
        (fixture.type_positions[0], 143_u32, last_offset),
        (fixture.type_positions[1], 30, 0),
        (fixture.type_positions[2], 143, 32),
    ] {
        expected[position..position + 4].copy_from_slice(&type_id.to_le_bytes());
        expected[position + 4..position + 12]
            .copy_from_slice(&u64::try_from(offset)?.to_le_bytes());
    }
    expected.resize(fixture.data_offset + aligned(last_offset + 28), 0);
    expected[fixture.data_offset..fixture.data_offset + 4].copy_from_slice(&[1, 2, 3, 4]);
    // Independently known all-+1 wire bytes: five trits encode to 255,
    // four trits plus the unused zero tail encode to 253; scales are raw FP16.
    for index in 0..=fixture.large_blocks {
        let offset = if index == fixture.large_blocks {
            last_offset
        } else {
            32 + index * 28
        };
        let position = fixture.data_offset + offset;
        expected[position..position + 24].fill(255);
        expected[position + 24..position + 26].fill(253);
        expected[position + 27] = if index == fixture.large_blocks {
            0xbc
        } else {
            0x3c
        };
    }
    assert_eq!(converted, expected);
    assert_eq!(report["CodecCommit"], CODEC_COMMIT);
    assert_eq!(report["ConvertedBlocks"], 1929);
    assert_eq!(report["ConvertedTensorCount"], 2);
    assert_eq!(report["OutputBytes"], u64::try_from(converted.len())?);
    assert_eq!(report["OutputSha256"], sha(&converted));
    assert_eq!(report["WorkingBufferBytes"], 258_720);
    assert_eq!(report["ExactPq2ReconstructionVerified"], true);
    assert_eq!(report["NonPq2TensorBytesCopiedVerbatim"], true);
    let inspected = inspect(
        &output,
        PRISM_DIALECT,
        false,
        u64::try_from(converted.len())?,
        &sha(&converted),
    )?;
    assert_eq!(inspected["Scalars"]["general.file_type"], 143);
    assert_eq!(
        inspected["Scalars"]["prism.hadamard.transform"],
        "unchanged-transform"
    );
    assert_eq!(
        inspected["TokenizerMetadataSha256"],
        report["TokenizerMetadataSha256"]
    );
    Ok(())
}

#[test]
fn late_invalid_symbol_retains_partial_and_existing_destination_is_preserved() -> Result<()> {
    let scratch = scratch()?;
    let source = scratch.join("source.gguf");
    let output = scratch.join("export.gguf");
    let mut fixture = fixture(1, 4)?;
    fixture.bytes[fixture.data_offset + fixture.last_offset + 2] = 0xff;
    fs::write(&source, &fixture.bytes)?;
    assert!(convert(&source, &output, &fixture.bytes).is_err());
    assert!(!output.exists());
    let partials = fs::read_dir(&scratch)?
        .collect::<std::result::Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("export.gguf.partial-")
        })
        .collect::<Vec<_>>();
    assert_eq!(partials.len(), 1);
    assert!(partials[0].metadata()?.len() > u64::try_from(fixture.data_offset)?);
    fs::write(&output, b"existing output evidence")?;
    assert!(convert(&source, &output, &fixture.bytes).is_err());
    assert_eq!(fs::read(&output)?, b"existing output evidence");
    Ok(())
}

#[test]
fn signed_file_type_with_same_numeric_value_cannot_be_rewritten() -> Result<()> {
    let scratch = scratch()?;
    let source = scratch.join("source.gguf");
    let output = scratch.join("export.gguf");
    let fixture = fixture(1, 5)?; // INT32, whose JSON number also equals 141.
    fs::write(&source, &fixture.bytes)?;
    let error = convert(&source, &output, &fixture.bytes)
        .err()
        .ok_or("signed file type was accepted")?;
    assert!(error.to_string().contains("UINT32"));
    assert!(!output.exists());
    Ok(())
}

#[test]
fn census_counts_raw_symbols_across_batches_and_disqualifies_invalid_scales() -> Result<()> {
    let scratch = scratch()?;
    let source = scratch.join("census.gguf");
    let mut fixture = fixture(1928, 4)?;
    // One block has a zero scale and every possible raw code in its first byte.
    // It still contributes raw symbols; a zero scale must not recode them.
    let position = fixture.data_offset + 64;
    fixture.bytes[position..position + 2].fill(0);
    fixture.bytes[position + 2] = 0xe4; // low-first codes: 0, 1, 2, 3
    fs::write(&source, &fixture.bytes)?;
    let report = inspect(
        &source,
        PRISM_DIALECT,
        true,
        u64::try_from(fixture.bytes.len())?,
        &sha(&fixture.bytes),
    )?;
    let census = &report["Pq2Census"];
    assert_eq!(census["Blocks"], 1929);
    assert_eq!(census["SerializedBytesRead"], 1929 * 34);
    assert_eq!(census["UnchangedOtherTensorBytes"], 4);
    assert_eq!(census["CodeMinusOne"], 1);
    assert_eq!(census["CodeZero"], 1);
    assert_eq!(census["CodePlusOne"], 1929 * 128 - 3);
    assert_eq!(census["CodePlusTwo"], 1);
    assert_eq!(census["ZeroScaleBlocks"], 1);
    assert_eq!(census["NonfiniteScaleBlocks"], 0);
    assert_eq!(census["LosslessThreeSymbolEligible"], false);
    assert_eq!(census["Scales"], 1929);
    assert_eq!(census["ScaleBytes"], 1929 * 2);
    assert_eq!(census["PayloadBufferBytes"], 65_536);
    Ok(())
}
