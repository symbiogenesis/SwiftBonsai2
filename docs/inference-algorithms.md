# Inference algorithms and mathematical constraints

The [research and proof register](research-register.md) indexes the wider literature, recent
implementations and engineering reports, distinguishing mathematical guarantees from experimental
results and project applicability.

This reference defines numerical and state constraints for the [Rust engine](native-rust-engine.md).
Outstanding algorithm and qualification work lives in [PLAN.md](../PLAN.md#runtime-and-correctness).
Algorithm descriptions and published results do not establish a Snapdragon speedup. Pinned Prism
controls, exact artifacts and the existing promotion criteria remain the comparison contract. Source
reuse requires immutable revisions and a license review.

The [X1/X2 hardware contract](hardware-profiles.md) treats qualified algorithms and placements
separately for prefill and decode. Batched projections and chunkwise GDN are NPU prefill candidates;
packed HVX, NEON and Adreno remain decode comparisons. Sequential block scheduling alone does not
provide tiled matrix-matrix reuse. Preserve each path's numerical/state identity and include phase
handoff costs before making it an automatic choice.

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

The August 2026 [bounded-program memory allocation preprint](https://arxiv.org/html/2608.14471v1)
identifies fixed-shape neural networks as a consumer of lifetime-based planning. Its tight storage
bounds allow defragmentation and control-flow expansion. For the current sequential graph, a small
fixed reuse is sufficient: Prism source shares attention projections with FFN up activations, shares
their gate storage, and convolves recurrent QKV in place after each raw element's last use.

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

The [Swift census](../config/swift.pq2-census.json) is ternary, but generic PQ2 code 3 means **+2**.
Ternary-only selection needs validated eligibility and fallback. For arbitrary signed i8
activations, including -128, a 128-trit dot is bounded by 16,384; generic PQ2 raises the bound to
32,768. Bound tables, partial sums and corrections independently with wide accumulators. SIMD
wrapping arithmetic is not checked by Rust overflow flags.

The [SIMD contract](simd-plan.md) defines packed controls. Validated immutable representations can
avoid repeated canonical decoding, but arbitrary public tensors still need rejection checks. The
identity `dot(code - 1, q) = dot(code, q) - sum(q)` also permits a candidate using `Q8KBlock.sums`;
this formulation and lane subtraction share the same scale contract.

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

[Saha and Ye](https://proceedings.mlr.press/v235/saha24a.html) prove FlashAttention I/O-optimal up
to constants for fast memory `M >= d^2` in their model. This motivates cache tiling and reduced
traffic; it is not a hardware forecast or an unconditional subquadratic exact algorithm.
[FlashAttention](https://arxiv.org/abs/2205.14135) is an attention-tiling technique subject to the
rounding boundary below.

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

Generic inverse-state rollback is unsafe mathematically: the transition `alpha (I - beta k k^T)` has
eigenvalues `alpha` and `alpha (1 - beta ||k||^2)` and can be singular/ill-conditioned. Floating
gates can underflow/saturate, and convolution overwrites history. Use snapshots, saved factors and
forward reconstruction with numerical checks.

### 2026 state-traffic and verification research

The [hybrid-state
catalog](research-register.md#hybrid-recurrence-prefix-caching-and-speculative-decoding) records
versions reviewed through **2026-10-29**. These are candidates, not selected-runtime features.

| Mechanism | Swift proof obligation |
| --- | --- |
| [KVBuffer](https://arxiv.org/html/2605.19049v1): base state plus buffered decays/keys/deltas, deferred writeback and chunk verification | Charge buffer reads, flush/materialization and precision conversion. Checkpoints must retain/materialize every pending update plus convolution history; buffered state cannot masquerade as materialized state. SGLang/Qwen3-Next four-L40S, FP32-state/FP16-vector results do not establish Prism bitwise replay. |
| [TreeWY](https://arxiv.org/html/2608.20961v2): ancestor-masked triangular WY equations retain pseudo-values and reconstruct the accepted path | Real-arithmetic exactness is not BF16 bit identity. B200/vLLM greedy evaluation disables prefix caching; wide trees can lose latency. Qualify topology, attention causality, convolution, failed commit and prefix-cache integration independently; smaller state does not prove faster batch-one decode. |

### Trained parallel proposals and full-state verification

| Proposal/verification route | Swift boundary |
| --- | --- |
| DFlash: diffusion block drafter conditioned on target hidden features, AR target verification | Nonzero-temperature/GPU results need compatible tokenizer, ternary weights, feature interface, adapter and CPU execution. Admit drafter weights/KV, retained target features and verification scratch; compare complete cycles. |
| DFlash 2: adjacent-candidate scoring, path selection and block-local convolution | The Qwen3.8-27B drafter is a lead, not Swift/LoRA evidence. This is an author report with released drafters, not another peer-reviewed paper. Expose actual conditional proposal probabilities for rejection sampling; coherent-path selection alone does not preserve the target distribution. Learned components require provisioned artifact identities. |
| VeriCache: compressed-KV drafting plus separately retained full-KV verification | Its identical-output claim is greedy except hardware nondeterminism; sampling needs correction. Multi-request GPU/transfer overlap does not transfer to this one-request CPU runner. Full plus compressed KV may increase laptop RAM, and KV-only verification omits GDN/convolution recovery. |

### Certified sparse attention is a separate approximation

The [approximation
catalog](research-register.md#approximation-quantization-theory-and-numerical-analysis) records
sources and [T9](research-register.md#t9-per-call-certificates-need-a-run-wide-failure-budget)
defines run-wide failure budgeting.

| Method | Guarantee boundary |
| --- | --- |
| vAttention | Important-token selection plus tail sampling gives relative operator-output bounds. Practical pilot/CLT budgets differ from conservative variants; `(epsilon, delta)` is neither exact output nor an answer-wide certificate. |
| [Certified Sparse Attention via Perturbed Retrieval](https://arxiv.org/html/2610.24991v1) | Anytime-valid per-call certificates require exact candidates and fresh independent perturbations per decode step. Budget failure across heads/layers/steps. Context-independent value-read bounds still include linear compressed-key scans; AVX-512 VNNI reference retains full 16-bit KV plus indexes (40% extra storage), not a smaller exact Snapdragon cache. |
| DoSA | Conservative score bounds safely skip entries outside an existing sparse indexer's Top-K, including numerical bounds/recovery. Exact indexer selection does not equal dense attention; Swift lacks the required learned indexer/artifacts. Transfer bounded early rejection with full fallback, not silent dense-token omission. |

Both certified approximation methods need references consistent with Prism's FP16 normalized-
probability rounding before their bounds transfer.

## Compression and approximation constraints

An independent 128-trit block has `3^128` possibilities and needs `ceil(128 log2(3)) = 203` bits:
**26 whole code bytes**, already used by PTQ1 plus its unchanged two-byte scale. This rules out a
general whole-byte saving per independent block, not correlated/cross-block coding. See the [packing
decision](swift-memory-options.md#packing-decision).

| Research | Constraint |
| --- | --- |
| [Qualcomm multiplication-only GDN inverse](https://arxiv.org/abs/2606.06034) | Truncated Neumann/residual correction is a separate approximation after faithful chunkwise GDN. Phone NPU results do not qualify Windows X Elite. |
| [Near-I/O-optimal approximate attention](https://arxiv.org/abs/2605.23751) | Transfer improvements use approximation; require separate quality/resource qualification. |
| [Signed-digit LUT KV](https://arxiv.org/abs/2608.03229) | Changes KV representation/precision for a hardware datapath; separate from exact FP16-bit storage. |
| [Gated DeltaNet-2](https://arxiv.org/abs/2605.22791), [Sherry](https://arxiv.org/abs/2601.07892), [ReaDy](https://arxiv.org/abs/2610.20503) | Changed gates, sparsity or trained speculative components require compatible artifacts, not inference-only substitution. |

Strict triangularity gives a finite exact Neumann expansion, but low-order error needs norm/entry
bounds. Spectral radius zero does not bound intermediate growth. Check signed Gram entries/ranges
before applying approximation arguments.

## Validation and promotion

The [validation policy](validation.md) applies independently to each change. Exact integer/storage
changes need bitwise fixtures, scales, malformed-input and -128/+2 cases. Reordered floating work
needs predeclared logits/KV/GDN and full- continuation tolerances. Both need checkpoint isolation,
edits, cancellation, failed recovery/reset and memory-budget checks.

Local builds and focused correctness checks are allowed. Full suites, benchmarks and
performance-profile captures remain CI-only. Promotion needs same-artifact, same-adapter comparisons
and standard LoRA TPS/quality gates on the target laptop. Synthetic parity, smaller files and other
devices' results do not establish acceptance; paper speedups cannot be multiplied.
