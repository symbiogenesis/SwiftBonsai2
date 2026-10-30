# OxiBonsai source reuse review

Reviewed **2026-10-29** against OxiBonsai
[`1d10c48e14754ff6999158f8fa13c7c92b40b0dc`](https://github.com/cool-japan/oxibonsai/tree/1d10c48e14754ff6999158f8fa13c7c92b40b0dc)
(v0.2.3). The downloaded source ZIP has SHA-256
`dbdf127a1fe8e98d753fdbbad5045190e684ec08a4247a88105ccebadaca2967`.
The comparison control is Runner's
[pinned Prism 0.1.6 source and release](../config/prism-inference.lock.json),
revision `02c8a24b31ac1c35f2ffd3530f6a753ce440bdc0`.

## License and ownership

OxiBonsai's [LICENSE](https://github.com/cool-japan/oxibonsai/blob/1d10c48e14754ff6999158f8fa13c7c92b40b0dc/LICENSE)
and workspace manifest declare **Apache-2.0**, copyright 2026 COOLJAPAN OU
(Team Kitasan). Runner's original code has the same license, so the declared
terms permit copying and modification without relicensing Runner. The reviewed
archive contains no `NOTICE` file or additional license file. This review covers
the selected GGUF/kernel sources; it does not approve every transitive dependency,
downloaded model or optional backend.

For a source import, retain applicable copyright/attribution notices, distribute
Apache-2.0, and prominently identify modified files. Preserve an upstream NOTICE
if one is introduced in a later source revision. See
[Apache-2.0 section 4](https://www.apache.org/licenses/LICENSE-2.0#redistribution).
Engine algorithms and the production loader remain owned by
[Prism Inference](native-rust-engine.md); Runner consumes an immutable released
runtime. A source port there and a reviewed release update here are separate work.

## GGUF: adopt regression scenarios

Useful source references are the
[metadata parser](https://github.com/cool-japan/oxibonsai/blob/1d10c48e14754ff6999158f8fa13c7c92b40b0dc/crates/oxibonsai-core/src/gguf/metadata.rs),
[tensor descriptors](https://github.com/cool-japan/oxibonsai/blob/1d10c48e14754ff6999158f8fa13c7c92b40b0dc/crates/oxibonsai-core/src/gguf/tensor_info.rs),
[streaming parser](https://github.com/cool-japan/oxibonsai/blob/1d10c48e14754ff6999158f8fa13c7c92b40b0dc/crates/oxibonsai-core/src/gguf/streaming.rs)
and [edge-case tests](https://github.com/cool-japan/oxibonsai/blob/1d10c48e14754ff6999158f8fa13c7c92b40b0dc/crates/oxibonsai-core/tests/gguf_edge_cases.rs).
They make hostile declared lengths/counts, duplicate metadata/tensor names,
bounded allocation before input arrives, and nesting depth explicit review topics.

Runner already delegates inspection to `bonsai-gguf.exe`. Its released-tool
checks exercise the actual provisioned binary, independent serialized fixtures,
artifact hashing, bounded string/array inspection and malformed tensor extents.
Keep the current little-endian v3 and explicit Prism-dialect boundary. Adding a
second parser would introduce another format authority without improving this
release boundary. The initial adoption adds the `HostileDescriptors` case in
[`tests/Test-Gguf.ps1`](../tests/Test-Gguf.ps1), automatically included in the
existing Foundation `-Case All` invocation. Independently written fixtures cover
over-limit and `u64::MAX` tensor/metadata counts, duplicate keys/names and
overlapping tensor extents. Positive controls retain distinct metadata keys and
valid out-of-order nonoverlapping tensors. Each hostile probe has a 15-second
deadline and requires the expected parser diagnostic, ordinary error exit and
empty standard output, so a crash or setup failure cannot satisfy rejection.
This protects future runtime updates; it does not change parsing speed or
establish that a previously accepted file is now rejected.

The Oxi parser is not a strict drop-in replacement. Its
[compatibility probe](https://github.com/cool-japan/oxibonsai/blob/1d10c48e14754ff6999158f8fa13c7c92b40b0dc/crates/oxibonsai-core/src/gguf/reader.rs#L146)
documents that a v1 file can be reported loadable although the strict parser
rejects it. Metadata array materialization and default memory mapping also need
an independent allocation and file-lifetime review against Runner's existing
bounded inspection and retained-handle contracts.

## NEON: retain concepts, preserve Swift arithmetic

The [type definitions](https://github.com/cool-japan/oxibonsai/blob/1d10c48e14754ff6999158f8fa13c7c92b40b0dc/crates/oxibonsai-core/src/gguf/types.rs)
and [ternary kernel](https://github.com/cool-japan/oxibonsai/blob/1d10c48e14754ff6999158f8fa13c7c92b40b0dc/crates/oxibonsai-kernels/src/gemv_ternary.rs#L15)
establish different format identities:

| OxiBonsai | Swift/Prism boundary |
| --- | --- |
| `Q1_0_g128`, type 41: one sign bit per weight | PTQ1 uses interleaved base-three digits, not sign bits. |
| `TQ2_0_g128`, type 42: two-bit codes; code 3 maps to zero | PQ2 is type 142 in the pinned Prism dialect; its fourth code means +2. |
| FP32 input with FP32 FMA and horizontal reductions | Swift packed matvecs quantize activations to Q8_K/Q8_0, compute integer dots and apply ordered scales. |
| Qwen3 full-attention graph | Swift's `qwen35` has full-attention plus GDN/convolution state. |

Do not rename types or substitute these kernels for PQ2/PTQ1. Even Swift's
ternary-only census does not make the activation arithmetic interchangeable.
The existing [SIMD contract](simd-plan.md) remains authoritative.

Potential implementation references are
[fused packed decode/dot and independent accumulators](https://github.com/cool-japan/oxibonsai/blob/1d10c48e14754ff6999158f8fa13c7c92b40b0dc/crates/oxibonsai-kernels/src/simd_neon.rs),
[row/cache tiling](https://github.com/cool-japan/oxibonsai/blob/1d10c48e14754ff6999158f8fa13c7c92b40b0dc/crates/oxibonsai-kernels/src/parallel_tiled.rs)
and [prefetch controls](https://github.com/cool-japan/oxibonsai/blob/1d10c48e14754ff6999158f8fa13c7c92b40b0dc/crates/oxibonsai-kernels/src/prefetch.rs).
Prism already has packed scalar/NEON paths. Compare these ideas only when a
measured hotspot justifies a bounded Prism-owned pilot, including original
activation groups, scale order, fourth-symbol behavior and scratch accounting.
Oxi's cache sizes and dispatch thresholds are heuristics, not Snapdragon evidence.

The [float SIMD module](https://github.com/cool-japan/oxibonsai/blob/1d10c48e14754ff6999158f8fa13c7c92b40b0dc/crates/oxibonsai-kernels/src/simd_float_ops.rs#L64)
also needs safety repairs before any direct reuse: public safe operations check
some slice lengths only with `debug_assert!`, then execute raw NEON loads/stores.
Those guards disappear in ordinary optimized builds. Its reciprocal estimates,
FMA and reassociated reductions would need a separately declared numerical path.

One concrete candidate adds detail to the existing CPU work: Oxi's
[two-accumulator loop](https://github.com/cool-japan/oxibonsai/blob/1d10c48e14754ff6999158f8fa13c7c92b40b0dc/crates/oxibonsai-kernels/src/simd_neon.rs#L750)
breaks a serial accumulation dependency. Pinned Prism's
[integer dot loop](https://github.com/symbiogenesis/prism-inference/blob/02c8a24b31ac1c35f2ffd3530f6a753ce440bdc0/src/kernels/neon.rs#L71)
adds eight `dot16` results through one `i32x4` accumulator. A pilot could split
those exact integer contributions across two accumulators and combine before
the final reduction, retaining signed widening and every floating scale/add.
All partial sums fit i32 under the existing 128-weight bounds, so this change
can preserve exact results without importing Oxi's floating-point arithmetic.
Compare only control versus two accumulators on three real Swift shapes,
checking generated assembly for spills and target-device CI for useful benefit.
Oxi's AArch64 prefetch helper is a no-op without its nightly compiler cfg;
the presence of a prefetch API alone does not establish executed hints.

No Oxi kernel, parser dependency, runtime selection or performance claim is
introduced by this review. Outstanding implementation work stays in
[PLAN.md](../PLAN.md#cpu-kernels).
