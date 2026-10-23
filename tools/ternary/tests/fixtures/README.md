# Native C++ codec reference

`ptq-reference.bin` preserves PQ2/PTQ bytes and binary16 conversions captured
from the native ARM64 C++ reference at revision
`d7bab354d0848215f0a9bb4d6699b218cc1e90f7`. It is correctness evidence.
The source codec contract is [Prism commit 73bfe6cc](https://github.com/PrismML-Eng/llama.cpp/tree/73bfe6cc485554822f80c473a3c0c4f25e80f41b).

The 161 cases contain 169 rows at widths 2, 40, 48 and 136 blocks, every nonzero
count from 0 through 128, and positive and negative single-weight cases at
encoding boundaries. Inputs include negative scales, FP16 subnormals and
extrema. A separate table records 26 binary16 conversion cases, including
signed zero, infinities and NaN payloads.

This is a byte-for-byte projection of the original 166,089-byte reference
fixture (SHA-256 `e49b6937fb0d40e3784c2e994740b280a7e4e9aa23f1ab7d4609a13a7a463431`)
retained in Git history. The projection preserves the original half-conversion
table, case order, dimensions and raw PQ2/PTQ blocks. It discards the capability
word, activation inputs, experimental storage bytes and arithmetic outputs;
none of those fields is part of the supported codec contract. The new file is
62,964 bytes with SHA-256
`de86d0ff0e31037635ae5f607bcb68d4e62ee7cb5d1b5db5e739ef900114fd79`.

The original capture used strict floating-point ARM64 MSVC Release mode. Before
emitting its PTQ bytes, it required native PTQ decoding to reproduce the original
PQ2 bytes. The temporary comparison exporter's SHA-256 was
`05960156c2b1bd839da5464b995c57591dcba93709d52086a661a1acb81f5420`.

All multibyte fields use little endian byte order. The projected format is:

1. Eight bytes `PTPAR001`.
2. A `u32` conversion count, then `(u16 FP16 input, u32 FP32 result bits)` records.
3. A `u32` case count, then cases containing `u32 rows`, `u32 blocks_per_row`,
   raw row-major PQ2 blocks (34 bytes each) and raw row-major PTQ blocks (28 bytes
   each).

The `cpp_reference_ptq_parity` test checks bounds, dimensions, counts, widths,
end of input, exact packed bytes, exact decoded bytes and half-conversion bits.