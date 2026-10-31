//! Model-free exact ABI comparisons with independently captured Philox draws.
//!
//! The frozen record format and original-source/compiler identities are in
//! `tests/fixtures/philox-v1.provenance.json`. Expected bits were captured before
//! retiring the standalone reference implementation; no compiler runs in tests.
#![forbid(unsafe_code)]

use std::error::Error;

use sha2::{Digest, Sha256};

const FIXTURE: &[u8] = include_bytes!("../../tests/fixtures/philox-v1.bin");
const RECORD_COUNT: usize = 7 * 9 * 5 + 4096;
const FIXTURE_SHA256: [u8; 32] = [
    0x66, 0x2a, 0xa6, 0x85, 0x0e, 0x57, 0x26, 0xb2, 0xe5, 0x17, 0x30, 0x02, 0xa0, 0x18, 0x2b, 0x9b,
    0x85, 0x48, 0x29, 0x38, 0x9b, 0x03, 0x5b, 0x79, 0x19, 0xc7, 0x18, 0xaa, 0xe1, 0x62, 0x1e, 0x59,
];

fn draw_cases() -> Vec<(i64, i64, u32)> {
    let mut cases = Vec::new();
    for seed in [0, 1, 42, -1, i64::MIN, i64::MAX, 0x1234_5678_9abc_def0] {
        for subsequence in [
            0,
            1,
            15,
            16,
            -1,
            i64::MIN,
            i64::MAX,
            0xffff_ffff,
            0x1_0000_0000,
        ] {
            for counter in [0, 1, 0x7fff_ffff, 0x8000_0000, u32::MAX] {
                cases.push((seed, subsequence, counter));
            }
        }
    }
    // Fixed, bounded integer corpus, independent of the generator under test.
    let mut state = 0x1234_5678_9abc_def0_u64;
    for _ in 0..4096 {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1);
        let seed = i64::from_le_bytes(state.to_le_bytes());
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1);
        let subsequence = i64::from_le_bytes(state.to_le_bytes());
        let [a, b, c, d, ..] = state.to_le_bytes();
        cases.push((seed, subsequence, u32::from_le_bytes([a, b, c, d])));
    }
    cases
}

#[test]
fn rust_philox_scalar_abi_matches_captured_bits() -> Result<(), Box<dyn Error>> {
    // Fixed framing and the independent original input schedule prevent silent
    // truncation, case reordering or replacement with a smaller passing corpus.
    let digest: [u8; 32] = Sha256::digest(FIXTURE).into();
    assert_eq!(digest, FIXTURE_SHA256);
    assert_eq!(FIXTURE.len(), 16 + RECORD_COUNT * 24);
    assert_eq!(&FIXTURE[..8], b"BPHXv1\0\0");
    assert_eq!(u32::from_le_bytes(FIXTURE[8..12].try_into()?), 1);
    assert_eq!(
        usize::try_from(u32::from_le_bytes(FIXTURE[12..16].try_into()?))?,
        RECORD_COUNT,
    );
    let cases = draw_cases();
    assert_eq!(cases.len(), RECORD_COUNT);
    let (records, remainder) = FIXTURE[16..].as_chunks::<24>();
    assert!(remainder.is_empty());
    assert_eq!(records.len(), RECORD_COUNT);
    for (index, (record, expected_case)) in records.iter().zip(cases).enumerate() {
        let seed = i64::from_le_bytes(record[..8].try_into()?);
        let subsequence = i64::from_le_bytes(record[8..16].try_into()?);
        let counter = u32::from_le_bytes(record[16..20].try_into()?);
        let expected = u32::from_le_bytes(record[20..].try_into()?);
        assert_eq!(
            (seed, subsequence, counter),
            expected_case,
            "Captured Philox input identity differs at record {index}.",
        );
        assert_eq!(
            super::bonsai_qt_philox_uniform(seed, subsequence, counter).to_bits(),
            expected,
            "Philox bits differ at record {index}: seed={seed}, subsequence={subsequence}, counter={counter}.",
        );
    }
    Ok(())
}
