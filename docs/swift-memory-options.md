# Swift memory options

This allocation audit binds the [exact PQ2 lock](../config/swift-assessment.lock.json),
[symbol census](../config/swift.pq2-census.json) and Prism
`73bfe6cc485554822f80c473a3c0c4f25e80f41b`. Calculations describe serialized bytes or
opportunities, not measured peak RAM/throughput. Keep the [assessment](swift-memory.md)
limits, reserve and watchdogs. Lossless work excludes older checkpoints, weight/KV
precision changes, adapters as "savings" and local training. Outstanding work lives in
[PLAN.md](../PLAN.md#creative-quality-adapters-and-memory).

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

PTQ1 stores each 128-symbol group in **28 bytes**, versus PQ2's **34**: 24 five-trit
bytes, two four-trit bytes and the unchanged two scale bytes. The pinned codec uses
ceiling-scaled base-3 codes, 16-lane then 8-lane order, and a canonical zero unused tail
trit. Conventional group-256 TQ1 cannot retain both original group-128 scales.

For arbitrary independent 128-trit blocks, `ceil(128 log2(3)) = 203` bits require 26
whole code bytes. PTQ1 meets this byte-aligned bound before its scale. Cross-block
compression has different tradeoffs; see the [counting
bound](inference-algorithms.md#compression-and-approximation-constraints). Physical FP16
KV in the experimental Rust reference and checkpoint-storage behavior are separate from
weight packing.

The [native
layout](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/ggml/src/ggml-common.h)
and
[codec](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/ggml/src/ggml-quants.c)
define the representation. The [Rust ternary
crate](https://github.com/symbiogenesis/prism-inference/blob/main/tools/ternary/README.md)
and [GGUF
exporter](https://github.com/symbiogenesis/prism-inference/blob/main/tools/gguf/README.md)
copy symbols/raw scales without floating-point quantization, using bounded caller-owned
buffers and exact PQ2 reconstruction checks.

For the actual census:

```text
209,920,000 blocks × (34 − 28) = 1,259,520,000 saved payload bytes
verified complete PTQ1 file  = 5,946,648,928 bytes
```

All converted tensor block counts are divisible by sixteen, retaining 32-byte alignment.
Non-tensor bytes occupy the 11,120,992-byte header/data-offset region; there are no
source inter-tensor or trailing gaps. The [export
receipt](../config/swift.ptq1-export.json) records the verified final length and digest.
Retain the original source on disk, but avoid holding complete old/new buffers
simultaneously.

The [ARM dot
kernel](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/ggml/src/ggml-cpu/arch/arm/quants.c)
decodes PTQ in place rather than retaining an expanded model. However, [CPU type
traits](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/ggml/src/ggml-cpu/ggml-cpu.c)
use Q8_K activations for PQ2 and Q8_0 for PTQ1. Equal decoded weights do not imply equal
logits, tokens or speed. Preserving PQ2's activation route would require a distinct
qualified PTQ1×Q8_K kernel.

## Packing decision

Current-weight PTQ1 is the supported lossless packing option.
[BITCOS](https://arxiv.org/abs/2609.16338v1) (2026-09-14 preprint) is not supported.
Swift's 32.76154% zero-symbol rate falls below BITCOS's 37.5% ideal same-scale PTQ1
break-even before offsets, padding or indexes. Its proposed layout would occupy
6,158,339,606 bytes before container/index overhead, **211,690,678 bytes more** than the
verified 5,946,648,928-byte PTQ1 export.

The rejected Cobalt 100 ARM64 synthetic prototype was also slower than PQ2. That result
does not establish a universal BITCOS limit, Snapdragon behavior or full-model
performance. The serialized-size disadvantage is sufficient to exclude the proposed
layout from this artifact's current packing choices.

## Mapping and startup

The [CPU check](../scripts/Invoke-CpuCheck.ps1) selects mmap, no repack, no warmup, CPU
buffers and 128/64 batch/microbatch. The
[loader](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/src/llama-model-loader.cpp)
can point supported CPU tensors into the read-only mapping; other buffer types may copy
them. Load receipts determine which allocations count as savings. Hashing/census/export
use bounded buffers rather than whole-file arrays or model-wide float conversion and
precede inference.

The pinned [Windows mapping
code](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/src/llama-mmap.cpp)
prefetches the complete mapping and cannot unmap fragments. Windows
[prefetch](https://learn.microsoft.com/en-us/windows/win32/api/memoryapi/nf-memoryapi-prefetchvirtualmemory)
populates the physical cache, not the process working set. Disabling it could avoid
eager startup caching, but does not reduce tensor bytes or prove sustained savings once
inference touches the weights. No current CLI switch exposes it.

## Graph and output buffers

The [context
allocator](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/src/llama-context.cpp)
reserves prompt/decode graphs and grows the persistent output buffer to the largest
requested count. With 248,320 vocabulary entries, reducing a retained 64-row F32 logits
buffer to one row would save **62,576,640 bytes**. This is conditional: completion may
already retain only one row, and graph logits can differ from persistent host output.
Single-row output, optional embeddings, capture layers, MTP and backend sampling each
have separate allocation contracts.

Microbatch 32 preserves context length but changes execution grouping, so the 64-token
profile's numerical/device evidence does not transfer to it. One F32 17,408-wide
activation occupies 4,456,448 bytes at 64 tokens and 2,228,224 at 32. These are
component sizes; do not multiply them by all layers because the planner reuses storage.
The [CPU
backend](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/ggml/src/ggml-cpu/ggml-cpu.cpp)
retains a maximum-sized work buffer. It has no current post-prefill trimming contract;
releasing it could cause later allocations.

## Other allocation bounds

The [Hadamard
implementation](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/src/llama-model.cpp)
already shares one 1024² F32 rotation per CPU buffer type and signs by width/type.
Retained rotation/sign/metadata payload is 4,423,680 bytes, alongside a temporary 4 MiB
initialization vector. Finished sign metadata accounts for 114,688 bytes. Counting one
rotation per transformed weight would count shared storage repeatedly. A butterfly
rewrite changes floating-point order rather than merely reducing storage.

F32/BF16 weights total 57,767,936 bytes. Narrowing all F32 to sixteen bits could save at
most 5,291,008 bytes and would require exactness proofs. BF16 already has a [native dot
route](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/ggml/src/ggml-cpu/ggml-cpu.c);
there is no demonstrated complete F32-expanded copy to remove. FP16 can lose BF16 range.
The 8K FP16 KV/recurrent estimates remain 536,870,912 and 156,893,184 bytes; lowering
context or precision changes the contract.

[Matching thread
settings](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/common/common.cpp)
avoid a separate batch pool. Four threads mean three workers plus the caller in the
non-OpenMP path. Windows [stack
reservation](https://learn.microsoft.com/en-us/windows/win32/procthread/thread-stack-size)
is not committed/resident memory. PE headers and actual stack depth determine whether
stack settings represent a material allocation bound.

Serialized byte savings alone do not justify reducing the service cap or physical
reserve. The [PTQ1 contract](ptq1-readiness.md) describes the supported selection and
unchanged admission threshold.

## 2026 state and cache research outside the lossless scope

Reviewed through **2026-10-29**; source details and proof limits live in the [research
register](research-register.md). These candidates do not change the selected lossless
packing, FP16 KV/FP32 recurrent state or admission. Swift's 16 full-attention and 48
recurrent layers need separate ledgers: KV compression does not compress GDN state or
model weights.

### Persistent checkpoint compression versus live-state quantization

| Source | Distinction and Swift boundary |
| --- | --- |
| [DASC](https://arxiv.org/html/2608.30386v2), v2 2026-10-09 | Ragged checkpoints omit selected GDN heads/KDA channels, then zero-fill or approximately refresh them. Live state stays full precision; convolution is uncompressed. Count selection, packing and tensor-parallel balancing. Multi-GPU SGLang capacity results do not justify omitting Swift state; compare exact sparse checkpoints plus suffix replay first. |
| [DAMP](https://arxiv.org/html/2608.27513v2), v2 2026-09-30 | Calibrated FP16 high-risk channels plus INT8 live state, scaling/reordering and stochastic rounding average 9.9 bits including specified metadata. Batches 16-256 and substantial uniform-four-bit accuracy loss do not establish exact continuation, batch-one gain or Swift/LoRA quality. |
| [Preserving Updates](https://arxiv.org/html/2610.15709v1), 2026-10-11 | Low-rank pending-update bypass and bounded directional rounding address repeated small-update loss. RTX 3060/batches 16-48 are approximation evidence. Charge factors, scales, merge scratch and checkpoint/rollback lifetimes; per-step MSE does not bound coherent sequence error. |
| [Minima W4A4](https://arxiv.org/html/2609.04098v1), 2026-09-03 | Quantizes projection weights/activations, including gates; perturbation replay keeps recurrence FP32. Neither four-bit state nor uniform stability is proved. Fused projections must retain per-module scales. Its new NVFP4 Qwen artifact is larger than Swift and is not lossless repacking. |

### Low-rank full-attention KV and reconstruction costs

| Source | Representation, evidence and accounting |
| --- | --- |
| [iS-KV](https://arxiv.org/html/2610.02815v1), 2026-10-02 | Exact recent window plus approximate block-incremental-SVD older history. Basis updates must transform historical coordinates. Count bases, coefficients, protected prompt/window, FP32 updates, reconstruction and simultaneous old/new factors; persistent-KV ratios exclude weights and scratch. |
| [JoLT](https://arxiv.org/html/2607.12550v4), v4 2026-09-24 | Tucker cache plus rotated low-bit residual allocates rank/precision under one byte budget. "Near-lossless" is empirical quality. Batch-one A100 fused decode is **7.5-65.9 times slower** than dense at every measured point; count decomposition, reconstruction and residual scratch. |
| [VeriCache](https://arxiv.org/html/2605.17613v1), 2026-05-17 | Compressed drafts plus a separately retained full verification cache; greedy-output qualification. GPU-residency savings can increase total storage. Count both caches, transfers, verification and hybrid state; one shared-memory request lacks the evaluated independent-resource overlap. |

An exact recent window does not make older low-rank history exact. Basis and
coefficient/checkpoint identities must commit or roll back together. None of these
replaces exact FP16-bit storage without separate qualification.

Every experiment reports `unique weights + unique KV + live recurrent state + retained
checkpoints + pending factors/bypass + rollback + peak scratch + metadata`. Keep
mappings/device allocations in separate counter domains; reserve transient old/new
representations and asynchronous users. The [algorithm
constraints](inference-algorithms.md#2026-state-traffic-and-verification-research) cover
buffered/speculative state. Approximation needs its own artifact or declared numerical
profile, full-continuation/quality checks and target-device CI evidence.

## 2026 entropy coding and alternative weight artifacts

Lossless byte coding and new quantized weights have different contracts. Smaller files
or lower reconstruction error do not establish fewer executable bytes or faster
generation; the [register](research-register.md) owns source details.

| Source | Applicable idea and boundary |
| --- | --- |
| [Shannonic](https://proceedings.mlsys.org/paper_files/paper/2026/hash/96f39c8de84678cb2a908cd52bfd7819-Abstract-Conference.html), MLSys 2026 | Range-partitioned ANS trades table state against independent-stream indexes/decode. Can preserve symbols/scales; communication-bound wins do not establish local inference savings. |
| [QTEA](https://arxiv.org/abs/2609.00224v2), v2 2026-09-02 | Ternary bulk plus salient-column residuals is a new quantizer/artifact with residual/index cost; GPU/architecture estimates do not establish CPU benefit. |
| [Tetra](https://arxiv.org/abs/2609.35465v1), 2026-09-28 | Lattice reconstruction separates serialized bitrate from kernel-streamed codebook/scale/layout bytes. New weights have measured quality losses. |
| [CoS-VQ](https://arxiv.org/abs/2610.37336v1), 2026-10-27 | Joint shared-scale/vector-codeword search on the same Qwen base family; restricted-surrogate optimality needs true-error verification and does not establish a global quality/memory optimum or Swift compatibility. |
| [YAQA](https://arxiv.org/abs/2505.22988v3), ICML 2026; [FastKron](https://arxiv.org/abs/2608.06291v2), v2 2026-09-27 | Hessian-aware rounding and parallel core quantization concern offline preparation. Charge Hessians/factors and preprocessing; work/depth gains do not imply faster generation. |
| [Search Your Block Floating Point Scales!](https://arxiv.org/abs/2605.12464v1), MLSys 2026 | Scale search can improve block MSE over absmax. Exhaustive finite-search optimality differs from a heuristic short window; changing scales changes Q8_K/Q8_0 arithmetic. |

Cross-block entropy coding does not contradict the independent fixed-block bound.
Changing codebooks, scales, rotations, sparsity or residual weights is not lossless
export. New artifacts require their own length/hash, activation, quality, compatibility
and runtime evidence. The [SIMD
review](simd-plan.md#cpu-kernel-research-reviewed-in-october-2026) and register also
retain unsupported and withdrawn lattice/rotation/hardware leads.
