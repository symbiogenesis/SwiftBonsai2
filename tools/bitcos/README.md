# Experimental ARM64 compressed-weight dot kernels

This standalone prototype consumes compact ternary weights directly into NEON
integer dot products. It does not change a model, register a ggml type, load a
GGUF, or enable an inference profile. No training, pruning, requantization or
activation quantization occurs. Production integration and performance acceptance
remain pending.

The source contract is [Prism commit 73bfe6cc](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/ggml/src/ggml-cpu/arch/arm/quants.c),
particularly `ggml_vec_dot_pq2_0_q8_K`, and its
[block definitions](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/ggml/src/ggml-common.h).
The [BITCOS paper](https://arxiv.org/html/2609.16338v1) motivates presence plus
compacted signs; this prototype's explicit block/row layout is its own experimental
encoding. The paper's Intel results do not predict this ARM implementation.
The upstream MIT notice is retained in [PRISM-NOTICE.txt](PRISM-NOTICE.txt).

## Representation and accounting

One independently indexed tile is one contiguous GGUF tensor row. Its column
count is a positive multiple of 256, satisfying the native Q8_K dot contract.
Each row streams its original 128-weight groups in order. Each group contains:

* Two unchanged little-endian FP16 scale bytes.
* Sixteen presence bytes: logical position `i` is bit `i % 8` of byte `i / 8`.
* `ceil(nonzero / 8)` sign bytes, in ascending present-position order; low bit
  first, zero means original code 0 (negative) and one means original code 2
  (positive). Absent positions restore original code 1. Unused high bits are zero.

Codes are relative to the original signed scale. Finite negative and subnormal
scales retain their exact bits. The conservative experimental eligibility rule
rejects raw code 3 (+2), zero scales and nonfinite scales. No normalization of scale
or symbols occurs. `unpack_row` is a correctness helper; the dot path never invokes it.

A matrix has a uint32 offset per row plus one sentinel. Offsets address its
single variable-length payload, with no per-group offset and no alignment padding.
The maximum payload is limited to uint32 addressing; the current Swift tensors
fit this restriction. `Matrix::load` validates every extent, prefix, scale, sign
length, canonical high padding and exact row termination before any kernel call.
No self-describing on-disk container or endian-converting index serializer is supplied.

For `R` rows and groups with nonzero counts `n[g]`, serialized bytes are exactly:

```text
4 * (R + 1) + sum[g](18 + ceil(n[g] / 8))
```

The actual Swift census gives 6,089,450,678 group-payload bytes before row indexes.
If every PQ2 row has at least 40 groups (5120 columns), total row-index overhead
is at most `4 * (209920000 / 40 + 402) = 20,993,608` bytes. That would retain at
least 1,026,835,714 bytes (0.9563 GiB) of the census's 0.9759 GiB block-rounded
saving. This conditional accounting is not a model conversion or measured RAM
reduction. Tensor shape metadata, a container and allocator bookkeeping remain
outside these numbers. `serialized_bytes()` measures payload plus index lengths;
it does not attest allocator overhead or caller-retained source data. Packing
uses two passes and allocates the compact payload at its final size, avoiding a
persistent PQ2-sized reserve. Correctness/benchmark construction retains its input
as a comparison reference; a future loader must release or unmap it separately.

## Fused computation and comparison scope

The BITCOS path popcounts a 128-bit bitmap once, then reconstructs sixteen int8
lanes at a time using a 1 KiB constant nibble lookup table. Sign reads are guarded;
the final block needs no readable padding/sentinel. The sixteen live lanes feed
NEON widening multiply/reduction, or ARM dot-product instructions when explicitly
enabled. There is no persistent PQ2, int8 or F32 expanded weight buffer.

Each integer dot covers 128 weights and uses the matching half of the original
256-element Q8_K block. It applies `(FP16_weight_scale * F32_activation_scale) *
integer_sum` once per weight group and accumulates groups in source order, matching
the pinned PQ2 algorithm for `nrc = 1`. The activation struct retains native Q8_K
layout (292 bytes); its `sums` field is unused by this symmetric ternary dot.
Exact float-bit correctness is checked with strict floating-point compilation.
Other `nrc`, tiled matrix-multiply/prefill paths, checkpointing, logits and model
behavior are outside this prototype.

The PQ2 NEON comparator uses the pinned algorithm's byte replication, table
shuffle, `{0,-2,-4,-6}` shifts and integer dot reduction, rather than a deliberately
slow scalar baseline. A separate scalar implementation supplies an oracle.
The PTQ comparator preserves the same updated symbols/scales in native group-128
base-3 packing and adapts native trit decoding to the **same Q8_K activations**.
It is labeled `ptq-neon-adapted-q8k`. Prism's released PTQ kernel uses Q8_0
activations; these timings cannot qualify that kernel or transfer its output
quality. At this Swift census's zero fraction, PTQ's fixed 28 bytes/group is
smaller than BITCOS before row indexes. The benchmark permits a fair packing
comparison with identical activations, not a blanket preference for BITCOS.

## Build and focused correctness

Use the build-only wrapper from native ARM64 PowerShell with installed
CMake/MSVC. It chooses dot-product instructions only when .NET reports support,
limits compiler workers to two and keeps outputs/logs in a fresh system-temp lane:

```powershell
$receipt = ./scripts/Build-BitcosPrototype.ps1 -CMakePath 'C:\path\to\cmake.exe'
$receipt
```

`BITCOS_DOTPROD=ON` requires a host that implements the ARM dot-product extension
(including the target Snapdragon X Elite). Keep it off for the widening NEON
fallback. There is no dynamic ISA dispatch. x64 builds support scalar correctness
only and refuse the representative synthetic benchmark. Clang/GCC ARM builds are
also supported by CMake, but are not certified by the local MSVC build.

Run a focused case locally with the native process working directory set to the
external build directory. Available cases are `codec`, `dot`, and `matrix`.
`bitcos-tests all` / CTest run the complete native correctness collection in CI.
The focused `dot` case compares scalar/NEON/compact results across 256, 5120, 6144
and 17408 columns, signed/subnormal/extreme scales and the full int8 activation
range. Other cases check bit positions, sign-byte boundaries, exact round trips,
index accounting and malformed input rejection.

## CI-only synthetic benchmark

`bitcos-bench --ci-synthetic` refuses execution unless `GITHUB_ACTIONS=true`, the
explicit flag is present and the executable contains the ARM NEON path. Never
set this variable locally to bypass repository policy. No other benchmark mode
exists. Building this target locally is permitted; benchmark execution is CI-only.

The bounded synthetic workload uses 5120-column rows with a deterministic
approximately 32.76% zero fraction, identical quantized activations and unchanged
scales for all kernels. Mandatory untimed parity precedes three rotated paired
samples. The hot footprint uses 8 rows; the streaming footprint uses 32,768 rows
(44,564,480 PQ2 bytes). These names describe
allocated footprints, not a measured cache-state or cold-model guarantee. All
comparators remain allocated during collection. Output is JSON Lines containing
the sample/kernel, dimensions, bytes, iteration count, elapsed nanoseconds,
checksum and explicit dot-product ISA choice. Report CPU/compiler/power identity
alongside those records. No default profile change follows from synthetic results;
device end-to-end correctness, latency, RAM and sustained comfort still require CI.
The main-only [manual workflow](../../.github/workflows/bitcos-synthetic.yml)
runs these comparisons and records host/compiler identity plus a median table.
Foundation runs the complete native correctness collection without benchmarks.
