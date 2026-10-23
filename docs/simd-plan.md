# Native Rust CPU SIMD plan

Planning decision, October 23, 2026. This expands R3 and the CPU part of R4 in
the [native engine roadmap](native-rust-engine.md). All work items below are
pending. The [current runtime](../src/prism-runtime/README.md) remains a scalar
reference; this document adds no dependency, kernel, dispatch switch or speed
claim. The publisher Prism engine remains the assessment/fallback path.

## Direction and evidence

Start with a fused packed PQ2 matrix-vector kernel on native Windows ARM64.
Keep activation quantization scalar initially, vectorize exact integer work,
and preserve scale application order. Evaluate `fearless_simd` first for safe
baseline NEON operations; use a small audited boundary only for operations the
chosen safe API cannot express efficiently. Qualify PTQ1 independently, then
expand according to measured hotspots. Keep Rust 1.99.1 and edition 2024.

Davidoff's September 25 [SIMD survey][survey] motivates separating automatic
vectorization, portable vectors and intrinsics, checking generated code, and
placing dispatch around substantial work. It also highlights floating-point
semantics and compiler sensitivity. Its author maintains Fearless SIMD; treat
the comparison as a research lead and check primary APIs before selection.
The recommendations below are project decisions based on that reading and the
existing code, not reported Snapdragon results.

| Approach | Decision for this engine |
| --- | --- |
| Ordinary Rust loops | Retain as oracle and fallback. Try fixed chunks and independent integer accumulators before adding an abstraction; inspect Release code generation. |
| [`fearless_simd` 1.1.0 documentation][fearless] | First implementation trial: safe slice operations, baseline NEON and explicit-intrinsic integration. Core and optional macro crate are separate dependencies; audit/pin each selected version and its expansion. Its documented ARM level is baseline NEON, so it does not settle dotprod/i8mm support. |
| [`pulp`][pulp] | Alternative if its dispatch and operation coverage better fit the measured kernel. Compare the same algorithm and exact arithmetic, including integration and compile costs. |
| [`wide`][wide] | Fixed-width alternative for baseline operations. Its integer operators wrap; prove range bounds. Do not infer runtime ISA dispatch from vector types. |
| [`multiversion`][multiversion] | Consider only if ordinary loops need explicit function variants. Select exact targets and dispatch at the matrix/tile boundary. |
| [`macerator`][macerator] | Reconsider only for a specific missing capability. Its documented raw load/store interfaces need unsafe callers; this is additional boundary work. |
| [`std::simd`][portable] | Remains nightly-only in the reviewed Rust docs. Keep outside production; the separate nightly solver trial is not a SIMD migration decision. |
| `std::arch` intrinsics | Narrow escape hatch after assembly shows an operation gap. Require checked slices, feature proof and bounded loads; inline assembly needs a separately justified compiler issue. |

These are alternatives, not a plan to add all libraries. S1 selects at most one
portable abstraction. Recheck moving API documentation and record exact source
revisions, license/security review, dependency tree and lockfile checksums when
implementation starts. A successful prototype must compile with this workspace's
strict lints and native MSVC ARM64 target, including the no-CLI library build.

## Existing contracts and first targets

Use [kernels.rs](../src/prism-runtime/src/kernels.rs),
[graph.rs](../src/prism-runtime/src/graph.rs), the
[ternary codec](../tools/ternary/src/lib.rs) and the immutable Prism source hashes
in the kernel module as the starting implementation map. The current Swift
[metadata](../config/swift.metadata.json) and
[symbol census](../config/swift.pq2-census.json) describe PQ2/F32/BF16 weights.
Q4_0/Q8_0/F16 are supported reference paths, not a claim that this runtime
implements the Qwen comparison artifact's complete mixed-type inventory.

