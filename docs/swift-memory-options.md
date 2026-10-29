# Swift memory options

This document records representation and allocation constraints. Outstanding
experiments and promotion requirements live only in
[PLAN.md](../PLAN.md#creative-quality-adapters-and-memory).

This audit covers the exact PQ2 artifact in
[swift-assessment.lock.json](../config/swift-assessment.lock.json), its
[symbol census](../config/swift.pq2-census.json) and Prism commit
`73bfe6cc485554822f80c473a3c0c4f25e80f41b`. Calculations describe serialized bytes
or allocation opportunities, not measured peak RAM or throughput. Keep the
[assessment's](swift-memory.md) Job limits, physical reserve and watchdogs.

The scope is current-weight lossless repacking and reviewed allocation changes.
New training/fine-tuning on this machine is excluded. Do not substitute an older
checkpoint, change weight/KV precision or count an adapter as lossless memory
savings.

| Option | Current contract | Quantified scope and limitation |
| --- | --- | --- |
| Current-symbol PQ2 → PTQ1 | Verified export and bounded native decode | Saves 1,259,520,000 weight bytes (1.1730 GiB); native output and performance are unqualified. |
| mmap, no repack | Selected | Avoids a separate complete private weight copy only for tensors supported by the mapped backend buffer type. |
| Microbatch | 64 selected | A 32-token variant changes execution grouping and its memory benefit is unmeasured. |
| Output rows | Completion normally samples one row | One F32 logits row is 993,280 bytes; graph logits and persistent host output have distinct lifetimes. |
| Hadamard/sign cleanup | Main buffers already shared | At most 4 MiB temporary staging plus 114,688 bytes retained sign metadata. |
| F32/BF16 narrowing | Original types retained | Only 10.09 MiB F32 and 45 MiB BF16 exist; narrowing is not a lossless default. |
| Thread stacks | Four matching decode/batch threads | Reserved address space is distinct from committed/resident memory. |
| Rust checkpoint restore | Pinned runtime 0.1.5 stages fallible KV copies and reuses live recurrent/convolution storage | Avoids 156,893,184 bytes (149.625 MiB) of transient Swift restore payload; not measured peak RAM or steady-state savings. |

## Exact PTQ1 packing

PTQ1 stores each 128-symbol group in **28 bytes**, versus PQ2's **34**: 24
five-trit bytes, two four-trit bytes and the unchanged two scale bytes. The
pinned codec uses ceiling-scaled base-3 codes, 16-lane then 8-lane order, and a
canonical zero unused tail trit. Conventional group-256 TQ1 cannot retain both
original group-128 scales.

For arbitrary independent 128-trit blocks, `ceil(128 log2(3)) = 203` bits require
26 whole code bytes. PTQ1 meets this byte-aligned bound before its scale.
Cross-block compression has different tradeoffs; see the
[counting bound](inference-algorithms.md#compression-and-approximation-constraints).
Physical FP16 KV in the experimental Rust reference and checkpoint-storage
behavior are separate from weight packing.

The [native layout](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/ggml/src/ggml-common.h)
and [codec](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/ggml/src/ggml-quants.c)
define the representation. The [Rust ternary crate](https://github.com/symbiogenesis/prism-inference/blob/main/tools/ternary/README.md)
and [GGUF exporter](https://github.com/symbiogenesis/prism-inference/blob/main/tools/gguf/README.md) copy symbols/raw scales without
floating-point quantization, using bounded caller-owned buffers and exact PQ2
reconstruction checks.

For the actual census:

```text
209,920,000 blocks × (34 − 28) = 1,259,520,000 saved payload bytes
verified complete PTQ1 file  = 5,946,648,928 bytes
```

All converted tensor block counts are divisible by sixteen, retaining 32-byte
alignment. Non-tensor bytes occupy the 11,120,992-byte header/data-offset region;
there are no source inter-tensor or trailing gaps. The
[export receipt](../config/swift.ptq1-export.json) records the verified final
length and digest. Retain the original source on disk, but avoid holding complete
old/new buffers simultaneously.

The [ARM dot kernel](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/ggml/src/ggml-cpu/arch/arm/quants.c)
decodes PTQ in place rather than retaining an expanded model. However,
[CPU type traits](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/ggml/src/ggml-cpu/ggml-cpu.c)
use Q8_K activations for PQ2 and Q8_0 for PTQ1. Equal decoded weights do not imply
equal logits, tokens or speed. Preserving PQ2's activation route would require
a distinct qualified PTQ1×Q8_K kernel.

## Packing decision

Current-weight PTQ1 is the supported lossless packing option.
[BITCOS](https://arxiv.org/abs/2609.16338v1) (2026-09-14 preprint) is not
supported. Swift's 32.76154% zero-symbol rate falls below BITCOS's 37.5% ideal
same-scale PTQ1 break-even before offsets, padding or indexes. Its proposed
layout would occupy 6,158,339,606 bytes before container/index overhead,
**211,690,678 bytes more** than the verified 5,946,648,928-byte PTQ1 export.

The rejected Cobalt 100 ARM64 synthetic prototype was also slower than PQ2.
That result does not establish a universal BITCOS limit, Snapdragon behavior
or full-model performance. The serialized-size disadvantage is sufficient to
exclude the proposed layout from this artifact's current packing choices.

## Mapping and startup

The [CPU check](../scripts/Invoke-CpuCheck.ps1) selects mmap, no repack, no warmup,
CPU buffers and 128/64 batch/microbatch. The
[loader](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/src/llama-model-loader.cpp)
can point supported CPU tensors into the read-only mapping; other buffer types
may copy them. Load receipts determine which allocations count as savings.
Hashing/census/export use bounded buffers rather than whole-file arrays or
model-wide float conversion and precede inference.

The pinned
[Windows mapping code](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/src/llama-mmap.cpp)
prefetches the complete mapping and cannot unmap fragments. Windows
[prefetch](https://learn.microsoft.com/en-us/windows/win32/api/memoryapi/nf-memoryapi-prefetchvirtualmemory)
populates the physical cache, not the process working set. Disabling it could
avoid eager startup caching, but does not reduce tensor bytes or prove sustained
savings once inference touches the weights. No current CLI switch exposes it.

## Graph and output buffers

The
[context allocator](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/src/llama-context.cpp)
reserves prompt/decode graphs and grows the persistent output buffer to the
largest requested count. With 248,320 vocabulary entries, reducing a retained
64-row F32 logits buffer to one row would save **62,576,640 bytes**. This is
conditional: completion may already retain only one row, and graph logits can
differ from persistent host output. Single-row output, optional embeddings,
capture layers, MTP and backend sampling each have separate allocation contracts.

Microbatch 32 preserves context length but changes execution grouping, so the
64-token profile's numerical/device evidence does not transfer to it. One F32 17,408-wide activation occupies
4,456,448 bytes at 64 tokens and 2,228,224 at 32. These are component sizes;
do not multiply them by all layers because the planner reuses storage. The
[CPU backend](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/ggml/src/ggml-cpu/ggml-cpu.cpp)
retains a maximum-sized work buffer. It has no current post-prefill trimming
contract; releasing it could cause later allocations.

## Other allocation bounds

The [Hadamard implementation](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/src/llama-model.cpp)
already shares one 1024² F32 rotation per CPU buffer type and signs by width/type.
Retained rotation/sign/metadata payload is 4,423,680 bytes, alongside a
temporary 4 MiB initialization vector. Finished sign metadata accounts for
114,688 bytes. Counting one rotation per transformed weight would count shared
storage repeatedly. A butterfly rewrite changes floating-point order rather
than merely reducing storage.

F32/BF16 weights total 57,767,936 bytes. Narrowing all F32 to sixteen bits could
save at most 5,291,008 bytes and would require exactness proofs. BF16 already
has a [native dot route](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/ggml/src/ggml-cpu/ggml-cpu.c);
there is no demonstrated complete F32-expanded copy to remove. FP16 can lose
BF16 range. The 8K FP16 KV/recurrent estimates remain 536,870,912 and 156,893,184
bytes; lowering context or precision changes the contract.

[Matching thread settings](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/common/common.cpp)
avoid a separate batch pool. Four threads mean three workers plus the caller
in the non-OpenMP path. Windows
[stack reservation](https://learn.microsoft.com/en-us/windows/win32/procthread/thread-stack-size)
is not committed/resident memory. PE headers and actual stack depth determine
whether stack settings represent a material allocation bound.

Serialized byte savings alone do not justify reducing the service cap or
physical reserve. The [PTQ1 contract](ptq1-readiness.md) describes the supported
selection and unchanged admission threshold.

## 2026 state and cache research outside the lossless scope

Reviewed through **2026-10-29**. The following work broadens the
[research register](research-register.md); it does not change this document's
selected lossless packing, FP16 KV/FP32 recurrent state or admission policy.
Compression ratios for a component do not apply to model weights, speech or the
whole process. Swift's 16 full-attention layers and 48 recurrent layers need
separate ledgers: full-attention KV compression does not compress GDN state.

### Persistent checkpoint compression versus live-state quantization

[DASC](https://arxiv.org/html/2608.30386v2) (v2 2026-10-09 preprint) stores selected
long-retention GDN heads/KDA channels in ragged checkpoints, then zero-fills or
approximately refreshes omitted units from a suffix. Its running states remain
full precision; convolution stays uncompressed. This can retain more prefixes
under a checkpoint budget, but the omitted state makes reuse approximate.
Head/channel selection, packing and tensor-parallel balancing are distinct costs.
Its multi-GPU SGLang results do not justify omitting Swift state or reducing the
reserve. Compare with exact sparse checkpoint placement plus suffix recomputation
before introducing approximate checkpoints.

[DAMP](https://arxiv.org/html/2608.27513v2) (v2 2026-09-30 preprint) quantizes the
**live recurrent state after each update**. Calibration retains high-risk key
channels in FP16 and stores the remainder in INT8, with scaling/reordering and
stochastic rounding. The 9.9-bit average includes its specified metadata; it is
not an arbitrary FP32-to-INT8 substitution. Its throughput results use batches
16–256, while uniform four-bit variants still lose substantial task accuracy.
Neither reported near-FP32 benchmark quality nor reduced state-update traffic
establishes exact continuation, batch-one benefit or Swift/LoRA quality.

[Preserving Updates in Low-Bit Recurrent State Quantization](https://arxiv.org/html/2610.15709v1)
(2026-10-11 preprint) addresses small updates repeatedly lost under requantization.
A low-rank bypass retains pending updates; bounded directional rounding avoids
some update reversals. Count bypass factors, merge scratch, scales and their
checkpoint/rollback lifetime. Its RTX 3060/batches 16–48 results are approximation
evidence, not a proof that low-bit state is equivalent to FP32. Minimizing one
step's quantization error alone does not control coherent error over a sequence.

The [Minima W4A4 study](https://arxiv.org/html/2609.04098v1) (2026-09-03 preprint)
addresses a different object: quantized **projection weights and activations**,
including gate projections. Its perturbation study runs clean/perturbed
recurrences in FP32; it does not establish four-bit recurrent-state storage.
Its measured error attenuation is not a uniform stability theorem for arbitrary
inputs or gates. Per-module scales must also remain correct when projections
are fused. This new NVFP4 Qwen artifact is substantially larger than packed Swift
and cannot count as lossless repacking of existing PQ2/PTQ1 weights.

### Low-rank full-attention KV and reconstruction costs

| Research | Representation and evidence | Required accounting / disposition |
| --- | --- | --- |
| [iS-KV](https://arxiv.org/html/2610.02815v1), 2026-10-02 preprint | Exact recent window plus a bounded-rank older history, updated through block-incremental SVD. Historical coordinates change with the basis to avoid drift. | Approximate full-attention cache only. Published persistent-KV ratios exclude weights and temporary reconstruction tensors; include bases, coefficients, protected prompt/window, FP32 updates, reconstruction and old/new factors during replacement. |
| [JoLT](https://arxiv.org/html/2607.12550v4), v4 2026-09-24 preprint | Partial Tucker cache plus rotated low-bit residual; allocates rank and precision under one byte budget. “Near-lossless” describes empirical quality, not byte/state equality. | Capacity/quality research. Its batch-one A100 fused decode kernel is **7.5–65.9 times slower** than dense at every measured point. Count decomposition/reconstruction and residual scratch; compression-stage speedups do not imply generation speedups. |
| [VeriCache](https://arxiv.org/html/2605.17613v1), 2026-05-17 preprint | Compressed cache drafts; separately retained full cache verifies, with greedy-output qualification and transfer/computation overlap. | Can reduce GPU residency while increasing total storage. Count both representations, transfer buffers, verify workspace and hybrid state; a shared-memory laptop and one request lack the evaluated independent-resource overlap. |

An exact recent window does not make the older low-rank cache exact. Basis
rotation changes all associated coefficient/checkpoint identities; an update
must commit consistently or roll back the entire representation. None of these
schemes is an automatic replacement for `Vec<u16>` FP16-bit storage.

For a proposed experiment report `unique weights + unique KV + live recurrent
state + retained checkpoints + pending factors/bypass + rollback + peak scratch
+ metadata`, with mappings/device allocations in their own counter domains.
Reserve space for transient old/new representations and asynchronous users.
Use the [algorithm constraints](inference-algorithms.md#2026-state-traffic-and-verification-research)
for buffered or speculative state. Any approximation needs separate artifacts
or a declared numerical profile, full-continuation/quality checks and target-device
evidence under the [CI-only performance policy](validation.md).

## 2026 entropy coding and alternative weight artifacts

The same source review distinguishes lossless byte compression from choosing new
quantized weights. Reaching a smaller file or a lower reconstruction objective
does not establish fewer executable bytes or faster generation.

| Source | Relevant memory/optimization idea | Current-artifact boundary |
| --- | --- | --- |
| [Shannonic](https://proceedings.mlsys.org/paper_files/paper/2026/hash/96f39c8de84678cb2a908cd52bfd7819-Abstract-Conference.html), MLSys 2026 | Range-partitioned ANS reduces decoder table state; independent streams can trade index overhead for parallel decode. | Lossless coding can be studied without changing symbols/scales. Charge stream boundaries, indexes, scratch and decoding; communication-bound results do not establish local inference savings. |
| [QTEA](https://arxiv.org/abs/2609.00224v2), v2 2026-09-02 | Ternary bulk plus salient-column residuals separates compact common work from exceptional values. | New quantizer/artifact with extra residuals and indexes; not lossless packing of current Swift. GPU/architecture estimates do not establish CPU benefit. |
| [Tetra](https://arxiv.org/abs/2609.35465v1), 2026-09-28 | Lattice reconstruction illustrates why serialized code bits can differ from the representation streamed by a kernel. | New weights and measured quality losses. Include execution layout/codebook/scale storage, not just the headline file bitrate. |
| [CoS-VQ](https://arxiv.org/abs/2610.37336v1), 2026-10-27 | Coordinate shared scales and vector codewords; verifies true reconstruction error after restricted surrogate search. | Same Qwen base family is relevant, but new quantized weights differ from Swift. Restricted optimality is not a global model-quality or memory optimum. |
| [YAQA](https://arxiv.org/abs/2505.22988v3), ICML 2026; [FastKron](https://arxiv.org/abs/2608.06291v2), v2 2026-09-27 | Hessian-aware rounding and a more parallel core quantization algorithm can improve offline artifact preparation. | Charge Hessian/factor storage and preparation separately. Offline work/depth improvements do not imply faster token generation. |
| [Search Your Block Floating Point Scales!](https://arxiv.org/abs/2605.12464v1), MLSys 2026 | Explicit scale search can lower block MSE compared with absmax. | Changing activation scales changes Q8_K/Q8_0 semantics; exhaustive finite-search optimality and a heuristic short window are different claims. |

Cross-block entropy coding does not contradict the independent fixed-block bound.
Conversely, changing codebooks, scales, rotations, sparsity or residual weights
does not qualify as a lossless export merely because its average bitrate is
smaller. Future artifacts need their own length/hash, activation arithmetic,
quality, compatibility and runtime qualification. The
[SIMD review](simd-plan.md#cpu-kernel-research-reviewed-in-october-2026) and
[register](research-register.md) retain additional lattice/rotation/hardware leads,
including withdrawn and unsupported proposals.
