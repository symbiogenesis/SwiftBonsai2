# Hexagon deployment and execution contracts

Current builds use CPU execution, including scalar/NEON in the [Rust runtime][rust-runtime].
X1/X2 Hexagon inference remains unqualified; automatic selection is unimplemented and the
[experimental lock](../config/hexagon-experiment.lock.json) keeps admission disabled.
[PLAN.md](../PLAN.md#hexagon-npu) owns outstanding work; [profile policy](hardware-capabilities.md#automatic-profile-selection)
governs device/workload eligibility and interactive/battery objectives.

The [source audit, placement ledger and access evidence](hexagon-source-audit.md) distinguish signed
reference access, the unpublished native Rust Prism boundary and the newer signed Q2 lead. The first
Swift prototype is one stateless FFN projection. CPU-only [layout](../tools/hexagon/README.md) and
[operation/wire controls](../tools/hexagon/packed-projection.md) qualify no signed device implementation,
measured VTCM/traffic or inference.

[Preflight](../scripts/Invoke-HexagonPreflight.ps1) inventories drivers and an explicit optional package
without loading/installing it. It has an aggregate deadline and normal cancellation cleanup; abrupt
parent-death Job containment remains a diagnostic-helper limitation. [Hosted CI](../.github/workflows/hexagon-qualification.yml)
owns full regressions and source-bound preparation. Its main-only opt-in owner job selects one inventory,
numeric dispatch or native session probe within five minutes total, including setup and cleanup.

## Device and route boundaries

Acceptance requires completed-response latency or energy benefit within memory, quality and sustained-comfort
limits against the same Swift artifact on CPU and qualified Adreno. TOPS, utilization and simultaneous
CPU/GPU/NPU activity do not qualify a backend. X1/X2 qualify independently.

| Device class | Execution boundary |
| --- | --- |
| Recorded X1E-78-100 / v73 | Packed HVX arithmetic, native signed access, mapping-window/dynamic-mapping costs |
| X2 / v81, initially Elite Extreme | Batched HMX prefill, packed HVX decode, verified DMA/VTCM limits and matching signed package |

Receipts bind exact SKU/capabilities, driver/runtime/kernel bytes, ABI and resource limits; processor names
establish none of these. Unknown or changed facts exclude the candidate. Shared profile policy governs
admission, fallback, bounded lifetime and teardown; packed work needs a trusted operation oracle.
[DMA64][hexagon-dma64] targets v81+ to avoid old-window dynamic weight mappings via DMA/VTCM.
[Q1/Q2 Hadamard matmul and row reads][hexagon-q2] are source leads; neither proves Swift PQ2/PTQ1,
full-graph or signed Windows coverage. Rebuilt source and bundled signed binaries have distinct identities.

| Route | Required boundary |
| --- | --- |
| Prism/ggml Hexagon | Packed types, transforms, hybrid graph, adapters and signed deployment together |
| GenieX `llama_cpp` | Exact plugin/package, supported format and actual placement |
| GenieX `qairt` or ONNX Runtime GenAI/QNN | Faithful prepared package, chipset/precision/shapes/cache identity and bounded memory |
| Native Rust device boundary | Safe ownership API, audited driver ABI, no C/C++ inference-library dependency |

[GGUF/QAIRT plugins][geniex-sdk], a [Q4_0 recommendation][geniex-models] and the [ternary HTP prototype][htp-prototype]
do not establish Swift support. Supported-type fixtures or separately pinned small models establish access
only; they cannot change model selection. Concurrent residency with another large control model exceeds the
32 GB contract. Wrapping an external engine in Rust does not satisfy native replacement.
[GenAI/QNN][qnn-genai] requires a prepared package, not a GGUF provider switch: 27B weights at one/two bytes
cost roughly 27/54 GB before scales/state/duplicates. Expansion needs a demonstrated bounded representation.

The [GenieX v0.8.0 catalog][geniex-signed] covers only its bundled v73/v81 kernels, not newer or rebuilt code.
Custom binaries require supported signing/distribution. [Test-signing][hexagon-windows], Secure Boot,
boot/signing policy and certificate trust changes require a separate owner decision. Missing signed
deployment blocks execution.

## Packed arithmetic and graph state

The [assessment lock](../config/swift-assessment.lock.json) identifies PQ2; [current-weight PTQ1](swift-memory.md)
is separate. Fix weights, tokenizer/template, sampler, transforms, attention and declared activation/cache
precision within comparisons. PQ2 Q8_K (odd-group Q8_0), PTQ1 Q8_0 and generic upstream types have distinct
arithmetic. [SIMD contracts](cpu-kernels.md) own fixtures; pinned Prism remains the whole-model oracle until
the relevant Rust parity scope qualifies.

Q2_0 has 64 weights per scale versus PQ2's 128. Splitting blocks and duplicating scales adds about 5.9%
packed storage while preserving symbols, raw scale bits and transform metadata. Equal decoded weights do
not establish activation quantization, reduction/scale order, logits or PQ2/Q8_K state parity.
[Operation fixtures](cpu-kernels.md#packed-arithmetic) cover fourth symbols, extremes/zeros, malformed/unaligned
blocks, tails, awkward dimensions, integer sums/scale order and independent PTQ1 canonicality/Q8_0. Initial
parity is bitwise; FMA/reassociation or precision changes need declared tolerances and quality acceptance.
HVX/HMX must support the arithmetic; HMX prefill needs batched weight reuse and compatible hybrid-state scheduling.

Transform normalization/signs/axes and grouped recurrent basis are artifact identity. [LoRA basis correctness](swift-lora.md),
including grouped SSM, survives fusion; no-LoRA and approved-adapter forwards have separate evidence. The
placement ledger covers embedding, packed/floating projections, norm/RoPE, attention, GDN/convolution,
FFN/output, adapters, caches/checkpoints, strides/shapes/precision and every prefill/decode fallback, transfer
and synchronization boundary. Contiguous regions and resident weights/scratch need explicit ownership.
KV/convolution/GDN state, checkpoints and phase layouts must preserve logits/state and deterministic streams
across fresh/reused/edited prefixes, reset, cancellation, failure and restored continuation.

[Chunkwise WY/UT GDN](inference-algorithms.md#chunkwise-gated-deltanet-prefill) has a separate schedule/float-order
contract. The [multiplication-only inverse][approximate-inverse] uses truncation/residual correction; strict
triangularity does not bound low-order error. Phone results do not qualify Windows X Elite.
[Low-bit LUT KV](inference-algorithms.md#compression-and-approximation-constraints) differs from lossless physical
FP16. Requantization, training, retrieval embeddings, drafters and speculation qualify separately.

## Memory, lifecycle and failure

Apply the [accelerator resource contract](hardware-capabilities.md#accelerator-storage-and-lifetime) to storage,
asynchronous buffers, recovery and private caches. Packed weights may stay in shared RAM; bound VTCM expansion,
DMA staging, padding/alignment and repack scratch. Persistent expansion needs its own budget. One session is
the control; partitions charge overhead and state duplication.

The [X1/v73 design][hexagon-design] describes an approximate 3.5 GB mapping window/dynamic mapping; the selected
signed X2/v81 runtime has separate DMA64/VTCM-only constraints. Address windows are not physical capacity.
Preserve the Swift 11.5 GiB service allowance and 4 GiB physical reserve, including allocations outside Job
commit; Qwen's 18 GiB allowance does not apply. Smaller placement needs a qualified profile; refusal cannot
silently shorten context or change model policy.

Retain reviewed libraries, fixed arguments, child-only environment, authentication, exact Host/Origin/routes,
canonical counts, one active conversation, caps, cancellation and deadlines. Readiness binds effective
placement/resources. Strict backend requests fail explicitly; preferred fallback precedes generation under
qualified eligibility and memory/state-transition rules. Arbitrary inherited overrides remain unsupported.
Helpers need owned lifetime, parent-death teardown and independent resource/isolation qualification;
drivers/services are not implicitly contained by the single-process Job.

## Qualification evidence

Correctness precedes timing. Shared [qualification](hardware-capabilities.md#qualification-scope) and
[measurement rules](performance-expectations.md#2026-literature-measurement-and-comparison) own identities,
workload/state/power cases, cost accounting, acceptance and upgrades. Separate NPU RPC/submission,
synchronization, mappings and supported counters. Verify effective power modes: the [guide][hexagon-guide]
describes burst aliasing and warning fallback for unknown modes. Utilization/overlap proves no benefit.

Admitted 8K trials use 512/2K/near-limit prompts, full output reservation, fixed 128/512-token and natural
creative outputs, occupied state, cancellation and failed recovery. Larger contexts qualify separately.
Compare same-contract CPU, qualified Vulkan and NPU; retain median/p95/variance and separate 15–30-minute
sustained runs in CI. Owner diagnostics/trials remain subject to the single five-minute total ceiling.
Missing evidence or critical failures keep profiles experimental; absence supplies no verdict.
Native diagnostics use absolute system-temp lanes with inherited Windows system/profile variables.

## 2026 research evidence

The [mobile catalog E3–E8](research-register.md#2026-mobile-acceleration-and-speech) owns the **2026-10-29** review
and counterexamples. Keep prepared/static graphs, direct ggml HVX/HMX and signed Windows packages distinct.
Attribute quantization, offload, polling/wake, profiling and fallback against a competitive CPU; fast HMX GEMM
can lose to graph breaks. Validate static-context limits/loading order without reducing reservations.
Different packages, contexts or always-NPU reranking cannot rank same-artifact Swift or establish a GPU ceiling.
Sparse attention and fitted rotations/scales change dense/GDN computation or dynamic Q8_K/Q8_0/lossless PTQ1
artifacts. Check foreground jank even at stable TPS; Windows arbitration needs independent evidence and
speculative yields need accepted-prefix verification/recovery.

These upstream regression leads are outside Runner's pin; moving links are not source pins:

- [DMA-ring fix #30189](https://github.com/ggml-org/llama.cpp/pull/30189), merged **2026-10-09**: descriptor-ring
  saturation silently dropped DDR-to-VTCM transfers. Check initialization, saturation, completion and reuse
  beyond small shapes; its convolution/patch-embedding failure is no proven Swift defect.
- [b11913](https://github.com/ggml-org/llama.cpp/releases/tag/b11913), prerelease **2026-10-27**: VTCM-size requests
  add no available VTCM or qualified concurrency. Bind allocation/refusal to the signed runtime; inherited
  overrides remain unsupported.

[rust-runtime]: https://github.com/symbiogenesis/prism-inference/blob/main/README.md
[geniex-sdk]: https://github.com/qualcomm/GenieX/blob/main/sdk/README.md
[geniex-models]: https://geniex.aihub.qualcomm.com/en/models/supported
[geniex-signed]: https://github.com/qualcomm/GenieX/releases/tag/v0.8.0
[hexagon-windows]: https://github.com/ggml-org/llama.cpp/blob/master/docs/backend/snapdragon/windows.md
[hexagon-guide]: https://github.com/ggml-org/llama.cpp/blob/master/docs/backend/snapdragon/README.md
[hexagon-design]: https://github.com/ggml-org/llama.cpp/blob/master/docs/backend/snapdragon/developer.md
[hexagon-dma64]: https://github.com/ggml-org/llama.cpp/pull/29197
[hexagon-q2]: https://github.com/ggml-org/llama.cpp/pull/30627
[qnn-genai]: https://github.com/microsoft/onnxruntime-genai/blob/main/docs/qnn.md
[htp-prototype]: https://github.com/aakar-mutha/llama.cpp/tree/ternary-bonsai-htp
[approximate-inverse]: https://arxiv.org/abs/2606.06034
