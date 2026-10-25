//! Native PTQ byte-domain and already-quantized activation regressions.
#![forbid(unsafe_code)]
#![allow(
    clippy::float_cmp,
    reason = "Unit-scale trits and bounded integer dots are exact."
)]

use std::sync::Arc;

use bonsai_prism_runtime::kernels::{packed_dot, row, validate_tensor};
use bonsai_prism_runtime::model::Tensor;

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

fn reference(packed: &[u8; 28], index: usize) -> i16 {
    let (lane, digit) = match index {
        0..80 => (index % 16, index / 16),
        80..120 => (16 + (index - 80) % 8, (index - 80) / 8),
        _ => (24 + (index - 120) % 2, (index - 120) / 2),
    };
    let powers = [1_u8, 3, 9, 27, 81];
    let fraction = packed[lane].wrapping_mul(powers[digit]);
    i16::from((u16::from(fraction) * 3 / 256).to_le_bytes()[0]) - 1
}

#[test]
fn every_native_ptq_byte_decodes_and_dots_with_original_activation_bits() -> TestResult {
    let values: [i8; 128] = std::array::from_fn(|index| {
        i8::from_ne_bytes([index.to_le_bytes()[0].wrapping_mul(197).wrapping_add(13)])
    });
    let mut activations = Vec::new();
    for group in values.as_chunks::<32>().0 {
        activations.extend(0x3c00_u16.to_le_bytes());
        activations.extend(group.iter().flat_map(|value| value.to_le_bytes()));
    }
    for position in 0..26 {
        for byte in 0..=255 {
            let mut packed = [0; 28];
            packed[26..].copy_from_slice(&0x3c00_u16.to_le_bytes());
            packed[position] = byte;
            let expected: Vec<_> = (0..128)
                .map(|index| f32::from(reference(&packed, index)))
                .collect();
            let tensor = Tensor {
                name: "native-ptq".to_owned(),
                kind: 143,
                dimensions: vec![128],
                data: Arc::from(packed),
            };
            validate_tensor(&tensor)?;
            assert_eq!(row(&tensor, 0)?, expected, "lane={position} byte={byte}");
            let integer: i16 = (0..128)
                .map(|index| reference(&packed, index) * i16::from(values[index]))
                .sum();
            assert_eq!(
                packed_dot(143, 128, &packed, &activations)?,
                f32::from(integer)
            );
        }
    }
    Ok(())
}

#[test]
fn packed_dot_rejects_wrong_extents_nonfinite_scales_and_incomplete_blocks() {
    let mut weights = [0; 28];
    weights[26..].copy_from_slice(&0x3c00_u16.to_le_bytes());
    let activations = [0; 136];
    assert!(packed_dot(143, 128, &weights[..27], &activations).is_err());
    assert!(packed_dot(143, 128, &weights, &activations[..135]).is_err());
    assert!(packed_dot(143, 127, &weights, &activations).is_err());
    assert!(packed_dot(143, 0, &[], &[]).is_err());
    weights[26..].copy_from_slice(&0x7c00_u16.to_le_bytes());
    assert!(packed_dot(143, 128, &weights, &activations).is_err());
    weights[26..].copy_from_slice(&0x3c00_u16.to_le_bytes());
    let mut invalid = activations;
    invalid[..2].copy_from_slice(&0x7e00_u16.to_le_bytes());
    assert!(packed_dot(143, 128, &weights, &invalid).is_err());
}
