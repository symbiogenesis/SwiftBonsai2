# Hexagon packed-weight audit

`pq2_bridge.py` is a bounded, model-free Python audit for the first Hexagon
projection experiment. It never loads an inference engine, opens a device or
rewrites a GGUF file. Native inference kernels and device ownership belong in
Prism; this tool prepares an independent layout control.

Run the inventory audit with Python 3.11 or later:

```powershell
python tools/hexagon/pq2_bridge.py
python -B -m unittest discover -s tools/hexagon -p 'test_*.py'
```

An optional raw PQ2 fixture can be checked with
`--payload ABSOLUTE_FIXTURE_PATH --columns 384 --rows 2`. The fixture must contain
exactly the declared complete rows, at most 1 MiB. This is a raw packed fixture,
not an entire model. JSON inventory inputs are individually capped at 1 MiB;
all reads enforce the limit even if the file grows. The tool prints JSON to
stdout and writes no converted weights.

Inventory JSON has a 64-container nesting limit and rejects duplicate names and
nonfinite JSON constants. Counts and derived storage extents must fit unsigned
64-bit values; tensor counts are positive and type IDs are exact integers.

The layout is pinned to the unchanged native oracle
[73bfe6cc](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/ggml/src/ggml-common.h)
and its [dequantizers](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/ggml/src/ggml-quants.c).
PQ2 uses 128 weights/34 bytes and Q2_0 uses 64 weights/18 bytes: two little-endian
FP16 scale bytes followed by adjacent low-to-high two-bit symbols. Codes 0, 1,
2 and 3 decode to -1, 0, +1 and +2. Split each 32-byte PQ2 symbol block in half
and duplicate its original scale bytes. There is no quantizer, scale conversion,
ternary-only restriction or symbol reorder. The inverse rejects unequal scale
bits, including +0 versus -0. Nonfinite scales and partial rows reject.

The retained Swift inventory has 209,920,000 PQ2 blocks. This bridge increases
packed weight storage from 7,137,280,000 to **7,557,120,000 bytes**, an additional
**419,840,000 bytes (5.88235%)**. The arithmetic-only artifact estimate is
7,626,008,928 bytes before changed tensor alignment. Retaining both full layouts
simultaneously costs both allocations; this estimate excludes staging, VTCM,
activation/state buffers and device mappings. A future tiling prototype should
bridge bounded tiles rather than persist a second whole model.

Tests use a hand-written split/order fixture, every possible packed byte,
the fourth symbol, positive/negative zero, subnormal and extreme finite scales,
unaligned views, odd 128-weight groups, awkward rows and malformed extents.
Decoded f32 comparison includes signed-zero bits. Inventory checks bind matching
artifact identities and reconcile histograms, geometry and raw symbol counts.
Metadata SHA-256 binds the retained summary, not the omitted actual sign arrays.

These checks establish weight-layout compatibility only. Ordinary Q2_0 Q8_0
activation quantization, accumulation order, Hadamard transforms, grouped SSM
basis, LoRA, complete logits/state and signed Windows kernels still need separate
qualification. In particular, renaming PQ2 to Q2_0 does not preserve the PQ2
Q8_K path. No performance, admission or backend-selection decision follows.
