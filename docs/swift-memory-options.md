# Swift memory options

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

| Option | Current state | Quantified scope / next check |
| --- | --- | --- |
| Current-symbol PQ2 → PTQ1 | Verified export and bounded native decode pass | Saves 1,259,520,000 weight bytes (1.1730 GiB); qualify native output and performance separately. |
| mmap, no repack | Selected | Avoids a separate complete private weight copy when the backend supports mapped tensors; inspect actual buffer types. |
| Lower microbatch | 64 selected | A 32-token variant may reduce graph memory; obtain allocation receipts before changing it. |
| Fewer output rows | Completion normally samples one row | One F32 logits row is 993,280 bytes; confirm the retained row count. |
| Hadamard/sign cleanup | Main buffers already shared | At most 4 MiB temporary staging plus 114,688 bytes retained sign metadata. |
| F32/BF16 narrowing | Original types retained | Only 10.09 MiB F32 and 45 MiB BF16 exist; narrowing is not a lossless default. |
| Thread stacks | Four matching decode/batch threads | A small allocation opportunity; require actual stack-depth evidence. |

## Exact PTQ1 packing

PTQ1 stores each 128-symbol group in **28 bytes**, versus PQ2's **34**: 24
five-trit bytes, two four-trit bytes and the unchanged two scale bytes. The
pinned codec uses ceiling-scaled base-3 codes, 16-lane then 8-lane order, and a
canonical zero unused tail trit. Conventional group-256 TQ1 cannot retain both
original group-128 scales.

For arbitrary independent 128-trit blocks, `ceil(128 log2(3)) = 203` bits require
26 whole code bytes. PTQ1 meets this byte-aligned bound before its scale.
Cross-block compression has different tradeoffs; see the
[counting bound](inference-algorithms.md#compression-bound-and-deferred-experiments).
Physical FP16 KV is implemented in the experimental Rust reference; immutable
checkpoint sharing remains pending. Both are separate from weight packing.

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
a separately qualified PTQ1×Q8_K kernel. BITCOS is [retired](bitcos-retirement.md).

## Mapping and startup

The [CPU check](../scripts/Invoke-CpuCheck.ps1) selects mmap, no repack, no warmup,
CPU buffers and 128/64 batch/microbatch. The
[loader](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/src/llama-model-loader.cpp)
can point supported CPU tensors into the read-only mapping; other buffer types
may copy them. Inspect actual load receipts before counting savings. Complete
hashing/census/export before inference; use bounded buffers rather than whole-file
arrays or model-wide float conversion.

A future native build could make whole-file prefetch optional. The pinned
[Windows mapping code](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/src/llama-mmap.cpp)
prefetches the complete mapping and cannot unmap fragments. Windows
[prefetch](https://learn.microsoft.com/en-us/windows/win32/api/memoryapi/nf-memoryapi-prefetchvirtualmemory)
populates the physical cache, not the process working set. Disabling it could
avoid eager startup caching, but does not reduce tensor bytes or prove sustained
savings once inference touches the weights. No current CLI switch exposes it.

## Graph and output buffers

Record graph/workspace high-water allocations before tuning. The
[context allocator](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/src/llama-context.cpp)
reserves prompt/decode graphs and grows the persistent output buffer to the
largest requested count. With 248,320 vocabulary entries, reducing a retained
64-row F32 logits buffer to one row would save **62,576,640 bytes**. This is
conditional: completion may already retain only one row, and graph logits can
differ from persistent host output. Set `n_outputs_max=1` only after confirming
all single-sequence paths. Avoid optional embeddings, capture layers, MTP and
backend sampling outputs unless requested.

Microbatch 32 preserves context length but changes execution grouping and needs
numerical/device qualification. One F32 17,408-wide activation occupies
4,456,448 bytes at 64 tokens and 2,228,224 at 32. These are component sizes;
do not multiply them by all layers because the planner reuses storage. The
[CPU backend](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/ggml/src/ggml-cpu/ggml-cpu.cpp)
retains a maximum-sized work buffer. Trimming it after prefill needs a native
change and may cause later allocations.

## Smaller opportunities

The [Hadamard implementation](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/src/llama-model.cpp)
already shares one 1024² F32 rotation per CPU buffer type and signs by width/type.
Retained rotation/sign/metadata payload is 4,423,680 bytes. Direct CPU-buffer
construction might remove a temporary 4 MiB vector; releasing finished sign
metadata could recover 114,688 bytes. Counting one rotation per transformed
weight would count shared storage repeatedly. A butterfly rewrite changes
floating-point order and needs numerical review.

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
is not committed/resident memory; inspect PE headers and stack depth before
claiming savings or lowering limits.

Qualify one explicit representation/allocation change at a time. Serialized
byte savings alone do not justify reducing the service cap or physical reserve.
