# Inference research and proof register

Research cutoff and last source review: **2026-10-29**.

This is the central index of mathematical results, algorithms, implementations
and engineering reports relevant to faster, smaller Swift Bonsai 2 inference.
It records what a result establishes, how it could apply here, and what still
needs checking. It is a curated literature survey, not an exhaustive catalog
of computer science or a claim that the newest paper is the best implementation.
The derivations below are reviewable arguments, not machine-checked proofs.

The project already had substantial research documentation. This register
connects it and adds a dated survey; the existing contracts remain authoritative.
Outstanding delivery work stays in [PLAN.md](../PLAN.md).

Jump to [priorities](#priorities-for-this-model-and-hardware),
[proofs](#mathematical-results-and-proof-obligations),
[papers](#research-catalog), [implementations](#implementation-sources-to-inspect),
[blogs/HN](#engineering-reports-and-hacker-news-leads),
[experiments](#turning-a-lead-into-an-experiment) or
[upkeep](#keeping-the-register-current).

## Existing evidence and project scope

| Existing document | Coverage |
| --- | --- |
| [Inference algorithms](inference-algorithms.md) | Numerical semantics, LUT kernels, chunkwise GDN, attention I/O, speculation and compression bounds |
| [Implementation research](implementation-research.md) | Design decisions and the limits of current evidence |
| [Native Rust engine](native-rust-engine.md) | Runtime ownership, supported graph, parity and replacement requirements |
| [Swift memory](swift-memory.md), [memory options](swift-memory-options.md), [PTQ1 readiness](ptq1-readiness.md) | Exact artifacts, census, packing, storage accounting and bounded qualification |
| [SIMD](simd-plan.md), [hardware profiles](hardware-profiles.md), [Adreno](adreno-gpu-plan.md), [Hexagon](hexagon-npu-plan.md) | ISA, placement, state migration and target-device qualification |
| [Inference defaults](inference-defaults.md), [performance expectations](performance-expectations.md) | Sampler, thinking, prompt and published benchmark context |
| [Streaming resources](streaming-resources.md), [speech memory](speech-memory.md) | Application work bounds, streaming lifetimes and ASR/TTS resource evidence |
| [Validation](validation.md), [runtime promotion](runtime-promotion.md) | Correctness, performance evidence and promotion gates |

The current target is a Windows ARM64 Snapdragon X/X2 laptop, initially the
32 GiB X1E-78-100. Swift's `qwen35` graph has **16 full-attention layers and 48
Gated DeltaNet/convolution layers**. The app reserves one 8,192-token conversation,
uses packed PQ2 weights by default, FP16 full-attention KV and FP32 recurrent
state. PTQ1 is a separate candidate. The [verified metadata](../config/swift.metadata.json)
and current locks take precedence over a paper's similarly named model.

Inference kernels belong in the separate [Prism Inference repository](https://github.com/symbiogenesis/prism-inference).
Runner owns provisioning, admission, orchestration and the native Rust UI.
Concepts from Python/C++/GPU systems can inform a Rust implementation without
adopting their UI, API surface or backend. Preserve the
[native UI architecture](ui-architecture.md) and explicit local HTTP/JSON/SSE subset.

Evidence labels used below:

- **Theorem:** a published mathematical result with stated assumptions; an
  arXiv theorem can still be an unreviewed claim.
- **Derivation:** an algebraic/counting argument recorded here or in this repo.
- **Paper:** an algorithm and its authors' evaluation; preprints are identified.
- **Implementation:** inspectable source, which still needs pinned revisions,
  licensing and conformance review before reuse.
- **Report:** a maintainer/author's engineering account. Hacker News is a
  discovery and discussion source; technical claims are traced to the original.

## Priorities for this model and hardware

These are research priorities inferred from applicability and present contracts,
not measured rankings. Several building blocks already exist in Prism; compare
against the pinned implementation before proposing them again.

| Priority | Candidate | Expected mechanism and decisive question |
| --- | --- | --- |
| 1 | Exact packed decode: BitNet-style tables, T-MAC ideas, NEON layouts | Reduce unpack/dot overhead without expanding the model. Can the implementation retain the exact activation quantizer, scales, integer bounds and floating reduction order? |
| 2 | Batched projection prefill: Vec-LUT and cache-sized weight tiles | Reuse each weight tile across prompt tokens. Does saved traffic exceed LUT construction, packing and scratch costs? Sequential token blocks alone do not give this reuse. |
| 3 | Faithful chunkwise Gated DeltaNet | Turn the 48 recurrent layers' prompt work into matrix operations. Does a declared floating tolerance hold for long prefixes, restores and the actual grouped/sign transforms? |
| 4 | State ownership, immutable KV pages and sparse recurrent checkpoints | Reduce copies and retained snapshots. Is the dominant allocation KV, recurrent state, transaction scratch or mapped weights in this workload? |
| 5 | Exact attention locality and bounded scratch | Reuse grouped-head KV reads and tile V. Can a faster normalization preserve the existing FP16 probability-rounding boundary, or does it need a separate numerical identity? |
| 6 | Suffix/prompt-lookup speculation with block verification | Avoid a second model's weights; amortize target weight reads over accepted tokens. Do creative prompts accept enough proposals to pay for verification and recurrent recovery? |
| 7 | Shape-specific fusion, persistent workers and precomputed execution plans | Remove dispatch, repeated transforms and allocations. Profile the complete request before introducing a more complex scheduler. |
| 8 | Adreno/Hexagon phase placement | Prefill and decode can favor different devices. Include signed Windows runtime access, packed-format support, mapping, handoff, duplicate weights and sustained power costs. |
| Separate approximation lane | KV quantization, approximate attention, low-order GDN, pruning or new trained drafters | Potential memory/traffic gains change precision, computation or artifacts. Compare quality and resource use as an explicitly different profile. |

## Mathematical results and proof obligations

### T1. Packing limits and the difference between sparse and small

**Derivation.** An independent 128-element ternary block has `3^128` possible
symbol strings. An injective fixed-length encoding needs
`ceil(log2(3^128)) = 203` bits, or 26 whole bytes. PTQ1's 26 code bytes plus
the unchanged two-byte scale reach this independent, byte-aligned bound.
This does not rule out correlated, variable-length or cross-block compression.
Those schemes must also pay for indexes, padding, decoding and random access.

The [actual packing assessment](swift-memory-options.md#packing-decision)
rejects the proposed BITCOS layout for this artifact: its 32.76154% zeros do
not reach that layout's 37.5% ideal break-even. Zero symbols do not make a
dense CPU loop skip work. A sparse representation must save more traffic/work
than its indexes and irregular execution add.

### T2. Traffic, arithmetic and the whole-request ceiling

The [Roofline model](https://escholarship.org/uc/item/5tz795vq) (Williams, Waterman
and Patterson, 2009) relates attainable arithmetic throughput to compute
capacity and arithmetic intensity: `P <= min(P_peak, B * F/Q)`.
Here `F` is work and `Q` is traffic across the chosen memory boundary.
This is a performance model, not a proof that an arbitrary kernel achieves
the roof or saturates advertised system bandwidth.

**Derived planning bounds:** `t >= max(F/P_peak, Q/B)` and, if every ordinary
decode step streams `W` weight bytes from that memory level,
`tokens/s <= B_effective/W`. Cache hits, batching and speculation change `Q`;
the GGUF file length is not itself a measurement of bytes per token.
Prefill can amortize a weight tile across many tokens, while single-token
decode usually has much less reuse.

For a baseline time fraction `f` improved by a factor `s`, the derived
[Amdahl relation](https://doi.org/10.1145/1465482.1465560) is
`speedup = 1 / ((1-f) + f/s)`, assuming the remaining work is unchanged.
A 42x drafting microbenchmark cannot mean 42x inference if drafting is a
small fraction of request time. Overlap also cannot remove the slowest stage's
service time; bounded queues trade latency and memory rather than create compute.

### T3. Exact low-bit dots need bounds at every intermediate

**Derivation.** For signed i8 `q_i`, including `-128`, and ternary
`w_i in {-1,0,1}`, `abs(sum(w_i*q_i)) <= 128*n`.
At `n=128`, 16,384 fits signed i16. Generic PQ2 also permits `+2`: its
conservative absolute bound is 32,768, while its tight signed final-dot range
is `[-32768, 32512]`, which does fit i16. This does not certify every evaluation
strategy: check LUT construction, partial sums and bias corrections independently.

The identity `sum((code_i-1)*q_i) = sum(code_i*q_i) - sum(q_i)` can avoid
per-lane subtraction, but with codes `0..3` its uncorrected 128-term dot ranges
over `[-49152, 48768]` and needs wider storage. Preserve group boundaries and
scale application order.
Rust checked scalar arithmetic does not make SIMD wrapping instructions checked.
[Bitnet.cpp](https://arxiv.org/abs/2502.11880) distinguishes exact INT16-table
variants from approximate INT8 tables; [T-MAC](https://arxiv.org/abs/2407.00088)
also uses table quantization and optional approximate aggregation. A LUT is not
automatically an exact replacement for Prism's Q8_K/Q8_0 path.

### T4. Attention I/O optimality has a computation model

[Saha and Ye, ICML 2024](https://proceedings.mlr.press/v235/saha24a.html)
prove FlashAttention I/O-optimal up to constants for fast-memory capacity
`M >= d^2` in their model, with separate small-memory results. This concerns
data movement for their attention computation; it does not prove a universal
latency optimum or an unconditional exact subquadratic attention algorithm.

[FlashAttention](https://arxiv.org/abs/2205.14135) uses tiled online softmax.
**Algebraic identity:** merging tile statistics `(m_a,l_a,o_a)` and
`(m_b,l_b,o_b)` rescales their unnormalized sums to `m=max(m_a,m_b)` before
adding. That identity holds over real arithmetic. Prism's reference rounds
globally normalized probabilities to FP16 before multiplying V; ordinary
online merging does not reproduce that rounding operation. A compatible path
must preserve it or declare different numerical tolerances and state identity.

### T5. Chunking recurrence and recovering rejected tokens

[Gated Delta Networks](https://arxiv.org/abs/2412.06464v3) derives a chunkwise
form using structured triangular/WY computations. In column-vector convention,
`S_t = alpha_t*S_(t-1)*(I-beta_t*k_t*k_t^T) + beta_t*v_t*k_t^T`.
Real-arithmetic equivalence permits parallel prompt processing; changed floating
evaluation order still requires state/logit and long-continuation checks.

**Derived rollback limit:** the transition matrix has eigenvalues `alpha_t`
and `alpha_t*(1-beta_t*||k_t||^2)`. It can be singular, and values near zero
amplify inversion error. Convolution also overwrites history. Thus arbitrary
inverse-state rollback is not a sound generic recovery strategy. Keep snapshots
or saved factors with forward reconstruction and atomic commit.

For strictly triangular `L` of size `c`, `L^c=0` and
`(I-L)^(-1)=I+L+...+L^(c-1)` in exact arithmetic. A short truncation's error
depends on powers/norms of `L`; spectral radius zero alone does not bound their
intermediate size. This is the key obligation for multiplication-only GDN
approximations. See [the existing recurrence constraints](inference-algorithms.md#chunkwise-gated-deltanet-prefill).

### T6. Exact speculative sampling preserves a distribution

[Leviathan, Kalman and Matias, ICML 2023](https://proceedings.mlr.press/v202/leviathan23a.html)
provides the acceptance/rejection construction. For normalized target `p` and
proposal `q`, propose `x~q`, accept with `min(1,p(x)/q(x))`, and on rejection
sample from normalized `(p-q)_+`. The accepted mass is `min(p,q)` and rejected
correction supplies the remainder, recovering `p`. Apply this conditionally at
each accepted prefix, using the actual temperature/top-k/top-p/repetition policy.

Distributional equality does not imply identical output for the same RNG seed
or the same draw ordering. Greedy proposal agreement is a narrower property.
Recurrent/convolution state must also match the accepted prefix; a KV watermark
alone is insufficient. Under a simplifying independent constant acceptance
probability `a`, `k` proposals plus one target token yield expected progress
`1+a+...+a^k`; net speed also charges drafting, block verification and recovery.
Creative writing can have very different acceptance from repeated code/text.

### T7. State size, lifetimes and prefix identity

**Derived payload accounting** from the [metadata](../config/swift.metadata.json):
full-attention KV at 8K is
`16 layers * 8192 tokens * 4 KV heads * (256 K + 256 V) * 2 bytes`
`= 536,870,912 bytes = 512 MiB`. Charge only the 16 full-attention layers here.
The 48 recurrent layers have separately sized FP32 matrix/convolution state;
the [current logical payload](native-rust-engine.md) is 149.625 MiB.
Neither number includes capacity, alignment, snapshots, scratch or allocator
metadata. A physical-storage ledger counts unique shared pages and every independent
recurrent snapshot, including transient restore/rollback storage. Current admission
is more conservative: it charges every referenced sealed page per snapshot without
depending on sharing. This survey does not change that admission policy.

Prefix reuse follows deterministic execution: equal exact tokens, starting/reset
state and absolute positions, with equal model, adapter, transforms, precision
and execution contract, imply the same
prefix state under that contract. A recurrent checkpoint represents its exact
prefix position; it cannot stand for an arbitrary shorter prefix. Immutable
sharing saves copies only while mutable tails remain independently owned.

For fixed execution, simultaneously live buffers give a storage lower bound.
Reuse is valid after the previous value's final use, including asynchronous
device work. [Memory Allocation for Constant-Bounded Programs](https://arxiv.org/abs/2608.14471v1)
(2026-08-14 preprint) studies tighter bounds under defragmentation/control-flow
transformations; its model is broader than a fixed no-moving arena. The existing
[scratch-lifetime change](inference-algorithms.md#scratch-lifetimes) saves 152 KiB
of nominal payload, not a proved process-level peak reduction.

### T8. Floating-point and approximation claims

[Goldberg's floating-point analysis](https://docs.oracle.com/cd/E19957-01/806-3568/ncg_goldberg.html)
explains why reassociation, FMA, rounding and underflow can change a mathematically
equivalent expression. Integer-dot equality, decoded-weight equality, tolerance
on logits, identical sampled tokens and task quality are different guarantees.
None implies all the others. KV compression changes attention; low-bit recurrent
state can accumulate error across the whole prefix.

Recent theoretical attention/quantization work is indexed below. Record the
error metric, Q/K norm assumptions, randomness, failure probability, dimensional
regime and decoding horizon before applying a bound. A guarantee on one head's
attention error or vector inner products is not a bound on creative-answer quality.

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
| H19: [DFlash](https://proceedings.mlr.press/v306/chen26bv.html), ICML 2026; [code](https://github.com/z-lab/dflash) | Block diffusion drafts multiple tokens in one parallel pass conditioned on target features, reducing sequential drafting work. | Requires a compatible trained drafter and hybrid block verifier. Published speedups do not establish CPU break-even or acceptance for creative prose. |

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

## Implementation sources to inspect

These are conceptual/code references, not additional dependencies or approved
runtime replacements. Pin the relevant source revision before an experiment.

| Source | Useful ideas | Transfer boundary |
| --- | --- | --- |
| [Prism Inference](https://github.com/symbiogenesis/prism-inference) and [PrismML native reference](https://github.com/PrismML-Eng/llama.cpp) | Actual PQ2/PTQ1, Hadamard/grouped transforms, scalar/NEON graph and pinned native arithmetic | Compare with the lock and existing implementations first; mutable `main` is not the provisioned release. |
| [Microsoft BitNet](https://github.com/microsoft/BitNet), [T-MAC](https://github.com/microsoft/T-MAC), [Vec-LUT](https://github.com/OpenBitSys/vlut.cpp) | Packed lookup kernels, table construction, activation tiling and CPU prefill | Model quantizers, scale groups, exact/approximate paths and temporary layouts differ. |
| [KleidiAI on Arm GitLab](https://gitlab.arm.com/kleidi/kleidiai), [official GitHub mirror](https://github.com/ARM-software/kleidiai) | Arm microkernels, packing APIs, fusion, DotProd/I8MM and architecture dispatch | Canonical GitLab was bot-protected during review; the official mirror was readable. C/assembly examples inform a Rust port; SME/SVE/optional ISA availability cannot be inferred for Snapdragon. |
| [Flash Linear Attention](https://github.com/fla-org/flash-linear-attention) | Chunkwise GDN, recurrent controls and state layouts | GPU/Triton implementation is an algebra/layout reference; retain Swift gates, normalization, convolution and grouped transforms. |
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

1. Identify the bottleneck and evidence gap. State the phase, shapes, current
   implementation and which source mechanism should remove work or traffic.
   First check whether Prism already implements it.
2. Write the numerical/state contract: exact integers/bytes, reordered floating
   work with predeclared tolerances, distribution-preserving sampling, or an
   explicit approximation/artifact change. Include adapter and checkpoint identity.
3. Account for packed/expanded weights, retained copies, LUTs, activations, KV,
   recurrent snapshots, rollback, scratch, mapped residency and device allocations.
   A smaller file, nominal buffer count and Windows Job commit are different metrics.
4. Review a pinned source revision and its license. Implement runtime mechanisms
   upstream in Prism; keep Runner policy and the Rust/native presentation boundary.
5. Run focused untimed conformance checks for the changed contract: boundaries,
   generic PQ2 `+2`, i8 `-128`, odd/tail groups, full continuation, edits, restore,
   cancellation and failed commit as applicable. Reuse existing fixtures where
   they actually test the hypothesis.
6. Under [the validation policy](validation.md), run **all benchmarks and full
   suites in CI only**. Device conclusions need the actual Snapdragon runner.
   Pair the same artifacts, LoRA, context/output limits and sampler; report cold,
   warm and prefix-hit cases, TTFT/first useful token, prefill, decode and complete
   task time. Include peak private/commit/resident/device memory, thermal behavior
   and energy when observable. Preserve losing cases and uncertainty.
7. Record a result link and disposition here, and delivery work in `PLAN.md`.
   Promotion still requires the existing correctness, quality, resource and
   standard-LoRA gates. Research inclusion does not authorize a lower admission
   reserve, a new default, or substituting a different model to make a trial fit.

Application overhead and speech belong in the same complete-task account:
queue backpressure, incremental parsing/search, shared immutable text and bounded
PCM buffers already have [explicit contracts](streaming-resources.md).
Do not attribute inference-kernel gains to UI delivery timing, or replace speech
models/precision without their separate [evaluation](speech-memory.md).

## Keeping the register current

The initial survey searched arXiv/proceedings, author blogs, GitHub, GitLab and
Hacker News through 2026-10-29, including recent GDN, low-bit CPU, attention,
hybrid caching, speculation and memory-planning work. Search is incomplete;
absence here is not evidence that an algorithm or implementation does not exist.
No recurring task is installed by this document.

For a refresh, search recent versions and citation chains around these subjects:
`ternary LUT ARM NEON inference`, `batched low-bit prefill`, `Gated DeltaNet
chunkwise speculative rollback`, `hybrid recurrent prefix checkpoint`,
`attention I/O lower bound`, `KV quantization error bound`, `Windows ARM64
Hexagon HMX HVX` and `bounded neural-network memory allocation`. Inspect new
releases, source diffs and open/merged PRs in the implementation directory;
resolve HN/blog claims to original code or papers.

For every added lead retain: title and primary URL; publication/version date and
review date; theorem assumptions or benchmark hardware/workload; exactness/error
contract; memory overhead; Swift/ARM64 applicability; source/experiment revision;
and disposition (`candidate`, `approximation`, `requires new artifacts`,
`implemented with bounded evidence`, `rejected`, or `deferred`). If a page or
artifact cannot be inspected, mark it unverified instead of elevating a snippet
to evidence. Update the cutoff only after an actual source review.
