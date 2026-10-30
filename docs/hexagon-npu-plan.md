# Hexagon deployment and execution contracts

Current builds/launch policies use CPU execution, including scalar/NEON in the [Rust
runtime][rust-runtime]. Hexagon deployment, performance and automatic selection remain unqualified
on X1/X2. [PLAN.md](../PLAN.md#hexagon-npu) tracks the work; the shared [hardware
contract](hardware-profiles.md) governs eligibility by device, workload and interactive/battery objective.

## Device and route boundaries

Hexagon acceptance concerns completed-response latency or energy within memory, quality and
sustained-comfort limits against the same Swift artifact on CPU and qualified Adreno.
Utilization/TOPS and simultaneous CPU/GPU/NPU operation are not acceptance criteria. X1 and X2 have
independent qualification scopes.

| Device class | Relevant execution boundary |
| --- | --- |
| Recorded X1E-78-100 / Hexagon v73 | Packed HVX arithmetic, native signed runtime access, mapping-window/dynamic-mapping costs |
| X2 / Hexagon v81, initially Elite Extreme | Batched HMX prefill and packed HVX decode, verified extended DMA/VTCM constraints and signed matching package |

Processor names do not establish architecture, mapping capabilities, signed kernels or readiness.
Receipts identify exact SKU/capabilities, driver/runtime/kernel package, ABI and resource limits.
Missing or changed facts exclude the candidate; any CPU fallback still requires its own
qualification and admission. Every prototype retains bounded process lifetime, deadlines, memory
supervision and safe device teardown. A trusted operation oracle precedes packed execution.

[64-bit DMA support][hexagon-dma64] targets v81+ and avoids dynamic weight-buffer mappings above the
old address window through DMA/VTCM execution. [Q1/Q2 Hadamard matmul and row reads][hexagon-q2] are
source research leads. Neither establishes this project's PQ2/PTQ1, full graph or signed Windows
coverage. Source/rebuilt DSP code and a bundled signed binary are distinct identities.

| Route | Contract boundary |
| --- | --- |
| Prism/ggml Hexagon | Packed types, transforms, hybrid graph, adapter and signed deployment together |
| GenieX `llama_cpp` | Exact device-access plugin/package, supported format and actual placement |
| GenieX `qairt` or ONNX Runtime GenAI/QNN | Faithful prepared model package, chipset/precision/shapes/cache identity and bounded memory |
| Native Rust device boundary | Safe ownership API, audited driver boundary and no C/C++ inference-library dependency |

[GGUF/QAIRT plugins][geniex-sdk] and a [Q4_0 recommendation][geniex-models] do not establish Swift
PQ2/PTQ1 support. A supported-type fixture or separately pinned small model can establish device
access only; it cannot qualify Swift or change model selection. Concurrent residency with another
large control model is outside the 32 GB memory contract. Wrapping an external engine in Rust does
not satisfy native replacement requirements. The [HTP ternary prototype][htp-prototype] is a
source/license/codec/transform research lead rather than an approved deployment.

[GenieX v0.8.0][geniex-signed] reports a signed HTP catalog for its bundled revision. That catalog
covers only the corresponding v73/v81 binaries and kernels, not later source or rebuilt DSP code.
Custom binaries require a supported signing/distribution route. The [Windows
guide][hexagon-windows] also describes test-signing; Secure Boot, boot/signing policy and
certificate trust changes require a separate owner decision. Missing signed deployment blocks
execution.

[ONNX Runtime GenAI/QNN][qnn-genai] uses a prepared package rather than a GGUF provider switch.
Expanding 27B weights to one/two bytes costs roughly 27/54 GB before scales/state/duplicate buffers;
that is outside the current device budget without a separately demonstrated bounded representation.

## Packed arithmetic and graph state

The [assessment lock](../config/swift-assessment.lock.json) identifies PQ2; [current-weight
PTQ1](swift-memory.md) is separate. Weight/tokenizer/template, sampler, transforms, attention and
declared activation/cache precision remain fixed within a comparison. PQ2's Q8_K or odd-group Q8_0
arithmetic differs from PTQ1's Q8_0 and from generic upstream types. [SIMD contracts](simd-plan.md)
define operation fixtures; pinned Prism is the whole-model oracle until the relevant Rust parity
scope is qualified.

