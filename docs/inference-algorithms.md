# Inference algorithms and mathematical constraints

Research reviewed October 23, 2026. This extends the existing T-MAC/R8 leads in
[the implementation plan](../PLAN.md#9-speculative-decoding-and-advanced-optimization)
and maps published algorithms to the [native Rust engine roadmap](native-rust-engine.md).
These are proposed implementation tasks and experiments, not enabled features
or measured Snapdragon improvements. The opt-in baseline PQ2 NEON kernel is
already implemented; scalar remains the default. Preserve the pinned Prism
comparison/fallback, exact model/adapter identities and existing qualification.

The source observations below were rechecked at repository revision
`632bd83dfa91f965f24871e7f5515a2a2bfe7499`. Refer to the named functions rather
than assuming an earlier research report's line numbers remain current. Pin
implementation sources, licenses and dependencies during R0 before reuse;
paper dates and another platform's results do not establish local support.

## Implementation order and current gaps

| Order / owner | Current code observation | Proposed change and exit |
| --- | --- | --- |
| R4.3: prompt ingestion | `Runtime::generate_inner` evaluates every prompt token through the full `Decoder` logits API; `Qwen35::evaluate_token_into` always runs output normalization and vocabulary projection | Add state-only prefill with logits for the final uncached token. Define its error/validation contract explicitly, preserve prefix hits, rollback and cancellation, and compare final logits/state with ordinary evaluation. |
| R4.4: activation preparation | `Transforms::project_into` repeats sign/Hadamard work; `matvec_into` quantizes again for each projection, even where sibling projections consume identical inputs | Share prepared activations only when input, basis/signs, permutation, width and quantizer match. Preserve Q8_K/Q8_0 distinctions and each projection's arithmetic; verify selected logits and reuse after errors/reset. |
| R4.5: physical KV storage | Full-attention `LayerState` holds `Vec<f32>`, although `Evaluation::full_attention` already rounds inserted K/V through binary16 | Store those exact FP16 bits and widen on reads. Halve KV element payload without another precision reduction; keep convolution/GDN state FP32. Verify signed zero, finite edges, overflow rejection, checkpoint/admission accounting and unchanged attention arithmetic. |
| R3 / SIMD S3: packed decode | Scalar packed paths provide independent quantizer/canonicality references; the SIMD plan owns per-format implementation status | Compare qualified NEON paths with the exact ternary LUT variants below, preserving generic PQ2 fourth-symbol behavior and original scale order. Qualify PTQ1 independently; keep malformed-input rejection. |
| R4.6 / SIMD S5: batched prefill | The graph and `Decoder` interface process one token at a time | Add bounded block evaluation, then compare tiled packed matrix operations, Vec-LUT and chunkwise GDN. Prove causal/state scheduling and declare floating-point tolerances before performance qualification. |
| R5.2: checkpoint storage | `Qwen35::checkpoint`, `restore` and `copy_state` deep-copy KV, convolution and recurrent arrays | Share immutable completed KV pages, copy the mutable tail and retain exact recurrent/convolution snapshots. Verify logical snapshot independence, edited-prefix restoration and unique-allocation plus worst-case copy-on-write budgets. |
| M6: speculation experiment | The native decoder is greedy-only and has no block verifier | Qualify block verification and sampling first, then model-free suffix proposals and compact recurrent-state recovery. Keep this separate from ordinary-decoding acceptance. |

Implementation references: [runtime](../src/prism-runtime/src/runtime.rs),
[decoder API](../src/prism-runtime/src/lib.rs),
[graph/state/transforms](../src/prism-runtime/src/graph.rs),
[packed kernels](../src/prism-runtime/src/kernels.rs) and
[ternary codecs](../tools/ternary/src/lib.rs).
Retained scratch capacity alone does not establish that repeated computation
or model-level allocation costs have been removed. R4.2 remains a separate
allocation/memory qualification task.

Skipping unused prompt logits preserves the dependency graph needed for later
tokens, but also skips their output-overflow checks. Specify which checks the
new state-only API performs and retain the existing single-token API contract.
Block prefill must preserve cancellation/backpressure bounds and atomic state
commit; a faster matrix kernel alone does not supply those runtime semantics.

## Packed lookup kernels

### Exact ternary tables for decode

[Bitnet.cpp, February 17, 2025, section 3.2](https://arxiv.org/html/2502.11880v1)
distinguishes exact **TL1_1/TL2_1** from **TL1_0/TL2_0**. The exact variants
retain INT16 lookup sums using two byte lookups; quantizing those sums to INT8
introduces another approximation. Audit the [official implementation](https://github.com/microsoft/BitNet)
as an algorithm/code lead, not a replacement model runtime or quantizer.

Preserve this engine's `q8k_into` and `q8_into`, the 128-weight scale boundaries,
Q8_K's 256-activation groups and Q8_0's 32-activation groups. Keep integer block
sums exact and retain the existing floating scale/add sequence. Stock BitNet
activation quantization is not this engine's numerical specification.

The pinned [Swift census](../config/swift.pq2-census.json) contains only three
symbols, but the generic PQ2 runtime decodes code 3 as **+2**. A ternary-only
fast path needs validated eligibility and a compatible fallback; it must not
silently narrow public tensor semantics. For 128 ternary weights and arbitrary
signed INT8 activations, the absolute dot is at most `128 * 128 = 16,384`;
including PQ2's +2 raises that bound to `32,768`. Bound every table, partial
sum and correction separately, use sufficiently wide accumulators and include
activation -128. SIMD wrapping arithmetic is not checked by Rust overflow flags.

The existing [SIMD plan](simd-plan.md) owns direct packed decoding and its
per-format controls. On paths still repeating canonical unpack/repack inside
each dot, a validated immutable PTQ1 representation could remove that work;
arbitrary public tensors must retain malformed-payload rejection.
The identity `dot(code - 1, q) = dot(code, q) - sum(q)` also permits a candidate
using existing `Q8KBlock.sums`; compare it with lane-wise subtraction. These
are local source/algebra observations, not published speedup claims.

### Vec-LUT for multiple prompt tokens

[Vec-LUT](https://arxiv.org/html/2512.06443v2) was submitted December 6, 2025,
revised April 14, 2026 and published at MobiSys 2026. It groups lookup results
across tokens, streams cache-sized table tiles and reuses precomputation.
Bounded INT16 partial sums feed INT32 accumulation. The
[implementation](https://github.com/OpenBitSys/vlut.cpp) is a lead for R4.6/S5
after block prefill exists.

Retain our quantizers and scale boundaries; the implementation's per-token
quantizer differs. Re-derive overflow limits for our full accepted input range
instead of copying table/group constants. Account for transformed layouts,
repacking time, retained copies and scratch. Its existing platform evidence
does not qualify native Windows ARM64 or predict Swift throughput.

## Chunkwise Gated DeltaNet prefill

[Gated Delta Networks](https://arxiv.org/html/2412.06464v3), submitted December
9, 2024 and revised for ICLR 2025 on March 6, derives chunkwise gated-delta
computation using compact WY/UT structure and matrix operations. The
[official FLA implementation](https://github.com/fla-org/flash-linear-attention/blob/main/fla/ops/gated_delta_rule/chunk.py)
is a reference for the algebra. Use the existing scalar recurrent path as the
single-token decode control and qualify chunk sizes by phase/backend.

For column keys/values, the update has the form
`S_t = alpha_t S_(t-1) (I - beta_t k_t k_t^T) + beta_t v_t k_t^T`.
Chunking represents products of these transitions without changing the real
arithmetic model. It changes evaluation order, so require declared state/logit
tolerances and long-prefix/restore checks. Preserve the pinned normalization,
`head % ssm_keys` mapping, convolution history, sign/group transforms and FP32
state; another Qwen implementation's defaults are not interchangeable contracts.

## Attention locality and the I/O bound

[Saha and Ye, ICML 2024](https://proceedings.mlr.press/v235/saha24a.html), prove
FlashAttention I/O-optimal up to constant factors for fast-memory size
`M >= d^2` in their model. This supports cache-sized tiling and reduced memory
traffic; it supplies neither an unconditional subquadratic exact algorithm nor
a hardware speed forecast. [FlashAttention](https://arxiv.org/abs/2205.14135)
is an algorithm reference for R4.6 and Adreno G4/R6.

The scalar nonflash reference rounds **globally normalized probabilities to
FP16 before multiplying V**. Ordinary online-softmax output merging does not
preserve that particular operation. First consider grouped-head tiling and
contiguous V access retaining each output's reduction order. A compatible
tiled normalization path needs retained scores or another score pass; a
conventional Flash path needs its own explicit numerical contract, identity and
qualification. Do not reuse checkpoints across incompatible backend identities.

## Hybrid cache reuse and speculative state

[Unified Radix Cache, August 11, 2026](https://www.lmsys.org/blog/2026-08-11-unified-radix-cache/)
separates full-attention KV path reuse from recurrent state valid only at an
exact prefix checkpoint. Apply that distinction to R5.2's immutable page sharing
and separately saved recurrent state. With at most eight saved prefixes, avoiding
payload copies is a more concrete first target than replacing linear lookup.
Budget unique pages, mutable tails, metadata, rollback and all retained GDN
snapshots; shared ownership never permits mutable cross-snapshot aliases.

[SuffixDecoding, NeurIPS 2025](https://arxiv.org/abs/2411.04975), is a later
model-free proposal method using prompt/history suffixes and adaptive draft
length. [SpecLA, July 18, 2026](https://arxiv.org/abs/2607.16673), retains
verification factors to reconstruct accepted recurrent states and delays state
commit. Borrow that state-handling idea only after a block verifier exists.
Its complete system uses a trained drafter and reports H100/GDN-1.3B evidence,
including small FP32 state differences; it establishes no Bonsai/Windows result
or bit-identical replay guarantee.

Stochastic speculation needs proven acceptance/rejection correction, as in
[Leviathan et al., ICML 2023](https://proceedings.mlr.press/v202/leviathan23a.html).
Greedy token agreement cannot qualify the creative sampler. Retain request-level
rollback snapshots even if speculative candidate recovery becomes cheaper.

A local algebraic constraint rules out generic inverse-state rollback: the
transition `alpha (I - beta k k^T)` has eigenvalues `alpha` and
`alpha (1 - beta ||k||^2)`. It can be singular or severely ill-conditioned;
floating-point decay/update gates can underflow/saturate, and convolution
overwrites history. Use snapshots, saved factors and forward reconstruction
with explicit numerical checks, rather than truncating recurrent state like KV.

## Compression bound and deferred experiments

An arbitrary independent 128-trit block has `3^128` possibilities and needs
`ceil(128 log2(3)) = 203` bits, hence **26 whole code bytes**. Existing PTQ1
already uses 26 plus the unchanged two-byte scale. This counting argument does
not exclude cross-block coding, correlations or compression of particular
blocks; it removes a general whole-byte reduction in independent block storage
as easy headroom. Keep the [PTQ1 assessment](swift-memory-options.md) and
[BITCOS retirement](bitcos-retirement.md) as the current representation decision.

| Research | Treatment |
| --- | --- |
| [Qualcomm multiplication-only GDN inverse, June 4, 2026](https://arxiv.org/abs/2606.06034) | Truncated Neumann expansion, masking and residual correction are a separate approximation experiment after faithful chunkwise GDN. Its phone Snapdragon 8 Elite Gen 5 NPU evidence does not qualify Windows X Elite; truncation bounds require assumptions beyond strict triangularity. |
| [Near-I/O-optimal approximate attention, May 22, 2026](https://arxiv.org/abs/2605.23751) | The improved asymptotic transfers use approximation. Keep outside the unchanged-arithmetic reference and require quality/resource qualification. |
| [Signed-digit LUT KV, August 4, 2026](https://arxiv.org/abs/2608.03229) | Changes runtime KV representation/precision and targets a hardware datapath. Treat separately from storing already-rounded FP16 bits. |
| [Gated DeltaNet-2, May 21, 2026](https://arxiv.org/abs/2605.22791), [Sherry, January 2026](https://arxiv.org/abs/2601.07892), and [ReaDy, October 2026](https://arxiv.org/abs/2610.20503) | Changed gates, structured sparsity or trained speculative components need compatible trained artifacts. These are future research leads, not inference-only substitutions for current weights. |

Strict lower triangularity gives a finite exact Neumann expansion, but a
low-order truncation's error bound needs additional norm/entry conditions.
Spectral radius zero alone does not bound intermediate growth. Check signed
current-model Gram entries and actual numeric ranges before applying the
Qualcomm paper's approximation argument to this engine.

## Validation and promotion

Implement one change at a time under the existing [validation policy](validation.md).
For exact integer/storage changes, check block boundaries, -128/+2 handling,
scales, malformed inputs and applicable bitwise outputs. For reordered floating
work, predeclare tolerances for selected logits, KV/GDN state and complete
continuations. Cover edited prefixes, checkpoint isolation, cancellation,
failure/reset recovery and memory-budget failures in both cases.

Run local builds and focused correctness checks as appropriate. Full suites,
all benchmarks and performance/profile captures remain CI-only. New algorithms
still need same-artifact, same-adapter-setting comparisons and the mandatory
standard LoRA TPS/quality gates on the target laptop. Do not multiply paper
speedups or promote a path based on synthetic parity, smaller serialized data
or another device's results. This review adds documentation, without changing
runtime defaults, model selection or acceptance verdicts.
