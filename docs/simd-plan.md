# Native Rust CPU SIMD plan

This expands R3/R4 in the [native engine roadmap](native-rust-engine.md).
Baseline NEON PQ2/PTQ1 kernels, including odd-group PQ2, are implemented and
selected automatically on eligible Windows ARM64 builds. Scalar remains the
reference/fallback. Profiling, per-path memory qualification,
optional ISAs and production promotion remain open. **No target-device speedup
or whole-model parity improvement is established.** Pinned Prism remains the
assessment and fallback engine.

## Current implementation

The default `experimental-neon` feature enables target-only
[`fearless_simd` 1.1.0][fearless]. Its safe core API works under the runtime's
`forbid(unsafe_code)` without a procedural macro or local unsafe allowance.
`MatvecBackend::Auto` selects once per matrix operation; the Qwen35 graph and
token driver inherit that default. Select a graph policy through
`Qwen35::with_backend` or the driver's `--backend auto|scalar|neon` option.
The completion record reports `backend_requested`, which describes the policy.
`backend_statistics` retains cumulative matrix attempts, successes, scalar/NEON
dispatches, preparation failures and fallback reasons, including provisional
work. Floating projections and ineligible
packed operations still use scalar. At the kernel level,
`MatvecScratch::with_backend(MatvecBackend::Scalar)` forces scalar and
`selected_backend()` reports the last matrix operation's actual backend.
`statistics()` retains aggregate counts without input data; counters saturate at
`u64::MAX`. Reset and checkpoint restoration preserve these diagnostics.

Eligibility requires `aarch64-pc-windows-msvc`, the feature and ambient baseline
NEON. Complete PQ2/PTQ1 rows use packed kernels; other formats/targets and
feature-disabled builds use scalar. Scalar also runs natively on ARM64.
There is no optional-ISA detector, global target override or persistent expanded
weight copy.

This kernel-level `Auto` policy does not detect Snapdragon generations or select
CPU/GPU/NPU profiles. The planned [X1/X2 profile selector](hardware-profiles.md)
will compose qualified CPU kernels with per-device placement and resource policy.

| Path | Retained arithmetic |
| --- | --- |
| PQ2, rows divisible by 256 | Q8_K activations; decode each 128-weight block's two-bit codes to -1, 0, 1, 2; preserve integer dots and ordered floating scale/add operations |
| Odd-group PQ2 | Four independent Q8_0 scales per 128 weights, with ordered sums |
| PTQ1 | Direct canonical base-three decode, four independent Q8_0 groups per 128 weights, then the block weight scale; no intermediate PQ2 block |

