// Keep the private contract explicit if the parent is reorganized later.
#![allow(clippy::redundant_pub_crate)]

use crate::{PRISM_DIALECT, Result, hash_range};
use serde_json::{Map, Number, Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

const HEADER_LIMIT: u64 = 256 * 1024 * 1024;
const STRING_LIMIT: u64 = 8 * 1024 * 1024;
const ARRAY_LIMIT: u64 = 2 * 1024 * 1024;
const TOTAL_ELEMENT_LIMIT: u64 = 8 * 1024 * 1024;
const BUFFER_BYTES: usize = 65_536;
const BUFFER_BYTES_U64: u64 = 65_536;

fn bad(message: impl Into<String>) -> crate::Error {
    crate::bad(format!("GGUF: {}", message.into()))
}

#[derive(Debug)]
pub(crate) struct Tensor {
    pub name: String,
    pub type_id: u32,
    pub offset: u64,
    pub bytes: u64,
    pub elements: u64,
    pub type_position: u64,
}

pub(crate) struct Inspection {
    pub report: Value,
    pub tensors: Vec<Tensor>,
    pub file_type_position: Option<u64>,
}

#[derive(Clone, Copy)]
struct Kind {
    name: &'static str,
    block: u64,
    bytes: u64,
}

const fn kind(id: u32, prism: bool) -> Option<Kind> {
    let (name, block, bytes) = match id {
        0 => ("F32", 1, 4),
        1 => ("F16", 1, 2),
        2 => ("Q4_0", 32, 18),
        3 => ("Q4_1", 32, 20),
        6 => ("Q5_0", 32, 22),
        7 => ("Q5_1", 32, 24),
        8 => ("Q8_0", 32, 34),
        9 => ("Q8_1", 32, 36),
        10 => ("Q2_K", 256, 84),
        11 => ("Q3_K", 256, 110),
        12 => ("Q4_K", 256, 144),
        13 => ("Q5_K", 256, 176),
        14 => ("Q6_K", 256, 210),
        15 => ("Q8_K", 256, 292),
        16 => ("IQ2_XXS", 256, 66),
        17 => ("IQ2_XS", 256, 74),
        18 => ("IQ3_XXS", 256, 98),
        19 => ("IQ1_S", 256, 50),
        20 => ("IQ4_NL", 32, 18),
        21 => ("IQ3_S", 256, 110),
        22 => ("IQ2_S", 256, 82),
        23 => ("IQ4_XS", 256, 136),
        24 => ("I8", 1, 1),
        25 => ("I16", 1, 2),
        26 => ("I32", 1, 4),
        27 => ("I64", 1, 8),
        28 => ("F64", 1, 8),
        29 => ("IQ1_M", 256, 56),
        30 => ("BF16", 1, 2),
        34 => ("TQ1_0", 256, 54),
        35 => ("TQ2_0", 256, 66),
        39 => ("MXFP4", 32, 17),
        40 => ("NVFP4", 64, 36),
        41 => ("Q1_0", 128, 18),
        42 => ("Q2_0", 64, 18),
        142 if prism => ("PQ2_0", 128, 34),
        143 if prism => ("PTQ1_0", 128, 28),
        _ => return None,
    };
    Some(Kind { name, block, bytes })
}

fn overflow() -> crate::Error {
    bad("descriptor/count arithmetic overflow.")
}

fn add(a: u64, b: u64) -> Result<u64> {
    a.checked_add(b).ok_or_else(overflow)
}

fn mul(a: u64, b: u64) -> Result<u64> {
    a.checked_mul(b).ok_or_else(overflow)
}

const fn width(type_id: u32) -> Option<u64> {
    match type_id {
        0 | 1 | 7 => Some(1),
        2 | 3 => Some(2),
        4..=6 => Some(4),
        10..=12 => Some(8),
        _ => None,
    }
}

struct Parser<'a> {
    file: &'a mut File,
    file_bytes: u64,
    elements: u64,
    last_float: f64,
    buffer: Box<[u8]>,
}

