# Native Rust CPU SIMD contracts

Outstanding CPU kernel work is tracked only in [PLAN.md](../PLAN.md#cpu-kernels). This document
defines arithmetic, dispatch and qualification boundaries. Baseline NEON PQ2/PTQ1 kernels include
odd-group PQ2; scalar is the reference and fallback. Synthetic parity does not establish
target-device speedup or whole-model parity. Pinned Prism remains the assessment/control engine.

## Execution and dispatch

The default `experimental-neon` feature uses the safe core API of [`fearless_simd` 1.1.0][fearless]
under the runtime's `forbid(unsafe_code)`. `MatvecBackend::Auto` selects once per matrix operation;
the Qwen35 graph and token driver inherit that policy. `Qwen35::with_backend` and the driver's
`--backend auto|scalar|neon` select an explicit policy.

`backend_requested` reports the requested policy. `backend_statistics` retains cumulative matrix
attempts/successes, scalar/NEON dispatches, preparation failures and fallback reasons, including
provisional work. `MatvecScratch::with_backend` forces a kernel policy; `selected_backend()` reports
the last operation and `statistics()` exposes aggregate counts without payloads. Counters saturate
at `u64::MAX`; reset and checkpoint restoration preserve diagnostics. Floating projections and
ineligible packed operations use scalar.

Baseline eligibility requires native `aarch64-pc-windows-msvc`, the compiled feature and baseline
NEON. Complete PQ2/PTQ1 rows use packed kernels; unsupported formats/targets and disabled features
use scalar. Scalar runs natively on ARM64. There is no optional-ISA detector, global target override
or persistent expanded weight copy. Kernel `Auto` does not select Snapdragon generations or
CPU/GPU/NPU profiles; that boundary belongs to the [hardware contract](hardware-profiles.md).

Runtime, loader, graph, codecs and broker retain `forbid(unsafe_code)`. Safe dependency APIs are the
first boundary. Intrinsics requiring local unsafe belong in a reviewed isolated crate with safe
slice APIs and explicit bounds, alignment, aliasing, initialization, lifetime and CPU/OS feature
invariants. Public vector types and a relaxed runtime prohibition are outside that contract; Rust's
[target-feature rules][target-feature] apply.

The checked facade validates kinds, dimensions, byte extents, input/output lengths and finite
activations. Immutable payloads are checked at load or per checked block; public arbitrary `Tensor`
calls still reject malformed payloads. Fallible scratch growth stays outside hot loops, errors leave
empty output, and immutable weights/LoRA B remain shareable. Unaligned loads, scalar tails or local
copies are bounded; overreads are invalid.

Optional dotprod and i8mm require verified Windows CPU/OS detection, independently qualified
layouts/shapes/arithmetic and useful device benefit. Compile-time enabled features can bypass
runtime probing; model names, Linux reports, compilation and fault recovery do not establish
eligibility. The relevant interfaces are [`is_aarch64_feature_detected!`][detection],
[`IsProcessorFeaturePresent`][windows-features] and the installed SDK. Distributable flags remain at
the reviewed baseline; global `target-cpu=native`, `+dotprod` or `+i8mm` are outside the contract.
Detection/policy selection stays outside row/block loops. Overrides can disable features or request
supported paths.

## Packed arithmetic

The [kernel module][kernels], [graph][graph], [ternary codec][ternary], [Swift
metadata](../config/swift.metadata.json) and [symbol census](../config/swift.pq2-census.json) define
format identities. Q4_0/Q8_0/F16 helpers do not establish loader support for every artifact
inventory.

| Operation | Required arithmetic |
| --- | --- |
| PQ2 rows divisible by 256 | Q8_K activation groups; each 128-weight block decodes two-bit codes to -1, 0, 1, 2; preserve integer dots and ordered floating scales/adds |
| Odd-group PQ2 | Four independent Q8_0 scales per 128 weights, then ordered sums |
| PTQ1 | Direct canonical base-three decode, four Q8_0 groups per 128 weights, then block scale; no intermediate PQ2 block |
| Hadamard/elementwise work | Preserve stage boundaries, signs, axes/basis, normalization and multiply/add order |
| Activation quantizers | Preserve maxima/ties, clipping, scales, rounding and rejection |
| Floating reductions/RMS/attention/recurrent work | Rounded f32 products and specified f64 accumulation; reassociation has a separate numerical contract |
| Prefill tiles | Bounded causal/state scheduling; faster matvec alone does not batch graph execution |

PQ2 code 3 means **+2**, despite the exporter's narrower ternary-only contract. PTQ1 digits are
interleaved: bytes 0–15 hold five digits at stride 16, bytes 16–23 hold five at stride 8, and bytes
24–25 hold four at stride 2. Q4_0 stores 16 low nibbles before 16 high nibbles. Canonicality and
byte order remain exact.

For arbitrary signed i8 activations, including -128, a 128-element PQ2 dot has tight signed range
`[-32768, 32512]`; the final integer does fit i16. That does not certify an unpacking or correction
scheme: an uncorrected dot with unsigned codes `0..3` instead ranges over `[-49152, 48768]` and
needs wider storage. A 32-element ternary sum has absolute bound 4,096 and its PQ2 counterpart
8,192. Bound every intermediate and correction independently before narrowing lanes; Rust overflow
flags do not check wrapping SIMD arithmetic. See the [integer
derivation](research-register.md#t3-exact-low-bit-dots-need-bounds-at-every-intermediate). Block
scales apply before accumulation into subsequent blocks. Malformed scales and noncanonical PTQ1
bytes remain errors even at zero scale.

Q8_K chooses the first signed value with strictly greater absolute magnitude, uses `-127 / maximum`,
ties-to-even rounding and an f32 scale. Q8_0 uses an absolute maximum, ties-away rounding and an
FP16-rounded stored scale, while quantizing with the original inverse scale. Opposite-sign ties,
signed zero, finite extremes and nonfinite rejection are part of the contract. Equal decoded
PQ2/PTQ1 weights can produce different matvec outputs; see the [packing
assessment](swift-memory.md).

## Numerical and resource acceptance

Optimized-Rust/scalar and Rust/Prism comparisons have separate purposes; an existing
scalar/reference tolerance does not authorize additional SIMD drift.

| Scope | Acceptance |
| --- | --- |
| Symbols, scales, activations, integer sums | Exact bytes/bits/integers, including ties and signs |
| Order-preserving packed dots, elementwise work, butterflies | Bitwise finite f32 parity, signed zero and matching failures; preserve grouping and avoid unintended FMA |
| Reassociated reductions/FMA | Predeclared absolute/relative bounds, error distribution and logits/state/token checks |
| Full graph | Fixed IDs/positions/context/cache/adapter/decoding; layer/logit, KV/GDN, greedy tie, long-prefix, restoration and cancellation checks |

Full attention retains FP16 Q/K/V and normalized-probability conversions. GDN retains
decay/delta/state-update/output order. Fusion cannot move reads across state mutation. Scalar
transcendental functions are the control; vector versions have explicit NaN/infinity/subnormal/mask
and accuracy contracts. Negative-infinity masking and all-masked-row rejection remain required.
[Algebraic float operators][algebraic] and fast-math have distinct numerical contracts.

Exact fixtures include packed codes/offsets, lane extrema, immutable C++ bytes, unaligned blocks,
scale/quantization edges, awkward rows, malformed extents, format changes and error/rollback
recovery. Incomplete packed blocks are invalid. Negative optional-capability cases are simulated;
real optional ISA execution requires eligible hardware. See [kernel fixtures][kernel-tests] and
[SIMD fixtures][simd-tests].

A performance receipt binds artifacts, workload, context, threads, power/cache state, adapter,
tolerances, primary metric, useful-benefit threshold and allowed regressions. It distinguishes
row/tile dispatch, hot-cache/streaming weights, quantization/unpack/dot/transforms/scheduling, cold
load/repack, prefill/first-token/ decode/task cost, scratch/residency/allocations and overlapping
source/destination storage. Compiler/LLVM/options, actual backend/fallback and native CPU/OS
identity are part of its scope. A microkernel result does not qualify a whole-model replacement or
another X1/X2 device. Standard LoRA TPS/quality remains independent.

Local builds, disassembly and focused correctness checks are permitted. Full suites and every
benchmark run only in CI under the [validation policy](validation.md). Hosted ARM64 supplies
correctness infrastructure; target-device CI supplies performance/thermal/energy evidence. Four
threads and total core count are control settings, not universal optima; phase placement and
mixed-device contention are part of a profile's declared scope.

## CPU kernel research reviewed in October 2026

The [research catalog](research-register.md#packed-projections-cpu-kernels-and-compression) records
sources and versions reviewed through **2026-10-29**. Its mechanisms suggest the following bounded
experiments; none changes current arithmetic or acceptance:

| Lead | Swift decision boundary |
| --- | --- |
| Ultra-low-bit shape-specific kernels | Interleaving must fit NEON lane geometry, quantizers and scale groups; charge repacking and retained copies. Published AVX2/VNNI/Xe2 results do not qualify ARM64. |
| ELUTQ, QTEA, HARP, LLVQ, Tetra, XOR-Trellis | Nonuniform codes, sparse compensation, fitted rotations, lattices and trellis palettes create new artifacts. GPU/source results do not establish exact PQ2/PTQ1 conversion or Windows CPU speed. |
| Shannonic | Independent entropy streams may feed a kernel, but count indexes, decode dependencies, scratch and consumed bytes; most evidence concerns 8-bit/communication workloads. |
| T-SAR / ExaGEMM | Simulated instruction/datapath changes require unavailable hardware; they are outside existing-Snapdragon software work. |

Keep original packed bytes as the storage control and separate decode from genuine multi-token
prefill. Attribute unpack, activation preparation, integer dots and scaling. Prove intermediate
bounds for every borrowed layout, including unsigned-code corrections. Avoid persistent expanded
weights. Larger LUTs/live accumulators can spill registers or evict useful cache lines; select
bounded tiles from complete-workload CI evidence. New quantizers, rotations and ISA retain separate
research identities.

[fearless]: https://docs.rs/fearless_simd/1.1.0/fearless_simd/
[target-feature]: https://doc.rust-lang.org/reference/attributes/codegen.html#the-target_feature-attribute
[detection]: https://doc.rust-lang.org/std/arch/macro.is_aarch64_feature_detected.html
[windows-features]: https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-isprocessorfeaturepresent
[algebraic]: https://doc.rust-lang.org/std/primitive.f32.html#algebraic-operators
[kernels]: https://github.com/symbiogenesis/prism-inference/blob/main/src/kernels.rs
[graph]: https://github.com/symbiogenesis/prism-inference/blob/main/src/graph.rs
[ternary]: https://github.com/symbiogenesis/prism-inference/blob/main/tools/ternary/src/lib.rs
[kernel-tests]: https://github.com/symbiogenesis/prism-inference/blob/main/tests/kernels.rs
[simd-tests]: https://github.com/symbiogenesis/prism-inference/blob/main/tests/simd.rs
