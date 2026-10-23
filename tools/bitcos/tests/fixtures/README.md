# C++ reference fixture

`cpp-reference.bin` preserves results from the original standalone BITCOS C++
implementation at revision `d7bab354d0848215f0a9bb4d6699b218cc1e90f7`, before the
implementation was removed. It is a correctness fixture, not a timing result.

The temporary exporter ran on Windows ARM64 in strict floating-point Release
mode, with NEON and ARM dot-product instructions enabled. Before emitting each
row's results it required exact `float` bits to match across PQ2 scalar, PQ2
NEON, PTQ NEON with Q8_K inputs, compact scalar, and compact NEON. It also required
PTQ and compact decoding to reproduce the original PQ2 bytes.

The fixture contains 161 cases and 169 rows: random ternary rows at widths 2,
40, 48, and 136 blocks; every nonzero count from 0 through 128; and positive and
negative single-weight cases at bitmap, sign-byte, and activation boundaries.
Inputs include signed int8 extremes, negative scales, FP16 subnormals and extrema,
and finite zero and smallest-normal FP32 activation scales. Products also
exercise FP32 subnormals. A separate table records 26
FP16 conversion cases including zero, infinities, and NaN payloads.

Provenance SHA-256 values:

| Artifact | SHA-256 |
| --- | --- |
| Fixture (166,089 bytes) | `e49b6937fb0d40e3784c2e994740b280a7e4e9aa23f1ab7d4609a13a7a463431` |
| Original `tools/bitcos/bitcos.cpp` | `2258495bcde9358cfc63d7b30a3b1eaa5df8d9765cf415d037b47dd4ac890c3d` |
| Original `tools/bitcos/bitcos.h` | `c34fc0232cf406dd2f3c702de731020334228807f1a5e41b72440500a5bac253` |
| Temporary comparison exporter | `05960156c2b1bd839da5464b995c57591dcba93709d52086a661a1acb81f5420` |

All multibyte fields use little endian byte order. The format is:

1. Eight bytes `BTPAR001`, then a `u32` capability word (bit 0: NEON; bit 1:
   dot-product instructions).
2. A `u32` half-conversion count, then `(u16 FP16 input, u32 FP32 result bits)`
   records.
3. A `u32` case count, then cases containing `u32 rows`, `u32 blocks_per_row`,
   raw PQ2 blocks (34 bytes each), raw PTQ blocks (28 bytes each), activation
   blocks (`u32 scale bits` and 256 raw signed-int8 bytes), `rows + 1` compact
   `u32` offsets, a `u32` payload length and payload bytes, and five `u32` result
   bit patterns per row in the kernel order listed above.

PQ2 and PTQ blocks are in row-major order. Activation count is
`blocks_per_row / 2`; those activations are shared by the case's rows. Native
Q8_K sums are intentionally omitted because the kernels do not consume them.

The `cpp_reference_parity` test reads the committed fixture by default. Set
`BONSAI_BITCOS_PARITY_FILE` to an absolute path to compare a newly exported
fixture before accepting a replacement. The parser checks bounds, dimensions,
counts, capability flags, and end of input. Rust must match the source and
packed bytes, offsets, decoded rows, serialized size, half conversions, and
every recorded arithmetic result exactly.