impl Parser<'_> {
    fn position(&mut self) -> Result<u64> {
        Ok(self.file.stream_position()?)
    }

    fn need(&mut self, bytes: u64) -> Result<()> {
        let position = self.position()?;
        if bytes > self.file_bytes.saturating_sub(position)
            || bytes > HEADER_LIMIT.saturating_sub(position)
        {
            return Err(bad(
                "truncated or over-limit metadata/tensor descriptor region.",
            ));
        }
        Ok(())
    }

    fn read<const N: usize>(&mut self) -> Result<[u8; N]> {
        self.need(u64::try_from(N)?)?;
        let mut bytes = [0; N];
        self.file.read_exact(&mut bytes)?;
        Ok(bytes)
    }

    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.read()?))
    }

    fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.read()?))
    }

    fn text(&mut self, limit: u64) -> Result<String> {
        let size = self.u64()?;
        if size > limit {
            return Err(bad("name/string exceeds inspection limit."));
        }
        self.need(size)?;
        self.read_text(usize::try_from(size)?)
    }

    fn read_text(&mut self, size: usize) -> Result<String> {
        self.file.read_exact(&mut self.buffer[..size])?;
        Ok(std::str::from_utf8(&self.buffer[..size])?.to_owned())
    }

    fn skip(&mut self, bytes: u64) -> Result<()> {
        let position = add(self.position()?, bytes)?;
        self.file.seek(SeekFrom::Start(position))?;
        Ok(())
    }

    fn scalar(&mut self, type_id: u32) -> Result<Value> {
        Ok(match type_id {
            0 => json!(self.read::<1>()?[0]),
            1 => json!(i8::from_le_bytes(self.read()?)),
            2 => json!(u16::from_le_bytes(self.read()?)),
            3 => json!(i16::from_le_bytes(self.read()?)),
            4 => json!(self.u32()?),
            5 => json!(i32::from_le_bytes(self.read()?)),
            6 => {
                let value = f32::from_le_bytes(self.read()?);
                if !value.is_finite() {
                    return Err(bad("non-finite float metadata."));
                }
                self.last_float = f64::from(value);
                Value::Number(value.to_string().parse()?)
            }
            7 => match self.read::<1>()?[0] {
                0 => Value::Bool(false),
                1 => Value::Bool(true),
                _ => return Err(bad("invalid boolean.")),
            },
            10 => json!(self.u64()?),
            11 => json!(i64::from_le_bytes(self.read()?)),
            12 => {
                let value = f64::from_le_bytes(self.read()?);
                self.last_float = value;
                Value::Number(
                    Number::from_f64(value).ok_or_else(|| bad("non-finite float metadata."))?,
                )
            }
            _ => return Err(bad("unknown scalar type.")),
        })
    }

    fn booleans(&mut self, mut count: u64) -> Result<()> {
        self.need(count)?;
        while count > 0 {
            let size = usize::try_from(count.min(BUFFER_BYTES_U64))?;
            self.file.read_exact(&mut self.buffer[..size])?;
            if self.buffer[..size].iter().any(|value| *value > 1) {
                return Err(bad("invalid boolean."));
            }
            count -= u64::try_from(size)?;
        }
        Ok(())
    }

    fn value(&mut self, type_id: u32, depth: u32, retain: bool) -> Result<Option<Value>> {
        if depth > 4 {
            return Err(bad("array nesting exceeds inspection limit."));
        }
        if type_id == 8 {
            let size = self.u64()?;
            if size > STRING_LIMIT {
                return Err(bad("string exceeds 8 MiB inspection limit."));
            }
            self.need(size)?;
            if retain && size <= BUFFER_BYTES_U64 {
                return Ok(Some(Value::String(self.read_text(usize::try_from(size)?)?)));
            }
            self.skip(size)?;
            return Ok(None);
        }
        if type_id != 9 {
            return Ok(Some(self.scalar(type_id)?));
        }
        let item_type = self.u32()?;
        if item_type > 12 {
            return Err(bad("unknown array element type."));
        }
        let count = self.u64()?;
        if count > ARRAY_LIMIT || count > TOTAL_ELEMENT_LIMIT - self.elements {
            return Err(bad("array element limit exceeded."));
        }
        self.elements += count;
        let keep = retain && count <= 4096 && item_type != 8 && item_type != 9;
        let descriptor = || retain.then(|| json!({"ElementTypeId": item_type, "Count": count}));
        if let Some(width) = width(item_type)
            && !keep
            && item_type != 7
        {
            let bytes = mul(count, width)?;
            self.need(bytes)?;
            self.skip(bytes)?;
            return Ok(descriptor());
        }
        if !keep && item_type == 7 {
            if count > 0 && depth == 4 {
                return Err(bad("array nesting exceeds inspection limit."));
            }
            self.booleans(count)?;
            return Ok(descriptor());
        }
        let mut values = if keep {
            Some(Vec::with_capacity(usize::try_from(count)?))
        } else {
            None
        };
        for _ in 0..count {
            let value = self.value(item_type, depth + 1, keep)?;
            if let Some(values) = &mut values {
                values.push(value.ok_or_else(|| bad("retained scalar cannot be null."))?);
            }
        }
        Ok(values.map(Value::Array).or_else(descriptor))
    }

    fn hash_record(&mut self, hash: &mut Sha256, start: u64, length: u64) -> Result<()> {
        let restore = self.position()?;
        self.file.seek(SeekFrom::Start(start))?;
        let result = (|| -> Result<()> {
            let mut remaining = length;
            while remaining > 0 {
                let size = usize::try_from(remaining.min(BUFFER_BYTES_U64))?;
                self.file.read_exact(&mut self.buffer[..size])?;
                hash.update(&self.buffer[..size]);
                remaining -= u64::try_from(size)?;
            }
            Ok(())
        })();
        self.file.seek(SeekFrom::Start(restore))?;
        result
    }
}

