//! Exact parity against the C++ implementation captured before its removal.
//!
//! `BONSAI_BITCOS_PARITY_FILE` selects a live comparison fixture at an absolute
//! path. Otherwise the checked-in C++ reference is used. Every reader is bounded,
//! the format is little endian, and unexpected trailing bytes are rejected.
#![expect(
    clippy::unwrap_used,
    reason = "corrupt fixtures and failed parity assertions must fail visibly"
)]

use std::{fs, path::PathBuf};

use bonsai_bitcos::{
    Matrix, Pq2Block, PtqBlock, Q8KBlock, dot_pq2_scalar, dot_ptq_scalar_q8k, half_to_float,
    pack_ptq, unpack_ptq,
};

struct Reader<'a> {
    bytes: &'a [u8],
    cursor: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> &'a [u8] {
        let end = self.cursor.checked_add(count).unwrap();
        let result = self.bytes.get(self.cursor..end).unwrap();
        self.cursor = end;
        result
    }

    fn array<const N: usize>(&mut self) -> [u8; N] {
        self.take(N).try_into().unwrap()
    }

    fn u16(&mut self) -> u16 {
        u16::from_le_bytes(self.array())
    }

    fn u32(&mut self) -> u32 {
        u32::from_le_bytes(self.array())
    }

    fn count(&mut self) -> usize {
        usize::try_from(self.u32()).unwrap()
    }
}

struct Case {
    rows: usize,
    blocks: usize,
    pq2: Vec<Pq2Block>,
    ptq: Vec<PtqBlock>,
    activations: Vec<Q8KBlock>,
    offsets: Vec<u32>,
    payload: Vec<u8>,
    outputs: Vec<[u32; 5]>,
}

impl Case {
    fn read(reader: &mut Reader<'_>) -> Self {
        let rows = reader.count();
        let blocks = reader.count();
        assert!((1..=3).contains(&rows));
        assert!([2, 40, 48, 136].contains(&blocks));
        let count = rows.checked_mul(blocks).unwrap();
        let pq2 = (0..count).map(|_| reader.array()).collect();
        let ptq = (0..count).map(|_| reader.array()).collect();
        let activations = (0..blocks / 2)
            .map(|_| Q8KBlock {
                scale: f32::from_bits(reader.u32()),
                values: reader.array::<256>().map(|byte| i8::from_ne_bytes([byte])),
                // Deliberately differs from the C++ reference's unused sums.
                sums: [i16::MAX; 16],
            })
            .collect();
        let offsets = (0..=rows).map(|_| reader.u32()).collect();
        let payload_length = reader.count();
        let payload = reader.take(payload_length).to_vec();
        let outputs = (0..rows)
            .map(|_| std::array::from_fn(|_| reader.u32()))
            .collect();
        Self {
            rows,
            blocks,
            pq2,
            ptq,
            activations,
            offsets,
            payload,
            outputs,
        }
    }

    fn assert_parity(&self, index: usize) {
        let matrix = Matrix::pack(&self.pq2, self.rows, self.blocks).unwrap();
        assert_eq!(matrix.rows(), self.rows, "case {index}");
        assert_eq!(matrix.blocks_per_row(), self.blocks, "case {index}");
        assert_eq!(matrix.offsets(), self.offsets, "case {index}");
        assert_eq!(matrix.payload(), self.payload, "case {index}");
        assert_eq!(
            matrix.serialized_bytes(),
            u64::try_from(self.payload.len() + self.offsets.len() * 4).unwrap()
        );
        let loaded = Matrix::load(self.blocks, self.offsets.clone(), self.payload.clone()).unwrap();
        for (pq2, ptq) in self.pq2.iter().zip(&self.ptq) {
            assert_eq!(pack_ptq(pq2).unwrap(), *ptq, "case {index}");
            assert_eq!(unpack_ptq(ptq).unwrap(), *pq2, "case {index}");
        }
        for row in 0..self.rows {
            let start = row * self.blocks;
            let pq2 = &self.pq2[start..start + self.blocks];
            let ptq = &self.ptq[start..start + self.blocks];
            assert_eq!(
                matrix.unpack_row(row).unwrap(),
                pq2,
                "case {index}, row {row}"
            );
            assert_eq!(
                loaded.unpack_row(row).unwrap(),
                pq2,
                "case {index}, row {row}"
            );
            let actual = [
                dot_pq2_scalar(pq2, &self.activations).unwrap().to_bits(),
                dot_ptq_scalar_q8k(ptq, &self.activations)
                    .unwrap()
                    .to_bits(),
                matrix.dot_scalar(row, &self.activations).unwrap().to_bits(),
                loaded.dot_scalar(row, &self.activations).unwrap().to_bits(),
            ];
            for expected in self.outputs[row] {
                for bits in actual {
                    assert_eq!(bits, expected, "case {index}, row {row}");
                }
            }
        }
    }
}

#[test]
fn cpp_reference_parity() {
    let fixture = std::env::var_os("BONSAI_BITCOS_PARITY_FILE").map_or_else(
        || include_bytes!("fixtures/cpp-reference.bin").to_vec(),
        |path| {
            let path = PathBuf::from(path);
            assert!(
                path.is_absolute(),
                "BONSAI_BITCOS_PARITY_FILE must be an absolute fixture path"
            );
            fs::read(path).unwrap()
        },
    );
    let mut reader = Reader {
        bytes: &fixture,
        cursor: 0,
    };
    assert_eq!(reader.take(8), b"BTPAR001");
    let flags = reader.u32();
    assert_eq!(flags & !3, 0, "unknown C++ capability flags");
    assert!(flags & 2 == 0 || flags & 1 != 0, "dotprod requires NEON");
    let half_count = reader.count();
    assert_eq!(half_count, 26);
    for _ in 0..half_count {
        let input = reader.u16();
        let expected = reader.u32();
        assert_eq!(half_to_float(input).to_bits(), expected, "half {input:04x}");
    }
    let count = reader.count();
    assert_eq!(count, 161);
    let mut rows = 0;
    let mut widths = [false; 4];
    for index in 0..count {
        let case = Case::read(&mut reader);
        rows += case.rows;
        widths[[2, 40, 48, 136]
            .iter()
            .position(|width| *width == case.blocks)
            .unwrap()] = true;
        case.assert_parity(index);
    }
    assert_eq!(rows, 169);
    assert!(widths.into_iter().all(|present| present));
    assert_eq!(reader.cursor, fixture.len(), "unparsed C++ fixture bytes");
}