Upstream Q2_0 has 64 weights per scale; Prism PQ2 has 128. A lossless weight-layout bridge can split
each PQ2 block and duplicate its scale at approximately 5.9% more packed-weight storage. Symbol
layout, raw scale bits and transform metadata remain exact. Equal decoded weights do not establish
activation quantization, reduction/scale-order, logits or state parity with PQ2/Q8_K.

Operator acceptance includes exact PQ2 symbols, activation quantization, integer sums and scale
order; PTQ1 canonicality/Q8_0 rounding is an independent path. Fourth symbols, extremes/zeros,
malformed bytes, unaligned blocks, tails and awkward dimensions remain covered. Initial operator
parity is bitwise; FMA/reassociation or alternate precision requires declared tolerances and quality
acceptance. HVX/HMX eligibility comes from actual supported arithmetic. HMX prefill requires genuine
batched work with compatible hybrid-state scheduling; sequential block matvec alone does not reuse
weights across tokens.

Transform normalization/signs/axes and grouped recurrent projection basis are part of identity.
Approved LoRA also requires [adapter basis correctness](swift-lora.md), including grouped SSM;
fusion cannot change its artifact or bypass that gate. No-LoRA and approved-adapter forward
execution have separate evidence.

The graph ledger includes embedding, packed/floating projections, norm/RoPE, full attention,
GDN/convolution, feed-forward/output, adapters, caches/checkpoints, strides/shapes, precision and
every fallback/transfer/synchronization boundary for prefill and decode. Contiguous regions and
resident weights/scratch require explicit ownership. KV/convolution/GDN state, checkpoints and
phase-change layouts retain logits/state and deterministic-stream behavior across
fresh/reused/edited prefixes, reset, cancellation, failed evaluation and restored continuation.

