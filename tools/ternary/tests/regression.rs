#![forbid(unsafe_code)]
//! Independent wire-format, numerical-layout, and malformed-input regressions.
#![expect(
    clippy::unwrap_used,
    reason = "test construction and successful-operation assertions must fail visibly"
)]

use bonsai_ternary::{Error, Pq2Block, Q8KBlock, half_to_float, pack_ptq, unpack_ptq};

fn zeros(scale: u16) -> Pq2Block {
    let mut block = [0x55; 34];
    block[..2].copy_from_slice(&scale.to_le_bytes());
    block
}

const fn set_code(block: &mut Pq2Block, index: usize, code: u8) {
    let shift = 2 * (index % 4);
    block[2 + index / 4] = (block[2 + index / 4] & !(3 << shift)) | (code << shift);
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
fn ptq_has_known_wire_bytes() {
    for (symbols, trit_byte, tail_byte) in [
        ([0; 32], 0, 0),
        ([0x55; 32], 128, 127),
        ([0xaa; 32], 255, 253),
    ] {
        let mut block = zeros(0xbc00);
        block[2..].copy_from_slice(&symbols);
        let packed = pack_ptq(&block).unwrap();
        assert_eq!(&packed[..24], &[trit_byte; 24]);
        assert_eq!(&packed[24..26], &[tail_byte; 2]);
        assert_eq!(&packed[26..], &[0, 0xbc]);
        assert_eq!(unpack_ptq(&packed).unwrap(), block);
    }
}

#[test]
fn every_symbol_position_preserves_ptq_round_trip() {
    for position in 0..128 {
        for code in 0..3 {
            let mut source = zeros(0x3c00);
            set_code(&mut source, position, code);
            assert_eq!(unpack_ptq(&pack_ptq(&source).unwrap()).unwrap(), source);
        }
    }
}

#[test]
fn every_density_preserves_ptq_round_trip() {
    for count in 0..=128 {
        let mut source = zeros(0x83ff);
        for index in 0..count {
            set_code(
                &mut source,
                (index * 37) % 128,
                if index % 2 == 0 { 0 } else { 2 },
            );
        }
        assert_eq!(unpack_ptq(&pack_ptq(&source).unwrap()).unwrap(), source);
    }
}

#[test]
fn every_pq2_byte_is_classified_by_the_ternary_symbol_rule() {
    for byte in 0..=255_u8 {
        let valid = (0..4).all(|index| (byte >> (2 * index)) & 3 != 3);
        for position in 2..34 {
            let mut source = zeros(0x3c00);
            source[position] = byte;
            if valid {
                assert_eq!(unpack_ptq(&pack_ptq(&source).unwrap()).unwrap(), source);
            } else {
                assert_eq!(pack_ptq(&source), Err(Error::NonTernarySymbol));
            }
        }
    }
}

#[test]
fn invalid_weight_scales_are_rejected() {
    let source = zeros(0x3c00);
    for invalid in [
        0, 0x8000, 0x7c00, 0xfc00, 0x7c01, 0xfc01, 0x7e00, 0xfe00, 0x7fff, 0xffff, 0x7e01,
    ] {
        assert_eq!(pack_ptq(&zeros(invalid)), Err(Error::InvalidWeightScale));
        let mut ptq = pack_ptq(&source).unwrap();
        ptq[26..].copy_from_slice(&invalid.to_le_bytes());
        assert_eq!(unpack_ptq(&ptq), Err(Error::InvalidWeightScale));
    }
}

#[test]
fn reviewed_finite_scales_preserve_exact_wire_bits() {
    for scale in [
        0x0001, 0x03ff, 0x0400, 0x3555, 0x3c00, 0x7bff, 0x8001, 0x83ff, 0x8400, 0xbc00, 0xfbff,
    ] {
        let mut source = zeros(scale);
        source[2] = 0x24;
        let packed = pack_ptq(&source).unwrap();
        assert_eq!(&packed[26..], &scale.to_le_bytes());
        assert_eq!(unpack_ptq(&packed).unwrap(), source);
    }
}

#[test]
fn noncanonical_ptq_bytes_are_rejected() {
    let mut negative = zeros(0x3c00);
    negative[2..].fill(0);
    let ptq = pack_ptq(&negative).unwrap();
    for index in 0..26 {
        // Byte 1 decodes to the same negative trits as canonical byte 0.
        let mut bad = ptq;
        bad[index] = 1;
        assert_eq!(unpack_ptq(&bad), Err(Error::NoncanonicalPtq));
    }
}

#[test]
fn every_ptq_byte_and_position_preserves_canonical_validation() {
    let mut source = zeros(0x3c00);
    source[2..].fill(0);
    let baseline = pack_ptq(&source).unwrap();
    // Enumerate each independent arithmetic oracle once. Canonical membership
    // depends on the lane width, so the same two sets cover every position.
    let canonical = [(243_u32, 256), (81, 768)].map(|(values, multiplier)| {
        let mut bytes = [false; 256];
        for value in 0..values {
            let byte = u8::try_from((value * multiplier).div_ceil(243)).unwrap();
            bytes[usize::from(byte)] = true;
        }
        bytes
    });
    for position in 0..26 {
        let canonical = &canonical[usize::from(position >= 24)];
        for byte in 0..=255_u8 {
            let mut packed = baseline;
            packed[position] = byte;
            if canonical[usize::from(byte)] {
                let decoded = unpack_ptq(&packed).unwrap();
                assert_eq!(
                    pack_ptq(&decoded).unwrap(),
                    packed,
                    "byte {byte}, position {position}"
                );
            } else {
                assert_eq!(
                    unpack_ptq(&packed),
                    Err(Error::NoncanonicalPtq),
                    "byte {byte}, position {position}"
                );
            }
        }
    }
}

#[test]
fn every_native_lane_word_has_independent_base_three_symbols() {
    let mut checked = 0;
    for lane in 0..26 {
        let (digits, words, first, stride) = match lane {
            0..16 => (5, 243_u32, lane, 16),
            16..24 => (5, 243, 80 + lane - 16, 8),
            _ => (4, 81, 120 + lane - 24, 2),
        };
        let mut canonical = [false; 256];
        for word in 0..words {
            let numerator = if digits == 4 { word * 3 } else { word };
            let stored = u8::try_from((numerator * 256).div_ceil(243)).unwrap();
            assert!(!canonical[usize::from(stored)], "lane {lane}, word {word}");
            canonical[usize::from(stored)] = true;
            let mut packed = [0; 28];
            packed[27] = 0x3c;
            packed[lane] = stored;
            let mut expected = zeros(0x3c00);
            expected[2..].fill(0);
            // The oracle uses independent integer base-three division and
            // pinned native lane positions, rather than production unpacking
            // or a pack/unpack round trip to derive the expected symbols.
            for trit in 0..digits {
                let digit = word / 3_u32.pow(u32::try_from(digits - 1 - trit).unwrap()) % 3;
                set_code(
                    &mut expected,
                    first + trit * stride,
                    u8::try_from(digit).unwrap(),
                );
            }
            assert_eq!(
                unpack_ptq(&packed).unwrap(),
                expected,
                "lane {lane}, word {word}"
            );
            assert_eq!(
                pack_ptq(&expected).unwrap(),
                packed,
                "lane {lane}, word {word}"
            );
            checked += 1;
        }
        for byte in 0..=255_u8 {
            if !canonical[usize::from(byte)] {
                let mut packed = [0; 28];
                packed[27] = 0x3c;
                packed[lane] = byte;
                assert_eq!(
                    unpack_ptq(&packed),
                    Err(Error::NoncanonicalPtq),
                    "lane {lane}, byte {byte}"
                );
            }
        }
    }
    assert_eq!(checked, 5994);
}
