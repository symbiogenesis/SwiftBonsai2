# Hexagon deployment and execution contracts

Outstanding NPU work is tracked only in [PLAN.md](../PLAN.md#hexagon-npu).
Current builds and launch policies use CPU execution, including scalar/NEON in
the [Rust runtime][rust-runtime]. Hexagon deployment, performance and automatic
selection are unqualified on both Snapdragon X1 and X2. The
[hardware selection contract](hardware-profiles.md) owns eligibility by actual
device, workload and interactive/battery objective.

## Device and route boundaries

Hexagon acceptance concerns completed-response latency or energy within memory,
quality and sustained-comfort limits against the same Swift artifact on CPU and
qualified Adreno. Utilization/TOPS and simultaneous CPU/GPU/NPU operation are not
acceptance criteria. X1 and X2 have independent qualification scopes.

| Device class | Relevant execution boundary |
| --- | --- |
| Recorded X1E-78-100 / Hexagon v73 | Packed HVX arithmetic, native signed runtime access, mapping-window/dynamic-mapping costs |
| X2 / Hexagon v81, initially Elite Extreme | Batched HMX prefill and packed HVX decode, verified extended DMA/VTCM constraints and signed matching package |

Processor names do not establish architecture, mapping capabilities, signed
kernels or readiness. Receipts identify exact SKU/capabilities, driver/runtime/
kernel package, ABI and resource limits. Missing or changed facts exclude the
candidate; any CPU fallback still requires its own qualification and admission.
Every prototype retains bounded process lifetime, deadlines, memory supervision
and safe device teardown. A trusted operation oracle precedes packed execution.

[64-bit DMA support][hexagon-dma64] targets v81+ and avoids dynamic weight-buffer
mappings above the old address window through DMA/VTCM execution.
[Q1/Q2 Hadamard matmul and row reads][hexagon-q2] are source research leads.
Neither establishes this project's PQ2/PTQ1, full graph or signed Windows
coverage. Source/rebuilt DSP code and a bundled signed binary are distinct identities.

| Route | Contract boundary |
| --- | --- |
| Prism/ggml Hexagon | Packed types, transforms, hybrid graph, adapter and signed deployment together |
| GenieX `llama_cpp` | Exact device-access plugin/package, supported format and actual placement |
| GenieX `qairt` or ONNX Runtime GenAI/QNN | Faithful prepared model package, chipset/precision/shapes/cache identity and bounded memory |
| Native Rust device boundary | Safe ownership API, audited driver boundary and no C/C++ inference-library dependency |

[GGUF/QAIRT plugins][geniex-sdk] and a [Q4_0 recommendation][geniex-models] do not
establish Swift PQ2/PTQ1 support. A supported-type fixture or separately pinned
small model can establish device access only; it cannot qualify Swift or change
model selection. Concurrent residency with another large control model is outside
the 32 GB memory contract. Wrapping an external engine in Rust does not satisfy
native replacement requirements. The [HTP ternary prototype][htp-prototype] is a
source/license/codec/transform research lead rather than an approved deployment.

[GenieX v0.8.0][geniex-signed] reports a signed HTP catalog for its bundled revision.
That catalog covers only the corresponding v73/v81 binaries and kernels, not
later source or rebuilt DSP code. Custom binaries require a supported signing/
distribution route. The [Windows guide][hexagon-windows] also describes test-signing;
Secure Boot, boot/signing policy and certificate trust changes require a separate
owner decision. Missing signed deployment blocks execution.

[ONNX Runtime GenAI/QNN][qnn-genai] uses a prepared package rather than a GGUF
provider switch. Expanding 27B weights to one/two bytes costs roughly 27/54 GB
before scales/state/duplicate buffers; that is outside the current device budget
without a separately demonstrated bounded representation.

## Packed arithmetic and graph state

The [assessment lock](../config/swift-assessment.lock.json) identifies PQ2;
[current-weight PTQ1](swift-memory.md) is separate. Weight/tokenizer/template,
sampler, transforms, attention and declared activation/cache precision remain
fixed within a comparison. PQ2's Q8_K or odd-group Q8_0 arithmetic differs from
PTQ1's Q8_0 and from generic upstream types. [SIMD contracts](simd-plan.md)
define operation fixtures; pinned Prism is the whole-model oracle until the
relevant Rust parity scope is qualified.

Upstream Q2_0 has 64 weights per scale; Prism PQ2 has 128. A lossless weight-layout
bridge can split each PQ2 block and duplicate its scale at approximately 5.9%
more packed-weight storage. Symbol layout, raw scale bits and transform metadata
remain exact. Equal decoded weights do not establish activation quantization,
reduction/scale-order, logits or state parity with PQ2/Q8_K.

Operator acceptance includes exact PQ2 symbols, activation quantization, integer
sums and scale order; PTQ1 canonicality/Q8_0 rounding is an independent path.
Fourth symbols, extremes/zeros, malformed bytes, unaligned blocks, tails and awkward
dimensions remain covered. Initial operator parity is bitwise; FMA/reassociation
or alternate precision requires declared tolerances and quality acceptance.
HVX/HMX eligibility comes from actual supported arithmetic. HMX prefill requires
genuine batched work with compatible hybrid-state scheduling; sequential block
matvec alone does not reuse weights across tokens.

Transform normalization/signs/axes and grouped recurrent projection basis are
part of identity. Approved LoRA also requires [adapter basis correctness](swift-lora.md),
including grouped SSM; fusion cannot change its artifact or bypass that gate.
No-LoRA and approved-adapter forward execution have separate evidence.

The graph ledger includes embedding, packed/floating projections, norm/RoPE,
full attention, GDN/convolution, feed-forward/output, adapters, caches/checkpoints,
strides/shapes, precision and every fallback/transfer/synchronization boundary
for prefill and decode. Contiguous regions and resident weights/scratch require
explicit ownership. KV/convolution/GDN state, checkpoints and phase-change layouts
retain logits/state and deterministic-stream behavior across fresh/reused/edited
prefixes, reset, cancellation, failed evaluation and restored continuation.

[Chunkwise WY/UT GDN](inference-algorithms.md#chunkwise-gated-deltanet-prefill)
has a separate scheduling/floating-order contract. Qualcomm's
[multiplication-only inverse][approximate-inverse] uses truncation/residual
correction and belongs to approximation, not exact execution. Strict triangularity
does not bound low-order error, and phone NPU results do not qualify Windows
X Elite. [Low-bit LUT KV](inference-algorithms.md#compression-and-approximation-constraints)
differs from lossless physical FP16 storage. Requantization, training, retrieval
embeddings, draft models and speculation are distinct qualification scopes.

## Memory, lifecycle and failure

Packed weights can remain in shared RAM with bounded expansion into VTCM.
VTCM tiles, DMA staging, padding/alignment and repack scratch have explicit bounds;
persistent expansion has a separate budget. The ledger covers host weights/
mappings, device tiles/repacked copies, RPC/shared buffers, state/checkpoints,
compilation/graph caches, CPU/GPU/NPU/adapter copies and peak load overlap. Unique
physical bytes count once; address-space budgets are separate. Shared memory does
not establish zero-copy.

One NPU session is the bounded control. The [upstream design][hexagon-design]
describes an approximate 3.5 GB mapping window/dynamic mapping for X1/v73; the
selected signed X2/v81 runtime has its own DMA64/VTCM-only access constraints.
Neither address window describes physical RAM capacity. Layer-partitioned sessions
include their overhead and state duplication. The existing Swift 11.5 GiB service
allowance and 4 GiB physical reserve remain fixed, including allocations outside
Job commit. Qwen's 18 GiB allowance is not a Swift budget. Placement rejection or
explicit qualified reduction cannot silently shorten context or change model policy.

The broker boundary retains reviewed libraries, fixed arguments, child-only
environment settings, authentication, exact Host/Origin/routes and canonical token
counts, one active conversation, caps, cancellation and deadlines. Readiness binds
effective placement/resources. Strict backend requirements fail explicitly;
preferred fallback happens before generation, using qualified eligibility and
memory/state-transition rules. Arbitrary inherited overrides remain unsupported.
Helpers beyond the single-process Job need owned lifetime, parent-death teardown
and independent resource/isolation qualification. Drivers/services are not
implicitly Job-contained.

Submitted work retains buffers until device use ends. Timeout, unsupported shape,
allocation/mapping failure, device loss or cancellation either restore all committed
state or close admission until verified teardown/reset. Hidden replay and partial
continuation on CPU violate the contract. Compiled/repacked caches are private,
bounded and keyed by artifacts/kernel/runtime/ABI/observable driver/firmware/shape/
precision/placement, with checked metadata and invalidation. Technical diagnostics
exclude tensor payloads and prompts.

## Qualification evidence

Correctness precedes timing. Profile costs include load/repack/compile, prefill,
first useful token, decode and completed-task time; mixed placement also includes
layout/state migration, cache misses, peak memory and shared bandwidth/power.
RPC/submission/synchronization, mappings and supported counters have separate
instrumented scope. Power modes require validated effective settings; the
[upstream guide][hexagon-guide] describes burst aliasing and warning fallback for
unknown modes. Utilization and overlapping device work do not establish benefit.

Qualification fixes artifacts, adapter, sampler, context, precision, tolerances,
failure criteria and useful-benefit thresholds. Admitted 8K workloads include
512/2K/near-limit prompts with full output reservation, fixed 128/512-token and
natural creative outputs, cold/warm/reused/edited/occupied state, cancellation and
failed recovery. Larger context has independent memory/state acceptance.

Repeated/interleaved same-contract CPU/qualified-Vulkan/NPU comparisons retain
median/p95/variance, prefill/first-token/decode, time/tokens/retries to acceptable
output, memory and sustained responsiveness/thermals. AC/battery and 15–30-minute
runs have distinct scope. Missing valid power telemetry means no energy verdict.
Each X1/X2 receipt binds device capabilities, source/build/kernel/driver, effective
placement/settings, tested main/run, raw captures and per-metric verdicts.
Unknown processors, absent NPU, unsupported mappings, stale identities and
insufficient memory require bounded visible failure/fallback, CPU rollback and
consistent state. Device absence cannot establish a device verdict.

Promotion requires full CI correctness, daily security/resources, no material
creative-quality regression and same-engine LoRA TPS/quality in addition to
fixed-adapter CPU/GPU comparisons. The useful-benefit criterion is at least 10%
lower end-to-end latency or a predeclared capacity/energy benefit within accepted
interactive latency and quality bounds. Critical failures or missing evidence
keep a profile experimental. Affected upgrades invalidate relevant evidence/caches.

Local builds, assembly inspection and focused untimed checks follow the
[validation policy](validation.md). Full suites, benchmarks and profiling/timing
captures run only in reviewed main-only CI; matching target devices supply
Hexagon evidence. Native diagnostics use absolute system-temp lanes with inherited
Windows system/profile variables. Moving links are research leads, not pins.

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
