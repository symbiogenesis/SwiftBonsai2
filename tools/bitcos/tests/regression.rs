//! Independent representation, arithmetic, and malformed-input regression checks.
#![expect(
    clippy::unwrap_used,
    reason = "test construction and successful-operation assertions must fail visibly"
)]

use bonsai_bitcos::{
    Matrix, Pq2Block, Q8KBlock, dot_pq2_scalar, dot_ptq_scalar_q8k, half_to_float, pack_ptq,
    unpack_ptq,
};

fn zeros(scale: u16) -> Pq2Block {
    let mut block = [0x55; 34];
    block[..2].copy_from_slice(&scale.to_le_bytes());
    block
}

const fn set_code(block: &mut Pq2Block, index: usize, code: u8) {
    let shift = 2 * (index % 4);
    block[2 + index / 4] = (block[2 + index / 4] & !(3 << shift)) | (code << shift);
}

fn activation(scale: f32) -> Q8KBlock {
    Q8KBlock {
        scale,
        values: std::array::from_fn(|i| i8::try_from(i16::try_from(i).unwrap() - 128).unwrap()),
        // These fields belong to Q8_K's ABI, but none of the kernels uses them.
        sums: [i16::MIN; 16],
    }
}

fn assert_dot_bits(source: &[Pq2Block], activations: &[Q8KBlock], expected: f32) {
    let trits = source
        .iter()
        .map(|block| pack_ptq(block).unwrap())
        .collect::<Vec<_>>();
    let matrix = Matrix::pack(source, 1, source.len()).unwrap();
    for actual in [
        dot_pq2_scalar(source, activations).unwrap(),
        dot_ptq_scalar_q8k(&trits, activations).unwrap(),
        matrix.dot_scalar(0, activations).unwrap(),
    ] {
        assert_eq!(actual.to_bits(), expected.to_bits());
    }
}

#[test]
fn half_edge_values_preserve_exact_bits() {
    for (half, expected) in [
        (0x0000, 0x0000_0000),
        (0x8000, 0x8000_0000),
        (0x0001, 0x3380_0000),
        (0x8001, 0xb380_0000),
        (0x03ff, 0x387f_c000),
        (0x83ff, 0xb87f_c000),
        (0x0400, 0x3880_0000),
        (0x3555, 0x3eaa_a000),
        (0x3c00, 0x3f80_0000),
        (0xbc00, 0xbf80_0000),
        (0x7bff, 0x477f_e000),
        (0xfbff, 0xc77f_e000),
        (0x7c00, 0x7f80_0000),
        (0xfc00, 0xff80_0000),
        (0x7c01, 0x7f80_2000),
        (0xfe00, 0xffc0_0000),
    ] {
        assert_eq!(half_to_float(half).to_bits(), expected);
    }
    assert_eq!(size_of::<Q8KBlock>(), 292);
    assert_eq!(std::mem::offset_of!(Q8KBlock, values), 4);
    assert_eq!(std::mem::offset_of!(Q8KBlock, sums), 260);
}

#[test]
fn compact_and_ptq_have_known_wire_bytes() {
    let negative = [0; 32];
    let positive = [0xaa; 32];
    for (symbols, trit_byte, tail_byte, sign_byte) in [
        (negative, 0, 0, 0),
        ([0x55; 32], 128, 127, 0),
        (positive, 255, 253, 255),
    ] {
        let mut block = zeros(0xbc00);
        block[2..].copy_from_slice(&symbols);
        let packed = pack_ptq(&block).unwrap();
        assert_eq!(&packed[..24], &[trit_byte; 24]);
        assert_eq!(&packed[24..26], &[tail_byte; 2]);
        assert_eq!(&packed[26..], &[0, 0xbc]);
        assert_eq!(unpack_ptq(&packed).unwrap(), block);
        let matrix = Matrix::pack(&[block, zeros(0x3c00)], 1, 2).unwrap();
        if symbols == [0x55; 32] {
            assert_eq!(matrix.offsets(), [0, 36]);
            assert_eq!(matrix.serialized_bytes(), 44);
            assert_eq!(&matrix.payload()[2..18], &[0; 16]);
        } else {
            assert_eq!(matrix.offsets(), [0, 52]);
            assert_eq!(&matrix.payload()[2..18], &[255; 16]);
            assert_eq!(&matrix.payload()[18..34], &[sign_byte; 16]);
        }
    }
}

