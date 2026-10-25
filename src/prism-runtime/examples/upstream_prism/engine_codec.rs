//! Adapts supported production Rust codec operations without substitute kernels.

use std::sync::Arc;

use bonsai_prism_runtime::kernels::{packed_dot, quantize_row, row};
use bonsai_prism_runtime::model::Tensor;
use serde_json::{Value, json};

use super::{Result, invalid};

fn bytes(value: &Value) -> Result<Vec<u8>> {
    let text = value
        .as_str()
        .ok_or_else(|| invalid("Rust codec bytes must be text"))?;
    if text.len() > 2 * 1024 * 1024 || !text.len().is_multiple_of(2) {
        return Err(invalid("Rust codec byte extent exceeds bound"));
    }
    text.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            let nibble = |byte| match byte {
                b'0'..=b'9' => Ok(byte - b'0'),
                b'a'..=b'f' => Ok(byte - b'a' + 10),
                _ => Err(invalid("Rust codec requires lowercase hexadecimal")),
            };
            Ok(16 * nibble(pair[0])? + nibble(pair[1])?)
        })
        .collect()
}

fn byte_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8] = b"0123456789abcdef";
    bytes
        .iter()
        .flat_map(|byte| {
            [
                char::from(DIGITS[usize::from(byte >> 4)]),
                char::from(DIGITS[usize::from(byte & 15)]),
            ]
        })
        .collect()
}

fn hex(values: &[f32]) -> String {
    byte_hex(
        &values
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect::<Vec<_>>(),
    )
}

fn header(operation: &Value) -> Result<(u32, usize)> {
    let kind = u32::try_from(
        operation["TypeId"]
            .as_u64()
            .ok_or_else(|| invalid("Rust codec TypeId must be unsigned"))?,
    )?;
    let count = usize::try_from(
        operation["Count"]
            .as_u64()
            .ok_or_else(|| invalid("Rust codec Count must be unsigned"))?,
    )?;
    if count == 0 || count > 8192 {
        return Err(invalid("Rust codec Count outside bound"));
    }
    Ok((kind, count))
}

pub(super) fn execute(request: &Value) -> Result<Option<Value>> {
    if request["SchemaVersion"] != 1 {
        return Err(invalid("Rust codec request version mismatch"));
    }
    let operations = request["Operations"]
        .as_array()
        .ok_or_else(|| invalid("Rust codec Operations must be an array"))?;
    if operations.is_empty() || operations.len() > 128 {
        return Err(invalid("Rust codec operation count outside bound"));
    }
    if operations
        .iter()
        .any(|operation| matches!(operation["Kind"].as_str(), Some("codec-inventory")))
    {
        return Ok(None);
    }
    let results = operations
        .iter()
        .map(|operation| match operation["Kind"].as_str() {
            Some("codec-quantize") => {
                let (kind, count) = header(operation)?;
                if !matches!(operation["Mode"].as_str(), Some("cpu" | "reference")) {
                    return Err(invalid(
                        "Rust codec Count or quantization Mode outside bound",
                    ));
                }
                let raw = bytes(&operation["DataHex"])?;
                if count.checked_mul(4) != Some(raw.len()) {
                    return Err(invalid("Rust codec float input extent differs"));
                }
                let values = raw
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|bytes| f32::from_le_bytes(*bytes))
                    .collect::<Vec<_>>();
                Ok(match quantize_row(kind, &values) {
                    Ok(bytes) => json!({"DataHex":byte_hex(&bytes)}),
                    Err(error) => json!({"Error":error.to_string()}),
                })
            }
            Some("codec-vec-dot") => {
                let (kind, count) = header(operation)?;
                Ok(
                    match packed_dot(
                        kind,
                        count,
                        &bytes(&operation["WeightsHex"])?,
                        &bytes(&operation["ActivationsHex"])?,
                    ) {
                        Ok(value) => json!({"DataHex":hex(&[value])}),
                        Err(error) => json!({"Error":error.to_string()}),
                    },
                )
            }
            Some("codec-dequantize") => {
                let (kind, count) = header(operation)?;
                let tensor = Tensor {
                    name: "upstream-codec-row".to_owned(),
                    kind,
                    dimensions: vec![count],
                    data: Arc::from(bytes(&operation["DataHex"])?),
                };
                Ok(match row(&tensor, 0) {
                    Ok(values) => json!({"DataHex":hex(&values)}),
                    Err(error) => json!({"Error":error.to_string()}),
                })
            }
            _ => Err(invalid("unknown Rust codec operation")),
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Some(json!({"SchemaVersion":1,"Results":results})))
}
