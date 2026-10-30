# Inference algorithms and mathematical constraints

This reference defines numerical/state constraints for the [Rust engine](native-rust-engine.md).
The [research register](research-register.md) owns source reviews and derivations;
[PLAN.md](../PLAN.md#runtime-and-correctness) owns outstanding work. Compare exact artifacts with
pinned Prism under existing promotion criteria; source reuse needs immutable revisions/license
review, and published results establish no Snapdragon speedup.

[Hardware policy](hardware-capabilities.md#automatic-profile-selection) qualifies algorithms and placement separately for prefill
and decode. Batched projections/chunkwise GDN are NPU prefill candidates; packed HVX, NEON and
Adreno remain decode comparisons. Sequential scheduling alone provides no matrix-matrix reuse.
Automatic selection must preserve numerical/state identity and charge phase handoffs.

## Runtime semantics

| Mechanism | Contract |
| --- | --- |
| Prompt ingestion | `Decoder::ingest`/Qwen35 skip output-only work for intermediate uncached prompt tokens; final tokens and prefix hits retain logits, with computed-state checks, cancellation and rollback |
| Activation preparation | Compatible sibling projections share scoped transforms/quantization only when input, basis/permutation, width and quantizer match |
| KV storage | Full-attention state retains exact binary16 bits in `Vec<u16>` and widens on reads; GDN/convolution stays FP32 and budgets charge actual mixed-state capacities |
| Packed decode | NEON PQ2/PTQ1 and odd-group PQ2 preserve scalar quantizers, scale order and generic PQ2's fourth symbol |
| Prefill | Bounded 1–256-token blocks retain sequential arithmetic and whole-request rollback; block scheduling alone gives no tiled matmul reuse |
| Checkpoints | Sealed immutable KV pages may be shared; mutable tails and complete convolution/GDN state remain independently owned, with conservative snapshot budgets |
| Speculation | Suffix proposals and compact recurrent recovery require a block verifier and the selected sampler/RNG's acceptance contract; ordinary decoding is a separate path |

Source map: [runtime](https://github.com/symbiogenesis/prism-inference/blob/main/src/runtime.rs),
[decoder API](https://github.com/symbiogenesis/prism-inference/blob/main/src/lib.rs),
[graph/state/transforms](https://github.com/symbiogenesis/prism-inference/blob/main/src/graph.rs),
[kernels](https://github.com/symbiogenesis/prism-inference/blob/main/src/kernels.rs) and
[codecs](https://github.com/symbiogenesis/prism-inference/blob/main/tools/ternary/src/lib.rs).

Retained scratch is storage reuse, not proof that repeated computation or model-level allocation
costs are removed. Qwen35 state-only ingestion deliberately omits unused output/logit checks while
validating every computed state value. Block prefill must also preserve cancellation/backpressure
bounds and atomic commit.

## Scratch lifetimes

The [lifetime-planning review](research-register.md#t7-state-size-lifetimes-and-prefix-identity)
covers the bounded-program allocation preprint and its transformation assumptions. The current
sequential graph needs only fixed reuse: Prism shares attention projections with FFN up activations,
shares their gate storage, and convolves recurrent QKV in place after each raw element's last use.

At Swift's widths, the affected buffers need 34,816 rather than 73,728 F32 slots: **152 KiB less
nominal high-water payload**, with four fewer separate vectors. Arithmetic/reduction order,
committed state, transaction deltas and checkpoints retain their existing contracts. Frozen
pre-change hybrid logits/state fixtures cover both FFN/QKV size orderings, failed evaluation, retry,
restore and reset. The [graph
implementation](https://github.com/symbiogenesis/prism-inference/blob/326d0dd45b2d3c61ea6ce8ebe67f8249bdfa22a6/src/graph.rs)
defines the storage ownership. Allocator capacity rounding and process memory are separate from this
finite buffer count; nominal scratch payload does not establish measured memory or throughput
benefit. Moving arenas and general placement search require actual-workload evidence.

## Packed lookup kernels

### Exact ternary tables for decode

[Bitnet.cpp, section 3.2](https://arxiv.org/html/2502.11880v1) distinguishes exact **TL1_1/TL2_1**,
using INT16 sums/two byte lookups, from approximate INT8-table **TL1_0/TL2_0**. The [official
implementation](https://github.com/microsoft/BitNet) is a technique reference subject to this
engine's `q8k_into`/`q8_into`, 128-weight scales, 256-activation Q8_K groups, 32-activation Q8_0
groups and floating scale/add order. Stock BitNet quantization is not this engine's specification.

The [Swift census](../config/swift.pq2-census.json) is ternary, but generic PQ2 code 3 means **+2**;
ternary-only dispatch needs validated eligibility/fallback. [Integer bounds and correction
arithmetic](research-register.md#t3-exact-low-bit-dots-need-bounds-at-every-intermediate) apply to
every table and partial sum, not only the final dot; Rust overflow flags do not check wrapping SIMD.

The [SIMD contract](simd-plan.md) owns packed controls. Validated immutable representations may
avoid repeated canonical decoding; arbitrary public tensors still need rejection checks.
`dot(code - 1, q) = dot(code, q) - sum(q)` permits using `Q8KBlock.sums` under the same scale contract
as lane subtraction.

### Vec-LUT for multiple prompt tokens

[Vec-LUT](https://arxiv.org/html/2512.06443v2) groups lookups across tokens and streams cache-sized
table tiles, using bounded INT16 partials and INT32 accumulation. Its
[implementation](https://github.com/OpenBitSys/vlut.cpp) is a batched-prefill technique.
Applicability depends on unchanged quantizers/scales, bounds for all accepted inputs and accounting
for layouts, repacking, retained copies and scratch. Published platforms do not qualify Windows
ARM64 or Swift.

## Chunkwise Gated DeltaNet prefill

[Gated Delta Networks](https://arxiv.org/html/2412.06464v3) derives chunkwise computation with
compact WY/UT structure; the [official FLA
implementation](https://github.com/fla-org/flash-linear-attention/blob/main/fla/ops/gated_delta_rule/chunk.py)
is an algebra reference. Scalar recurrent execution is the decode control; each chunk size, backend
and phase has a separate qualification identity.

For column keys/values, `S_t = alpha_t S_(t-1) (I - beta_t k_t k_t^T) + beta_t v_t k_t^T`. Chunking
preserves the real-arithmetic model while changing floating evaluation order. Its numerical contract
needs declared state/logit tolerances and long-prefix restoration checks, preserving normalization,
`head % ssm_keys`, convolution history, grouped/sign transforms and FP32 recurrent state.

## Attention locality and the I/O bound

The [attention I/O theorem](research-register.md#t4-attention-io-optimality-has-a-computation-model)
motivates tiling under its stated memory/computation model; it forecasts neither device speed nor
unconditional exact subquadratic work. FlashAttention also faces the rounding boundary below.

Scalar nonflash attention rounds **globally normalized probabilities to FP16 before multiplying V**.
Ordinary online-softmax merging changes that operation. Grouped-head tiling and contiguous V reads
can retain reduction order. A rounding-compatible normalization may retain scores or recompute them;
a conventional Flash path needs a distinct numerical contract and qualification. Do not share
checkpoints across incompatible backend identities.

## Hybrid cache reuse and speculative state

[Unified Radix Cache](https://www.lmsys.org/blog/2026-08-11-unified-radix-cache/) distinguishes
reusable full-attention KV paths from recurrent state valid at an exact prefix checkpoint. For at
most eight saved prefixes, linear-lookup cost is separate from payload-copy cost. State budgets
include unique pages, mutable tails, metadata, rollback and every retained GDN snapshot; shared
ownership cannot create mutable cross-snapshot aliases.

[SuffixDecoding](https://arxiv.org/abs/2411.04975) proposes tokens from prompt/history suffixes.
[SpecLA](https://arxiv.org/abs/2607.16673) retains verification factors and delays commit to
reconstruct accepted recurrent states. Consider these only after a block verifier exists. SpecLA's
trained drafter/H100 evidence and reported FP32 state differences do not establish Bonsai
compatibility or exact replay.

Stochastic speculation needs proven acceptance/rejection correction, as in [Leviathan et
al.](https://proceedings.mlr.press/v202/leviathan23a.html); greedy agreement does not qualify
creative sampling. Retain request rollback even if candidate recovery becomes cheaper.

[Inverse-state rollback can be singular or unstable](research-register.md#t5-chunking-recurrence-and-recovering-rejected-tokens);
floating gates can underflow/saturate and convolution overwrites history. Use snapshots or saved
factors with checked forward reconstruction.

### 2026 state-traffic and verification research

The [hybrid-state catalog, H20 to H21](research-register.md#hybrid-recurrence-prefix-caching-and-speculative-decoding)
retains KVBuffer/TreeWY mechanisms, versions, hardware and negative results through **2026-10-29**.
Neither is a selected-runtime feature:

| Candidate | Swift proof obligation |
| --- | --- |
| KVBuffer deferred state writes | Charge buffer reads, flush/materialization and precision conversion. Checkpoints must retain/materialize every pending decay/key/delta plus convolution history; buffered state cannot claim materialized-state identity. |
| TreeWY accepted-path reconstruction | Qualify topology, attention causality, convolution, failed commit and prefix-cache integration. Real-arithmetic equivalence does not imply bit identity; smaller state does not prove faster batch-one decode. |

### Trained parallel proposals and full-state verification

| Proposal/verification route | Swift boundary |
| --- | --- |
| DFlash target-feature diffusion drafts | Require compatible tokenizer, ternary weights, feature interface, adapter and CPU execution. Admit drafter weights/KV, retained target features and verification scratch; compare complete cycles. |
| DFlash 2 candidate scoring/path selection | Require provisioned drafter identities and actual conditional proposal probabilities: coherent paths alone do not preserve the target distribution. Its released Qwen3.8-27B drafter is no Swift/LoRA qualification. |
| VeriCache compressed drafts/full-KV verification | Correct stochastic acceptance separately from greedy identity. Full plus compressed KV can increase RAM; KV-only verification omits GDN/convolution recovery, and multi-request GPU overlap does not transfer to this one-request CPU runner. |

The [catalog, H19/H22 to H23](research-register.md#hybrid-recurrence-prefix-caching-and-speculative-decoding)
retains sources, mechanisms and evaluation scope. DFlash 2 is an author report with released
drafters, not another peer-reviewed paper; learned components remain separate artifacts.

### Certified sparse attention is a separate approximation

The [approximation
catalog](research-register.md#approximation-quantization-theory-and-numerical-analysis) records
sources and [T9](research-register.md#t9-per-call-certificates-need-a-run-wide-failure-budget)
defines run-wide failure budgeting.

| Method | Guarantee boundary |
| --- | --- |
| vAttention (Q22) | Practical pilot/CLT budgets differ from conservative relative operator-output certificates. `(epsilon, delta)` gives neither exact output nor an answer-wide certificate. |
| Perturbed retrieval (Q23) | Require exact candidates, fresh independent per-step perturbations and a run-wide head/layer/step failure budget. Charge the linear key scan and retained full KV/indexes; bounded value reads do not imply bounded total work or smaller RAM. |
| DoSA (Q24) | Bounds preserve an existing sparse indexer's Top-K, including numerical recovery, rather than dense attention. Swift lacks its learned indexer/artifacts. Transfer bounded early rejection with full fallback, not silent dense-token omission. |

Both certified approximation methods need references consistent with Prism's FP16 normalized-
probability rounding before their bounds transfer.

## Compression and approximation constraints

[T1's packing bound](research-register.md#t1-packing-limits-and-the-difference-between-sparse-and-small)
requires 26 code bytes per independent 128-trit block, already used by PTQ1 plus its two-byte scale.
It leaves correlated/cross-block coding open; the [packing decision](swift-memory-options.md#packing-decision)
accounts for execution and indexing costs.

| Research | Constraint |
| --- | --- |
| [Qualcomm multiplication-only GDN inverse](https://arxiv.org/abs/2606.06034) | Truncated Neumann/residual correction is a separate approximation after faithful chunkwise GDN. Phone NPU results do not qualify Windows X Elite. |
| [Near-I/O-optimal approximate attention](https://arxiv.org/abs/2605.23751) | Transfer improvements use approximation; require separate quality/resource qualification. |
| [Signed-digit LUT KV](https://arxiv.org/abs/2608.03229) | Changes KV representation/precision for a hardware datapath; separate from exact FP16-bit storage. |
| [Gated DeltaNet-2](https://arxiv.org/abs/2605.22791), [Sherry](https://arxiv.org/abs/2601.07892), [ReaDy](https://arxiv.org/abs/2610.20503) | Changed gates, sparsity or trained speculative components require compatible artifacts, not inference-only substitution. |

[T5](research-register.md#t5-chunking-recurrence-and-recovering-rejected-tokens) derives the finite
Neumann expansion and explains why spectral radius zero cannot bound low-order error or
intermediate growth. Check signed Gram entries/ranges before applying approximation arguments.

## Validation and promotion

The [validation policy](validation.md) applies independently to each change. Exact integer/storage
changes need bitwise fixtures, scales, malformed-input and -128/+2 cases. Reordered floating work
needs predeclared logits/KV/GDN and full continuation tolerances. Both need checkpoint isolation,
edits, cancellation, failed recovery/reset and memory-budget checks.

Local builds and focused correctness checks are allowed. Full suites, benchmarks and
performance-profile captures remain CI-only. Promotion needs same-artifact, same-adapter comparisons
and standard LoRA TPS/quality gates on the target laptop. Synthetic parity, smaller files and other
devices' results do not establish acceptance; paper speedups cannot be multiplied.