#[test]
fn every_symbol_position_preserves_presence_and_sign_order() {
    for position in 0..128 {
        for code in 0..3 {
            let mut source = [zeros(0x3c00); 2];
            set_code(&mut source[0], position, code);
            let matrix = Matrix::pack(&source, 1, 2).unwrap();
            assert_eq!(matrix.unpack_row(0).unwrap(), source);
            assert_eq!(
                unpack_ptq(&pack_ptq(&source[0]).unwrap()).unwrap(),
                source[0]
            );
            let mut expected_prefix = [0; 18];
            expected_prefix[..2].copy_from_slice(&0x3c00_u16.to_le_bytes());
            if code != 1 {
                expected_prefix[2 + position / 8] = 1 << (position % 8);
                assert_eq!(matrix.payload()[18], u8::from(code == 2));
            }
            assert_eq!(&matrix.payload()[..18], &expected_prefix);
        }
    }
}

#[test]
fn every_density_has_exact_extent_and_round_trip() {
    for count in 0..=128 {
        let mut source = [zeros(0x83ff), zeros(0x7bff)];
        for index in 0..count {
            set_code(
                &mut source[0],
                (index * 37) % 128,
                if index % 2 == 0 { 0 } else { 2 },
            );
        }
        let matrix = Matrix::pack(&source, 1, 2).unwrap();
        assert_eq!(matrix.payload().len(), 36 + count.div_ceil(8));
        assert_eq!(
            matrix.serialized_bytes(),
            u64::try_from(matrix.payload().len() + 8).unwrap()
        );
        assert_eq!(matrix.unpack_row(0).unwrap(), source);
        assert_eq!(
            unpack_ptq(&pack_ptq(&source[0]).unwrap()).unwrap(),
            source[0]
        );
        let loaded = Matrix::load(2, matrix.offsets().to_vec(), matrix.payload().to_vec()).unwrap();
        assert_eq!(loaded.unpack_row(0).unwrap(), source);
    }
}

#[test]
fn every_pq2_byte_is_classified_by_the_ternary_symbol_rule() {
    for byte in 0..=255_u8 {
        let valid = (0..4).all(|index| (byte >> (2 * index)) & 3 != 3);
        for position in 2..34 {
            let mut source = [zeros(0x3c00); 2];
            source[0][position] = byte;
            if valid {
                assert_eq!(
                    Matrix::pack(&source, 1, 2).unwrap().unpack_row(0).unwrap(),
                    source
                );
                assert_eq!(
                    unpack_ptq(&pack_ptq(&source[0]).unwrap()).unwrap(),
                    source[0]
                );
            } else {
                assert!(Matrix::pack(&source, 1, 2).is_err());
                assert!(pack_ptq(&source[0]).is_err());
                assert!(dot_pq2_scalar(&source, &[activation(1.0)]).is_err());
            }
        }
    }
}

#[test]
fn every_activation_lane_and_signed_limit_has_independent_dot_result() {
    let a = activation(1.0);
    for position in 0..256 {
        for (code, sign) in [(0, -1.0), (2, 1.0)] {
            let mut source = [zeros(0x3c00); 2];
            set_code(&mut source[position / 128], position % 128, code);
            let expected = if a.values[position] == 0 {
                0.0
            } else {
                sign * f32::from(a.values[position])
            };
            assert_dot_bits(&source, std::slice::from_ref(&a), expected);
        }
    }
    for (bits, scale) in [
        (0x0001, f32::from_bits(0x3380_0000)),
        (0x83ff, f32::from_bits(0xb87f_c000)),
        (0xbc00, -1.0),
        (0x7bff, 65504.0),
        (0xfbff, -65504.0),
    ] {
        let mut source = [zeros(bits), zeros(0x3c00)];
        source[0][2..].fill(0); // 128 weights of -1.
        let mut a = activation(1.0);
        a.values.fill(i8::MIN);
        assert_dot_bits(&source, &[a], scale * 16384.0);
    }
}

