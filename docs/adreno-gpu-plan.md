# Snapdragon X1/X2 Adreno contracts

Outstanding Adreno work is tracked only in [PLAN.md](../PLAN.md#adreno-vulkan). Current inference
paths are CPU-only. A native Windows ARM64 GPU build, whole-model GPU correctness and target-device
performance are unqualified on both families. GPU profiles share the [hardware selection
contract](hardware-profiles.md) with CPU and Hexagon profiles.

## Backend and device boundaries

Pinned Prism Vulkan is the initial implementation route. Rust GPU execution has its own
safety/coverage boundary after Rust CPU parity; reusable Rust storage does not provide GPU
execution. PQ2 and [current-weight PTQ1](swift-memory.md) are distinct qualification lanes. The
[Prism backend audit][prism-audit] describes packed scalar, coopmat1 and integer-dot
paths/transforms, with no direct coopmat2 decoder for these formats. Source support alone does not
establish Adreno kernel or graph coverage.

| Repository fact | Execution boundary |
| --- | --- |
| [Assessment lock](../config/swift-assessment.lock.json): 6.71 GiB PQ2, Prism `73bfe6cc485554822f80c473a3c0c4f25e80f41b`, `win-cpu-arm64` | Installed runtime is a CPU reference |
| [Baseline build](../scripts/Build-Engine.ps1) and [overlay build][overlay] disable Vulkan/OpenCL | CPU recipes do not produce an accelerated runtime |
| [Completion](../src/cpu-policy/src/lib.rs), [server](../scripts/Bonsai.ServerPolicy.psm1) and [broker](../src/broker/src/launch_policy.rs) use `--gpu-layers 0` / `--device none` | Drivers do not activate offload; server policy also disables KV/operation offload |
| [Doctor](../scripts/Doctor.ps1) reports GPU identity/driver | Inventory does not establish loader/ICD, API features, kernels or budgets |

[OpenCL][opencl] Windows ARM64/X1-85 guidance does not establish Swift PQ2/PTQ1 support; X2
prebuilt-kernel claims do not qualify X1-85. [DirectML][directml]/ Windows ML requires a faithful
model/runtime integration rather than a GGUF switch. Conversion/custom DX12 is a distinct
implementation route, justified only by a measured limitation. Hexagon discovery is independent of
Vulkan.

The recorded X1 device is Adreno X1-85 in X1E-78-100. X2 variants, including X2-90, have independent
capability/driver and graph evidence. Family names and advertised APIs do not prove subgroup,
integer-dot, cooperative-matrix or allocation support. Native ARM64 executable/loader/ICD/dependency
identities, enabled extensions, queues, selected device and shader build tools are separate facts;
software adapters and emulated inference are outside this native-device contract.

Feature evidence includes buffer/allocation/workgroup limits, heaps/types, subgroup properties,
FP16/storage, integer-dot support **and** acceleration, cooperative-matrix shapes and memory-budget
availability. See [capability queries][vulkan-query], [subgroup properties][subgroups], [integer-dot
properties][integer-dot] and [memory budgets][memory-budget]. Missing budget telemetry requires
conservative admission. Shared-memory/UMA reports and 128 MB dedicated memory are not total-capacity
or allocation entitlements.

## Swift arithmetic and placement

A placement ledger covers packed/floating projections, activation quantizers/scales, transforms,
embedding/output, attention/RoPE, GDN/convolution, KV/recurrent state, approved LoRA and every
fallback, transfer and synchronization boundary. A layer offload count is insufficient. Fixed token
IDs/settings, same-artifact CPU controls and declared precision/tolerances define numerical scope;
plausible text does not.

CPU PQ2/Q8_K, PTQ1/Q8_0 and odd-width PQ2/Q8_0 differ from the Vulkan integer-dot [Q8_1 shader
family][prism-matvec] ([PTQ1 shader][prism-ptq1]). Exact weights do not imply bit-identical logits
across activation arithmetic. Awkward shapes, supported strides, extremes/zeros and explicit
fallback/rejection belong to packed-kernel acceptance. Whole-model scope includes prefill/decode
logits and KV/GDN state, tokenizer/template/sampler, prefixes, edits, reset, cancellation and
recovery; no-LoRA and approved-adapter basis/forward execution are separate lanes.

The pinned [Qwen35 graph][prism-graph] and [Vulkan guards][prism-vulkan] disable row-indexed
recurrent reads and raw-gate fusion. The gathered-state/activated-gate route is the numerical
control. Swift's `[5120, 248320]` embedding has about 322.07 MiB PQ2 payload or 265.23 MiB PTQ1;
heap space alone does not prove descriptor limits or indexing can address it. Forced-limit overrides
are outside the observed capability contract. [LoRA sharing][lora-pool] admits CPU buffers only; its
2.5 MiB storage saving does not establish device allocations, basis or adapter execution.

Scalar nonflash attention rounds globally normalized probabilities to FP16 before multiplying V.
Ordinary online softmax changes that intermediate and therefore has a distinct numerical contract.
[Attention/locality and chunkwise GDN](inference-algorithms.md) also retain state/logit, adapter and
lifecycle requirements after any scheduling or reduction change. Approximation and speculation have
separate qualification scopes.

## Resources, failure and acceptance

Apply [profile eligibility/lifecycle](hardware-profiles.md) and
[measurement rules](performance-expectations.md#2026-literature-measurement-and-comparison).
Screen CPU-only, partial and maximum offload independently for prefill/decode; maximum offload
is not automatically best. Charge cold pipeline compilation/repack, submissions/waits, transfers,
phase/state migrations, duplicate storage and complete-task latency. Contiguous residency needs
correct numerical/ownership contracts.

The ledger covers packed/repacked weights, staging, scratch, adapters, caches/checkpoints,
mappings and load overlap, counting unique physical storage once. UMA does not prove zero-copy;
Job commit omits mapped/device bytes. Persistent expansion pays cold-load/residency cost.
Reused pipelines/descriptors/allocations and compiled caches bind source/options/toolchain/device/
driver and checked invalidation; precision/decoding changes remain separate contracts.

Experimental profiles retain authentication, exact counting, one slot, ownership, deadlines,
cancellation/admission and effective-placement/resource readiness. Allocation refusal, device loss,
driver changes or cancellation require safe teardown and consistent committed state. Never
continue partial work on CPU; explicit fallback needs an eligible qualified profile.

Receipts bind artifacts/adapter/source/build, capabilities/driver, effective arguments/placement,
power/workload, tested main/CI run and raw captures. Separate cold/warm/first-use/prefill/decode/task,
peak/resident/commit/headroom, faults/evictions, energy, thermals, responsiveness and sustained failures.
Queue/kernel timestamps, submission/waits and critical-path traces have distinct scope; overlapping
durations cannot simply be added, and utilization is not energy. [GPUView][gpuview] requires
compatible native ARM64 tooling.

Promotion preserves independent [LoRA TPS/quality](swift-lora.md), creative quality, security,
headroom/comfort and upgrade/rollback gates; fixed-adapter CPU/GPU comparisons do not qualify
adapters. One SKU/driver/family cannot qualify another. [Validation](validation.md) permits local
builds/focused untimed checks; full suites, benchmarks and timing/profile captures remain matching-
device CI. Native diagnostics use absolute system-temp lanes and inherited Windows system/profile
variables. Moving source links below are leads, not immutable build identities.

## 2026 research evidence

The [mobile-acceleration catalog](research-register.md#2026-mobile-acceleration-and-speech) records
sources reviewed **2026-10-29**; none enables a backend or power-policy change.

| Lead | Relevant lesson and transfer limit |
| --- | --- |
| Qualcomm IWOCL Adreno slides | Vector unpack/GEMV, separated quantization fields, per-kernel tiles, image-backed weights, compilation caches and recordable submissions are layout/scheduling leads. Several X2 MoE kernels use internal assembly; public OpenCL C and Vulkan are separate implementations, with no Swift PQ2/PTQ1 result. |
| TurboInfer | Millisecond GPU stalls motivate measuring submission gaps and task energy. Phone PMU/frequency-control access does not establish Windows counters, authority or power savings. |
| PowerBench | Separate CPU/GPU/NPU energy, quantization, offload, polling and wake-up overheads. Rooted Android SM8650/8750/8850 plus QPT instrumentation cannot rank laptop Vulkan/CPU paths. |

OpenCL-only image/texture or residency features need independently observed Vulkan support, byte
accounting and shaders. Submission replay needs supported APIs and valid resource lifetimes.
Attribute CPU preparation, queue service and shared-DRAM traffic separately: faster kernels may
leave host gaps unchanged, while greater residency can increase register pressure or contention with
native rendering/audio. Include foreground frame delivery and speech underruns in sustained comfort;
[Hexagon evidence](hexagon-npu-plan.md#2026-research-evidence) explains how stable TPS can hide that
contention. Vendor power controls remain research leads, not launch prerequisites.

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
