# Snapdragon X Elite Adreno GPU plan

Assessment and roadmap, October 23, 2026. **The implemented inference paths are
CPU-only; optimal Adreno use has not been demonstrated.** GPU acceleration is
planned, with no qualified native Windows ARM64 GPU build, whole-model GPU
correctness result, or target-device performance verdict recorded here.

The first objective is to qualify the existing pinned Prism Vulkan backend for
Swift. This is [M2](../PLAN.md#12-delivery-milestones-and-stopgo-decisions), and
does not depend on completing the native Rust replacement. The later
[Rust R6](native-rust-engine.md#ordered-work) path must meet the same device
and model contracts after Rust CPU parity. All steps below are pending.

## What the repository does today

| Evidence | Consequence |
| --- | --- |
| [Swift artifact/runtime lock](../config/swift-assessment.lock.json) pins the 6.71 GiB PQ2 model and Prism commit `73bfe6cc485554822f80c473a3c0c4f25e80f41b`, with a `win-cpu-arm64` package | A native CPU loading/decode reference exists; the installed package does not qualify Vulkan |
| [Baseline build](../scripts/Build-Engine.ps1) and [Prism overlay build](../scripts/Build-PrismEngine.ps1) set `GGML_VULKAN=OFF` and `GGML_OPENCL=OFF` | Current build recipes do not produce the proposed accelerator |
| [CPU completion arguments](../scripts/Bonsai.Completion.psm1), [server preflight](../scripts/Bonsai.ServerPolicy.psm1), and [owned broker launch policy](../src/broker/src/launch_policy.rs) explicitly select `--gpu-layers 0` and `--device none` | Installing a GPU driver alone will not activate inference offload; server policy also disables KV and operation offload |
| [Native Rust reference](../src/prism-runtime/README.md) implements scalar CPU execution | Its storage reuse is useful groundwork, not GPU execution or evidence of a GPU speed gain |
| [Doctor](../scripts/Doctor.ps1) records graphics identity/driver through Windows inventory | It does not yet qualify the Vulkan loader, ICD, features, memory budgets or selected kernels |

The existing CPU paths are deliberate correctness controls. GPU work needs a
separate explicit development profile, build receipt and bounded launch policy.
Keep the CPU control reproducible while the GPU profile is experimental.

## Backend choice and evidence boundary

Use **Prism Vulkan first** for the exact Swift PQ2 artifact and, independently,
the [verified current-weight PTQ1 export](swift-memory.md). Prism's
[backend audit][prism-audit] documents released PQ2/PTQ1 scalar/coopmat1 and
integer-dot matrix-vector paths plus Bonsai transforms. Neither packing has a
direct coopmat2 decoder in that audit. Actual kernel selection depends on the
device and driver; packed decoding alone does not implement the complete graph.
This is source support, not an Adreno result.

Treat **OpenCL as a conditional alternative**. The [OpenCL guide][opencl]
documents Windows ARM64/X1-85 and conventional quantizations, but does not
establish Swift PQ2/PTQ1 and transform support. Its prebuilt binary kernel
library targets X2 GPUs and selected non-ternary operations; do not apply those
claims to X1-85. A compatible conventional model can test platform access under
its own manifest, but cannot qualify Swift or replace its comparison weights.

DirectML/Windows ML would require a separate faithful model/runtime integration.
[Microsoft's overview][directml] describes DirectML operators and ONNX Runtime
integration, not a GGUF provider toggle. Defer that conversion or a custom DX12
backend until Vulkan exposes a concrete limitation worth solving. Hexagon NPU
work remains independent; simultaneous CPU/GPU/NPU utilization is not a goal.

### Specific opportunities in the pinned implementation

The receipt-backed local Prism source at the locked commit exposes concrete
checks for bring-up and later optimization:

- The [Qwen35 graph][prism-graph] disables row-indexed recurrent-state reads and
  raw-gate fusion for Vulkan. Its [Vulkan operation checks][prism-vulkan] reject
  those variants. First qualify the existing gathered-state/activated-gate path;
  then profile the gathers and gate dispatches before adding equivalent shader
  support. Existing guards make this a tuning opportunity, not an identified
  correctness defect in normal execution.
- The same Vulkan operation checks enforce buffer/descriptor limits. From the
  [Swift dimensions](../config/swift.metadata.json), its `[5120, 248320]` token
  embedding alone has about 322.07 MiB of PQ2 payload, or 265.23 MiB after the
  current PTQ1 export. Verify that operation's actual placement and indexing
  path; a sufficient heap budget does not prove one tensor fits. Preserve
  discovered limits rather than copying `FORCE_MAX_*` overrides.
- [LoRA payload sharing](../tools/prism/src/pool.rs) currently admits CPU
  buffers only. Its verified 2.5 MiB saving is not a Vulkan allocation result;
  separately audit device storage, adapter basis and forward execution.

## Delivery sequence and exits

### G0 — Record the actual native device contract

- [ ] Extend inventory with a bounded Vulkan capability report. Record the
  X1-85 device/vendor IDs, OS build, driver version, Vulkan API/loader/ICD
  identity and architecture, compute queues, enabled features/extensions and
  runtime-selected device. Reject accidental software adapters or an emulated
  inference executable. Verify deployed executable and in-process dependencies
  are native ARM64; record build-time shader tools separately.
- [ ] Record storage-buffer/allocation limits, memory heaps/types, workgroup
  limits, subgroup sizes/operations, FP16/storage support, integer-dot features
  and accelerated properties, and cooperative-matrix types/shapes if exposed.
  Compare them with the pinned backend's actual shader requirements.
  [Query capabilities][vulkan-query] and [subgroup properties][subgroups]; do not
  hard-code another Adreno generation's subgroup width. An exposed integer-dot
  feature and an [accelerated dot-product property][integer-dot] are distinct.
- [ ] Query per-heap budget/usage through [VK_EXT_memory_budget][memory-budget]
  when available; retain an explicit unavailable result and conservative
  admission if absent. Budgets can change and are not reservations. Include
  Windows available physical memory, commit and GPU residency/eviction evidence.
  The reported 128 MB dedicated memory is not this integrated GPU's capacity;
  neither installed RAM nor a reported shared-memory maximum is an allocation
  entitlement.

**Exit:** a retained device/build capability record and a list of unsupported
requirements. Driver changes are separate managed changes, followed by renewed
qualification; a newer version alone is not an improvement verdict.

### G1 — Build an isolated Prism Vulkan candidate

- [ ] Add a separate GPU build mode at the Swift lock's immutable Prism commit,
  beginning with `GGML_VULKAN=ON`. Pin the Vulkan SDK/headers, shader compiler,
  ARM64 link libraries, compiler/options, source overlay and output hashes.
  Audit that revision's CMake and shader-generation requirements before writing
  a runnable recipe. Keep unrelated accelerators disabled for initial diagnosis.
- [ ] Preserve CPU build/install lanes. Do not combine the x64 Vulkan release
  package with ARM64 CPU binaries, or load unrelated ggml DLLs. Check the native
  dependency chain and enumerate the candidate's effective devices/options.
- [ ] Add an explicit experimental GPU launch policy using only switches verified
  against that binary. Capture requested and effective offload, device, cache,
  batch and memory settings. Retain authentication, cancellation, deadlines,
  process ownership and existing admission controls. Reassess their coverage of
  GPU allocations: a Windows Job commit cap does not bound every mapped or
  GPU-resident byte. Do not merely remove the CPU-only arguments globally.

**Exit:** a reproducible native GPU build and bounded diagnostic launch; no
daily-profile promotion or performance claim yet.

### G2 — Prove exact-model execution and placement

- [ ] Produce an operation/placement ledger for the actual Swift tensor
  inventory: PQ2/PTQ1 and remaining types; activation quantization/scales;
  Hadamard/sign/group transforms; embeddings/output; full attention/RoPE; Gated
  Delta Net/convolution; KV and recurrent state; and the approved LoRA path.
  Record CPU fallbacks and each CPU/GPU transfer or synchronization boundary.
  Layer-offload count alone cannot demonstrate graph coverage.
- [ ] Begin with focused packed-kernel and transform fixtures, including awkward
  dimensions, extreme/zero values, non-contiguous inputs where supported, and
  unsupported-shape rejection or explicit fallback. Declare tolerances and
  accumulation/activation precision before GPU comparisons. Native CPU
  matrix-vector traits include PQ2/Q8_K and PTQ1/Q8_0 paths, with a PQ2/Q8_0
  fallback for widths not divisible by QK_K. The pinned Vulkan integer-dot
  matrix-vector [shader family][prism-matvec] uses Q8_1 activation blocks
  ([PTQ1 shader][prism-ptq1]). Record the selected path's actual arithmetic;
  weight parity does not imply bit-identical CPU/GPU logits. Qualify existing
  backend differences explicitly before optimizing that contract further.
- [ ] Compare each GPU lane against its own same-artifact CPU reference with
  fixed token IDs/settings. Check selected logits and recurrent/KV state over
  prefill and multiple decode steps, then template/tokenizer/sampler behavior,
  prefix restoration, reset, edited turns, cancellation and failure recovery.
  Test the no-LoRA control and approved adapter, including transformed basis
  and actual forward execution. Successful loading or plausible text is
  insufficient. Full suites remain CI-only.

**Exit:** retained numerical/correctness evidence and an honest placement ledger
for each supported profile, with unresolved fallbacks visible.

### G3 — Screen a small set of placements in device CI

- [ ] Compare CPU-only, a small set of partial offloads, and the maximum feasible
  offload within the memory envelope. Hold model/adapter, context, sampler,
  precision and workload constant. Start at the admitted 8K/one-slot reference;
  qualify longer contexts separately. Freeze batch/thread choices for the first
  placement comparison, then vary one factor at a time on promising profiles.
- [ ] Measure prefill and decode separately. Use the existing plan's bounded
  batch/microbatch and CPU thread/polling candidates rather than a full Cartesian
  sweep. Evaluate attention/cache placement only when supported and correct;
  qualify fused/flash attention separately. Phase-specific device placement is
  a later experiment if the runtime exposes it or a justified implementation
  adds it; include migration costs rather than inventing a launch switch.
- [ ] Retain kernel/queue timestamps where supported, CPU submission/wait time,
  transfer bytes, allocation/repack copies and the end-to-end critical path.
  Use Windows ETW/[GPUView][gpuview] when compatible to inspect scheduling and
  synchronization; verify tool/counter support on native Windows ARM64. GPU
  utilization percentages alone do not identify the bottleneck, and overlapping
  CPU/GPU durations cannot simply be added.

**Exit:** paired target-laptop CI results identifying a useful placement or a
documented no-go. Maximum offload is a candidate, not the expected winner.

### G4 — Optimize the demonstrated bottleneck

| Measured cost | First experiment | Evidence required |
| --- | --- | --- |
| Packed matrix work / memory traffic | Fused unpack-and-dot, shape-specific tiling, coalesced packed loads and supported integer-dot paths; compare scalar/coopmat1 choices where available | Same arithmetic contract, kernel correctness, actual selected shader and lower full-task cost |
| Intermediate tensors / CPU fallback crossings | Fuse compatible transforms or projections; keep recurrent/KV state and scratch resident; place contiguous supported work together | Correct state ownership, fewer transfers and lower peak residency; count LoRA temporaries |
| Submission / synchronization overhead | Reuse persistent allocations, descriptors/pipelines and command/graph work where safe; batch dispatches and narrow barriers | Critical-path trace improvement without stale buffers, lifetime violations or cancellation/reset failures |
| Shader compilation / cold startup | Qualify backend-supported persistent caches, keyed by source/options/toolchain/device/driver | Separate cold and warm startup, reliable invalidation and bounded private cache storage |
| Shared-memory pressure / sustained throttling | Reduce microbatch, offload or retained cache/checkpoints; tune CPU activity alongside GPU work | No paging/eviction regression, useful context preserved and better energy/task or sustained latency |

These are hypotheses, not promised gains. Budget packed weights, expanded or
repacked copies, staging, scratch, LoRA, KV, recurrent checkpoints and CPU mappings
together without double-counting shared allocations. UMA does not establish
zero-copy access. Prefer register/tile unpacking to a persistent expanded model
unless measured evidence justifies its startup and residency costs. Preserve
model/cache precision and decoding semantics; lower precision, speculation and
other quality-changing methods remain separate lanes. Apply Amdahl/bandwidth
bounds before a backend rewrite.

### G5 — Qualify and promote a useful profile

- [ ] Retain exact model/export/adapter hashes, source/patch/build identities,
  OS/driver/device capabilities, effective arguments, placement, power mode,
  workload, CI revision/run and raw results. Repeat interleaved CPU/GPU trials
  with predeclared per-metric tolerances and sustained workloads.
- [ ] Record cold load/compile/repack time, warm prefill, first streamed token,
  first useful final-answer token, decode tokens/second and completed creative
  task time. Include peak/resident memory, available RAM, commit, hard faults,
  GPU evictions, energy/task, thermals/fan behavior, UI responsiveness and
  failures. Mark unavailable counters explicitly. Use an energy measurement
  method with known scope instead of inferring battery savings from utilization.
- [ ] Require correct behavior, useful creative quality, resource headroom and
  sustained comfort. Preserve the mandatory [standard LoRA TPS and quality
  comparison](swift-lora.md#required-standard-profile-qualification) within the
  same GPU engine, as well as CPU/GPU comparisons at the same adapter setting.
  Select validated interactive/battery profiles from measured tradeoffs; GPU
  percentage and a short tokens/second peak are not acceptance criteria.
- [ ] Integrate the accepted profile into broker admission and expose an
  actionable unavailable/fallback reason. Verify device loss, allocation failure,
  driver changes and cancellation clean up owned resources. Fail the affected
  request safely; switch to an explicitly admitted CPU profile rather than
  silently changing execution during a conversation. Retain rollback receipts.

**Exit:** a profile-specific verdict with evidence links, limits and CPU fallback,
or a documented no-go. A finite screen establishes a useful configuration, not
a global optimum. No current checkbox is satisfied by this documentation change.

## Validation and implementation order

G0/G1 can proceed with local builds and focused capability/correctness checks.
G2 precedes performance capture; **all benchmarks, profiling captures for timing,
and full test suites run only in CI**, on the target laptop for Adreno evidence.
No such runs or runner setup are required for this documentation-only assessment.
Follow [validation policy](validation.md) and [the device CI plan](../PLAN.md#10-validation-and-performance-evidence).
Diagnostics use an absolute system-temp working directory, preserve inherited
Windows system/profile variables, and retain useful evidence outside Git.

Complete initial Prism G0–G3 before committing to custom shaders or a new API.
Carry its capability report, fixtures and placement ledger into Rust R6 after
CPU parity; reassess GPU bindings/shaders and native safety boundaries there.
Do not assume a Rust port inherits Prism's GPU coverage or performance.

Primary sources below were checked October 23, 2026. Moving documentation is
research evidence; implementation must pin its dependencies and recheck the
selected revision. Recommendations and exits above are proposed engineering
work, not measurements or certifications of this laptop.

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
