# Swift memory options

This allocation audit uses the [PQ2 lock](../config/swift-assessment.lock.json), [symbol
census](../config/swift.pq2-census.json) and Prism
`73bfe6cc485554822f80c473a3c0c4f25e80f41b`. Calculated byte savings are not measured
peak RAM or throughput. The [assessment's](swift-memory.md#native-cpu-status) limits,
reserve and watchdogs still apply. Lossless work excludes older checkpoints, weight/KV
precision changes, adapters as "savings" and local training; [PLAN.md](../PLAN.md#creative-quality-adapters-and-memory)
owns outstanding work.

| Option | Current contract | Quantified scope and limitation |
| --- | --- | --- |
| Current-symbol PQ2 → PTQ1 | Verified export and bounded native decode | Saves 1,259,520,000 weight bytes (1.1730 GiB); broader [numerical/output qualification](ptq1-readiness.md#numerical-and-admission-boundaries) and performance remain pending. |
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
preserve symbols, raw scale bits, other tensor bytes and tokenizer/template/transform
metadata without floating-point quantization. Bounded caller-owned buffers and
batchwise byte-identical PQ2 reconstruction checks avoid a complete expanded model.

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

The [PTQ1 contract](ptq1-readiness.md#supported-selection-and-resource-policy) owns
selection and unchanged admission: serialized savings alone cannot lower the service
cap or physical reserve.

## 2026 state and cache research outside the lossless scope

Reviewed through **2026-10-29**. The [research register](research-register.md) owns paper
versions, source evidence and proof limits; the tables below summarize memory-policy
implications. These candidates do not change lossless packing, FP16 KV/FP32 recurrent
state or admission. Account separately for Swift's 16 attention and 48 recurrent layers:
KV compression does not compress GDN state or weights.

### Persistent checkpoint compression versus live-state quantization

| Candidate ([register Q18–Q21](research-register.md#approximation-quantization-theory-and-numerical-analysis)) | Swift boundary |
| --- | --- |
| DASC | Omits checkpoint GDN heads/KDA channels, with zero-fill or approximate refresh; live state stays full precision and convolution uncompressed. Compare exact sparse checkpoints plus suffix replay first. |
| DAMP | Mixed FP16/INT8 live state is approximate. Its 9.9-bit average and batch-16–256 results establish neither exact continuation, batch-one gain nor Swift/LoRA quality. |
| Preserving Updates | Low-rank bypass and directional rounding address small-update loss. Charge factors/scales, merge scratch and checkpoint/rollback lifetimes; per-step MSE does not bound sequence error. |
| Minima W4A4 | Quantizes projections/gates, retaining FP32 recurrence and per-module fused scales. Its larger NVFP4 Qwen artifact is neither four-bit state nor lossless Swift repacking; uniform stability remains unproved. |

### Low-rank full-attention KV and reconstruction costs

| Candidate ([register Q25–Q26](research-register.md#approximation-quantization-theory-and-numerical-analysis), [H23](research-register.md#hybrid-recurrence-prefix-caching-and-speculative-decoding)) | Representation and accounting |
| --- | --- |
| iS-KV | Exact recent window, approximate older SVD history; basis updates transform historical coordinates. Count bases, coefficients, protected prompt/window, FP32 updates, reconstruction and simultaneous old/new factors; persistent-KV ratios exclude weights/scratch. |
| JoLT | Tucker cache plus rotated low-bit residual, jointly budgeted. "Near-lossless" describes empirical quality; retained batch-one A100 decode is **7.5–65.9× slower** than dense. Count decomposition, reconstruction and residual scratch. |
| VeriCache | Compressed drafts plus full verification cache, qualified for greedy output. Count both caches, transfers, verification and hybrid state: GPU-residency savings can increase total storage, and one shared-memory request lacks the evaluated resource overlap. |

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
generation. Source versions and results live in the register's [compression](research-register.md#packed-projections-cpu-kernels-and-compression)
and [quantization](research-register.md#approximation-quantization-theory-and-numerical-analysis) catalogs.

| Candidate / register ID | Memory and arithmetic boundary |
| --- | --- |
| Shannonic / L12 | Lossless ANS can preserve symbols/scales; charge table state, independent-stream indexes and decode work. Communication wins do not prove local inference savings. |
| QTEA / L13 | New ternary/residual artifact; charge residuals/indexes. GPU/architecture estimates do not establish CPU benefit. |
| Tetra / L16 | Distinguish serialized bits from streamed codebook/scale/layout bytes; new weights have measured quality losses. |
| CoS-VQ / Q27 | Shared-scale/codeword search on the Qwen base family; surrogate optimality needs true-error verification and proves no global quality/memory optimum or Swift compatibility. |
| YAQA / Q28; FastKron / Q29 | Offline rounding/quantization: charge Hessians/factors and preprocessing. Work/depth gains do not imply faster generation. |
| Search Your Block Floating Point Scales! / Q30 | Exhaustive legal-scale optimality differs from a heuristic short window; changing scales changes Q8_K/Q8_0 arithmetic. |

Cross-block entropy coding does not contradict the independent fixed-block bound.
Changing codebooks, scales, rotations, sparsity or residual weights is not lossless
export. New artifacts require their own length/hash, activation, quality, compatibility
and runtime evidence. The [SIMD
review](simd-plan.md#cpu-kernel-research-reviewed-in-october-2026) and register also
retain unsupported and withdrawn lattice/rotation/hardware leads.
