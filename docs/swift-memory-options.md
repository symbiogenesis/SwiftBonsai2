# Additional Swift Bonsai 2 memory options

This October 23, 2026 source audit uses the current Swift PQ2 artifact pinned in
[the assessment lock](../config/swift-assessment.lock.json), the actual
[symbol census](../config/swift.pq2-census.json), and Prism commit
`73bfe6cc485554822f80c473a3c0c4f25e80f41b`. It describes allocation ceilings and
implementation choices, not measured peak RAM or throughput. Runtime and device
qualification remain governed by [the assessment](swift-memory.md).

The owner excludes **new retraining or fine-tuning on this machine**. This audit
is limited to exact repacking of the current weight symbols and scale bits, and
reviewed runtime allocation choices. Do not substitute older checkpoints or
introduce a local training/quantization workflow to reach a smaller file.
An independently pretrained adapter can be evaluated separately for compatibility
and quality; no adapter is enabled by this audit, and an adapter is not a
lossless memory optimization of the original model function.

The largest additional opportunity is **losslessly repacking the current PQ2
symbols into Prism's existing PTQ1 format**. This preserves this artifact's
merged weights and scale bits; it does not select the publisher's older PTQ
checkpoint. Native ARM PTQ kernels already exist, but their activation rounding
differs from the PQ2 route, so native output correctness still needs separate
qualification. The alternate packing experiment was [retired](bitcos-retirement.md).

| Option | Current state | Quantified scope | Recommended next action |
| --- | --- | ---: | --- |
| Current-symbol PQ2 → PTQ1 | Complete verified export and bounded native decode pass | 1,259,520,000 fewer weight bytes, **1.1730 GiB** | Qualify native PTQ output quality and performance; keep selection explicit. |
| mmap and no repack | Already selected by the Swift CPU profile | Avoids a separate complete private weight copy; no new saving to count twice | Preserve these choices and verify actual backend buffer types. |
| Lower prefill microbatch | Already 64, versus the old Qwen profile's 128 | Graph-dependent; no defensible whole-graph byte saving yet | Consider an explicit 32-token variant only after a successful 64-token baseline. |
| Avoid unnecessary output rows | Completion normally needs one sampled row | One F32 row is 993,280 bytes; 64 rows are 63,569,920 bytes | Inspect reserve/output logs before changing native output limits. |
| Hadamard construction/sign metadata cleanup | Main buffers already shared | At most 4 MiB temporary matrix staging plus 114,688 bytes retained sign-array payload | Defer until larger opportunities are qualified. |
| F32/BF16 weight narrowing | Original types already retained | Only 10.09 MiB F32 and 45 MiB BF16 exist | Do not silently narrow or requantize them. |
| Thread stacks | Four matching decode/batch threads configured | A few worker stacks, not GiB | Keep matching pools; do not lower stack limits without stack-depth evidence. |

