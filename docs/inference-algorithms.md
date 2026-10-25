# Inference algorithms and mathematical constraints

This maps algorithm candidates to the [Rust roadmap](native-rust-engine.md)
and [advanced experiments](../PLAN.md#9-speculative-decoding-and-advanced-optimization).
Baseline PQ2/PTQ1 NEON, state-only prompt ingestion and physical FP16 KV storage
are implemented. The candidates below remain unqualified; none establishes a
Snapdragon speedup. Preserve pinned Prism controls, exact artifacts and existing
promotion gates. Pin implementation sources/licenses during R0 before reuse.

## Current implementation and next work

| Task | Current state | Remaining work |
| --- | --- | --- |
| R4.3: prompt ingestion | `Decoder::ingest`/Qwen35 skip output-only work for intermediate uncached prompt tokens; final tokens and prefix hits retain logits | Full-model correctness/performance qualification; retain computed-state checks, cancellation and rollback |
| R4.4: activation preparation | Compatible sibling projections share scoped preparation, including transforms and quantization | Qualify full-model allocation and performance benefit; retain input, basis/permutation and quantizer compatibility checks |
| R4.5: KV storage | Full-attention state retains exact binary16 bits in `Vec<u16>` and widens on reads; GDN/convolution stays FP32 | Qualify process-memory/throughput benefit; retain capacity-based mixed-state accounting |
| R3 / S3: packed decode | NEON PQ2/PTQ1 and odd-group PQ2 have exact synthetic scalar comparisons | Compare exact ternary LUTs under unchanged quantizers/scales and generic PQ2 semantics |
| R4.6 / S5: prefill | Bounded 1–256-token block scheduling retains sequential graph arithmetic and whole-request rollback | Compare tiled matmul, Vec-LUT and chunkwise GDN under causal/state and numerical gates |
| R5.2: checkpoints | Snapshots share sealed immutable KV pages and copy mutable tails plus complete convolution/GDN state | Qualify full-model process-memory benefit while retaining conservative snapshot budgets |
| M6: speculation | Native decoder supports greedy and explicit seeded reference sampling, without a block verifier | Qualify Prism sampler/RNG parity and verification before suffix proposals and compact recurrent recovery; keep separate from ordinary decoding |

Source map: [runtime](https://github.com/symbiogenesis/prism-inference/blob/main/src/runtime.rs),
[decoder API](https://github.com/symbiogenesis/prism-inference/blob/main/src/lib.rs),
[graph/state/transforms](https://github.com/symbiogenesis/prism-inference/blob/main/src/graph.rs),
[kernels](https://github.com/symbiogenesis/prism-inference/blob/main/src/kernels.rs) and
[codecs](https://github.com/symbiogenesis/prism-inference/blob/main/tools/ternary/src/lib.rs).

Retained scratch is storage reuse, not proof that repeated computation or
model-level allocation costs are removed; R4.2 remains open. Qwen35 state-only
ingestion deliberately omits unused output/logit checks while validating every
computed state value. Block prefill must also preserve cancellation/backpressure
bounds and atomic commit.

## Packed lookup kernels

### Exact ternary tables for decode

[Bitnet.cpp, section 3.2](https://arxiv.org/html/2502.11880v1) distinguishes exact
**TL1_1/TL2_1**, using INT16 sums/two byte lookups, from approximate INT8-table
**TL1_0/TL2_0**. Audit the [official implementation](https://github.com/microsoft/BitNet)
as a technique, preserving this engine's `q8k_into`/`q8_into`, 128-weight scales,
256-activation Q8_K groups, 32-activation Q8_0 groups and floating scale/add order.
Stock BitNet quantization is not this engine's specification.

The [Swift census](../config/swift.pq2-census.json) is ternary, but generic PQ2
code 3 means **+2**. Ternary-only selection needs validated eligibility and fallback.
For arbitrary signed i8 activations, including -128, a 128-trit dot is bounded
by 16,384; generic PQ2 raises the bound to 32,768. Bound tables, partial sums and
corrections independently with wide accumulators. SIMD wrapping arithmetic is
not checked by Rust overflow flags.

[SIMD S3](simd-plan.md#delivery-sequence-and-exits) owns packed controls. Validated
immutable representations can avoid repeated canonical decoding, but arbitrary
public tensors still need rejection checks. The identity
`dot(code - 1, q) = dot(code, q) - sum(q)` also permits a candidate using
`Q8KBlock.sums`; compare with lane subtraction under the same scale contract.

### Vec-LUT for multiple prompt tokens

[Vec-LUT](https://arxiv.org/html/2512.06443v2) groups lookups across tokens and
streams cache-sized table tiles, using bounded INT16 partials and INT32
accumulation. Its [implementation](https://github.com/OpenBitSys/vlut.cpp) is an
R4.6/S5 lead after block prefill exists. Retain our quantizers/scales, rederive
bounds for all accepted inputs and account for layouts, repacking, retained
copies and scratch. Published platforms do not qualify Windows ARM64 or Swift.

## Chunkwise Gated DeltaNet prefill

[Gated Delta Networks](https://arxiv.org/html/2412.06464v3) derives chunkwise
computation with compact WY/UT structure; the [official FLA implementation](https://github.com/fla-org/flash-linear-attention/blob/main/fla/ops/gated_delta_rule/chunk.py)
is an algebra reference. Retain scalar recurrent execution as the decode control
and qualify chunk sizes per backend/phase.

For column keys/values,
`S_t = alpha_t S_(t-1) (I - beta_t k_t k_t^T) + beta_t v_t k_t^T`.
Chunking preserves the real-arithmetic model while changing floating evaluation
order. Declare state/logit tolerances and test long prefixes/restoration.
Preserve normalization, `head % ssm_keys`, convolution history, grouped/sign
transforms and FP32 recurrent state.

## Attention locality and the I/O bound

[Saha and Ye](https://proceedings.mlr.press/v235/saha24a.html) prove FlashAttention
I/O-optimal up to constants for fast memory `M >= d^2` in their model. This
motivates cache tiling and reduced traffic; it is not a hardware forecast or an
unconditional subquadratic exact algorithm. [FlashAttention](https://arxiv.org/abs/2205.14135)
is a lead for R4.6 and Adreno G4/R6.

Scalar nonflash attention rounds **globally normalized probabilities to FP16
before multiplying V**. Ordinary online-softmax merging changes that operation.
First consider grouped-head tiling and contiguous V reads retaining reduction
order. A rounding-compatible normalization may retain scores or recompute them;
a conventional Flash path needs a distinct numerical contract and qualification.
Do not share checkpoints across incompatible backend identities.

## Hybrid cache reuse and speculative state

[Unified Radix Cache](https://www.lmsys.org/blog/2026-08-11-unified-radix-cache/)
distinguishes reusable full-attention KV paths from recurrent state valid at an
exact prefix checkpoint. Apply that boundary to R5.2. With at most eight saved
prefixes, avoid payload copies before replacing linear lookup. Budget unique
pages, mutable tails, metadata, rollback and every retained GDN snapshot;
shared ownership cannot create mutable cross-snapshot aliases.

[SuffixDecoding](https://arxiv.org/abs/2411.04975) proposes tokens from prompt/history
suffixes. [SpecLA](https://arxiv.org/abs/2607.16673) retains verification factors
and delays commit to reconstruct accepted recurrent states. Consider these only
after a block verifier exists. SpecLA's trained drafter/H100 evidence and reported
FP32 state differences do not establish Bonsai compatibility or exact replay.

Stochastic speculation needs proven acceptance/rejection correction, as in
[Leviathan et al.](https://proceedings.mlr.press/v202/leviathan23a.html); greedy
agreement does not qualify creative sampling. Retain request rollback even if
candidate recovery becomes cheaper.

Generic inverse-state rollback is unsafe mathematically: the transition
`alpha (I - beta k k^T)` has eigenvalues `alpha` and
`alpha (1 - beta ||k||^2)` and can be singular/ill-conditioned. Floating gates
can underflow/saturate, and convolution overwrites history. Use snapshots,
saved factors and forward reconstruction with numerical checks.

## Compression bound and deferred experiments

An independent 128-trit block has `3^128` possibilities and needs
`ceil(128 log2(3)) = 203` bits: **26 whole code bytes**, already used by PTQ1
plus its unchanged two-byte scale. This rules out a general whole-byte saving
per independent block, not correlated/cross-block coding. See the
[PTQ1 decision](swift-memory-options.md) and [BITCOS retirement](bitcos-retirement.md).

| Research | Constraint |
| --- | --- |
| [Qualcomm multiplication-only GDN inverse](https://arxiv.org/abs/2606.06034) | Truncated Neumann/residual correction is a separate approximation after faithful chunkwise GDN. Phone NPU results do not qualify Windows X Elite. |
| [Near-I/O-optimal approximate attention](https://arxiv.org/abs/2605.23751) | Transfer improvements use approximation; require separate quality/resource qualification. |
| [Signed-digit LUT KV](https://arxiv.org/abs/2608.03229) | Changes KV representation/precision for a hardware datapath; separate from exact FP16-bit storage. |
| [Gated DeltaNet-2](https://arxiv.org/abs/2605.22791), [Sherry](https://arxiv.org/abs/2601.07892), [ReaDy](https://arxiv.org/abs/2610.20503) | Changed gates, sparsity or trained speculative components require compatible artifacts, not inference-only substitution. |

Strict triangularity gives a finite exact Neumann expansion, but low-order error
needs norm/entry bounds. Spectral radius zero does not bound intermediate growth.
Check signed Gram entries/ranges before applying approximation arguments.

## Validation and promotion

Implement one change at a time under [validation policy](validation.md).
Exact integer/storage changes need bitwise fixtures, scales, malformed-input and
-128/+2 cases. Reordered floating work needs predeclared logits/KV/GDN and full-
continuation tolerances. Both need checkpoint isolation, edits, cancellation,
failed recovery/reset and memory-budget checks.

Local builds and focused correctness checks are allowed. Full suites, benchmarks
and performance-profile captures remain CI-only. Promotion needs same-artifact,
same-adapter comparisons and standard LoRA TPS/quality gates on the target laptop.
Synthetic parity, smaller files and other devices' results do not establish
acceptance; paper speedups cannot be multiplied.
