# Snapdragon X1/X2 Adreno contracts

Outstanding Adreno work is tracked only in [PLAN.md](../PLAN.md#adreno-vulkan).
Current inference paths are CPU-only. A native Windows ARM64 GPU build,
whole-model GPU correctness and target-device performance are unqualified on
both families. GPU profiles share the [hardware selection contract](hardware-profiles.md)
with CPU and Hexagon profiles.

## Backend and device boundaries

Pinned Prism Vulkan is the initial implementation route. Rust GPU execution has
its own safety/coverage boundary after Rust CPU parity; reusable Rust storage does
not provide GPU execution. PQ2 and [current-weight PTQ1](swift-memory.md) are distinct
qualification lanes. The [Prism backend audit][prism-audit] describes packed scalar,
coopmat1 and integer-dot paths/transforms, with no direct coopmat2 decoder for these
formats. Source support alone does not establish Adreno kernel or graph coverage.

| Repository fact | Execution boundary |
| --- | --- |
| [Assessment lock](../config/swift-assessment.lock.json): 6.71 GiB PQ2, Prism `73bfe6cc485554822f80c473a3c0c4f25e80f41b`, `win-cpu-arm64` | Installed runtime is a CPU reference |
| [Baseline build](../scripts/Build-Engine.ps1) and [overlay build][overlay] disable Vulkan/OpenCL | CPU recipes do not produce an accelerated runtime |
| [Completion](../src/cpu-policy/src/lib.rs), [server](../scripts/Bonsai.ServerPolicy.psm1) and [broker](../src/broker/src/launch_policy.rs) use `--gpu-layers 0` / `--device none` | Drivers do not activate offload; server policy also disables KV/operation offload |
| [Doctor](../scripts/Doctor.ps1) reports GPU identity/driver | Inventory does not establish loader/ICD, API features, kernels or budgets |

[OpenCL][opencl] Windows ARM64/X1-85 guidance does not establish Swift PQ2/PTQ1
support; X2 prebuilt-kernel claims do not qualify X1-85. [DirectML][directml]/
Windows ML requires a faithful model/runtime integration rather than a GGUF switch.
Conversion/custom DX12 is a distinct implementation route, justified only by a
measured limitation. Hexagon discovery is independent of Vulkan.

The recorded X1 device is Adreno X1-85 in X1E-78-100. X2 variants, including X2-90,
have independent capability/driver and graph evidence. Family names and advertised
APIs do not prove subgroup, integer-dot, cooperative-matrix or allocation support.
Native ARM64 executable/loader/ICD/dependency identities, enabled extensions,
queues, selected device and shader build tools are separate facts; software
adapters and emulated inference are outside this native-device contract.

Feature evidence includes buffer/allocation/workgroup limits, heaps/types,
subgroup properties, FP16/storage, integer-dot support **and** acceleration,
cooperative-matrix shapes and memory-budget availability. See
[capability queries][vulkan-query], [subgroup properties][subgroups],
[integer-dot properties][integer-dot] and [memory budgets][memory-budget]. Missing
budget telemetry requires conservative admission. Shared-memory/UMA reports and
128 MB dedicated memory are not total-capacity or allocation entitlements.

## Swift arithmetic and placement

A placement ledger covers packed/floating projections, activation quantizers/scales,
transforms, embedding/output, attention/RoPE, GDN/convolution, KV/recurrent state,
approved LoRA and every fallback, transfer and synchronization boundary. A layer
offload count is insufficient. Fixed token IDs/settings, same-artifact CPU controls
and declared precision/tolerances define numerical scope; plausible text does not.

CPU PQ2/Q8_K, PTQ1/Q8_0 and odd-width PQ2/Q8_0 differ from the Vulkan integer-dot
[Q8_1 shader family][prism-matvec] ([PTQ1 shader][prism-ptq1]). Exact weights do not
imply bit-identical logits across activation arithmetic. Awkward shapes, supported
strides, extremes/zeros and explicit fallback/rejection belong to packed-kernel
acceptance. Whole-model scope includes prefill/decode logits and KV/GDN state,
tokenizer/template/sampler, prefixes, edits, reset, cancellation and recovery;
no-LoRA and approved-adapter basis/forward execution are separate lanes.

The pinned [Qwen35 graph][prism-graph] and [Vulkan guards][prism-vulkan] disable
row-indexed recurrent reads and raw-gate fusion. The gathered-state/activated-gate
route is the numerical control. Swift's `[5120, 248320]` embedding has about
322.07 MiB PQ2 payload or 265.23 MiB PTQ1; heap space alone does not prove descriptor
limits or indexing can address it. Forced-limit overrides are outside the observed
capability contract. [LoRA sharing][lora-pool] admits CPU buffers only; its 2.5 MiB
storage saving does not establish device allocations, basis or adapter execution.

Scalar nonflash attention rounds globally normalized probabilities to FP16 before
multiplying V. Ordinary online softmax changes that intermediate and therefore
has a distinct numerical contract. [Attention/locality and chunkwise GDN](inference-algorithms.md)
also retain state/logit, adapter and lifecycle requirements after any scheduling or
reduction change. Approximation and speculation have separate qualification scopes.

## Resources, failure and acceptance

Profile costs include cold pipeline compilation/repacking, submission/waits,
transfers/copies, phase/state migration, duplicated storage, prefill/decode and
completed-task time. GPU prefill and GPU decode can have different outcomes.
Contiguous resident regions reduce crossings only when ownership and numerical
contracts hold; maximum offload is one candidate rather than an optimum.

The memory ledger includes packed/repacked weights, staging, scratch, adapters,
caches/checkpoints, mappings and load overlap, counting each physical allocation
once. UMA does not establish zero-copy. Persistent expansion includes its cold
load/residency cost. Pipeline/descriptor/allocation reuse and cached compilation
require valid lifetime, source/options/toolchain/device/driver identity and reliable
invalidation. Precision/decoding changes are separate contracts.

An experimental profile retains authentication, exact count, one slot, process
ownership, deadlines, cancellation and admission. Job commit does not cover all
mapped/device bytes. Readiness includes effective placement/resources. Allocation
failure, device loss, changed drivers and cancellation require safe teardown and
consistent committed state; an interrupted request cannot continue partially on
CPU. Fallback is explicit and requires an eligible qualified profile.

Device receipts bind artifacts/adapters/source/build, capabilities/driver,
effective arguments/placement, power mode/workload, tested main revision, CI run
and raw captures. Measurements distinguish cold load/compile/repack, warm prefill,
first useful answer, decode TPS/task cost, peak/resident memory, commit/headroom,
hard faults/evictions, energy, thermals, UI responsiveness and sustained failures.
Queue/kernel timestamps, submission/wait and critical-path traces have separate
scope; overlapping durations cannot simply be added and utilization is not an
energy proxy. [GPUView][gpuview] requires compatible native ARM64 tooling.

Promotion requires profile-specific correctness, creative quality, headroom and
sustained comfort, plus same-engine [LoRA TPS/quality requirements](swift-lora.md).
Fixed-adapter CPU/GPU comparisons remain separate from adapter qualification.
Unknown/unavailable devices, stale evidence/runtime/driver, memory pressure and
affected upgrades require explicit eligibility/fallback behavior and rollback.
Evidence from one SKU, driver or family does not qualify another.

Local builds and focused untimed capability/correctness checks follow the
[validation policy](validation.md). Full suites, benchmarks and timing/profile
captures are CI-only; each target X1/X2 device supplies its own performance evidence.
Native diagnostics use absolute system-temp working/output paths and inherited
Windows system/profile variables. Moving links below are research leads rather
than immutable build identities.

## 2026 research evidence

Sources reviewed **2026-10-29**. These findings inform experiments under the
contracts above; they enable no backend or power-policy change. The
[research register](research-register.md) indexes the wider literature.

| Source and date | Mechanism worth testing | Scope and transfer limit |
| --- | --- | --- |
| Qualcomm's [IWOCL Adreno presentation](https://www.iwocl.org/wp-content/uploads/IWOCL-2026-Wang-Llamacpp.pdf), [2026-05-06 program](https://www.iwocl.org/iwocl-2026/conference-program/) | Vectorized unpack/GEMV, separated quantization fields, image-backed weights, per-kernel tiling, compilation caches and recordable submissions. | Includes Windows-on-Snapdragon X/X2, but several headline X2 MoE kernels use internal inline assembly. Public OpenCL C, internal kernels and Vulkan are distinct implementations. No Swift PQ2/PTQ1 result is supplied. |
| [Act Before It's Too Late: Power-Efficient LLM Inference on Mobile Device](https://doi.org/10.1145/3745756.3809208), MobiSys, published 2026-06-20 | TurboInfer detects short GPU stalls and adapts frequency at millisecond granularity instead of inferring useful work from coarse utilization. | Phone GPU/PMU and frequency-control evidence motivates measuring submission gaps and energy. It does not establish equivalent counters or control authority on Windows; its power savings are not Vulkan/Snapdragon X measurements. |
| [Is Your NPU Ready for LLMs?](https://arxiv.org/html/2607.05475v2), v2 2026-10-16 | PowerBench separates CPU/GPU/NPU energy and exposes quantization, offload, polling and wake-up overheads. | Root-enabled Android phones, SM8650/8750/8850 and QPT-driver instrumentation. Phase winners depend on framework, shapes and arithmetic; these observations do not rank this laptop's Vulkan and CPU implementations. |

The Qualcomm slides explicitly distinguish Adreno features exposed through
OpenCL from their availability in Vulkan. Image/texture-backed weights and
on-chip residency are useful layout leads, but a Vulkan experiment needs its
own observable features, byte accounting and shader implementation. Recordable
submission ideas likewise require supported APIs and valid resource lifetimes;
an internal-kernel result cannot establish a public-source implementation's speed.

Treat CPU preparation, GPU queue service and shared-DRAM traffic as separate
candidate bottlenecks. Reducing a kernel's duration can leave host submission
gaps unchanged. Conversely, keeping more work resident can increase register
pressure or interfere with the native UI and codec. Include foreground frame
delivery and speech underruns in the existing sustained-comfort observation;
the [Hexagon research evidence](hexagon-npu-plan.md#2026-research-evidence)
includes a measured example where inference TPS hid foreground contention.
Vendor power controls are research leads, not new ordinary-launch requirements.

[prism-audit]: https://github.com/PrismML-Eng/Bonsai-demo/blob/main/BACKEND-SUPPORT.md
[opencl]: https://github.com/PrismML-Eng/llama.cpp/blob/prism/docs/backend/OPENCL.md
[directml]: https://learn.microsoft.com/en-us/windows/ai/directml/dml
[vulkan-query]: https://docs.vulkan.org/guide/latest/querying_extensions_features.html
[subgroups]: https://docs.vulkan.org/guide/latest/subgroups.html
[integer-dot]: https://docs.vulkan.org/refpages/latest/refpages/source/VkPhysicalDeviceShaderIntegerDotProductProperties.html
[memory-budget]: https://docs.vulkan.org/refpages/latest/refpages/source/VK_EXT_memory_budget.html
[gpuview]: https://learn.microsoft.com/en-us/windows-hardware/drivers/display/using-gpuview
[prism-graph]: https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/src/models/qwen35.cpp
[prism-vulkan]: https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/ggml/src/ggml-vulkan/ggml-vulkan.cpp
[prism-matvec]: https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/ggml/src/ggml-vulkan/vulkan-shaders/mul_mat_vecq.comp
[prism-ptq1]: https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/ggml/src/ggml-vulkan/vulkan-shaders/mul_mat_vecq_ptq1_0.comp
[overlay]: https://github.com/symbiogenesis/prism-inference/blob/main/scripts/Build-PrismEngine.ps1
[lora-pool]: https://github.com/symbiogenesis/prism-inference/blob/main/tools/prism/src/pool.rs