| Order | Target and retained contract | First implementation |
| --- | --- | --- |
| 1 | `dot_pq2`: 34-byte weight blocks, 128 symbols each; Q8_K activations cover 256 columns | Decode 2-bit symbols in registers, subtract one, widen/multiply and accumulate into i32. Reuse the already quantized activation vector across rows. Keep each 128-element integer dot and the current floating scale/add sequence. |
| 2 | `dot_q8_groups`: 28-byte PTQ1 blocks; four separate 32-element Q8_0 activation groups | Decode validated base-3 bytes without an intermediate PQ2 buffer where practical. Preserve each group's scale and ordered sum, then the block weight scale. Keep PQ2's odd-group Q8_0 fallback separate. |
| 3 | `hadamard` and elementwise graph operations | Vectorize independent butterflies/lanes at the existing stage boundaries and preserve sign, basis, normalization and multiply/add order. Profile residual add/gates and row conversion before expanding. |
| 4 | `q8k_into` / `q8_into` | Optimize only after packed-dot parity. Preserve maximum selection, tie behavior, scales, clipping and errors exactly. |
| 5 | `float_dot`, RMS norm, attention/recurrent reductions | Separate numerical qualification; retain rounded f32 products and the current f64 accumulation where specified. Vectorizing products does not authorize changing the reduction. |
| 6 | Prefill matrix/tile execution | R4 work after a useful decode kernel: reuse quantized activations across rows/tokens, compare small tile sizes and bound scratch. The current graph evaluates tokens sequentially; batching needs graph/state scheduling work, not just a faster matvec. |

R1 profiling may change this order. Record the reason and affected milestone
before expanding scope. Embeddings and uncommon tensor formats keep their
reference implementation until they justify optimization.

PQ2 native codes decode to **-1, 0, 1, 2**. The codec/exporter's narrower
three-symbol contract must not make the runtime reject code 3 or reinterpret it
as zero. PTQ1 canonicality, scale locations and little-endian byte decoding
remain unchanged. Never skip a zero-scale block's validation or finite-input
checks as a shortcut.

PTQ1 digit groups are interleaved, not contiguous runs of trits: bytes 0–15
hold five digits at stride 16, bytes 16–23 five at stride 8, and bytes 24–25
four at stride 2. Preserve that layout in SIMD shuffles/lookup tables. Q4_0
similarly places 16 low nibbles before 16 high nibbles in decoded order.

For the first PQ2 dot, `128 * 2 * 127 = 32,512` bounds the absolute integer sum
with the current activation range. Use i32 accumulators and prove every
intermediate, including any bias correction in an alternate unsigned-dot
formulation. If an API accepts caller-provided Q8_K blocks, include activation
-128 in its proof; the corresponding PQ2 bound becomes 32,768. Bound legacy
Q8 weights separately, including -128. Do not rely on
Release overflow checks to detect overflow inside wrapping SIMD instructions,
and do not accumulate an unbounded row before applying per-block scales.

Q8_K selects the first signed value with strictly greater absolute magnitude,
uses `-127 / maximum`, ties-to-even rounding, and an f32 scale. Q8_0 uses an
absolute maximum, ties-away rounding, and an FP16-rounded stored scale while
quantizing with the original inverse scale. Equal-magnitude opposite signs,
signed zero, tiny/huge finite values and nonfinite rejection need explicit
fixtures. Equal decoded PQ2/PTQ1 weights do not imply equal matvec output:
their activation quantizers differ. See the [packing assessment](swift-memory.md).

## Architecture and dispatch

Keep the runtime, loader, graph, codecs and broker under `forbid(unsafe_code)`.
Proposed internal layout: a scalar implementation/oracle, a checked dispatch
facade, portable NEON kernels, and optional architecture-specific kernels.
These names describe responsibilities, not files or APIs already present.
Avoid a public vector type in the model/runtime API.