// Match Convert.ToUInt64 for the architecture's count metadata, including its
// accepted booleans, numeric strings and rounded finite floating point values.
fn float_count(value: f64) -> Result<u64> {
    let rounded = value.round_ties_even();
    if rounded == 0.0 {
        return Ok(0);
    }
    if !(0.0..18_446_744_073_709_551_616.0).contains(&rounded) {
        return Err(overflow());
    }
    Ok(format!("{rounded:.0}").parse()?)
}

fn count_value(value: Option<&Value>, original_float: Option<&f64>) -> Result<u64> {
    if let Some(value) = original_float {
        return float_count(*value);
    }
    let Some(value) = value else { return Ok(0) };
    match value {
        Value::Bool(value) => Ok(u64::from(*value)),
        Value::String(value) => {
            let value = value.trim_matches([' ', '\t', '\r', '\n', '\x0b', '\x0c']);
            if value
                .strip_prefix('-')
                .is_some_and(|digits| !digits.is_empty() && digits.bytes().all(|byte| byte == b'0'))
            {
                return Ok(0);
            }
            value.parse().map_err(Into::into)
        }
        Value::Number(value) => {
            if let Some(value) = value.as_u64() {
                return Ok(value);
            }
            if value.is_i64() {
                return Err(bad("architecture count cannot be negative."));
            }
            let value = value
                .as_f64()
                .ok_or_else(|| bad("invalid architecture count."))?;
            float_count(value)
        }
        _ => Err(bad("invalid architecture count.")),
    }
}