[Chunkwise WY/UT GDN](inference-algorithms.md#chunkwise-gated-deltanet-prefill) has a separate
scheduling/floating-order contract. Qualcomm's [multiplication-only inverse][approximate-inverse]
uses truncation/residual correction and belongs to approximation, not exact execution. Strict
triangularity does not bound low-order error, and phone NPU results do not qualify Windows X Elite.
[Low-bit LUT KV](inference-algorithms.md#compression-and-approximation-constraints) differs from
lossless physical FP16 storage. Requantization, training, retrieval embeddings, draft models and
speculation are distinct qualification scopes.

## Memory, lifecycle and failure

Packed weights can remain in shared RAM with bounded expansion into VTCM. VTCM tiles, DMA staging,
padding/alignment and repack scratch have explicit bounds; persistent expansion has a separate
budget. The ledger covers host weights/mappings, device tiles/repacked copies, RPC/shared buffers,
state/checkpoints, compilation/graph caches, CPU/GPU/NPU/adapter copies and peak load overlap.
Unique physical bytes count once; address-space budgets are separate. Shared memory does not
establish zero-copy.

One NPU session is the bounded control. The [upstream design][hexagon-design] describes an
approximate 3.5 GB mapping window/dynamic mapping for X1/v73; the selected signed X2/v81 runtime has
its own DMA64/VTCM-only access constraints. Neither address window describes physical RAM capacity.
Layer-partitioned sessions include their overhead and state duplication. The existing Swift 11.5 GiB
service allowance and 4 GiB physical reserve remain fixed, including allocations outside Job commit.
Qwen's 18 GiB allowance is not a Swift budget. Any smaller placement needs its own qualified
profile; rejection cannot silently shorten context or change model policy.

The broker boundary retains reviewed libraries, fixed arguments, child-only environment settings,
authentication, exact Host/Origin/routes and canonical token counts, one active conversation, caps,
cancellation and deadlines. Readiness binds effective placement/resources. Strict backend
requirements fail explicitly; preferred fallback happens before generation, using qualified
eligibility and memory/state-transition rules. Arbitrary inherited overrides remain unsupported.
Helpers beyond the single-process Job need owned lifetime, parent-death teardown and independent
resource/isolation qualification. Drivers/services are not implicitly Job-contained.

Submitted work retains buffers until device use ends. Timeout, unsupported shape, allocation/mapping
failure, device loss or cancellation either restore all committed state or close admission until
verified teardown/reset. Hidden replay and partial continuation on CPU violate the contract.
Compiled/repacked caches are private, bounded and keyed by artifacts/kernel/runtime/ABI/observable
driver/firmware/shape/precision/placement, with checked metadata and invalidation. Technical
diagnostics exclude tensor payloads and prompts.

## Qualification evidence

Correctness precedes timing. Follow [profile qualification](hardware-profiles.md#qualification-scope)
and [measurement rules](performance-expectations.md#2026-literature-measurement-and-comparison),
fixing artifacts, adapter, sampler, context/precision, tolerances, failures and useful-benefit gates.
Charge load/repack/compile, migrations/cache misses, peak storage, shared bandwidth/power and
complete task; separately attribute RPC/submission/synchronization, mappings and supported counters.
Validate effective power modes: the [upstream guide][hexagon-guide] describes burst aliasing and
warning fallback for unknown modes. Utilization/overlap alone proves no benefit.

Admitted 8K trials include 512/2K/near-limit prompts with full output reservation, fixed 128/512-token
and natural creative outputs, cold/warm/reused/edited/occupied state, cancellation and failed recovery.
Larger contexts need separate memory/state acceptance. Repeated/interleaved same-contract CPU,
qualified-Vulkan and NPU runs retain median/p95/variance, prefill/first-token/decode, time/tokens/
retries to acceptable output, memory and sustained thermals/responsiveness. AC/battery and
15–30-minute runs have distinct scope; missing power telemetry means no energy verdict.

Use the shared [profile receipt/promotion contract](hardware-profiles.md#qualification-scope),
including exact kernel/driver identities and per-metric verdicts. Absent/unknown devices,
unsupported mappings, stale identity or insufficient budgets require bounded visible failure or
qualified CPU fallback with consistent state; absence supplies no device verdict. Missing evidence
or critical failures keep profiles experimental, and affected upgrades invalidate evidence/caches.

[Validation](validation.md) allows local builds, assembly inspection and focused untimed checks;
full suites, benchmarks and timing/profile captures require reviewed main-only matching-device CI.
Native diagnostics use absolute system-temp lanes with inherited Windows system/profile variables.
Moving links below are research leads, not pins.

## 2026 research evidence

The [mobile-acceleration catalog, E3 to E8](research-register.md#2026-mobile-acceleration-and-speech)
retains the **2026-10-29** source review, hardware/configurations and counterexamples. Apply its NPU
lessons without conflating prepared/static graphs, direct ggml HVX/HMX and signed Windows packages:

| Evidence | NPU decision rule |
| --- | --- |
| PowerBench / When NPUs Are Not Always Faster | Attribute quantization, offload, polling/wake-up and profiling overhead. Keep a competitive CPU control: different hardware/frameworks/quantizers/scheduling can reverse phase winners. |
| Energy-Efficient On-Device RAG | Validate static-context limits/loading order while preserving context/output reservations. Different packages, contexts and always-NPU reranking cannot rank same-artifact Swift or establish a GPU ceiling. |
| ShadowNPU / Quant.npu | Sparse attention and fitted static rotations/scales are approximation/new-artifact routes, not unchanged dense/GDN execution or dynamic Q8_K/Q8_0/lossless PTQ1. |
| Sereno | Include foreground jank even with stable TPS. Windows arbitration needs independent evidence; speculative yield points need a verifier and accepted-prefix recovery. |

Charge communication, activation preparation and fallback regions: fast HMX GEMM alone cannot
overcome graph breaks.

Two upstream changes provide concrete regression patterns, without updating Runner's pin:

- [DMA-ring fix #30189](https://github.com/ggml-org/llama.cpp/pull/30189), merged **2026-10-09**,
  fixes silently dropped DDR-to-VTCM transfers on descriptor-ring saturation. Its convolution/
  patch-embedding failure is not a demonstrated Swift defect. Check saturation, initialization,
  completion and buffer reuse beyond small shapes.
- [b11913](https://github.com/ggml-org/llama.cpp/releases/tag/b11913), prerelease **2026-10-27**,
  adds a VTCM-size request override, not extra available VTCM or qualified concurrent capacity.
  Bind effective allocation/refusal to the signed runtime; inherited overrides remain unsupported.

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