First try safe dependency APIs within the current crate. Inspect procedural
macro expansion and compile it with `forbid(unsafe_code)`; a safe call surface
does not guarantee a macro passes the lint. If feature transitions, raw memory
operations or expansion require local unsafe, place them in a proposed separate
`bonsai-cpu-kernels` boundary crate with narrow documented allowances and a safe
slice-based API. Do not relax the runtime-wide prohibition. Every unsafe
operation needs bounds, alignment, aliasing, initialization, lifetime and CPU/OS
feature invariants; arithmetic intrinsics alone do not prove safe memory access.
Rust's [target-feature rules][target-feature] still apply to calls and inlining.

The facade checks kind, dimensions, byte extents and input/output lengths before
the kernel, with finite activation validation on each call. Validate immutable
tensor payloads at load time or retain checked per-block validation; do not add
a redundant whole-matrix scan to every matvec. Public calls accepting arbitrary
`Tensor` values must retain malformed/nonfinite payload rejection. Keep fallible
scratch growth outside the hot loop, prohibit allocation there, and retain
`matvec_into`'s empty-output-on-error behavior. Immutable packed payloads and LoRA
B sharing stay intact.
Use unaligned, bounds-checked loads and an explicit scalar remainder or bounded
local copy; never read beyond a slice because a mapped file has spare pages.

| Backend | Eligibility and fallback |
| --- | --- |
| Scalar | Always available as reference and for unsupported kinds/shapes. A test override selects it without changing model semantics. |
| Baseline NEON | Native `aarch64-pc-windows-msvc`, verified target/build configuration and baseline operation support. Start with 128-bit lanes and widening integer arithmetic; no optional extension assumption. |
| NEON + dotprod | Separate compiled function requiring verified Windows CPU/OS detection for `dotprod`, exact arithmetic parity and a useful target-device result. Otherwise retain baseline NEON. |
| i8mm | Separate later tile candidate, requiring its own detection, compatible operand layout/shape, parity and benefit after packing costs. Do not assume dotprod implies i8mm. |
| x86 / other targets | Scalar remains valid. Add optimized variants only for an actual supported deployment; the ARM milestone does not require AVX2/AVX-512, SVE, SME, WASM or RISC-V kernels. |

Audit Rust 1.99.1's actual Windows detector before optional ISA work. The
[`is_aarch64_feature_detected!` docs][detection] list feature names but warn that
platform coverage varies; compile-time-enabled features can return true without
a runtime probe. Compare the pinned implementation with Microsoft's
[`IsProcessorFeaturePresent` contract][windows-features] and the installed SDK.
If a required Windows feature has no reliable supported query, leave that
variant disabled. A model name, Linux feature report or successful compilation
is insufficient. Do not execute an instruction and recover from a fault as
feature detection.

Detect hardware once, then select by operation, format, shape and qualified
policy. Dispatch outside the row/block loop, ideally once per matvec or tile;
inline small helpers inside the matching feature context. Keep distributable
builds on reviewed baseline flags: no global `target-cpu=native`, `+dotprod`,
`+i8mm` or `+sve` that can contaminate fallback code. Developer overrides can
disable features or request a supported path, never enable an unsupported one.
Tests inject absent/unknown capabilities to check selection without executing
those instructions. Log requested/selected backend and fallback reason with
format/shape and build identity, without tensor contents or private prompts.

## Numerical acceptance

S0 records acceptance before implementation. Keep two comparisons: optimized
Rust versus the scalar Rust oracle, and Rust versus the pinned Prism reference.
Existing scalar/reference tolerance does not authorize extra SIMD drift.

| Operation class | Initial acceptance |
| --- | --- |
| Symbols, scales, quantized activations, integer partial sums | Exact bytes/bits/integers, including tie and sign rules; no tolerance. |
| First packed dot and order-preserving elementwise/butterfly kernels | Bitwise finite f32 output parity with scalar Rust, including signed zero and failure behavior. Preserve multiplication/addition grouping and disable unintended FMA contraction. |
| Proposed reassociated floating reductions or FMA | Separate candidate with declared absolute/relative bounds and error distribution, plus logits/state/token checks. Derive tolerances from the operation and reference fixtures before CI timing; reject an undeclared change. |
| Full graph | Fixed token IDs, positions, context, cache precision, adapter and decoding settings; compare selected layer outputs/logits, KV and recurrent states, greedy tie cases, prefix restoration and cancellation recovery. Track long-sequence error growth. |