Safe slice loads bound reads, including unaligned inputs. Malformed scales and
noncanonical PTQ1 bytes remain errors even at zero scale. Integer/differential
fixtures cover packed bytes/offsets, lane extrema, deterministic unaligned blocks,
immutable C++ codec bytes, bitwise matvec outputs, scale/quantization edges,
ordered sums, fallback and error recovery. These fixtures establish synthetic
parity, not full-model performance. See [tests](https://github.com/symbiogenesis/prism-inference/blob/main/tests/simd.rs).

## Existing contracts and first targets

Start from [kernels.rs](https://github.com/symbiogenesis/prism-inference/blob/main/src/kernels.rs),
[graph.rs](https://github.com/symbiogenesis/prism-inference/blob/main/src/graph.rs), the
[ternary codec](https://github.com/symbiogenesis/prism-inference/blob/main/tools/ternary/src/lib.rs) and the pinned Prism source identities
in the kernel module. [Swift metadata](../config/swift.metadata.json) and the
[symbol census](../config/swift.pq2-census.json) describe the current PQ2 artifact.
Q4_0/Q8_0/F16 kernel helpers do not establish loader support for every comparison
artifact's tensor inventory.

| Priority | Target | Contract |
| --- | --- | --- |
| 1 | `dot_pq2` | 34-byte blocks/128 symbols; keep Q8_K's 256-column activation groups, each 128-element integer dot and floating scale order |
| 2 | `dot_q8_groups` | 28-byte PTQ1 blocks with four Q8_0 groups; retain odd-group PQ2 as its own path |
| 3 | Hadamard and elementwise work | Preserve stage boundaries, signs, basis, normalization and multiply/add order |
| 4 | `q8k_into` / `q8_into` | Preserve maxima/ties, clipping, scales, rounding and errors |
| 5 | Floating reductions, RMS norm, attention/recurrent work | Retain rounded f32 products and specified f64 accumulation; reassociation needs separate qualification |
| 6 | Prefill tiles | Add bounded block/state scheduling before reusing work across tokens; a faster matvec alone does not batch the graph |

Profiling may reorder these targets. Embeddings and uncommon formats retain
the reference path until measured benefit justifies expansion.

PQ2 code 3 means **+2**, even though the exporter's ternary-only contract is
narrower. PTQ1 digits are interleaved: bytes 0–15 hold five digits at stride 16,
16–23 hold five at stride 8, and 24–25 hold four at stride 2. Q4_0 places its
16 low nibbles before 16 high nibbles. Preserve canonicality and byte order.

For arbitrary signed i8 activations, including -128, a 128-element PQ2 dot has
absolute sum at most 32,768. A 32-element ternary sum is at most 4,096; the
corresponding PQ2 sum is at most 8,192. Use i32 accumulators and prove every
intermediate/correction for each formulation. Rust overflow flags do not check
wrapping SIMD arithmetic. Apply block scales before accumulating further blocks.

Q8_K selects the first signed value with strictly greater absolute magnitude,
uses `-127 / maximum`, ties-to-even rounding and an f32 scale. Q8_0 uses an
absolute maximum, ties-away rounding and an FP16-rounded stored scale, while
quantizing with the original inverse scale. Check opposite-sign ties, signed
zero, finite extremes and nonfinite rejection. Equal PQ2/PTQ1 decoded weights
can produce different matvec results; see the [packing assessment](swift-memory.md).

## Architecture and dispatch

Keep runtime, loader, graph, codecs and broker under `forbid(unsafe_code)`.
Use safe dependency APIs first. If future intrinsics or macro expansion require
local unsafe, isolate them in a reviewed boundary crate with a safe slice API;
do not relax the runtime prohibition. Document bounds, alignment, aliasing,
initialization, lifetime and CPU/OS feature invariants. Respect Rust's
[target-feature rules][target-feature]; avoid public vector types.

The checked facade validates kind, dimensions, byte extents and input/output
lengths, with finite activations on every call. Validate immutable payloads on
load or retain checked block validation; public arbitrary `Tensor` calls must
still reject malformed payloads. Keep fallible scratch growth outside hot loops,
retain empty output on errors, and preserve immutable weights/LoRA B sharing.
Use bounded unaligned loads and scalar tails or local copies; never overread.

| Backend | Eligibility |
| --- | --- |
| Scalar | Reference and fallback for unsupported formats/shapes/targets; explicit forcing supported |
| Baseline NEON | Native Windows ARM64 MSVC, compiled feature and baseline operations |
| NEON + dotprod | Future separate function with verified Windows CPU/OS detection, exact parity and target-device benefit |
| i8mm | Future tile candidate with independent detection, layout/shape parity and benefit after packing costs |
| Other optimized targets | Add only for a supported deployment; ARM work does not require other ISAs |

Before optional-ISA work, audit the pinned Rust Windows detector against
[`is_aarch64_feature_detected!`][detection], Microsoft's
[`IsProcessorFeaturePresent`][windows-features] and the installed SDK. Compile-time
enabled features can bypass runtime probing. Leave variants disabled without
reliable detection; model names, Linux reports, compilation and fault recovery
are insufficient.

Detect hardware once, then select by operation, format, shape and qualified
policy outside row/block loops. Keep distributable flags at the reviewed baseline;
no global `target-cpu=native`, `+dotprod` or `+i8mm`. Overrides may disable features
or request supported paths. Test missing/unknown capabilities without executing
unsupported instructions. Log selected backend/fallback and build identity without
private prompts or tensor contents. Use paired evidence to refine automatic
selection, including cases where vector execution loses.

### X1/X2 CPU policy integration

Retain scalar and baseline NEON as the portable Windows ARM64 foundation for
Snapdragon X1 and X2. Optional dotprod/i8mm eligibility comes from verified
Windows CPU/OS capabilities and exact kernel contracts; a processor family or
SKU name alone cannot enable an ISA. Qualify performance independently for each
supported family/SKU and relevant OS/runtime revision, with accelerator driver
identity included when evaluating mixed-device profiles.

Measure prefill and decode thread counts, worker placement, batch/tile sizes and
polling separately. The current four-thread development policy and the machine's
total core count are controls, not universal X1/X2 optima. Compare CPU-only work
with CPU participation in GPU/NPU profiles, including shared-memory contention,
submission waits and sustained power behavior. Retain separate interactive and
battery candidates where measurements justify them.

Publish only bounded, qualified entries to the planned profile selector. Unknown
hardware or an unmatched capability/runtime record uses the supported baseline
CPU policy, subject to normal admission; an unavailable optional ISA falls back
without executing it. Future selection must retain actual-backend diagnostics
and numerical identity across checkpoints and phase changes.

## Numerical acceptance

Keep optimized-Rust/scalar comparisons separate from Rust/Prism comparisons.
Existing scalar/reference tolerance does not authorize additional SIMD drift.

| Operation | Acceptance |
| --- | --- |
| Symbols, scales, activations, integer sums | Exact bytes/bits/integers, including ties and signs |
| Order-preserving packed dots, elementwise work and butterflies | Bitwise finite f32 parity, signed zero and matching failure behavior; preserve grouping and avoid unintended FMA |
| Reassociated reductions or FMA | Predeclared absolute/relative bounds, error distribution and logits/state/token checks |
| Full graph | Fixed token IDs, positions, context, cache, adapter and decoding; check layer outputs/logits, KV/GDN state, greedy ties, long prefixes, restoration and cancellation |

Preserve FP16 Q/K/V and normalized-probability conversions in full attention,
and GDN decay/delta/state-update/output order. Fusion cannot move reads across
state mutation. Keep scalar transcendental functions initially. Vector replacements
need explicit accuracy and NaN/infinity/subnormal/mask review. Preserve negative-
infinity masking and all-masked-row rejection. [Algebraic float operators][algebraic]
and fast-math modes need separate numerical qualification.

## Delivery sequence and exits

- [ ] **S0 — Freeze contracts and profile (R0/R1/R3).** Record artifact/runtime
  hashes, format/shape histogram, fixtures, compiler/options and CPU/OS evidence.
  Predeclare numerical and benefit criteria. Use CI hotspot captures, Amdahl's
  bound `1 / ((1 - p) + p / s)` and bandwidth/dispatch/packing costs to scope work.
- [x] **S1 — Select the safe implementation route (R3).** The core Fearless SIMD
  API is selected. Native Release CLI/no-CLI, strict analyzer and code-generation
  checks support the route without a new unsafe boundary. Recheck dependency,
  license and code generation when relevant inputs change.
- [ ] **S2 — Complete baseline PQ2 selection (R3).** Kernel, automatic/explicit
  selection, actual-backend reporting and focused exact regressions are implemented.
  Fallback/build diagnostics distinguish explicit scalar, unsupported tensor kinds,
  disabled features and unsupported targets, with attempt/completion flags.
  Complete S6
  qualification; retain bounded loads,
  scalar quantization, ordered scales and negative controls.
  - [x] **S2.1 — Report aggregate graph dispatch.** Retained matrix statistics
    distinguish mixed scalar/NEON work, shared preparations, preparation failures,
    failed matrix attempts and scalar fallback reasons. The graph and JSON driver
    expose lifetime counts, including provisional work, alongside the latest
    operation record. Focused regressions cover mixed paths and failure recovery;
    no backend or performance verdict is inferred from counts.
- [ ] **S3 — Qualify PTQ1 and odd-group PQ2 (R3).** Direct decoding, 128/256/384-
  column and multirow cases, exact scalar comparisons, C++ byte fixtures and
  recovery with retained scratch are implemented. Complete independent per-path
  memory evidence. Compare [exact TL1_1/TL2_1 LUTs](inference-algorithms.md#exact-ternary-tables-for-decode)
  without changing quantizers, scales, fourth-symbol semantics or rejection.
- [ ] **S4 — Evaluate dotprod, then i8mm (R3/R4).** Audit Windows detection first;
  compare widening NEON and optional variants including unpack/scale/dispatch/
  packing costs. Inspect fallback ISA leakage. Qualify each variant separately;
  absent detection or useful benefit leaves it disabled.
- [ ] **S5 — Expand measured CPU work (R4).** Qualify transforms, shared activation
  preparation, quantizers, floating reductions and bounded prefill tiles one at
  a time. [Vec-LUT/chunkwise GDN](inference-algorithms.md) require block scheduling.
  Preserve numerical contracts, state ownership, bounded scratch and error recovery.
- [ ] **S6 — Qualify and refine selection (R3/R7).** Run paired, repeated/
  interleaved manual main-only target-device CI. Compare scalar Rust, optimized
  Rust and applicable pinned Prism controls. Publish successful and losing entries,
  refine selection by X1/X2 device identity, format/shape/backend, phase and
  interactive/battery objective, and retain scalar forcing/Prism fallback.
  A microkernel win does not qualify the replacement.

Synthetic S1–S3 work can precede full-model parity. S0 determines investment;
S6 promotion requires the relevant R2/R5 exits. If the first kernel misses its
benefit criterion, retain evidence and reprofile before expanding the inventory.

## Validation and performance evidence

Extend [kernel fixtures](https://github.com/symbiogenesis/prism-inference/blob/main/tests/kernels.rs) and SIMD fixtures
with all codes, valid/invalid encodings, scales/ties/extremes, unaligned slices,
block boundaries, awkward rows, malformed extents, format changes and error/rollback
recovery. Incomplete packed blocks remain invalid. Verify non-ARM fallback and
mock absent optional capabilities; execute real optional ISAs only when eligible.

Inspect Release assembly for intended instructions, calls/spills/copies,
scalarization and forbidden-ISA leakage. Record compiler/LLVM/options and repeat
on relevant changes. Local builds, disassembly and focused correctness checks
are allowed. Full suites and **every benchmark** remain CI-only; see
[validation policy](validation.md).

Before CI timing, fix artifacts, workload, context, threads, power/cache state,
adapter, tolerances, primary metric, useful-benefit threshold and allowed
regressions. Retain:

- Representative/awkward row or tile costs, packed bandwidth, small-work dispatch
  costs and distinct hot-cache/streaming-weight cases.
- Cold load/repack, prefill, first-token, decode and completed-task latency,
  attributing quantization, unpack/dot, transforms and scheduling.
- Peak/resident memory, scratch, allocations, repack amortization and overlapping
  source/destination storage.
- CPU/OS features, architecture, thermal/energy/sustained behavior, build hashes,
  selected backends, raw results and per-metric verdicts.

Hosted ARM64 is correctness infrastructure; target-laptop CI supplies device
performance evidence. Standard LoRA TPS/quality and R7 gates remain required.

[fearless]: https://docs.rs/fearless_simd/1.1.0/fearless_simd/
[target-feature]: https://doc.rust-lang.org/reference/attributes/codegen.html#the-target_feature-attribute
[detection]: https://doc.rust-lang.org/std/arch/macro.is_aarch64_feature_detected.html
[windows-features]: https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-isprocessorfeaturepresent
[algebraic]: https://doc.rust-lang.org/std/primitive.f32.html#algebraic-operators
