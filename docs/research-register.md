# Inference research and proof register

Research cutoff and last source review: **2026-10-29**.

This curated index connects mathematical results, algorithms, implementations and
engineering reports to Swift Bonsai 2. It records applicability, assumptions and
unresolved checks, not implementation approval or a complete literature search.
Derivations are reviewable arguments, not machine-checked proofs. Topic contracts
remain authoritative; [PLAN.md](../PLAN.md) alone tracks delivery work.

Jump to [priorities](#priorities-for-this-model-and-hardware),
[proof obligations](#mathematical-results-and-proof-obligations),
[papers](#research-catalog), [implementations](#implementation-sources-to-inspect),
[reports/HN](#engineering-reports-and-hacker-news-leads),
[experiments](#turning-a-lead-into-an-experiment) or [upkeep](#keeping-the-register-current).

## Existing evidence and project scope

Use the [documentation index](README.md) for topic contracts. The target is native
Windows ARM64 on Snapdragon X/X2, initially the 32 GiB X1E-78-100. Swift's `qwen35`
graph has **16 full-attention and 48 Gated DeltaNet/convolution layers**: one
8,192-token conversation, default packed PQ2 weights, FP16 full-attention KV and
FP32 recurrent state; PTQ1 is a separate candidate. [Verified metadata](../config/swift.metadata.json)
and locks take precedence over similarly named paper models.

Kernels belong in [Prism Inference](https://github.com/symbiogenesis/prism-inference);
Runner owns provisioning, admission, orchestration and [native Rust presentation](ui-architecture.md).
Ideas from Python/C++/GPU systems do not require their UI, broad API or backend.

Evidence labels: **theorem** = published result under stated assumptions (preprints
can remain unreviewed); **derivation** = local algebra/counting; **paper** = authors'
algorithm/evaluation; **implementation** = inspectable source requiring pin/license/
conformance review; **report** = author/maintainer account. HN is a discovery/discussion
source; trace technical claims to originals.

## Priorities for this model and hardware

These are applicability judgments, not measured rankings. Check the pinned Prism
implementation before proposing existing work again; PLAN owns execution order.

| Priority | Candidate | Decisive question |
| --- | --- | --- |
| 1 | Exact packed decode: BitNet/T-MAC tables, NEON layouts | Can unpack/dot savings retain quantizers, scales, integer bounds and floating reduction order without expansion? |
| 2 | Vec-LUT/cache-tiled batched projections | Does weight reuse across prompt tokens repay LUT construction, packing and scratch? Sequential blocks alone do not reuse tiles. |
| 3 | Faithful chunkwise GDN | Can matrix prompt work preserve grouped/sign transforms within declared long-prefix/restore tolerances? |
| 4 | Immutable KV pages/sparse recurrent checkpoints | Which allocation dominates: KV, recurrent snapshots, transaction scratch or mappings? |
| 5 | Grouped-head KV reuse/bounded attention scratch | Preserve FP16 probability rounding, or declare a distinct numerical/state identity. |
| 6 | Suffix/prompt-lookup speculation | Does creative-prose acceptance pay for block verification and recurrent recovery without second-model weights? |
| 7 | Fusion, persistent workers, prepared plans | Does complete-request profiling justify more scheduling complexity? |
| 8 | Adreno/Hexagon phase placement | Include signed Windows access, packed support, mappings, state handoff, duplicate weights and sustained power. |
| Separate approximation lane | KV quantization, approximate attention/GDN, pruning, trained drafters | Compare changed precision/computation/artifacts as distinct profiles. |

## Mathematical results and proof obligations

### T1. Packing limits and the difference between sparse and small

**Derivation:** an independent 128-symbol ternary block has `3^128` strings, so an
injective fixed-length encoding needs `ceil(log2(3^128)) = 203` bits, or 26 bytes.
PTQ1's 26 code bytes plus unchanged two-byte scale reach that independent,
byte-aligned bound. Correlated/variable-length/cross-block coding remains possible
but must pay for indexes, padding, decoding and random access.
[Swift's 32.76154% zeros](swift-memory-options.md#packing-decision) miss BITCOS's
37.5% ideal break-even. Zeros do not skip dense-loop work; sparsity must repay
indexes and irregular execution.

### T2. Traffic, arithmetic and the whole-request ceiling

[Roofline](https://escholarship.org/uc/item/5tz795vq) (Williams/Waterman/Patterson, 2009)
models `P <= min(P_peak, B*F/Q)`, hence `t >= max(F/P_peak, Q/B)` for work `F` and
traffic `Q` at a chosen memory boundary. If each decode step streams `W` weight
bytes there, `tokens/s <= B_effective/W`. This does not prove achieved peak or
advertised-bandwidth saturation; cache/batching/speculation change traffic, and
GGUF length does not measure bytes/token. Prefill can amortize tiles across tokens.

For fraction `f` sped up by `s`, [Amdahl's relation](https://doi.org/10.1145/1465482.1465560)
is `speedup = 1/((1-f)+f/s)` if other work is unchanged. A 42x drafting microbenchmark
cannot imply 42x inference. Overlap retains the slowest stage's service-time limit;
bounded queues trade latency against memory rather than create compute.

### T3. Exact low-bit dots need bounds at every intermediate

**Derivation:** signed i8 `q_i` (including `-128`) and ternary `w_i` give
`abs(sum(w_i*q_i)) <= 128*n`; at `n=128`, 16,384 fits i16. Generic PQ2 also permits
`+2`: conservative absolute bound 32,768, tight final range `[-32768,32512]`, still
i16. That does not certify LUT construction, partial sums or bias corrections.
For codes `0..3`, `sum((code_i-1)*q_i) = sum(code_i*q_i)-sum(q_i)`, but the
uncorrected 128-term dot spans `[-49152,48768]` and needs wider storage.
Preserve grouping/scale order; checked scalar Rust does not check wrapping SIMD.
[Bitnet.cpp](https://arxiv.org/abs/2502.11880) distinguishes exact INT16 and approximate
INT8 tables; [T-MAC](https://arxiv.org/abs/2407.00088) also quantizes tables and may
approximate aggregation. LUTs are not automatically exact Q8_K/Q8_0 replacements.

### T4. Attention I/O optimality has a computation model

[Saha/Ye, ICML 2024](https://proceedings.mlr.press/v235/saha24a.html) prove
FlashAttention I/O-optimal up to constants for fast memory `M >= d^2` in their
model, with separate small-memory results: no universal latency optimum or
unconditional exact subquadratic attention follows.
[FlashAttention](https://arxiv.org/abs/2205.14135) merges tiled online-softmax
statistics `(m_a,l_a,o_a)`/`(m_b,l_b,o_b)` by rescaling sums to `m=max(m_a,m_b)`.
That real-arithmetic identity does not reproduce Prism's globally normalized
probabilities rounded to FP16 **before V**. Preserve that boundary or declare
separate tolerances and state identity.

### T5. Chunking recurrence and recovering rejected tokens

[Gated Delta Networks](https://arxiv.org/abs/2412.06464v3) derives triangular/WY
chunking. In column-vector convention,
`S_t = alpha_t*S_(t-1)*(I-beta_t*k_t*k_t^T) + beta_t*v_t*k_t^T`.
Real-arithmetic equivalence permits parallel prompt work; reordered floats still
need logits/state and long-continuation checks.

**Derived rollback limit:** transition eigenvalues `alpha_t` and
`alpha_t*(1-beta_t*||k_t||^2)` can be zero/near-zero, making the transition
singular or inversion unstable; convolution overwrites history too. Retain snapshots or saved factors
with forward reconstruction and atomic commit. For strictly triangular size-`c`
`L`, `L^c=0` and `(I-L)^(-1)=I+L+...+L^(c-1)` exactly, but short-truncation error
depends on powers/norms of `L`, not spectral radius zero alone. This constrains
[multiplication-only approximations](inference-algorithms.md#chunkwise-gated-deltanet-prefill).

### T6. Exact speculative sampling preserves a distribution

[Leviathan/Kalman/Matias, ICML 2023](https://proceedings.mlr.press/v202/leviathan23a.html):
for normalized target `p`, proposal `q`, propose `x~q`, accept with
`min(1,p(x)/q(x))`, otherwise sample normalized `(p-q)_+`. Accepted mass `min(p,q)`
plus rejected correction recovers `p`. Apply conditionally at each accepted prefix
with the actual temperature/top-k/top-p/repetition policy. Distributional equality
does not imply seed/draw-order identity; greedy agreement is narrower.
Recurrent/convolution state must match the accepted prefix, not just a KV watermark.
Under independent constant acceptance `a`, `k` proposals plus a target token give
expected progress `1+a+...+a^k`; charge drafting/verification/recovery. Creative
prose acceptance can differ sharply from repeated code/text.

### T7. State size, lifetimes and prefix identity

**Derived payload** from [metadata](../config/swift.metadata.json):
`16*8192*4*(256 K+256 V)*2 = 536,870,912 bytes = 512 MiB` full-attention KV at 8K.
Only the 16 full-attention layers count here; the 48 recurrent layers separately
retain [149.625 MiB FP32 matrix/convolution state](native-rust-engine.md).
Capacity, alignment, snapshots, scratch and allocator metadata are extra.
Physical accounting counts unique pages and independent recurrent snapshots,
including transient restore/rollback. Admission conservatively charges every
referenced sealed page per snapshot without relying on sharing; this survey
changes neither budget nor policy.

For deterministic execution, exact tokens/reset state/positions plus model, adapter,
transforms, precision and execution contract determine exact prefix state. A recurrent checkpoint cannot
stand for a shorter position; immutable sharing saves copies only with independent
mutable tails. Simultaneously live buffers bound storage; reuse waits for the
last use, including asynchronous device work. [Memory Allocation for Constant-Bounded Programs](https://arxiv.org/abs/2608.14471v1)
(2026-08-14 preprint) allows defragmentation/control-flow transformations beyond
a fixed arena. Existing [scratch reuse](inference-algorithms.md#scratch-lifetimes)
saves 152 KiB nominal payload, not a proved process peak.

### T8. Floating-point and approximation claims

[Goldberg](https://docs.oracle.com/cd/E19957-01/806-3568/ncg_goldberg.html) explains
reassociation, FMA, rounding and underflow differences. Integer dots, decoded
weights, logit tolerances, sampled-token identity and task quality are distinct
guarantees; none implies all others. KV compression changes attention; recurrent
quantization can accumulate prefix-wide error. Instantiate metric, Q/K norms,
randomness/failure probability, dimension and horizon before using a bound:
head/vector error does not certify creative-answer quality.

### T9. Per-call certificates need a run-wide failure budget

**Derivation:** conditional per-call failure bounds `delta_i` imply
`P(any failed certificate) <= sum(delta_i)` by the union bound. For at most `K`
calls, `delta_i <= delta_total/K` suffices without independence, but guarantees
must survive adaptive queries. Reused randomness can violate assumptions;
[perturbed retrieval](https://arxiv.org/abs/2610.24991v1) uses fresh perturbations
per decoding call. An output certificate still certifies neither token nor answer.

### T10. Recurrent quantization error depends on the trajectory

**Derivation:** for the same affine transitions `A_t`, state difference obeys
`E_t=E_(t-1)*A_t+R_t`, where `R_t` is local update error. Submultiplicativity gives
`norm(E_T) <= norm(E_0)*product(norm(A_j)) + sum_i(norm(R_i)*product_(j>i)(norm(A_j)))`.
This conditional bound does not establish uniform stability when perturbed hidden
states change future gates. Small one-step MSE misses coherent persistent error.
[Low-rank bypass/directional rounding](https://arxiv.org/abs/2610.15709v1) and
[decay-aware precision](https://arxiv.org/abs/2608.27513v2) address it experimentally,
without proving exact FP32 continuation.

### T11. Quantization search theorems optimize a stated objective

[CoS-VQ](https://arxiv.org/abs/2610.37336v1) proves surrogate-search properties over
finite candidate pools, not a global quantized-model optimum.
[YAQA](https://arxiv.org/abs/2505.22988v3) bounds second-order error under Hessian,
rounding and incoherence assumptions, not unconditional nonlinear KL/answer error.
[FastKron](https://arxiv.org/abs/2608.06291v2) proves core rounding equivalence and
reduced depth/work in its arithmetic model, excluding other offline preparation
and token generation. Retain objective, feasible set and included work for every
"optimal" or "equivalent" claim.

### T12. A logit-error bound preserves greedy choice only with a margin

**Derivation:** absolute per-logit error `epsilon` can shrink the top-two gap
`Delta` by `2*epsilon`, so `Delta > 2*epsilon` retains a unique greedy winner.
Arbitrarily small errors can swap near ties. Maximum-logit difference alone does
not prove token identity; greedy ranks do not prove equal sampling probabilities.
Reordered kernels/quantizers still need sampler and continuation checks.

## Research catalog

Dates below identify the cited paper version, venue or author report. Mutable
repositories are observations as of the review date, not immutable evidence.

### Packed projections, CPU kernels and compression

| ID / source and date | Result and applicability | Disposition |
| --- | --- | --- |
| L1: [Bitnet.cpp](https://arxiv.org/abs/2502.11880), 2025-02-17 | Exact versus approximate ternary lookup variants; useful for packed decode without model-wide expansion. See T3 for accumulator and quantizer obligations. | High-priority candidate; some packed NEON controls already exist. |
| L2: [BitNet CPU optimization](https://github.com/microsoft/BitNet/blob/main/src/README.md), [announcement](https://github.com/microsoft/BitNet) 2026-01-15 | Recent I2_S GEMM/GEMV tiling and parallel activation processing amortize unpacking across tokens. Optional embedding quantization is a separate artifact change. | Implementation candidate; match the actual format and ARM arithmetic. |
| L3: [T-MAC](https://arxiv.org/abs/2407.00088v2), v2 2025-03-25, EuroSys 2025 | Bit-plane tables, NEON lookup layouts and cache/register tiling. Table quantization and optional fast aggregation are approximate in the paper; layouts can be studied independently. | High-priority technique reference; exactness must be specified separately. |
| L4: [Vec-LUT](https://arxiv.org/abs/2512.06443v2), v2 2026-04-14, MobiSys 2026 | Multi-token table entries, contiguous token results and cache-sized LUT tiles with INT16 partials/INT32 accumulation. Charges LUT construction and layouts against weight reuse. | High-priority batched-prefill candidate. Native Windows ARM64 remains unqualified. |
| L5: [PolyQ](https://arxiv.org/abs/2607.14618), 2026-07-16, paper lists ICCAD 2026 acceptance | Mixed per-channel precision with compile-time clustering/permutation merging and SIMD/LUT kernels. Offline compatible permutations could remove runtime work, but its quantizer changes weights. | Layout candidate; quantization requires new artifacts. |
| L6: [FluxBin](https://arxiv.org/abs/2608.15602), 2026-08-16 preprint | LUT/PTQ co-design uses binary decomposition, saliency-aware bases and fused scaling. CUDA/A100 evidence addresses a different representation. Linked code repository returned 404 during review. | Deferred GPU/artifact research; code availability unverified. |
| L7: [Sherry](https://arxiv.org/abs/2601.07892), 2026-01-12; [ACL 2026 paper](https://aclanthology.org/2026.acl-long.513.pdf) | Trained fine-grained sparsity constrains allowed patterns to reach 1.25 bits/weight. Does not encode arbitrary existing ternary blocks below T1's bound. | Requires new trained artifacts. |
| L8: [FairyFuse](https://arxiv.org/abs/2604.20913), 2026-04-22 preprint | Fused masked-add/subtract kernels for a particular widely-linear ternary model on AVX-512. Fusion concepts transfer more readily than its model or instruction sequence. | Deferred; different model/ISA. |
| L9: [ShiftAdd ggml proposal](https://github.com/ggml-org/llama.cpp/discussions/31051), 2026-10-23 | Recent sign-plane representation proposal. The author's CPU results trail conventional quants; faster layout/LUT work is still proposed. | Unmerged watch item; retain the negative result. |
| L10: [Pushing the Envelope of LLM Inference with Ultra-Low-Bit Quantized Models](https://arxiv.org/abs/2508.06753v3), v3 2026-08-27 | Interleaved two-bit weights unpack directly into VNNI's consumption layout; shape-specific kernels remove redundant rearrangement. | High-priority layout idea, AVX2/VNNI/Xe2 evidence. Base Bonsai results are not Swift; derive an independent NEON layout. |
| L11: [ELUTQ](https://arxiv.org/abs/2510.19482v4), v4 2026-02-01; [code](https://github.com/Nkniexin/ELUTQ) | Hierarchical nonuniform quantization remains executable through bit-serial LUTs. | New artifacts; RK3588/M2 CPU evidence is useful but does not establish PQ2/PTQ1 or Windows support. |
| L12: [Shannonic](https://proceedings.mlsys.org/paper_files/paper/2026/hash/96f39c8de84678cb2a908cd52bfd7819-Abstract-Conference.html), MLSys 2026 | Lossless range-partitioned ANS reduces table state; its theorem compares specified probability-allocation/offset losses. | Not universally better than ideal ANS. Communication-bound wins do not prove faster local matvec; charge independent streams and decode work. |
| L13: [QTEA](https://arxiv.org/abs/2609.00224v2), v2 2026-09-02, EMNLP 2026 acceptance listed; [code](https://github.com/Intelligent-Microsystems-Lab/QTEA) | Ternary bulk plus salient-column 1:4 residuals and columnwise refinement. | New PTQ artifact/residual memory. GPU/CUDA Graph results and architecture estimates are not post-layout silicon or NEON evidence. |
| L14: [HARP](https://arxiv.org/abs/2605.29843v2), v2 2026-09-02; [code](https://github.com/brain-lab-research/HARP) | Learned sparse butterfly/mixed-radix orthogonal transforms replace fixed preprocessing. | New fitted artifacts. Full-precision equivalence does not preserve stored quantized Swift; fitting, parameters and transforms count. Throughput evidence uses RTX 5080. |
| L15: [Leech Lattice Vector Quantization](https://arxiv.org/abs/2603.11021v2), v2 2026-07-07 | Structured 24D indexing avoids materializing a huge codebook. | New-artifact research; sphere-packing optimality is not universal model-quantization optimality. |
| L16: [Tetra](https://arxiv.org/abs/2609.35465v1), 2026-09-28; [Rust/CUDA code](https://github.com/pjmalandrino/llvq); [author report](https://huggingface.co/blog/Pier-Jean/llvq-init), 2026-08-29 | Fused lattice reconstruction distinguishes serialized bitrate from bytes read by execution. | L40S/new-artifact evidence includes quality losses. Independent implementation; Rust source does not qualify Windows Arm. |
| L17: [XOR-Trellis](https://arxiv.org/abs/2610.00432v1), submission history 2026-09-30 | Parity/XOR structure and a small FP4 palette replace a larger state-to-value lookup. | New trellis quantizer; gate-level analysis is not commodity-CPU end-to-end speed. |
| L18: [T-SAR](https://past.date-conference.com/proceedings-archive/2026/DATA/705.pdf), DATE 2026; [2025 preprint](https://arxiv.org/abs/2511.13676v1) | Activation/output-persistent LUT scheduling with reorganized SIMD hardware. | Requires new ISA/datapath support; modified gem5 results do not apply as a stock-Snapdragon software patch. |
| L19: [ExaGEMM](https://arxiv.org/abs/2607.14622v1), 2026-07-16, ICCAD 2026 acceptance listed | Cost/register-feasibility models prune low-bit layout/instruction design choices. | Hardware/compiler methodology; new ISA and simulation underlie reported gains. |
| L20: [BITCOS / Breaking the 1.58-bit Barrier](https://arxiv.org/abs/2609.16338v1), 2026-09-14 preprint | Zero-density-dependent lossless ternary representation. | Primary reference for the existing rejected layout: Swift misses its break-even versus same-scale PTQ1. See T1 and the packing decision. |

Useful counterexamples from upstream reports: [missing blocked ternary ARM
paths](https://github.com/ggml-org/llama.cpp/issues/27276) (2026-08-17),
[overlapping mapped/repacked copies](https://github.com/ggml-org/llama.cpp/issues/19578)
(2026-02-12), and [device-dependent KleidiAI results](https://github.com/ggml-org/llama.cpp/issues/27078)
(2026-08-14). The first two were closed as not planned and are not evidence
of implemented fixes. These reports motivate checking prefill separately,
charging both representations and avoiding a universal ARM-kernel preference.

### Hybrid recurrence, prefix caching and speculative decoding

| ID / source and date | Result and applicability | Disposition |
| --- | --- | --- |
| H1: [Gated Delta Networks](https://arxiv.org/abs/2412.06464v3), 2024 preprint, cited v3 | Chunkwise structured recurrence for parallel prompt computation; T5 records its real-arithmetic and recovery limits. | High-priority faithful prefill candidate; separate numerical qualification. |
| H2: [Sparse Prefix Caching for Hybrid and Recurrent LLM Serving](https://arxiv.org/abs/2605.05219v1), v1 submission history 2026-04-17 | Balanced placement is optimal for its uniform-overlap/worst-case setting; an exact `O(N*M)` DP minimizes expected recurrent suffix recomputation under its overlap model (`N` prefix tokens, `M` checkpoints). Resume exact state and recompute the suffix. | Exact-storage candidate for edited/branching histories. Append-only chat already benefits from retaining just the latest state. |
| H3: [Marconi](https://arxiv.org/abs/2411.19379), 2024-11-28 | Hybrid cache admission/eviction values saved computation relative to state footprint and reuse likelihood. | Candidate accounting policy; serving-scale gains do not predict local chat. |
| H4: [Unified Radix Cache](https://www.lmsys.org/blog/2026-08-11-unified-radix-cache/), 2026-08-11 | One prefix identity with separately valid full-attention, sliding-window and recurrent components. Exact recurrent checkpoints remain position-specific. | High-priority ownership reference. A small fixed cache may need no radix tree. |
| H5: [SGLang Unified Memory](https://www.sglang.io/blog/unified-memory), index dated 2026-10-22 | Shared capacity for KV and recurrent checkpoints; eviction accounts for all recoverable bytes. The reported returning-request gain compares eviction policies with unified allocation in both arms; four-chunk prefill removed it, with no stable overall-throughput gain. | Candidate accounting lesson. Qwen3.5-4B/RTX5090 evidence; this means shared GPU cache capacity, not CPU/GPU coherent memory. |
| H6: [SGLang HiCache](https://www.sglang.io/blog/hicache), index dated 2026-10-19 | Tiered cache restores complete hybrid/draft state when restore cost is less than recomputation. | Deferred tiering reference. Multi-H200 input-throughput results do not establish decode gains; local disk state adds storage/privacy policy. |
| H7: [SuffixDecoding](https://arxiv.org/abs/2411.04975v3), v3 2025-10-07, NeurIPS 2025 | Suffix-tree prompt/history proposals without a second model's weights. Acceptance depends strongly on repetition in the workload. | High-priority proposal control after a block verifier exists; T6 still applies. |
| H8: [Speculative sampling proof](https://proceedings.mlr.press/v202/leviathan23a.html), ICML 2023 | Conditional accept/reject correction preserves the target distribution. It does not promise seed-for-seed draw identity or recurrent recovery. | Mathematical prerequisite; see T6. |
| H9: [llama.cpp recurrent speculative checkpointing](https://github.com/ggml-org/llama.cpp/pull/19493), merged 2026-04-19 | Restore complete state and re-evaluate the accepted shorter batch on partial acceptance. Simple reference before compact-factor replay. | Implemented external reference; copying/recompute costs remain. |
| H10: [SpecLA](https://arxiv.org/abs/2607.16673v1), 2026-07-18 preprint | Saves compact verification factors and delays recurrent-state commit. H100/pure-GDN-1.3B/greedy evidence; one replay comparison has nonzero FP32 state difference despite equivalent-state wording. | Candidate after faithful block verification; no bitwise or stochastic Swift guarantee. |
| H11: [vLLM RecoverSSM PR #58863](https://github.com/vllm-project/vllm/pull/58863), opened 2026-09-26, open at review | Saves FP32 correction values, normalized keys and gates; replays accepted updates once, including convolution commit. Larger records affect page alignment/reuse; author notes short-draft slowdowns. | Unmerged implementation lead; useful together with SpecLA. |
| H12: [vLLM ReplaySSM PR #56466](https://github.com/vllm-project/vllm/pull/56466), opened 2026-09-11, draft at review | Checkpoint/ring-buffer prefix reconstruction and cursor management. Author reports unresolved mixed-history token differences (28/32 exact cases). | Experimental; correctness unresolved. |
| H13: [ReaDy](https://arxiv.org/abs/2610.20503v2), v2 2026-10-18; [code](https://github.com/StartLuxLabs/ReaDy) | Trained draft readers retrieve context from committed recurrent state. Qwen3.8-27B is close architecturally, but evidence uses BF16/B200/greedy and trained DSpark/readers. | Requires compatible trained artifacts, extra memory and separate Swift/LoRA/sampling checks. |
| H14: [CPU speculation / SubTernary](https://arxiv.org/abs/2610.28671v1), submission history 2026-10-17 | Persistent execution arena/threadpool and isolated speculative KV suffix. Xeon/Ubuntu/AVX2/Llama3.2/mostly greedy/fast-math study; no linked implementation found. Its masking argument does not prove acceptance correction. | Lower-confidence preprint concept; does not solve GDN/convolution rollback. |
| H15: [Tail-Replay](https://arxiv.org/abs/2608.30310v2), v2 2026-10-15 | Omits recurrent snapshots and approximately rebuilds GDN from a short recent suffix. Benchmark quality retention is not identical state or output. | Approximation lane only; cannot replace exact restoration silently. |
| H16: [Multiplication-only GDN inverse approximation](https://arxiv.org/abs/2606.06034v1), 2026-06-04 preprint | Truncated Neumann terms, masking and residual correction target NPU-friendly matmuls. T5 explains the missing step between nilpotence and a short accurate truncation. | Approximation after faithful chunking; phone NPU evidence does not qualify Windows X/X2. |
| H17: [Gated DeltaNet-2](https://arxiv.org/abs/2605.22791v1), 2026-05-21 technical report | Decouples erase/write gates and changes the recurrence, with a new chunkwise derivation. | Requires new architecture/artifacts; no inference-only substitution for Swift. |
| H18: [EAGLE-3](https://arxiv.org/abs/2503.01840), 2025-03-03; [code](https://github.com/SafeAILab/EAGLE) | Trains direct-token proposals using multi-layer target features rather than predicting only top-layer features. | Trained-drafter reference; target feature/adapter compatibility, extra memory and T6's sampling contract remain prerequisites. |
| H19: [DFlash](https://proceedings.mlr.press/v306/chen26bv.html), ICML 2026; [v2 2026-05-28](https://arxiv.org/abs/2602.06036v2); [code](https://github.com/z-lab/dflash) | Block diffusion drafts multiple tokens in one parallel pass conditioned on target features. Includes nonzero-temperature evaluations. | Requires a compatible trained drafter and hybrid block verifier. GPU results do not establish CPU break-even or acceptance for creative prose. |
| H20: [KVBuffer](https://arxiv.org/abs/2605.19049v1), 2026-05-18 preprint | Buffer keys/deltas/decays and defer full state writes; verify without a full state per draft position. | High conceptual priority. SGLang/Qwen3-Next on four L40S, FP32 state/FP16 buffers; no Prism bitwise-replay evidence. Charge reads/flush/materialization/conversion, and include pending updates plus convolution in checkpoints/commit. |
| H21: [TreeWY](https://arxiv.org/abs/2608.20961v2), v2 2026-10-22 preprint | Ancestor-masked triangular WY verification retains pseudo-values and reconstructs the accepted path. | Candidate after verifier infrastructure. B200/vLLM/greedy with prefix caching disabled; real-arithmetic equivalence is not BF16 bit identity. Reports token differences and wider-tree latency losses. Qualify attention causality, convolution and prefix-cache integration separately. |
| H22: [DFlash 2](https://inco.ai/blog/dflash2/), 2026-08-18 author report | Parallel candidate scoring, path selection and block-local convolution improve learned draft coherence. | Compatible trained-artifact research. Qwen3.8-27B support does not establish Swift/LoRA compatibility; expose actual conditional proposal probabilities. |
| H23: [VeriCache](https://arxiv.org/abs/2605.17613v1), 2026-05-17 preprint | Compressed KV drafts while separately retained full KV verifies. | Greedy-output qualification; sampling needs correction. Charge transfers, verification, complete hybrid state and full-plus-compressed storage, which may increase total RAM. Multi-request overlap does not establish laptop savings. |

The source history of H2 gives an April submission despite its May arXiv ID
and later HTML front matter. H14's submission history likewise supplies its date.
The dates here are those inspected on the primary pages, not inferred from IDs.

### Attention kernels and memory hierarchy

| ID / source and date | Result and applicability | Disposition |
| --- | --- | --- |
| A1: [Online Normalizer Calculation for Softmax](https://arxiv.org/abs/1805.02867), 2018-05-08 | Running max/rescaled normalizer reduces passes. Real-arithmetic invariant does not preserve every FP16 rounding boundary. | Foundational derivation; T4 applies. |
| A2: [FlashAttention](https://arxiv.org/abs/2205.14135), 2022; [attention I/O theorem](https://proceedings.mlr.press/v235/saha24a.html), ICML 2024 | Tiling avoids full attention-matrix materialization. Optimality depends on memory/computation assumptions, not a product benchmark. | Foundational theorem/technique; preserve or separately qualify attention arithmetic. |
| A3: [Flash-Decoding](https://crfm.stanford.edu/2023/10/12/flashdecoding.html), 2023-10-12 author report | Split a long KV sequence into independent local statistics and merge; targets single-query decode occupancy. | Candidate parallelism idea; GPU evidence and changed reductions are separate. |
| A4: [PagedAttention](https://arxiv.org/abs/2309.06180), 2023-09-12, SOSP 2023 | Virtual token pages, physical-block allocation and shared immutable prefixes reduce fragmentation/duplication. | Storage candidate; multi-request throughput is not a single-request speed guarantee. |
| A5: [FlashInfer](https://arxiv.org/abs/2501.01005), 2025-01-02 | Composable attention across cache layouts and planning separate from execution. | Implementation/planning reference; CUDA kernels are not an ARM backend. |
| A6: [FlashAttention-4](https://arxiv.org/abs/2603.05451), 2026-03-05 | Rebalances tiling/pipelining and non-matmul bottlenecks for B200/GB200. | Low direct-port priority; hardware-specific numerics and scheduling. |
| A7: [FlashAttention-V](https://arxiv.org/abs/2608.18656v1), 2026-08-19 preprint | Head packing and register locality for scalable vectors; includes real RISC-V board work. Large headline gains also include simulated comparisons with scalar controls. | Layout watch item; RVV/SVE is not 128-bit NEON or evidence of Windows ISA availability. |
| A8: [Reorienting the Vector Axis](https://arxiv.org/abs/2610.29489v1), 2026-10-21 preprint | Make queries the contiguous vector axis and keep polynomial coefficients out of vector registers to reduce spills. | Prefill/register-pressure inspiration; results use RISC-V with 2 KiB vector registers. |
| A9: [LLM in a Flash](https://machinelearning.apple.com/research/efficient-large-language), December 2023 | Flash/DRAM-aware weight access, windowing and bundling for models exceeding available DRAM. | Deferred capacity research. Existing ternary zeros, SSD latency/endurance and dense access do not automatically provide useful skipped reads. |

### Approximation, quantization theory and numerical analysis

These entries contain meaningful theoretical ideas, but do not authorize
replacing exact KV/state or re-quantizing the selected model by default.

| ID / source and date | Guarantee or concept | Applicability and limits |
| --- | --- | --- |
| Q1: [QuIP](https://arxiv.org/abs/2307.13304), 2023-07-25; [code](https://github.com/Cornell-RelaxML/QuIP) | Quantization analysis uses incoherence and a quadratic proxy involving weight/Hessian geometry. | Basis for transform research; proxy error is not a downstream creative-quality theorem. Requires a new quantized artifact. |
| Q2: [QuIP#](https://arxiv.org/abs/2402.04396v2), v2 2024-06-04, ICML 2024; [code](https://github.com/Cornell-RelaxML/quip-sharp) | Hadamard incoherence and lattice codebooks, with fine-tuning. | Future re-quantization reference; not lossless conversion of existing PQ2/PTQ1 scales/symbols. |
| Q3: [GPTQ](https://arxiv.org/abs/2210.17323), 2022; [AWQ](https://arxiv.org/abs/2306.00978), 2023; [SmoothQuant](https://arxiv.org/abs/2211.10438), 2022 | Second-order weight quantization, activation-aware scaling and migration of activation outliers into weights are different methods. | Foundational alternatives for new artifacts. Their INT4/INT8 results do not make re-quantizing already ternary Swift lossless or faster. |
| Q4: [TurboQuant](https://arxiv.org/abs/2504.19874v1), 2025-04-28 | Random rotations/scalar quantization and residual QJL give near-optimal distortion results for specified vector MSE/inner-product objectives. | Interesting lossy KV theory. Unbiased inner products do not imply unbiased softmax or exact text; charge rotations, norms, metadata and codebooks. |
| Q5: [E8 Lattice Key Quantization](https://arxiv.org/abs/2610.15384), 2026-10-11 preprint | Lattice quantization of keys with low-bit values and lookup scoring avoids explicit dense key expansion. | Recent lossy full-attention KV candidate, not a GDN-state codec. Compression ratios exclude other model/state costs; ARM kernels and long-context quality remain unqualified. |
| Q6: [ApexQuant](https://arxiv.org/abs/2610.07904), 2026-10-06 preprint | Residual re-isotropization builds progressively refinable quantized artifact prefixes; analyzes error decay under its randomized model. | Deferred new-artifact research; no lossless current-weight or end-to-end quality/speed guarantee. |
| Q7: [Signed-digit K/V LUT inference](https://arxiv.org/abs/2608.03229v1), 2026-08-04 preprint | Online signed-digit planes coordinate cache scales with lookup reductions and incomplete value blocks. | Hardware/representation co-design; changes KV precision and needs a different numerical profile. |
| Q8: [Runtime-Certified Bounded-Error Quantized Attention](https://arxiv.org/abs/2605.20868v1), 2026-05-20; [code](https://github.com/DeanoC/certified-quantized-attention) | Bounds local key-distribution/value reconstruction error with fallback, under range and quantization-metadata assumptions. | Methodology candidate; keeps original FP16 data in host RAM. Smaller GPU cache may increase total laptop memory. Certification is local and references a particular attention arithmetic path. |
| Q9: [Quantize What Counts: More for Keys, Less for Values](https://aclanthology.org/2026.findings-acl.1314/), July 2026; [code](https://github.com/mohsenhariri/spectral-kv) | Analyzes asymmetric K/V precision using expected norms, isotropic-input/training approximations and scalar quantization assumptions. | Measure Swift's actual layers first. Higher key precision is not a universal quality theorem for arbitrary hidden states. |
| Q10: [Fast Attention Requires Bounded Entries](https://papers.neurips.cc/paper_files/paper/2023/hash/c72861451d6fa9dfa64831102b9bb71a-Abstract-Conference.html), NeurIPS 2023 | Near-linear approximation for bounded entries in a stated dimensional/error regime; conditional SETH hardness beyond a threshold. | Theory guardrail: retain its `d=O(log n)`, entry bounds, inverse-polynomial additive error and `QK^T/d` scaling. Not a universal impossibility result for exact or approximate attention. |
| Q11: [Support Basis](https://arxiv.org/abs/2510.01643v2), v2 2026-03-19, AISTATS 2026 | Splits outlier contributions from bounded dense components; exact sparse contribution plus polynomial approximation. | Longer-term approximate attention; inspect actual supports, ranks, norms, normalization and retained workspace. |
| Q12: [Towards Tight Bounds for Streaming Attention](https://arxiv.org/abs/2606.07205v1), 2026-06-05 preprint | Randomized space/error bounds under bounded Q/K norms and specified dimension/precision regimes; error scales with epsilon times the softmax L2 norm times the V Frobenius norm. | Relevant proof content, not fixed-memory exact attention. Radius/error dependence and failure probability must be instantiated for a workload. |
| Q13: [Approaching I/O-optimality for Approximate Attention](https://arxiv.org/abs/2605.23751v2), v2 2026-10-22, paper lists NeurIPS 2026 | Almost-linear context-length dependence in transfers with approximation, and nearly matching algorithm-specific bounds in stated regimes. | Recent theory watch item. Does not supersede the exact-arithmetic/rounding contract or provide a qualified Snapdragon implementation. |
| Q14: [The Accuracy of Floating Point Summation](https://nhigham.com/wp-content/uploads/2023/10/high93s.pdf), Higham, 1993; [Mixed Precision Algorithms](https://doi.org/10.1017/S0962492922000022), Higham/Mary, 2022 | Rounding error depends on reduction and precision. The standard `gamma_n=n*u/(1-n*u)` analysis requires `n*u<1` and its range/rounding assumptions. | Mathematical foundation for tolerances; does not justify narrowing accumulators or promise stable sampled tokens. |
| Q15: [Analysis and Detection of Stagnation in Floating-Point Summation](https://arxiv.org/abs/2610.10819v2), v2 2026-10-09 preprint | Detects contributions disappearing under accumulation rounding. | Diagnostic watch item for proposed low-precision reductions, not evidence of a present Swift defect. |
| Q16: [Energy Cost of Completing Tasks with Quantized Language-Model Agents](https://arxiv.org/abs/2610.32417v2), v2 2026-10-26 preprint | Task-level evaluation shows how reduced energy/token can coexist with more energy/completed task through longer outputs or extra attempts. | Evaluation lesson; agent/GPU results are not creative-writing or Snapdragon measurements. |
| Q17: [KIVI](https://proceedings.mlr.press/v235/liu24bz.html), ICML 2024; [code](https://github.com/jy-yuan/KIVI) | Per-channel key quantization and per-token value quantization address their different observed distributions. | Foundational lossy-KV control for comparing newer codecs. Residual windows, scales, layout and compute overhead count; reported serving gains do not qualify Swift or recurrent-state compression. |
| Q18: [DASC](https://arxiv.org/abs/2608.30386v2), v2 2026-10-09 preprint | Ragged checkpoints omit shorter-retention GDN heads/KDA channels, zero-filling or approximately refreshing them. | Approximate checkpoint capacity in multi-GPU SGLang; count selection, packing and tensor-parallel balancing. Live state/convolution remain separate; not exact suffix restoration. |
| Q19: [DAMP](https://arxiv.org/abs/2608.27513v2), v2 2026-09-30 preprint | Calibrated mixed FP16/INT8 live recurrent state keeps high-risk channels in FP16, with stochastic rounding/reordering. | Approximation, batches 16–256. The 9.9-bit figure includes specified metadata; uniform four-bit accuracy loss is substantial. Latest v2 differs from earlier indexed speed figures; no batch-one/Swift quality acceptance. |
| Q20: [Preserving Updates in Low-Bit Recurrent State Quantization](https://arxiv.org/abs/2610.15709v1), 2026-10-11 preprint | Low-rank bypass and bounded directional rounding retain small coherent updates. | Approximation on RTX 3060, batches 16–48. Charge factors/scales, bypass, merge scratch and rollback/checkpoint lifetimes; T10 explains why local MSE is insufficient. |
| Q21: [Minima / Why Gated DeltaNet Survives 4-Bit Quantization](https://arxiv.org/abs/2609.04098v1), 2026-09-03 preprint | W4A4 projection/gate quantization with empirical error attenuation and per-module scales in fused projections. | New NVFP4 artifact, not four-bit recurrent state; FP32 perturbation replay does not prove uniform stability. Larger than current packed Swift. |
| Q22: [vAttention: Verified Sparse Attention via Sampling](https://proceedings.iclr.cc/paper_files/paper/2026/hash/55cb562b1f5af71f6707f3ff3c7941e6-Abstract-Conference.html), ICLR 2026; [code](https://github.com/skylight-org/sparse-attention-hub) | Important-token selection plus residual sampling with relative operator-output analysis. | Approximation; practical pilot/CLT budgets differ from conservative certificates. Distinct from Microsoft's same-name memory project. |
| Q23: [Certified Sparse Attention via Perturbed Retrieval](https://arxiv.org/abs/2610.24991v1), 2026-10-18 preprint | Anytime-valid attention-output certificates require exact candidates and fresh independent perturbations per decoding call. | AVX512-VNNI reference retains full 16-bit KV plus indexes: reported 40% storage overhead and a linear compressed-key scan. Context-independent sample/value-read bounds are not total-work bounds. Budget failure across heads/layers/steps under T9. |
| Q24: [DoSA](https://arxiv.org/abs/2610.13990v1), submission history 2026-09-26 | Conservative bounds skip candidates unable to enter an existing sparse indexer's exact Top-K. | Exact sparse-indexer selection, not dense softmax equality. Requires learned indexers absent from Swift. |
| Q25: [iS-KV](https://arxiv.org/abs/2610.02815v1), 2026-10-02 preprint | Recent exact window plus block-incremental SVD for older history; update coefficients when bases change. | Lossy capacity research. Persistent-KV figures omit weights/reconstruction; charge protected prompt/window, FP32 updates, simultaneous old/new factors and scratch. |
| Q26: [JoLT](https://arxiv.org/abs/2607.12550v4), v4 2026-09-24; [code](https://github.com/rahulk98/JoLT-Master-Thesis) | Joint Tucker-rank/residual-bit allocation under a byte budget, with rotated low-bit residuals. | Capacity/quality research, under ICLR 2027 review. Its batch-one A100 fused decode is 7.5–65.9x slower than dense at all measured points; near-lossless is empirical. Charge decomposition/reconstruction/residual scratch. |
| Q27: [CoS-VQ](https://arxiv.org/abs/2610.37336v1), 2026-10-27 preprint; [code](https://github.com/xzktx003/Cos_VQ) | Joint shared-scale/codeword search through tangent majorization and lower envelopes; evaluates Qwen3.8-27B. | Promising new-artifact research. Optimality is restricted to surrogate/candidate pools; accepted reconstruction needs true-error verification. A100/RTX4090 evidence, not Swift. |
| Q28: [YAQA / Model-Preserving Adaptive Rounding](https://arxiv.org/abs/2505.22988v3), v3 2026-06-03, ICML 2026; [code](https://github.com/Cornell-RelaxML/yaqa-quantization) | Kronecker-factored Hessian sketches incorporate output-side sensitivity in rounding. | Offline/new-artifact method; theorem bounds second-order error under assumptions, not full-model KL or quality. See T11. |
| Q29: [FastKron](https://arxiv.org/abs/2608.06291v2), v2 2026-09-27, originally titled BaKron | Anti-diagonal parallelism/deferred updates reduce core rounding to `O(m+n)` depth and `O(m*n*(m+n))` work. | Offline quantization algorithm. Equivalence/work bounds exclude preprocessing and do not forecast inference speed. |
| Q30: [Search Your Block Floating Point Scales!](https://arxiv.org/abs/2605.12464v1), 2026-05-12, MLSys 2026 | Search representable scales instead of assuming absmax minimizes block MSE. | Exhaustive legal-scale search is blockwise optimal; a short window is empirical. Changed Q8_K/Q8_0 scale selection changes the contract. |
| Q31: [HyperQuant](https://arxiv.org/abs/2606.23406v1), 2026-06-22 preprint | Hadamard/lattice quantization, bit stripping and Rice coding combine representation and entropy techniques. | Deferred H100/new-artifact research. Charge transforms and executable bytes; unbiased inner products are not exact softmax. |

### 2026 mobile acceleration and speech

The [Adreno](adreno-gpu-plan.md#2026-research-evidence),
[Hexagon](hexagon-npu-plan.md#2026-research-evidence) and
[speech](performance-expectations.md#2026-speech-inference-research) guides retain device,
runtime-access and measurement details.

| ID / source and date | Mechanism or observation | Disposition |
| --- | --- | --- |
| E1: [Qualcomm IWOCL Adreno talk](https://www.iwocl.org/wp-content/uploads/IWOCL-2026-Wang-Llamacpp.pdf), [2026-05-06 program](https://www.iwocl.org/iwocl-2026/conference-program/) | Packed layouts, vector GEMV/unpack, separated quantization fields, per-kernel tiles, image-backed weights, compilation caches and recorded submissions, including X/X2. | High-priority implementation reference. Several X2 MoE kernels use internal assembly; public OpenCL C and Vulkan differ in availability/features. No Swift PQ2/PTQ1 result. |
| E2: [TurboInfer / Act Before It's Too Late](https://doi.org/10.1145/3745756.3809208), MobiSys, 2026-06-20 | Detect short GPU stalls and adapt frequency at millisecond scale. | Phone PMU/frequency-control research; no equivalent Windows counters, authority or Vulkan/X1 power result established. |
| E3: [PowerBench / Is Your NPU Ready for LLMs?](https://arxiv.org/html/2607.05475v2), v2 2026-10-16 | Separate CPU/GPU/NPU energy, quantization, offload, polling and NPU wake/sleep costs. | Rooted Android SM8650/8750/8850 with QPT instrumentation; combined tuning estimate cannot rank Windows laptop paths. Paper lists SIGMETRICS 2027 acceptance. |
| E4: [When NPUs Are Not Always Faster](https://arxiv.org/html/2605.27435v1), 2026-05-22 preprint | CPU wins prefill with Android 15/v75/Q4_0/llama.cpp b7588. | Phase-ranking counterexample, not a Swift ranking; synchronous profiling affects scheduling. |
| E5: [Energy-Efficient On-Device RAG](https://arxiv.org/html/2606.11257v1), 2026-06-09 preprint | Windows Arm X Elite prepared-package deployment exposes static-context/loading-order constraints. | 64 GB X1E80100, Qwen3-4B, different QAIRT/GGUF/context configurations and NPU reranking in every arm; neither a same-artifact 32 GB Swift comparison nor a GPU ceiling. |
| E6: [ShadowNPU](https://arxiv.org/html/2508.16703v4), v4 2026-04-08, MobiSys 2026 | NPU pilot scores select sparse higher-precision attention with graph bucketing/per-head pipelining. | Approximation on phones; does not optimize GDN or preserve existing dense attention. |
| E7: [Quant.npu](https://arxiv.org/html/2605.20295v1), 2026-05-19 preprint | Optimized rotations/scales enable fully static mixed-precision quantization. | New-artifact/calibration lane; fixed scales do not preserve current dynamic Q8_K/Q8_0 arithmetic. |
| E8: [Sereno](https://www.usenix.org/conference/osdi26/presentation/xin), [paper](https://www.usenix.org/system/files/osdi26-xin.pdf), OSDI, July 2026 | Bandwidth-aware yield points protect foreground work when model TPS hides contention. | High-priority comfort lesson. Android arbitration does not establish Windows control; speculative yield points need a verifier and accepted-prefix recovery. |
| E9: [Hexagon DMA-ring fix](https://github.com/ggml-org/llama.cpp/pull/30189), merged 2026-10-09 | Repairs dropped DDR-to-VTCM descriptors on queue saturation. | Implementation correctness reference for DMA bounds/completion; not a demonstrated Swift defect. |
| E10: [llama.cpp b11913](https://github.com/ggml-org/llama.cpp/releases/tag/b11913), prerelease 2026-10-27 | VTCM-size override adds a capacity-control input. | Watch item: requested versus effective storage/failure semantics; does not increase hardware capacity or update Runner's pin. |
| P1: [Qwen3-TTS Technical Report](https://arxiv.org/abs/2601.15621), 2026-01-22 | Talker, residual-code predictor and codec have different rates and first-packet dependencies. | Stage-accounting reference; publisher first-packet results omit this native path's bootstrap/playback costs. |
| P2: [RVQ Position Aware Speculative Decoding](https://arxiv.org/html/2609.37007v1), 2026-09-29 preprint | Small learned offsets plus tree verification reduce residual-code serial work. | New artifacts, 0.6B CustomVoice/Apple evidence; RVQ speed is not complete 1.7B GGML readout speed. |
| P3: [One Megakernel Is Not Enough](https://arxiv.org/html/2610.35336v1), 2026-10-26 preprint | Fusion boundaries depend on rates, register pressure, resource residency and first-audio dependencies. | CUDA/H100 reference; best among measured supported designs, not globally optimal fusion. |
| P4: [Pushing the Limits of On-Device Streaming ASR](https://arxiv.org/html/2604.14493v2), v2 2026-04-19 preprint | Cache-aware streaming, right context and selective quantization/fusion. | CPU timings use 32 EPYC cores; Canary-1B-v2 comparison differs from selected Canary-Qwen-2.5B. File size is not peak RAM. |
| P5: [Beyond the Benchmark: Qwen3-TTS Failure Modes](https://arxiv.org/html/2610.23678v1), 2026-10-16 preprint | Conditioning/domain affect timbre, timing and prosody; automatic and listening rankings can disagree. | Listening-methodology reference. Voice-cloning evidence does not qualify the app's designed reference. |

### 2026 systems, measurement and output-budget work

The follow-up survey also considers overhead around tensor math and the number
of tokens needed to complete a task. These are different optimization objectives.
The [implementation guide](implementation-research.md#2026-literature-runtime-overhead-and-work-placement),
[performance guide](performance-expectations.md#2026-literature-measurement-and-comparison)
and [defaults guide](inference-defaults.md#2026-literature-token-budgets-are-a-separate-experiment)
record the project implications in more detail.

| ID / source and date | Mechanism or evidence | Disposition |
| --- | --- | --- |
| S1: [ProfInfer](https://proceedings.mlsys.org/paper_files/paper/2026/hash/03dbc11a22e79cd38bea53cf518c2371-Abstract-Conference.html), MLSys 2026 | Operator/graph tracing and hardware counters locate real work/traffic bottlenecks. | Profiling methodology; eBPF implementation is not a Windows tracer. |
| S2: [Optimizing Deployment Configurations](https://proceedings.mlsys.org/paper_files/paper/2026/hash/97dc07f1253ab33ee514f395a82fa7cc-Abstract-Conference.html), MLSys 2026 | Large configuration search under latency constraints shows phase, hardware and architecture dependence. | Evaluation reference; production Llama/GPU evidence does not rank local profiles. |
| S3: [Beyond the Buzz](https://proceedings.mlsys.org/paper_files/paper/2026/hash/d49cee5f3a79d97d719df255689d83d7-Abstract-Conference.html), MLSys 2026 | Prefill/decode disaggregation depends on workload mix, rate matching and coordination. | Phase-placement lesson; no automatic benefit for one local request. |
| S4: [Elastic Gang](https://arxiv.org/abs/2607.04668v1), 2026-07-06 | Acknowledged epochs permit safe per-token changes in a barriered CPU inference gang. | Bare-metal x86 Rust OS preprint; inspect protocol assumptions before any threadpool adaptation. |
| S5: [Rust rvllm copy-elimination report](https://ermolushka.github.io/posts/rvllm-memcpy/), 2026-09-24 | Hidden tensor/cache copies can dominate decode despite fast arithmetic. | Author report, M5/360M/F32-expanded model; candidate diagnostic, not evidence of a Prism defect. |
| S6: [Event-triggered local inference](https://bobodell.com/blog/event-triggered-local-llm-inference-sglang-idle-cpu), 2026-08-17 | Distinguish busy polling from useful work; evaluate idle and active wait policies separately. | Linux/SGLang author report; wake latency and active decode remain part of a comparison. |
| S7: [Shepherd Model Gateway](https://pytorch.org/blog/lightseek-smg/), 2026-04-30, updated June 2 | Native Rust tokenization/stream processing separates CPU pipeline work from GPU execution. | Architecture report; Runner already has Rust boundaries and a narrower protocol. |
| S8: [Sequential Isolation Protocol](https://arxiv.org/abs/2610.09778v1), 2026-10-07 | Repeated controlled runs reduce comparison variance on a specified A100/vLLM stack. | Evaluation methodology; its variance/saturation numbers are not laptop acceptance thresholds. |
| S9: [Edge-continuum measurement study](https://arxiv.org/abs/2609.08307v1), 2026-09-08, WIMS 2026 acceptance listed | Joint latency/quality/energy frontier can change after delivery overhead is included. | Jetson/server QA workload; no general CPU-versus-GPU or Snapdragon verdict. |
| S10: [MLPerf Client v2.0](https://mlcommons.org/benchmarks/client/), documentation reviewed 2026-10-29 | Client prompt/output-length workloads, separate TTFT/decode and quality gates; Windows Arm/X/X2 coverage. | Benchmark design reference using other models; experimental power tools and vendor paths do not qualify Swift. |
| B1: [ESTAR](https://arxiv.org/abs/2602.10004v1), 2026-02-10 | Learned trajectory/stop signals and RL reduce reasoning length. | New trained components/artifacts; not unchanged-target inference acceleration. |
| B2: [Budget Guidance](https://aclanthology.org/2026.findings-acl.1866/), ACL Findings, July 2026 | A remaining-length predictor guides token sampling toward a budget. | Base fine-tuning is avoided, but a compatible predictor and changed output policy remain. |
| B3: [BudgetThinker](https://aclanthology.org/2026.findings-emnlp.1465/), EMNLP Findings, October 2026 | Specialized remaining-budget tokens and supervised/RL training regulate reasoning length. | New training/control-token contract; not a setting of the selected Swift artifact. |
| B4: [CASE](https://aclanthology.org/2026.findings-emnlp.1983/), EMNLP Findings, October 2026 | Confidence-based filtering and adaptive stopping save work relative to fixed-budget self-consistency. | Multiple-candidate baseline; not a demonstrated win over the current single completion. |

### Withdrawn or unresolved reports

| ID / source and date | Finding | Disposition |
| --- | --- | --- |
| N1: [PolarQuant: Optimal Gaussian Weight Quantization via Hadamard Rotation](https://arxiv.org/abs/2603.29078v2), withdrawn 2026-04-20 | Author withdrawal reports errors needing correction; positive snippets remain indexed. | Withdrawn; do not use its performance/quality claims. Distinct from Google's differently titled PolarQuant. |
| N2: [Litespark CPU report](https://mindbeam.ai/research/litespark-inference), 2026-06-16; [HN discovery thread](https://news.ycombinator.com/item?id=48910545) | Fusion/SDOT/prefill ideas coexist with unresolved storage arithmetic and logit-ranking claims. | Unverified report; no speed/memory conclusion adopted. A logit difference alone cannot establish unchanged ranking without T12's margin condition. |

## Implementation sources to inspect

These are conceptual/code references, not additional dependencies or approved
runtime replacements. Pin the relevant source revision before an experiment.

| Source | Useful ideas | Transfer boundary |
| --- | --- | --- |
| [Prism Inference](https://github.com/symbiogenesis/prism-inference) and [PrismML native reference](https://github.com/PrismML-Eng/llama.cpp) | Actual PQ2/PTQ1, Hadamard/grouped transforms, scalar/NEON graph and pinned native arithmetic | Compare with the lock and existing implementations first; mutable `main` is not the provisioned release. |
| [OxiBonsai, reviewed at `1d10c48`](oxibonsai-reuse.md) | GGUF hostile-descriptor regressions, fused packed decode/dot, row tiling and prefetch controls | Apache-2.0 permits reuse. Q1/TQ2 encodings, FP32 activation/FMA arithmetic and Qwen3 state differ from Swift; float SIMD slice guards need review before reuse. |
| [Microsoft BitNet](https://github.com/microsoft/BitNet), [T-MAC](https://github.com/microsoft/T-MAC), [Vec-LUT](https://github.com/OpenBitSys/vlut.cpp) | Packed lookup kernels, table construction, activation tiling and CPU prefill | Model quantizers, scale groups, exact/approximate paths and temporary layouts differ. |
| [KleidiAI on Arm GitLab](https://gitlab.arm.com/kleidi/kleidiai), [official GitHub mirror](https://github.com/ARM-software/kleidiai) | Arm microkernels, packing APIs, fusion, DotProd/I8MM and architecture dispatch | Canonical GitLab was bot-protected during review; the official mirror was readable. C/assembly examples inform a Rust port; SME/SVE/optional ISA availability cannot be inferred for Snapdragon. |
| [Flash Linear Attention](https://github.com/fla-org/flash-linear-attention) | Chunkwise GDN, recurrent controls and state layouts | GPU/Triton implementation is an algebra/layout reference; retain Swift gates, normalization, convolution and grouped transforms. |
| [cuLA v0.2.0 release notes](https://github.com/inclusionAI/cuLA/releases/tag/v0.2.0), reviewed 2026-10-29 | Lists KVBuffer MTP verification/commit, packed KDA decode, convolution fusion and synchronization repairs | Release-note evidence only; individual PR fetches were unavailable. KDA/Lightning/CUDA paths are not a qualified Swift GDN/Rust/ARM64 implementation. |
| [llama.cpp](https://github.com/ggml-org/llama.cpp), including [Hexagon backend](https://github.com/ggml-org/llama.cpp/tree/master/ggml/src/ggml-hexagon) | Packed kernels, Vulkan, KV placement, threading and HVX/HMX examples | Upstream model/backend availability does not establish signed Windows deployment or exact Swift support. |
| [FlashInfer](https://github.com/flashinfer-ai/flashinfer) and [MLC-LLM](https://github.com/mlc-ai/mlc-llm) | Attention planning, cache layouts, kernel specialization and compilation | CUDA/TVM and platform-specific paths; study mechanisms without assuming CPU/Adreno performance transfer. |
| [SGLang](https://github.com/sgl-project/sglang), [vLLM](https://github.com/vllm-project/vllm) | Prefix ownership, hybrid cache budgets, chunk scheduling and speculative commit | Multi-request GPU serving has different objectives from one local conversation. A PR is not a merged, qualified implementation. |
| [Resin](https://github.com/ekzhang/resin) | Rust scheduler, immutable batch plans, unified KV/window/recurrent pool and execution transactions | Explicitly experimental. Python runners/CUDA examples and a broader HTTP surface are separate from its Rust core; no Swift/Snapdragon qualification. |
| [Magnitude](https://github.com/magnitudedev/magnitude), [compatibility contract](https://github.com/magnitudedev/magnitude/blob/main/inference/docs/compatibility.md) | Rust execution, per-shape kernels, dynamic cache allocation and device-specific tuning | Its compatibility document excludes Adreno X from Vulkan and lists Windows x64 in its CPU OS coverage. No Windows ARM64/Swift claim follows from general Windows/CPU marketing. Its KV quantization also changes precision. |
| [Camelid Bonsai 2 implementation](https://github.com/timtoole02/Camelid/blob/main/docs/BONSAI2.md) | Packed PTQ1, independent small-model drafting, hybrid rollback, allocation receipts and losing experiments | Published Windows x86/CUDA results, changed activation paths and speculative modes have distinct identities; Bonsai is not the Swift adapter profile. |
| [Orangu internals](https://mnemosyne-systems.github.io/orangu/doc/server-internals.html) | Native Rust inference, packed representations, layout and allocation ideas | CIX/Mali and alternate activation/repacking evidence do not qualify Snapdragon. Its documented GPLv3 licensing needs review before any source reuse. |

## Engineering reports and Hacker News leads

| Date and original source | Concept worth investigating | Evidence limit |
| --- | --- | --- |
| 2026-10-18: [SGLang Low-Latency Mode](https://www.lmsys.org/blog/2026-10-18-sglang-low-latency-mode) | Specialize small-batch kernels, overlap independent preparation, publish outputs earlier and amortize launches | Author report on multi-GPU NVIDIA/AMD setups with DSpark. Transfer the critical-path analysis; its latency multipliers are not Snapdragon forecasts. |
| September 2026 launch, reviewed 2026-10-29: [Magnitude author's HN post](https://news.ycombinator.com/item?id=49911995) and [source](https://github.com/magnitudedev/magnitude) | Model/shape specialization, dynamic memory and prefix locality | The post's M4 Pro/DGX Spark results use another model and quantized KV. Treat on-device tuning as an offline research technique here: Runner's qualified profiles do not benchmark at ordinary startup. |
| 2026-09-26: [Hayder Tirmazi's prompt-lookup report](https://jadidbourbaki.github.io/blog/prompt-lookup-llama-cpp/), [HN discussion](https://news.ycombinator.com/item?id=49859982), [Lemire follow-up PR](https://github.com/jadidbourbaki/llama.cpp/pull/12) | Remove nested-map copies, use compact lookup structures, prebuild immutable maps and avoid scoring candidates that cannot pass thresholds | The reported 42x and later up-to-140x concern drafting on an M4 Pro, not model inference. Static corpus indexes consume memory. Interesting only after a sound verifier/recovery path exists. |
| 2026-08-28: [TurinTech's fused dequantization/GEMM report](https://www.turintech.ai/blog/improving-vllm-cpu-inference-rebuilding-a-dequantisation-gemm-kernel) | Profile unpacking, scaling, conversion and packing separately; fuse where it removes intermediate traffic | Author-reported AWQ/W4A16 results on 48-core dual-socket Intel Granite Rapids. Neither AMX kernels nor its 10–30% throughput range establishes a NEON/PQ2 benefit. |

Watch [SGLang's author blog](https://www.lmsys.org/blog/), the repositories above,
and original links in relevant HN threads. Community comments can reveal
confounders or missing comparisons; votes, stars and repeated headlines do not
validate numerical equivalence or speed.

## Turning a lead into an experiment

1. Identify phase, shapes, bottleneck and missing evidence; check existing Prism
   work before choosing a mechanism to remove traffic/compute.
2. Declare numerical/state identity: exact bytes/integers, reordered floats with
   tolerances, distribution-preserving sampling, or explicit approximation/new
   artifacts. Include adapter and checkpoint identity.
3. Account for packed/expanded weights, copies, LUTs, activations, KV/recurrent
   snapshots, rollback, scratch, mappings and device allocations. File size,
   nominal buffers and Job commit measure different things.
4. Pin source/license; implement kernels upstream, preserving Runner policy and
   Rust/native presentation. Predeclare candidate count, shapes, sample, peak RAM
   and deadline under [PLAN's practical budgets](../PLAN.md#research-scope-and-practical-budgets).
5. Run focused untimed conformance for relevant tails, generic PQ2 `+2`, i8 `-128`,
   continuation, edits, restores, cancellation and failed commit; reuse suitable fixtures.
6. Run all benchmarks/full suites in [CI only](validation.md), on actual Snapdragon
   hardware for device claims. Pair identical artifacts/LoRA/context/output/sampler;
   retain cold/warm/prefix-hit TTFT, first useful token, prefill/decode/complete-task
   time, private/commit/resident/device peaks, comfort and observable energy.
   Preserve losing cases and uncertainty.
7. Record result/disposition here and delivery work in PLAN. Promotion still needs
   correctness, quality, resources and standard LoRA gates; research inclusion
   authorizes neither lower reserves, a new default nor a smaller substitute model.

Include [application queues/parsing/search/PCM](streaming-resources.md) and
[speech evaluation](performance-expectations.md#q4-speech-resource-comparison) in
complete-task accounting; UI delivery timing is not inference-kernel throughput.

## Keeping the register current

The bounded survey searched accessible arXiv/proceedings, original source repos,
author/vendor reports and HN links through 2026-10-29. Absence is not evidence of
nonexistence/completeness. No models, benchmark tools or third-party code were
installed, and this document installs no recurring task.

### 2026 follow-up search coverage

The 2026-10-29 expansion followed citations, revised versions, source changes,
counterexamples and withdrawn work, prioritizing 2026 submissions/proceedings.

| Source family | Retained review evidence |
| --- | --- |
| arXiv/full texts: low-bit kernels, recurrent compression, attention, speculation, mobile/speech, measurement | Submission/revision dates, assumptions, arithmetic identity, hardware, overhead exclusions and negative results |
| MLSys, ICLR, ACL/EMNLP, MobiSys, OSDI, DATE and related proceedings | Inspectable venue-backed papers; future acceptance claims remain labelled as authors' statements |
| GitHub/Arm GitLab implementations, PRs/releases, codecs/bugs | Observed source/PR status, platform limits and availability; no adoption or pin update |
| Author/vendor blogs and HN | Dated primary report/workload/caveats; comments/headlines only supply leads |

Refresh queries: `ternary LUT ARM NEON inference`, `batched low-bit prefill`,
`Gated DeltaNet chunkwise speculative rollback`, `hybrid recurrent prefix checkpoint`,
`attention I/O lower bound`, `KV quantization error bound`, `Windows ARM64 Hexagon HMX HVX`,
`bounded neural-network memory allocation`. Follow versions/citations, inspect
implementation release diffs and PR status, and resolve community claims to originals.

Each lead needs title/primary URL, publication/version/review dates, assumptions
or benchmark hardware/workload, exactness/error contract, memory overhead,
Swift/ARM64 applicability, source/experiment revision and disposition: `candidate`,
`approximation`, `requires new artifacts`, `implemented with bounded evidence`,
`rejected` or `deferred`. Mark inaccessible material unverified; update the cutoff
only after reviewing sources. Topic-specific implications remain in their
[contract pages](README.md), with this register as the source catalog.
