//! Immutable native-production bytes for focused row-quantizer verification.
#![forbid(unsafe_code)]

use bonsai_prism_runtime::kernels::{packed_dot, quantize_row, row, validate_tensor};
use bonsai_prism_runtime::model::Tensor;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::sync::Arc;

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

fn unhex(value: &Value) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
    let value = value.as_str().ok_or("hex fixture must be text")?;
    if !value.len().is_multiple_of(2) {
        return Err("hex fixture length must be even".into());
    }
    value
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| Ok(u8::from_str_radix(std::str::from_utf8(pair)?, 16)?))
        .collect()
}

#[test]
fn row_quantizers_match_native_production_bytes_for_zeros_ties_and_lcg_inputs() -> TestResult {
    let expected_hash = unhex(&Value::String(
        "7b520199c64e045fb2b153df38fe488f3043557c04248727766bd2a6b8944d54".to_owned(),
    ))?;
    assert_eq!(
        Sha256::digest(include_bytes!("fixtures/row-quantizers.json")).as_slice(),
        expected_hash
    );
    let fixture: Value = serde_json::from_str(include_str!("fixtures/row-quantizers.json"))?;
    assert_eq!(fixture["SchemaVersion"], 1);
    assert_eq!(
        fixture["UpstreamCommit"],
        "b87ca7225456b06b0975b8239c66b5aecf07e37c"
    );
    let cases = fixture["Cases"]
        .as_array()
        .ok_or("quantizer Cases must be an array")?;
    assert_eq!(cases.len(), 28);
    for case in cases {
        let kind = u32::try_from(
            case["TypeId"]
                .as_u64()
                .ok_or("fixture type must be unsigned")?,
        )?;
        let input = unhex(&case["InputHex"])?;
        let values = input
            .as_chunks::<4>()
            .0
            .iter()
            .map(|bytes| f32::from_le_bytes(*bytes))
            .collect::<Vec<_>>();
        let output = quantize_row(kind, &values)?;
        assert_eq!(
            output,
            unhex(&case["OutputHex"])?,
            "type={kind}, case={}",
            case["Name"]
        );
        let tensor = Tensor {
            name: "native-quantizer-reference".to_owned(),
            kind,
            dimensions: vec![values.len()],
            data: Arc::from(output),
        };
        validate_tensor(&tensor)?;
        assert_eq!(row(&tensor, 0)?.len(), values.len());
    }
    Ok(())
}

#[test]
fn row_quantizers_reject_incomplete_nonfinite_and_unrepresentable_packed_scales() {
    for kind in [0, 1, 2, 8, 15, 30, 142, 143] {
        assert!(quantize_row(kind, &[]).is_err());
        assert!(quantize_row(kind, &[f32::NAN; 256]).is_err());
    }
    for kind in [2, 8, 15, 142, 143] {
        assert!(quantize_row(kind, &[1.0; 127]).is_err());
    }
    for kind in [1, 2, 8, 142, 143] {
        assert!(quantize_row(kind, &[f32::MAX; 256]).is_err());
    }
    assert!(packed_dot(15, 256, &[0; 292], &[0; 292]).is_err());
}