Preserve full attention's FP16 Q/K/V and probability conversions. Gated Delta
Net must retain its decay, delta computation, state update and output-dot order;
fusion cannot move a read across a recurrent state mutation.

Rust's [algebraic float operators][algebraic] permit transformations that change
floating-point results. Do not insert them or a fast-math mode as a mechanical
vectorization fix. Keep scalar `exp`, `sin`/`cos`, sigmoid/SiLU and softmax math
initially. A vector transcendental implementation needs an explicit accuracy,
mask/NaN/infinity/subnormal and full-graph review; a vector-looking API alone
does not establish equivalent arithmetic or useful instructions. Preserve
attention's negative-infinity mask and all-masked-row rejection.

## Delivery sequence and exits

- [ ] **S0 — Freeze contracts and select one target (R0/R1/R3).** Record model,
  adapter and Prism hashes, actual format/shape histogram, scalar fixtures,
  compiler/LLVM/options and CPU/OS feature evidence. Retain per-phase hotspot
  captures in CI when requested. Use measured kernel share `p` and candidate
  gain `s` to bound end-to-end gain by `1 / ((1 - p) + p / s)`; account separately
  for bandwidth, dispatch and packing. Exit with the first PQ2 shape, numerical
  contract, benefit criterion and bounded experiment scope.
- [ ] **S1 — Select the safe implementation route (R3).** Compare chunked Rust
  and one Fearless SIMD prototype on the same synthetic packed dot. Build native
  Release with pinned stable, strict Clippy/rustdoc, CLI and no-CLI library;
  inspect expansion and emitted assembly in context. Review dependency/license
  and feature coverage. Exit with one chosen abstraction or a documented reason
  for the narrow boundary crate, without broad lint exceptions.
- [ ] **S2 — Land baseline PQ2 NEON behind internal selection (R3).** Retain the
  scalar quantizer and per-block scale order. Add bounded load/tail behavior,
  scalar selection and dispatch diagnostics. Exit with focused exact differential
  checks and negative controls; it stays experimental until S6 evidence.
- [ ] **S3 — Add PTQ1 and PQ2 odd-group coverage (R3).** Fuse canonical trit decode
  with dot arithmetic; preserve four Q8_0 scales per 128-element block. Exercise
  real stored byte offsets, rows of 128/256/384 columns and multiple rows. Compare
  each format against its own scalar activation path and immutable C++ fixtures.
  Exit with independent parity and memory evidence for each path.
- [ ] **S4 — Evaluate optional dotprod, then i8mm (R3/R4).** Audit Windows
  detection first. Compare widening NEON, dot-product and eligible tile kernels
  with the same representation; count unpacking, scale and dispatch costs.
  Inspect unsupported-feature fallback code. Exit separately for each variant;
  unavailable detection or no useful benefit leaves it disabled.
- [ ] **S5 — Broaden only to demonstrated hotspots (R4).** Try Hadamard and
  independent elementwise lanes, then quantization or float reductions under
  their stronger numerical contract. For multirow/interleaved or prefill tiles,
  bound repack time, simultaneous copies, scratch and retained capacity; preserve
  logical weights and basis. Qualify graph scheduling, recurrent causality,
  checkpoint independence and cancellation granularity. Keep each change in a
  separately attributable comparison.
- [ ] **S6 — Qualify and record selection policy (R3/R7).** Run paired manual
  main-only target-device CI with fixed model/adapter/workload and repeated
  interleaved trials. Publish the kernel and end-to-end evidence described below,
  including regressions. Promote only successful format/shape/backend entries;
  retain scalar forcing and a reproducible Prism fallback. A microkernel win
  alone does not complete R3 or qualify the Rust replacement.