PTQ stores the same 128-symbol scale group in 28 bytes rather than PQ2's 34:
24 five-trit bytes, two four-trit bytes, then the unchanged two scale bytes.
The code bytes use the pinned ceiling-scaled base-3 representation and its
16-lane then 8-lane ordering. The tail contains a canonical zero unused trit.
Conventional 256-group TQ1 cannot preserve both original 128-group scales.
For arbitrary independent 128-trit blocks, `ceil(128 log2(3)) = 203` bits need
26 whole code bytes; PTQ1 already meets that byte-aligned bound before its
unchanged scale. Cross-block coding/correlations have different tradeoffs. See
the [counting bound and pending Rust cache-storage work](inference-algorithms.md#compression-bound-and-deferred-experiments);
physical FP16 KV and immutable checkpoint sharing are distinct from weight
repacking and from this pinned native engine's existing FP16 cache allocation.
The [native block definitions](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/ggml/src/ggml-common.h)
and [codec](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/ggml/src/ggml-quants.c)
define this layout. [Pq2Ptq1Codec.cs](../scripts/Pq2Ptq1Codec.cs) copies symbols
and raw scales directly, never running the float quantizer. Its batch methods
write into caller-owned buffers without per-block allocations; the
[focused regressions](../tests/Test-Pq2Ptq1Codec.ps1) independently cover every
canonical trit word at every lane, all symbol positions, scale bits and malformed
representations.

There are 209,920,000 source blocks, so the exact payload reduction is
`209,920,000 × (34 − 28) = 1,259,520,000` bytes. All current PQ2 tensor block
counts are divisible by sixteen, preserving 32-byte alignment after conversion.
The original non-tensor bytes equal its 11,120,992-byte data offset, with no
inter-tensor or trailing gaps. A same-length header and tightly rebuilt tensor
layout therefore account for **5,946,648,928 bytes total**. The final converter
must verify its actual length and digest. Counting the old and new full weight
buffers at once would negate the claimed steady-state saving.

The [ARM implementation](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/ggml/src/ggml-cpu/arch/arm/quants.c)
decodes PTQ trits inside its dot product rather than retaining a whole expanded
model. However, the [CPU type traits](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/ggml/src/ggml-cpu/ggml-cpu.c)
select Q8_K activations for PQ2 and Q8_0 for PTQ1. Exact decoded weights do not
imply identical activation quantization, reduction order, logits or generated
tokens. Keep the representation choice explicit and retain the original source
artifact. To preserve PQ2's numerical route as well, a separately qualified
PTQ1×Q8_K kernel would be needed. No throughput advantage is inferred here.

The current [Swift check](../scripts/Invoke-CpuCheck.ps1) already selects mmap,
no repack, no warmup, CPU-only buffers, four threads and 128/64 batch/microbatch.
The [loader](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/src/llama-model-loader.cpp)
can point CPU tensor data into the read-only mapping; unsupported buffer choices
can instead require copies, so inspect the actual load receipt. Run artifact
hashing/census/conversion to completion and close their buffers before starting
inference. Use bounded buffers and a new verified output artifact, not whole-file
byte arrays or model-wide dequantized floats. Keeping the original file on disk
does not itself require another live model allocation.

There is a narrower startup option in a future native build: avoid whole-file
prefetch. The pinned model loader calls mapping initialization with prefetch
enabled. On Windows, the [mapping code](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/src/llama-mmap.cpp)
requests the file's entire range; fragment unmapping is a no-op on that platform.
Microsoft documents that [prefetch populates the physical cache, not the
process working set](https://learn.microsoft.com/en-us/windows/win32/api/memoryapi/nf-memoryapi-prefetchvirtualmemory).
Making this optional could avoid eagerly caching up to the mapped file size
at startup. It does not reduce live tensor bytes or establish sustained savings
after dense inference touches the weights. No current CLI switch exposes this
prefetch choice, and no global cache or locking policy change is warranted.

Graph memory needs a successful native initialization receipt before further
tuning. The [context allocator](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/src/llama-context.cpp)
reserves prompt and decode graphs and restores the prompt reservation. Its
output buffer grows to the largest requested output count and is not normally
shrunk. The vocabulary has 248,320 entries, so reducing an actually retained
64-row F32 logits allocation to one row would save 62,576,640 bytes. This is a
conditional calculation, not a claim that the current completion allocates
64 persistent output rows. A future native integration can set `n_outputs_max`
to one only after confirming its single-sequence prompt/decode paths request
one output. Reserve-time graph logits can also differ from the persistent
host output allocation. Avoid embeddings, capture layers, MTP and optional
backend sampling outputs unless their functionality is requested.

Reducing microbatch from 64 to 32 keeps the context size unchanged but changes
execution grouping, so numerical/device qualification still applies. As a
component calculation, one F32 17,408-wide FFN activation occupies 4,456,448
bytes at 64 tokens and 2,228,224 at 32. These are not whole-graph totals and must
not be multiplied by all 64 layers: the graph planner reuses non-overlapping
lifetimes. The [CPU backend](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/ggml/src/ggml-cpu/ggml-cpu.cpp)
also retains a maximum-sized work buffer rather than allocating all operator
scratch simultaneously. Trimming it after prefill would require a native change
and may force later allocations; measure its actual high-water size first.

The [model's Hadamard construction](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/src/llama-model.cpp)
already shares one 1024² F32 rotation for the CPU buffer type and shares signs
by width/buffer type. Retained payload is 4,423,680 bytes including its three
F32 sign vectors and retained int32 metadata. Its extra 4 MiB construction
vector expires after upload. Filling an eligible CPU buffer directly could
remove that temporary vector; releasing sign metadata after every consumer
finishes could recover only 114,688 payload bytes. A butterfly Hadamard rewrite
would also change floating-point operation order and needs separate numerical
review. Multiplying 4 MiB by 401 transformed weight names would incorrectly
count buffers that are already deduplicated.

The original F32/BF16 tensors total only 57,767,936 bytes. Even narrowing every
F32 element to sixteen bits could save at most 5,291,008 bytes and would require
proving every value and affected operation exact. BF16 is already sixteen bits
and has a [native BF16 dot route](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/ggml/src/ggml-cpu/ggml-cpu.c);
there is no demonstrated complete F32-expanded BF16 weight copy to eliminate.
FP16 conversion can lose BF16 range and is not a lossless default. The existing
8K FP16 KV and recurrent-state estimates are 536,870,912 and 156,893,184 bytes.
Shrinking context or changing their types would alter the requested contract
and is outside this audit's lossless savings.

[Matching thread-pool settings](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/common/common.cpp)
avoid a separate batch pool. In the non-OpenMP CPU path, four threads mean three
worker threads plus the calling thread. Windows uses executable-header stack
defaults when creation supplies zero. [Reserved stack address space is distinct
from committed/resident pages](https://learn.microsoft.com/en-us/windows/win32/procthread/thread-stack-size),
and the usual 1 MiB linker reserve is not evidence of 1 MiB resident per worker.
Actual PE headers and stack depths would be needed for a tighter bound. This is
a small opportunity compared with repacking more than a GiB of live weights.

The audit changed no runtime switches or model/KV precision and launched no
model. Validate the settled native runtime first, then qualify one explicit
representation change at a time with exact artifact provenance. Keep the
physical reserve and Job/watchdog policy; serialized byte arithmetic alone does
not justify reducing them.