#[test]
fn malformed_scales_symbols_dimensions_and_rows_are_rejected() {
    let source = [zeros(0x3c00); 2];
    let a = [activation(1.0)];
    let matrix = Matrix::pack(&source, 1, 2).unwrap();
    for invalid in [0, 0x8000, 0x7c00, 0xfc00, 0x7e01] {
        let bad = [zeros(invalid), source[1]];
        assert!(pack_ptq(&bad[0]).is_err());
        assert!(Matrix::pack(&bad, 1, 2).is_err());
        assert!(dot_pq2_scalar(&bad, &a).is_err());
        let mut ptq = pack_ptq(&source[0]).unwrap();
        ptq[26..].copy_from_slice(&invalid.to_le_bytes());
        assert!(unpack_ptq(&ptq).is_err());
        assert!(dot_ptq_scalar_q8k(&[ptq, pack_ptq(&source[1]).unwrap()], &a).is_err());
        let mut payload = matrix.payload().to_vec();
        payload[..2].copy_from_slice(&invalid.to_le_bytes());
        assert!(Matrix::load(2, matrix.offsets().to_vec(), payload).is_err());
    }
    for (rows, blocks) in [
        (0, 2),
        (1, 0),
        (1, 1),
        (2, 2),
        (usize::MAX, 2),
        (1, usize::MAX),
    ] {
        assert!(Matrix::pack(&source, rows, blocks).is_err());
    }
    for blocks in [0, 1, usize::MAX] {
        assert!(Matrix::load(blocks, vec![0, 36], matrix.payload().to_vec()).is_err());
    }
    assert!(matrix.unpack_row(1).is_err());
    assert!(matrix.dot_scalar(1, &a).is_err());
    assert!(matrix.dot_scalar(0, &[]).is_err());
    assert!(
        matrix
            .dot_scalar(0, &[activation(1.0), activation(1.0)])
            .is_err()
    );
    assert!(dot_pq2_scalar(&[], &[]).is_err());
    assert!(dot_pq2_scalar(&source[..1], &a).is_err());
    assert!(dot_pq2_scalar(&source, &[]).is_err());
    assert!(dot_ptq_scalar_q8k(&[], &[]).is_err());
}

#[test]
fn malformed_compact_extents_and_sign_padding_are_rejected() {
    let mut source = [zeros(0x3c00); 2];
    set_code(&mut source[0], 0, 2);
    let matrix = Matrix::pack(&source, 1, 2).unwrap();
    let payload = matrix.payload().to_vec();
    let end = u32::try_from(payload.len()).unwrap();
    for offsets in [
        vec![],
        vec![0],
        vec![1, end],
        vec![0, end - 1],
        vec![0, end + 1],
        vec![0, end, end - 1],
        vec![0, u32::MAX, end],
    ] {
        assert!(Matrix::load(2, offsets, payload.clone()).is_err());
    }
    for length in 0..payload.len() {
        assert!(
            Matrix::load(
                2,
                vec![0, u32::try_from(length).unwrap()],
                payload[..length].to_vec()
            )
            .is_err()
        );
    }
    let mut extra = payload.clone();
    extra.push(0);
    assert!(Matrix::load(2, vec![0, end + 1], extra).is_err());
    for bit in 1..8 {
        let mut bad = payload.clone();
        bad[18] |= 1 << bit;
        assert!(Matrix::load(2, vec![0, end], bad).is_err());
    }
    // A fully populated bitmap must provide all 16 sign bytes, even in row 0.
    let mut short_signs = vec![0; 36];
    short_signs[..2].copy_from_slice(&0x3c00_u16.to_le_bytes());
    short_signs[2..18].fill(255);
    assert!(Matrix::load(2, vec![0, 36], short_signs).is_err());
}

#[test]
fn noncanonical_ptq_and_nonfinite_activation_scales_are_rejected() {
    let source = [zeros(0x3c00); 2];
    let ptq = [pack_ptq(&source[0]).unwrap(); 2];
    for index in 0..26 {
        // Byte 1 decodes to the same zero trits as byte 0 and is noncanonical.
        let mut bad = ptq;
        bad[0][index] = 1;
        assert!(unpack_ptq(&bad[0]).is_err());
        assert!(dot_ptq_scalar_q8k(&bad, &[activation(1.0)]).is_err());
    }
    let matrix = Matrix::pack(&source, 1, 2).unwrap();
    for scale in [f32::INFINITY, f32::NEG_INFINITY, f32::NAN] {
        let a = [activation(scale)];
        assert!(dot_pq2_scalar(&source, &a).is_err());
        assert!(dot_ptq_scalar_q8k(&ptq, &a).is_err());
        assert!(matrix.dot_scalar(0, &a).is_err());
    }
    for scale in [0.0, -0.0] {
        assert_dot_bits(&source, &[activation(scale)], 0.0);
    }
}