S1/S2 synthetic correctness work can proceed before complete model parity.
S0 profiling determines investment priority; S6 performance promotion still
requires the relevant R2/R5 correctness/integration exits. If the first packed
kernel fails its predeclared benefit criterion, keep its evidence and stop the
remaining kernel expansion until profiling identifies another credible target.

## Verification and performance evidence

Extend [existing kernel fixtures](../src/prism-runtime/tests/kernels.rs) rather
than building a second codec. Use deterministic differential inputs for all PQ2
codes, PTQ1 valid/invalid encodings, scale bits, quantization ties, extreme sums,
zero/nonfinite values, truncated/oversized extents and format changes in retained
scratch. Cover vector-boundary tails, deliberately unaligned byte slices,
single/multiple rows and malformed dimensions. Reject incomplete packed blocks;
a vector remainder does not legalize an incomplete format block. Include output
clearing, error recovery and runtime rollback tests with optimized selection.
Check the portable fallback on a non-ARM build and exercise capability selection
with mocked missing features. Real optional instructions execute only on
confirmed eligible hardware.

Inspect Release assembly for actual shifts/masks/widening or qualified dot
instructions, per-block calls, spills, redundant copies, scalarized operations
and forbidden ISA leakage. Repeat code-generation review on toolchain, dependency
or relevant caller changes; record compiler/LLVM and options. Local compilation,
disassembly and focused correctness tests are allowed. They establish no timing
or speedup verdict. Full suites and **all benchmarks**, including single-kernel,
short, smoke and dry runs, remain CI-only under the [validation policy](validation.md).

Before timing, declare the primary metric, minimum useful improvement,
repeat/dispersion policy and permitted regressions in task latency, memory,
energy and sustained comfort. Keep production arithmetic, context, threads,
power mode, cache state and adapter fixed. Measure scalar Rust, optimized Rust
and pinned Prism under the same applicable contract, with kernel comparisons
separate from full-graph comparisons. Record:

- Packed bytes/second and time per row/tile across representative and awkward
  shapes, including small work where dispatch costs matter; hot-cache and
  streaming-weight cases must be distinct.
- Cold load/repacking, prefill, first-token latency, steady decode and useful
  completed-task latency. Separate quantization, unpack/dot, transforms and
  scheduling so savings are attributable.
- Peak/resident bytes, scratch capacity, allocations, repack amortization and
  simultaneous source/destination storage; no persistent expanded matrix is
  assumed affordable.
- Target laptop CPU/OS features, native architecture, threads, power, thermal
  state, energy and sustained behavior. Generic hosted ARM64 is correctness
  infrastructure, not Oryon performance evidence.
- Model/adapter/build hashes, selected backend per format/shape, tolerances,
  correctness/quality failures, raw results and the accepted/rejected/pending
  verdict. Retain useful captures outside the checkout in owned evidence lanes.

No runner is configured or dispatched by this planning change. Future CI work
uses reviewed `main` revisions and the existing target-device isolation policy;
the session's local-only publication instruction leaves remote dispatch and
performance qualification pending. Standard LoRA TPS/quality gates and the
overall R7 replacement gates remain in force.

[survey]: https://shnatsel.github.io/state-of-simd-rust-2026/
[fearless]: https://docs.rs/fearless_simd/1.1.0/fearless_simd/
[pulp]: https://docs.rs/pulp/0.22.3/pulp/
[wide]: https://docs.rs/wide/1.7.1/wide/
[multiversion]: https://docs.rs/multiversion/latest/multiversion/
[macerator]: https://docs.rs/macerator/latest/macerator/
[portable]: https://doc.rust-lang.org/std/simd/index.html
[target-feature]: https://doc.rust-lang.org/reference/attributes/codegen.html#the-target_feature-attribute
[detection]: https://doc.rust-lang.org/std/arch/macro.is_aarch64_feature_detected.html
[windows-features]: https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-isprocessorfeaturepresent
[algebraic]: https://doc.rust-lang.org/std/primitive.f32.html#algebraic-operators
