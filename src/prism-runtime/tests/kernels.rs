//! Focused scalar kernel fixtures pinned to Prism revision
//! `73bfe6cc485554822f80c473a3c0c4f25e80f41b`.
//! Expected quantized results below follow the C reference integer sums,
//! independently of float row decoding, and deliberately differ from float dots.

#![allow(
    clippy::float_cmp,
    reason = "Fixtures require exact representable reference values."
)]

use std::sync::Arc;

use bonsai_prism_runtime::kernels::{
    hadamard, matvec, rms_norm, round_f16, row, softmax, validate_tensor,
};
use bonsai_prism_runtime::model::Tensor;
use bonsai_ternary::pack_ptq;

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

fn tensor(kind: u32, dimensions: Vec<usize>, data: Vec<u8>) -> Tensor {
    Tensor {
        name: "fixture".to_owned(),
        dimensions,
        kind,
        data: Arc::from(data),
    }
}

fn pq2(scale: u16, symbols: &[(usize, u8)]) -> [u8; 34] {
    let mut block = [0x55; 34];
    block[..2].copy_from_slice(&scale.to_le_bytes());
    for &(index, symbol) in symbols {
        let shift = 2 * (index % 4);
        block[2 + index / 4] &= !(3 << shift);
        block[2 + index / 4] |= symbol << shift;
    }
    block
}

#[test]
fn pq2_matvec_uses_q8k_ties_even_and_per_half_scales() -> TestResult {
    let first = pq2(0x3c00, &[(1, 2), (2, 2), (3, 2)]);
    let second = pq2(0x3800, &[(0, 0)]);
    let packed = [first.as_slice(), second.as_slice()].concat();
    let weights = tensor(142, vec![256, 1], packed);
    let mut input = vec![0.0; 256];
    input[0] = 127.0;
    input[1] = 0.5;
    input[2] = 1.5;
    input[3] = 2.5;
    input[128] = -1.5;
    // Equal magnitudes preserve the first maximum's sign.
    input[255] = -127.0;
    // Signed maximum +127 gives iscale=-1, d=-1. Native nearest_int
    // maps -.5,-1.5,-2.5 to 0,-2,-2; second half contributes +1.
    assert_eq!(matvec(&weights, &input)?, [5.0]);
    let decoded = row(&weights, 0)?;
    let float_dot: f32 = decoded.iter().zip(&input).map(|(&a, &b)| a * b).sum();
    assert_eq!(float_dot, 5.25);
    Ok(())
}

#[test]
fn pq2_native_symbol_three_zero_scale_and_zero_activations() -> TestResult {
    let first = pq2(0x3c00, &[(0, 3)]);
    let second = pq2(0, &[(0, 3)]);
    let weights = tensor(
        142,
        vec![256],
        [first.as_slice(), second.as_slice()].concat(),
    );
    assert_eq!(row(&weights, 0)?[0], 2.0);
    assert_eq!(row(&weights, 0)?[128], 0.0);
    let mut input = vec![0.0; 256];
    input[0] = -127.0;
    assert_eq!(matvec(&weights, &input)?, [-254.0]);
    assert_eq!(matvec(&weights, &[0.0; 256])?, [0.0]);
    Ok(())
}

#[test]
fn ptq1_matvec_uses_q8_0_round_away_and_32_wide_scales() -> TestResult {
    let block = pack_ptq(&pq2(0x4000, &[(1, 2), (2, 2), (33, 2)]))?;
    let weights = tensor(143, vec![128, 1], block.to_vec());
    let mut input = vec![0.0; 128];
    input[0] = 127.0;
    input[1] = 0.5;
    input[2] = 1.5;
    input[32] = 63.5;
    input[33] = 0.25;
    // Q8_0 rounds .5 and 1.5 away from zero. Blocks 0 and 1 have
    // scales 1 and .5. The PTQ1 weight scale is 2: 2*(1+2+.5)=7.
    assert_eq!(matvec(&weights, &input)?, [7.0]);
    let decoded = row(&weights, 0)?;
    let float_dot: f32 = decoded.iter().zip(&input).map(|(&a, &b)| a * b).sum();
    assert_eq!(float_dot, 4.5);
    Ok(())
}

#[test]
fn ptq1_activation_scales_are_rounded_to_fp16() -> TestResult {
    let block = pack_ptq(&pq2(0x3c00, &[(0, 2)]))?;
    let weights = tensor(143, vec![128], block.to_vec());
    let mut input = [0.0; 128];
    input[0] = 1.0;
    // Q8_0 stores half bits 0x2008 for 1/127, then multiplies by 127.
    assert_eq!(matvec(&weights, &input)?, [0.999_938_96]);
    Ok(())
}

#[test]
fn pq2_odd_group_rows_use_native_q8_0_fallback() -> TestResult {
    let first = pq2(0x4000, &[(1, 2), (2, 3), (33, 2)]);
    let mut input = [0.0; 128];
    input[0] = 127.0;
    input[1] = 0.5;
    input[2] = 1.5;
    input[32] = 63.5;
    input[33] = 0.25;
    let weights = tensor(142, vec![128], first.to_vec());
    // Native Q8_0 rounds .5 and 1.5 away from zero; second subblock
    // scale is .5. Native symbol3 is +2: 2*(1+2*2+.5)=11.
    assert_eq!(matvec(&weights, &input)?, [11.0]);
    let decoded = row(&weights, 0)?;
    assert_eq!(
        decoded.iter().zip(input).map(|(&a, b)| a * b).sum::<f32>(),
        7.5
    );

    let second = pq2(0x3800, &[(1, 2), (2, 3), (33, 2)]);
    let third = pq2(0x3c00, &[(1, 2), (2, 3), (33, 2)]);
    let weights = tensor(
        142,
        vec![384],
        [first.as_slice(), second.as_slice(), third.as_slice()].concat(),
    );
    let input = [input.as_slice(), input.as_slice(), input.as_slice()].concat();
    assert_eq!(matvec(&weights, &input)?, [19.25]);
    assert!(matvec(&weights, &input[..383]).is_err());
    Ok(())
}