// Keep validation and report assembly adjacent in serialized file order.
#[allow(clippy::too_many_lines)]
pub(crate) fn inspect(file: &mut File, path: &Path, dialect: &str) -> Result<Inspection> {
    if dialect != "Stock" && dialect != PRISM_DIALECT {
        return Err(bad(
            "unknown dialect; use Stock or the explicitly pinned Prism dialect.",
        ));
    }
    file.seek(SeekFrom::Start(0))?;
    let file_bytes = file.metadata()?.len();
    let mut parser = Parser {
        file,
        file_bytes,
        elements: 0,
        last_float: 0.0,
        buffer: vec![0; BUFFER_BYTES].into_boxed_slice(),
    };
    if parser.u32()? != 0x4655_4747 {
        return Err(bad("invalid magic."));
    }
    if parser.u32()? != 3 {
        return Err(bad("only little-endian version 3 is supported."));
    }
    let tensor_count = parser.u64()?;
    let metadata_count = parser.u64()?;
    if tensor_count > 100_000 || metadata_count > 10_000 {
        return Err(bad("tensor/KV count exceeds inspection limit."));
    }
    let mut keys = HashSet::new();
    let mut scalars = Map::new();
    let mut float_scalars = BTreeMap::new();
    let mut arrays = Map::new();
    let mut templates = Map::new();
    let mut special = Map::new();
    let mut alignment = 32u32;
    let mut file_type_position = None;
    let mut tokenizer_hash = Sha256::new();
    let mut tokenizer_bytes = 0;
    let mut tokenizer_records = 0u64;
    for _ in 0..metadata_count {
        let start = parser.position()?;
        let key = parser.text(1024)?;
        if key.is_empty() || !keys.insert(key.clone()) {
            return Err(bad("empty or duplicate metadata key."));
        }
        let type_id = parser.u32()?;
        if key == "general.file_type" {
            file_type_position = Some(parser.position()?);
        }
        let template =
            key == "tokenizer.chat_template" || key.starts_with("tokenizer.chat_template.");
        if template && type_id == 8 {
            let value_start = parser.position()?;
            let size = parser.u64()?;
            if size > STRING_LIMIT {
                return Err(bad("template exceeds inspection limit."));
            }
            parser.need(size)?;
            parser.skip(size)?;
            let digest = hash_range(parser.file, add(value_start, 8)?, size)?;
            templates.insert(key.clone(), json!({"Utf8Bytes": size, "Sha256": digest}));
        } else {
            let value = parser.value(type_id, 0, true)?;
            if matches!(type_id, 6 | 12) {
                // Count conversion must use the original binary value, while
                // JSON preserves the source type's shortest decimal form.
                float_scalars.insert(key.clone(), parser.last_float);
            }
            if type_id == 9 {
                arrays.insert(
                    key.clone(),
                    value
                        .clone()
                        .ok_or_else(|| bad("retained array descriptor is required."))?,
                );
            } else if let Some(value) = &value {
                scalars.insert(key.clone(), value.clone());
            }
            if key.starts_with("tokenizer.") && key.ends_with("_token_id") {
                if !matches!(type_id, 4 | 5 | 10 | 11) {
                    return Err(bad("special token ID must be an integer."));
                }
                special.insert(
                    key.clone(),
                    value
                        .clone()
                        .ok_or_else(|| bad("special token ID is required."))?,
                );
            }
            if key == "general.alignment" {
                if type_id != 4 {
                    return Err(bad("general.alignment must be UINT32."));
                }
                alignment = u32::try_from(
                    value
                        .and_then(|value| value.as_u64())
                        .ok_or_else(|| bad("general.alignment must be UINT32."))?,
                )?;
                if alignment == 0 || !alignment.is_power_of_two() || alignment > 1_048_576 {
                    return Err(bad("invalid/over-limit alignment."));
                }
            }
        }
        if key.starts_with("tokenizer.") {
            let bytes = parser.position()? - start;
            parser.hash_record(&mut tokenizer_hash, start, bytes)?;
            tokenizer_bytes = add(tokenizer_bytes, bytes)?;
            tokenizer_records += 1;
        }
    }
    let metadata_end = parser.position()?;
    let architecture = scalars
        .get("general.architecture")
        .and_then(Value::as_str)
        .filter(|arch| !arch.is_empty())
        .ok_or_else(|| bad("general.architecture string is required."))?
        .to_owned();
    let mut names = HashSet::new();
    let mut tensors = Vec::with_capacity(usize::try_from(tensor_count)?);
    for _ in 0..tensor_count {
        let name = parser.text(1024)?;
        if name.is_empty() || !names.insert(name.clone()) {
            return Err(bad("empty or duplicate tensor name."));
        }
        let dimensions = parser.u32()?;
        if !(1..=4).contains(&dimensions) {
            return Err(bad("invalid tensor dimension count."));
        }
        let mut elements = 1;
        let mut first = 0;
        for dimension in 0..dimensions {
            let size = parser.u64()?;
            if size == 0 || i64::try_from(size).is_err() {
                return Err(bad("invalid tensor dimension."));
            }
            if dimension == 0 {
                first = size;
            }
            elements = mul(elements, size)?;
        }
        let type_position = parser.position()?;
        let type_id = parser.u32()?;
        let kind = kind(type_id, dialect == PRISM_DIALECT)
            .ok_or_else(|| bad(format!("unknown/unsupported tensor type ID {type_id}.")))?;
        if first % kind.block != 0 {
            return Err(bad(
                "tensor row is not divisible by quantization block size.",
            ));
        }
        let bytes = mul(elements / kind.block, kind.bytes)?;
        let offset = parser.u64()?;
        if offset % u64::from(alignment) != 0 {
            return Err(bad("unaligned tensor offset."));
        }
        tensors.push(Tensor {
            name,
            type_id,
            offset,
            bytes,
            elements,
            type_position,
        });
    }
    let descriptors_end = parser.position()?;
    let data_offset = if tensor_count > 0 {
        mul(
            add(descriptors_end, u64::from(alignment) - 1)? / u64::from(alignment),
            u64::from(alignment),
        )?
    } else {
        descriptors_end
    };
    if data_offset > file_bytes {
        return Err(bad("tensor data start exceeds file length."));
    }
    let mut ordered: Vec<_> = tensors.iter().collect();
    ordered.sort_by_key(|tensor| tensor.offset);
    let mut previous_end = 0;
    for tensor in ordered {
        let end = add(tensor.offset, tensor.bytes)?;
        if end > file_bytes - data_offset || tensor.offset < previous_end {
            return Err(bad("tensor payload extent is truncated or overlapping."));
        }
        previous_end = end;
    }
    let mut groups: BTreeMap<u32, (u64, u64, u64)> = BTreeMap::new();
    for tensor in &tensors {
        let group = groups.entry(tensor.type_id).or_default();
        group.0 += 1;
        group.1 = add(group.1, tensor.bytes)?;
        group.2 = add(group.2, tensor.elements)?;
    }
    let mut histogram = Vec::with_capacity(groups.len());
    for (type_id, (count, bytes, elements)) in groups {
        let kind =
            kind(type_id, dialect == PRISM_DIALECT).ok_or_else(|| bad("unknown tensor kind."))?;
        histogram.push(json!({"TypeId": type_id, "Name": kind.name, "Count": count, "Bytes": bytes, "Elements": elements}));
    }
    let block_key = format!("{architecture}.block_count");
    let nextn_key = format!("{architecture}.nextn_predict_layers");
    let blocks = count_value(scalars.get(&block_key), float_scalars.get(&block_key))?;
    let nextn = count_value(scalars.get(&nextn_key), float_scalars.get(&nextn_key))?;
    if nextn > blocks {
        return Err(bad("MTP block count exceeds block count."));
    }
    let mtp: Vec<_> = tensors
        .iter()
        .filter(|tensor| {
            if tensor.name.starts_with("nextn.") || tensor.name.contains(".nextn.") {
                return true;
            }
            if nextn > 0
                && let Some(rest) = tensor.name.strip_prefix("blk.")
                && let Some((block, _)) = rest.split_once('.')
                && !block.is_empty()
                && block.bytes().all(|byte| byte.is_ascii_digit())
                && let Ok(block) = block.parse::<u64>()
            {
                return block >= blocks - nextn && block < blocks;
            }
            false
        })
        .map(|tensor| tensor.name.clone())
        .collect();
    let report = json!({
        "SchemaVersion": 1, "Format": "GGUF", "Version": 3, "ByteOrder": "little", "ModelPath": path.to_string_lossy(),
        "FileBytes": file_bytes, "Architecture": architecture, "Alignment": alignment, "MetadataCount": metadata_count,
        "MetadataEndOffset": metadata_end, "TensorDescriptorsEndOffset": descriptors_end, "TensorDataOffset": data_offset,
        "TensorCount": tensor_count, "TensorTypeHistogram": histogram, "Scalars": scalars, "Arrays": arrays,
        "SpecialTokenIds": special, "EmbeddedTemplates": templates, "MtpTensorCount": mtp.len(), "MtpTensorNames": mtp,
        "TokenizerMetadataSha256": format!("{:x}", tokenizer_hash.finalize()), "TokenizerMetadataBytes": tokenizer_bytes,
        "TokenizerMetadataRecords": tokenizer_records,
        "TokenizerDigestDefinition": "SHA-256 of complete raw serialized tokenizer.* KV records concatenated in file order, including chat templates",
        "InspectionLimits": {"HeaderBytes": HEADER_LIMIT, "StringBytes": STRING_LIMIT, "ArrayElements": ARRAY_LIMIT,
            "TotalArrayElements": TOTAL_ELEMENT_LIMIT, "ArrayNesting": 4, "MetadataRecords": 10_000, "Tensors": 100_000},
        "TensorPayloadRead": false, "Dialect": dialect
    });
    Ok(Inspection {
        report,
        tensors,
        file_type_position,
    })
}
