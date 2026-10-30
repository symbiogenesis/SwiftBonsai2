# Adreno GPU contracts and OpenCL feasibility

Inference remains CPU-only; native Windows ARM64 GPU builds, correctness and performance are
unqualified on X1/X2. [PLAN.md](../PLAN.md#adreno-vulkan) owns pending work. The shared
[profile policy](hardware-capabilities.md#automatic-profile-selection) governs selection,
admission, state transitions and promotion for both Vulkan and OpenCL.

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

[OpenCL access and source coverage](#opencl-access-and-source-audit) remain unqualified for Swift;
X2 prebuilt-kernel claims do not qualify X1-85. [DirectML][directml]/Windows ML needs faithful
model/runtime integration rather than a GGUF switch. Conversion/custom DX12 is a separate route
requiring a measured limitation; Hexagon discovery is independent of Vulkan.

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

Apply [profile eligibility/lifecycle](hardware-capabilities.md#automatic-profile-selection) and
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

Apply the shared [profile lifecycle](hardware-capabilities.md#prefill-decode-and-complete-state)
to allocation refusal, device loss, driver changes and cancellation: preserve committed state,
teardown safely and use only a qualified fallback, never partial CPU continuation.

[Profile receipts](hardware-capabilities.md#qualification-scope) bind identities, settings/workloads,
main/CI/raw captures and resource/comfort verdicts. Distinguish peak/resident/commit/headroom,
faults/evictions and sustained failures. GPU attribution separates pipeline first use, queue/kernel
timestamps, submission/waits and critical-path traces. Overlapping durations cannot be added, and
utilization is not energy. [GPUView][gpuview] needs
compatible native ARM64 tooling.

Independent [LoRA TPS/quality](lora-qualification.md) remains a promotion gate; comparisons bind
the tested adapter/SKU/driver/family. [Validation](validation.md) requires matching-device CI for
full suites and timing/profile captures. Local builds/focused untimed checks are allowed. Native
diagnostics use absolute system-temp lanes and inherited Windows system/profile variables.
Moving source links below are leads, not build pins.

## 2026 research evidence

The [mobile-acceleration catalog, E1 to E3/E8](research-register.md#2026-mobile-acceleration-and-speech)
retains the **2026-10-29** source review and platform limits. Its Adreno implications are:

- Qualcomm's layout/submission techniques need public implementations: internal X2 MoE assembly,
  OpenCL C and Vulkan are separate routes. Image/texture/residency features require observed Vulkan
  support, byte accounting and shaders; replay needs supported APIs and valid resource lifetimes.
- TurboInfer/PowerBench motivate separating CPU preparation, queue service, shared-DRAM traffic,
  polling/wake-ups and task energy. Faster kernels can leave host gaps unchanged; phone telemetry
  and power controls establish no Windows authority/savings and are not launch prerequisites.
- Greater residency can raise register pressure or rendering/audio contention. Include foreground
  frame delivery and speech underruns in sustained comfort: stable TPS can hide worse responsiveness.

These leads enable neither a backend nor a power-policy change.

## OpenCL access and source audit

The pinned public OpenCL backend has **no reusable Swift PQ2/PTQ1 packed projection**: neither
format appears in `GGML_OP_MUL_MAT` eligibility, and PQ2/PTQ1/Hadamard implementations are absent.
Generic Q1/Q4/Q8 or an installed compiler cannot supply them. Public kernels remain possible;
this information-query/source audit selects no profile and is independent of Vulkan qualification.

The audit covers one installed implementation and one immutable source revision: a 30-minute
feasibility budget, zero model/kernel compute, and no prompts, tokens or projection shapes.
The ten-second information query uses a 512 MiB one-process Job and 1 MiB per-output-stream bounds;
it creates no context, queue, program, kernel or device allocation.

### Reproducible native query

`bonsai-tools host-opencl --request <absolute-empty-object.json>` launches its own ARM64 child with
the ordinary headless worker mitigations. It loads only the absolute Windows system `OpenCL.dll`,
restricting initial dependency search to that directory and System32. `OCL_*`, `OPENCL_*` and `CL_*`
environment overrides reject. Loader/ICD calls still execute installed native code and remain an
operator trust boundary. The internal `host-opencl-query` command has no intrinsic driver timeout;
callers must use the owning wrapper.

The query bounds eight platforms, sixteen total GPU devices, 16 KiB per native string and 512
extension names per device. Malformed UTF-8, missing terminators, invalid native sizes, non-GPU
handles and excessive counts reject. CPU devices are not requested. A successful empty enumeration
is observed, while load, driver, framing or deadline failure returns unknown and retains bounded
stderr. There is no retry with a weaker process policy.

The separate `runner-native-opencl-observations` report records platform/device, driver and OpenCL C
versions, compiler/device availability, address width, extensions and capacity limits. The loader
size/SHA-256 is a bounded file observation after querying, not proof that exactly those bytes or
every dependency were executed. DLL signatures, ICD dependencies and provenance remain unverified.
Global/local memory and maximum-allocation capacities are not live free-memory budgets and cannot
change admission. `backend_ready` stays false.

`Doctor.ps1 -ToolPath <existing-bonsai-tools.exe>` adds this report as `NativeOpenClObservations`.
It neither builds tools nor changes the existing CPU compatibility digest or launch policy. The
native query follows the Khronos
[platform](https://registry.khronos.org/OpenCL/specs/unified/refpages/man/html/clGetPlatformIDs.html),
[GPU
enumeration](https://registry.khronos.org/OpenCL/specs/unified/refpages/man/html/clGetDeviceIDs.html)
and
[device-information](https://registry.khronos.org/OpenCL/specs/unified/refpages/man/html/clGetDeviceInfo.html)
contracts.

### Retained X2 information-query evidence

The [2026-10-29 access receipt](evidence/opencl-x2-access-evidence.json) binds the tested maintenance
executable hash and separates two model-free observations on this native Snapdragon X2 Elite Extreme
X2E94100:

- The production `host-opencl` wrapper's child exits with `3221225477`
  (`0xC0000005`, access violation). Its capability report stays unknown and its
  raw output files remain in external temporary evidence. No worker access or
  loader/device capability is inferred from that failure.
- One separately labelled ten-second ordinary-tool-policy diagnostic succeeds.
  It reports Adreno X2-90, OpenCL and OpenCL C 3.0, an available online compiler,
  and driver `QUALCOMM build: 863.0 Compiler DX.50.39.00`. The advertised
  extensions include FP16 and integer-dot features. It observes a 535,112-byte
  system loader with SHA-256
  `b5b0c8e990c06d4477ee8d9957460b466e52e7d8bf80e413d7e41aa7039dab7f`.

The successful diagnostic used ordinary tool ownership/deadline without the worker's 512 MiB Job
or mitigation envelope. It identifies a combined policy/resource incompatibility without isolating
its cause, DLL or mitigation. The shipped command has no weaker-policy retry. Neither result
includes device compute or qualifies worker access, compiler output, provenance or Swift arithmetic.
Device execution requires resolving access under unchanged worker policy.

### Immutable public source audit

The audit uses Runner's existing C++ runtime revision `73bfe6cc485554822f80c473a3c0c4f25e80f41b` in
`PrismML-Eng/llama.cpp`. The retained [source identities](evidence/opencl-source-identities.json) bind all
three reviewed files to their byte counts and SHA-256. No source is executed or copied into the
application.

| Checked source | Result |
| --- | --- |
| [OpenCL dispatch](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/ggml/src/ggml-opencl/ggml-opencl.cpp#L7734) | Packed PQ2/PTQ1 projections are ineligible; the limited FP32 GDN and convolution branches do not establish complete Swift coverage. |
| [Build recipe](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/ggml/src/ggml-opencl/CMakeLists.txt) | Public OpenCL source kernels can be embedded; an OpenCL SDK and Python are build requirements. No accelerated Runner binary is built in this screen. |
| [Backend documentation](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/docs/backend/OPENCL.md) | Windows ARM64 and X2-90 are listed, but optional Qualcomm prebuilt kernels are a separate manually provisioned library for selected other formats. They are excluded from this public-source comparison. |

The [release asset snapshot](evidence/opencl-release-assets.json), reviewed **2026-10-29** for
[prism-b10787-73bfe6c](https://github.com/PrismML-Eng/llama.cpp/releases/tag/prism-b10787-73bfe6c),
contains Windows ARM64 CPU/CUDA bundles and no OpenCL bundle. It records public names, sizes and
reported digests without downloads; metadata proves neither executed bytes nor publisher trust.
The shipped runtime remains the CPU archive in `config/swift-assessment.lock.json`.

The [Prism pilot contract](../PLAN.md#adreno-vulkan) requires one real Swift packed-projection shape
against scalar CPU arithmetic and proven unchanged-policy worker access before execution. New
kernels require reviewed builds, arithmetic/device and partial-placement transfer/state/fallback
evidence; changed formats and whole-model expansion are outside this screen. Neither query result
resolves Vulkan access or establishes speed/quality. Model/kernel comparisons and full suites remain
[CI-only](validation.md).

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