#[test]
fn packed_rows_match_lossless_ptq1_codec() -> TestResult {
    let symbols: Vec<(usize, u8)> = (0..128)
        .map(|index| Ok((index, u8::try_from((index * 7 + index / 3) % 3)?)))
        .collect::<Result<_, std::num::TryFromIntError>>()?;
    let native = pq2(0x3555, &symbols);
    let packed = pack_ptq(&native)?;
    let source = tensor(142, vec![128], native.to_vec());
    let destination = tensor(143, vec![128], packed.to_vec());
    assert_eq!(row(&source, 0)?, row(&destination, 0)?);
    assert_eq!(matvec(&source, &[0.0; 128])?, [0.0]);
    assert!(row(&source, 1).is_err());
    Ok(())
}

#[test]
fn floating_rows_and_matvec_preserve_native_activation_precision() -> TestResult {
    let f32_tensor = tensor(
        0,
        vec![2, 2],
        [1.0_f32, 2.0, -1.0, 0.5]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect(),
    );
    assert_eq!(row(&f32_tensor, 1)?, [-1.0, 0.5]);
    assert_eq!(matvec(&f32_tensor, &[2.0, 3.0])?, [8.0, -0.5]);
    let half = tensor(
        1,
        vec![2],
        [0x3c00_u16, 0x4000]
            .into_iter()
            .flat_map(u16::to_le_bytes)
            .collect(),
    );
    assert_eq!(row(&half, 0)?, [1.0, 2.0]);
    assert_eq!(matvec(&half, &[1.000_1, 0.0])?, [1.0]);
    let brain = tensor(
        30,
        vec![2],
        [0x3f80_u16, 0xc000]
            .into_iter()
            .flat_map(u16::to_le_bytes)
            .collect(),
    );
    assert_eq!(row(&brain, 0)?, [1.0, -2.0]);
    assert_eq!(matvec(&brain, &[1.001, 0.0])?, [1.0]);
    Ok(())
}

#[test]
fn half_roundtrip_covers_ties_subnormals_and_signed_zero() {
    assert_eq!(round_f16(1.000_488_3), 1.0);
    assert_eq!(round_f16(1.001_464_8), 1.001_953_1);
    assert_eq!(
        round_f16(f32::from_bits(0x3380_0000)).to_bits(),
        0x3380_0000
    );
    assert_eq!(round_f16(f32::from_bits(0x3300_0000)).to_bits(), 0);
    assert_eq!(round_f16(-0.0).to_bits(), (-0.0_f32).to_bits());
    assert_eq!(round_f16(65_504.0), 65_504.0);
    assert_eq!(round_f16(65_520.0), f32::INFINITY);
    assert!(round_f16(f32::NAN).is_nan());
}

#[test]
fn malformed_packed_tensors_and_nonfinite_inputs_fail_closed() -> TestResult {
    assert!(row(&tensor(142, vec![128], vec![0; 33]), 0).is_err());
    assert!(row(&tensor(142, vec![127], vec![0; 34]), 0).is_err());
    assert!(row(&tensor(999, vec![1], vec![0]), 0).is_err());
    assert!(row(&tensor(0, vec![1], f32::NAN.to_le_bytes().to_vec()), 0).is_err());
    let block = pack_ptq(&pq2(0x3c00, &[]))?;
    let weights = tensor(143, vec![128], block.to_vec());
    assert!(matvec(&weights, &[0.0; 127]).is_err());
    assert!(matvec(&weights, &[f32::NAN; 128]).is_err());
    let mut corrupted = block;
    corrupted[24] = 255;
    assert!(row(&tensor(143, vec![128], corrupted.to_vec()), 0).is_err());
    Ok(())
}

#[test]
fn normalization_and_attention_mask_fixture() -> TestResult {
    assert_eq!(
        rms_norm(&[3.0, 4.0], &[1.0, 2.0], 0.0)?,
        [3.0 / 12.5_f32.sqrt(), 8.0 / 12.5_f32.sqrt()]
    );
    assert!(rms_norm(&[0.0], &[1.0], 0.0).is_err());
    let mut values = [0.0, f32::NEG_INFINITY, 0.0];
    softmax(&mut values)?;
    assert_eq!(values, [0.5, 0.0, 0.5]);
    assert!(softmax(&mut [f32::NEG_INFINITY]).is_err());
    Ok(())
}

#[test]
fn hadamard_and_nonexpanding_validation_fixture() -> TestResult {
    let mut values = [1.0, 2.0, 3.0, 4.0];
    hadamard(&mut values)?;
    assert_eq!(values, [5.0, -1.0, -2.0, 0.0]);
    hadamard(&mut values)?;
    assert_eq!(values, [1.0, 2.0, 3.0, 4.0]);
    assert!(hadamard(&mut []).is_err());
    assert!(hadamard(&mut [1.0; 3]).is_err());
    let valid = tensor(142, vec![128], pq2(0, &[(0, 3)]).to_vec());
    validate_tensor(&valid)?;
    assert!(validate_tensor(&tensor(30, vec![1], 0x7f80_u16.to_le_bytes().to_vec())).is_err());
    assert!(validate_tensor(&tensor(142, vec![128], vec![0; 33])).is_err());
    Ok(())
}
