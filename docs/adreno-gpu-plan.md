# Snapdragon X1/X2 Adreno GPU plan

**Current inference paths are CPU-only.** No native Windows ARM64 GPU build,
whole-model GPU correctness result or target-device performance verdict is
qualified on either family. All G0–G5 exits remain pending. The eventual GPU
profiles participate in the planned [hardware selector](hardware-profiles.md)
alongside CPU and Hexagon profiles.

First qualify pinned Prism Vulkan for Swift under [M2](../PLAN.md#12-delivery-milestones-and-stopgo-decisions).
This can precede the Rust replacement. [Rust R6](native-rust-engine.md#ordered-work)
follows Rust CPU parity and needs its own GPU implementation and evidence.
Prism Vulkan is the first GPU implementation route. Compare it with Hexagon and
qualify X1 and X2 independently; prioritize packed NPU prefill/decode exploration
on X2 alongside the Vulkan comparison.

## Current constraints and backend choice

| Repository evidence | Constraint |
| --- | --- |
| [Assessment lock](../config/swift-assessment.lock.json) pins the 6.71 GiB PQ2 artifact, Prism `73bfe6cc485554822f80c473a3c0c4f25e80f41b` and `win-cpu-arm64` package | Installed runtime is a CPU reference |
| [Baseline](../scripts/Build-Engine.ps1) and [overlay](https://github.com/symbiogenesis/prism-inference/blob/main/scripts/Build-PrismEngine.ps1) builds disable Vulkan/OpenCL | A separate GPU recipe is required |
| [Completion](../src/cpu-policy/src/lib.rs), [server](../scripts/Bonsai.ServerPolicy.psm1) and [broker](../src/broker/src/launch_policy.rs) select `--gpu-layers 0` / `--device none` | Drivers alone do not activate offload; server policy also disables KV/operation offload |
| [Rust runtime](https://github.com/symbiogenesis/prism-inference/blob/main/README.md) uses scalar/NEON CPU kernels | Reusable storage does not provide GPU execution |
| [Doctor](../scripts/Doctor.ps1) inventories graphics identity/driver | Vulkan loader/ICD, features, kernels and budgets still need qualification |

Use **Prism Vulkan first**, testing PQ2 and [verified current-weight PTQ1](swift-memory.md)
independently. The [Prism backend audit][prism-audit] documents packed scalar,
coopmat1 and integer-dot paths/transforms, with no direct coopmat2 decoder for
these formats. Source support does not establish actual Adreno kernel selection
or complete graph coverage.

[OpenCL][opencl] is conditional: its Windows ARM64/X1-85 guidance does not establish
Swift PQ2/PTQ1 support. X2 prebuilt-kernel claims do not qualify X1-85.
[DirectML][directml]/Windows ML needs a faithful model/runtime integration, not a
GGUF switch. Defer conversion or custom DX12 until Vulkan exposes a measured
limitation. Hexagon discovery remains independent.

### Family and capability selection

The recorded X1 target is Adreno X1-85 in the X1E-78-100. X2 GPU variants,
including X2-90 in X2 Elite Extreme, need their own G0 inventory, shader/graph
checks and device qualification. Neither family names nor advertised API support
prove the required subgroup, integer-dot, cooperative-matrix or allocation
capabilities. Select kernels from queried features and accepted profile records.

The selector should compare complete CPU/GPU/NPU candidates for the observed
SKU/device, runtime/driver, workload size, available memory and interactive or
battery objective. GPU prefill and GPU decode may win different cases. Include
state migration, duplicated allocations and cold pipeline/repack costs before
selecting a mixed phase profile; prefer contiguous resident regions. Keep a
qualified CPU fallback when GPU access, capabilities, resources or evidence are
missing. Automatic policy and accelerated execution remain future work.

The pinned source identifies three bring-up checks:

- [Qwen35][prism-graph]/[Vulkan operation guards][prism-vulkan] disable row-indexed
  recurrent reads and raw-gate fusion. Qualify the existing gathered-state/
  activated-gate route before optimizing gathers or dispatches.
- Buffer/descriptor limits can constrain individual tensors. Swift's
  `[5120, 248320]` embedding has about 322.07 MiB PQ2 payload, or 265.23 MiB PTQ1.
  Verify actual placement/indexing; sufficient heap space does not prove it fits.
  Preserve discovered limits rather than copying `FORCE_MAX_*` overrides.
- [LoRA sharing](https://github.com/symbiogenesis/prism-inference/blob/main/tools/prism/src/pool.rs) admits CPU buffers only. Its 2.5 MiB
  storage saving does not qualify device allocations, basis or adapter execution.

## Delivery sequence and exits

### G0 — Record the native device contract

- [ ] Capture actual X1/X2 GPU IDs, processor SKU, OS/driver, Vulkan API/loader/ICD
  architecture, queues, enabled features/extensions and selected device. Verify native ARM64
  executable/dependencies; reject software adapters and emulated inference.
  Record build-time shader tools separately.
- [ ] Record buffer/allocation/workgroup limits, heaps/types, subgroups, FP16/
  storage support, integer-dot features/acceleration and cooperative-matrix shapes.
  Compare with pinned shader requirements. Use [capability queries][vulkan-query]
  and [subgroup properties][subgroups]; exposed dot support and
  [accelerated properties][integer-dot] are distinct.
- [ ] Query [memory budget/usage][memory-budget] where available; record absence
  with conservative admission. Include physical RAM, commit and residency/evictions.
  UMA/shared-memory reports are not allocation entitlements; 128 MB dedicated
  memory is not total GPU capacity.

**Exit:** a retained record per target device/build and unsupported requirements.
Driver changes require renewed qualification.

### G1 — Build an isolated Prism Vulkan candidate

- [ ] Add a separate mode at the locked Prism revision with `GGML_VULKAN=ON`.
  Pin SDK/headers, shader compiler, ARM64 libraries, compiler/options, overlay
  and output hashes after auditing that revision's CMake/shader requirements.
  Keep unrelated accelerators disabled initially.
- [ ] Preserve CPU build/install lanes. Verify native dependency identity;
  do not mix x64 GPU packages or unrelated ggml DLLs into ARM64 binaries.
  Enumerate effective devices/options.
- [ ] Add an explicit bounded experimental GPU launch policy using verified
  switches. Record requested/effective placement, cache, batches and memory.
  Retain authentication, cancellation, deadlines, process ownership and admission;
  a Job commit cap does not cover all mapped/device bytes.

**Exit:** reproducible GPU build and bounded diagnostic launch. CPU-only policy
changes apply only to the new profile.

### G2 — Prove exact-model execution and placement

- [ ] Build an operation ledger covering packed/floating types, activation
  quantizers/scales, transforms, embeddings/output, attention/RoPE, GDN/convolution,
  KV/recurrent state and approved LoRA. Record every CPU fallback, transfer and
  synchronization boundary; layer-offload count is insufficient.
- [ ] Check packed kernels/transforms with awkward shapes, extremes/zeros,
  supported strides and explicit fallback/rejection. Predeclare tolerances and
  precision. CPU PQ2/Q8_K, PTQ1/Q8_0 and odd-width PQ2/Q8_0 differ from the
  Vulkan integer-dot [Q8_1 shader family][prism-matvec] ([PTQ1][prism-ptq1]).
  Record selected arithmetic and qualify backend differences; weight parity does
  not imply bit-identical logits.
- [ ] Compare each lane with its same-artifact CPU control using fixed token IDs/
  settings. Check logits and KV/GDN state across prefill/decode, then tokenizer,
  template/sampler, prefixes, edits, reset, cancellation and recovery. Test no-LoRA
  and approved-adapter lanes, including basis and real forward execution.

**Exit:** numerical/state evidence and a placement ledger for each supported
profile, with unresolved fallbacks visible. Plausible text is insufficient.

### G3 — Screen placements in device CI

- [ ] Compare CPU-only, a few partial offloads and maximum feasible offload at
  fixed model/adapter, context, precision, sampler and workload. Start at 8K/one
  slot; qualify longer contexts separately. Freeze batches/threads initially,
  then vary one factor at a time.
  Run separate X1/X2 screens and compare qualified Hexagon candidates at the same
  artifact/numerical contract. No X1 or X2 result automatically qualifies another
  SKU, driver or GPU variant.
- [ ] Measure prefill/decode separately using bounded batch/microbatch and thread/
  polling candidates. Qualify attention/cache placement and flash fusion separately.
  Phase-specific placement requires runtime support and must include migration.
- [ ] Retain queue/kernel timestamps where supported, CPU submission/wait,
  transfers, copies and critical-path traces. Use compatible Windows ETW/
  [GPUView][gpuview] tooling after checking ARM64 support. Utilization alone is
  insufficient; overlapping durations cannot simply be added.

**Exit:** paired per-device results identifying useful prefill/decode placements
and interactive/battery tradeoffs or no-go, suitable for selector qualification.
Maximum offload is one candidate.

### G4 — Optimize the demonstrated bottleneck

| Cost | Candidate | Required evidence |
| --- | --- | --- |
| Packed work/traffic | Fused decode/dot, shape tiles, coalesced loads, supported integer-dot paths | Correct arithmetic, actual shader and lower task cost |
| Intermediates/fallback crossings | Compatible fusion, resident state/scratch and contiguous placement | Correct ownership, fewer transfers and bounded residency including LoRA |
| Submission/synchronization | Allocation/descriptor/pipeline reuse, batching and narrower barriers | Better critical path without stale buffers or lifecycle failures |
| Cold compilation | Bounded backend-supported caches keyed by source/options/toolchain/device/driver | Separate cold/warm results and reliable invalidation |
| Memory pressure/throttling | Smaller microbatch/offload/cache; coordinate CPU activity | No paging/eviction regression; improved sustained latency or energy/task |

Budget packed/repacked weights, staging, scratch, LoRA, caches/checkpoints and
mappings together, counting shared physical allocations once. UMA does not prove
zero-copy. Prefer tile/register unpacking unless persistent expansion justifies
its load/residency cost. Preserve precision/decoding; approximation and speculation
need separate lanes. Apply bandwidth/Amdahl bounds before a rewrite.

[Attention/locality and chunkwise GDN candidates](inference-algorithms.md#attention-locality-and-the-io-bound)
need declared numerical contracts. Scalar nonflash attention rounds globally
normalized probabilities to FP16 before multiplying V; ordinary online softmax
changes that intermediate. Preserve it or qualify a distinct contract. Retain
G2 state/logit, adapter and lifecycle checks after scheduling/reduction changes.

### G5 — Qualify and promote a profile

- [ ] Retain artifact/adapter/source/build hashes, capabilities/driver, effective
  arguments/placement, power mode, workload, tested main revision, CI run and raw
  results. Repeat/interleave CPU/GPU trials with predeclared per-metric criteria.
- [ ] Capture cold load/compile/repack, warm prefill, first streamed/useful answer
  token, decode TPS and completed-task time. Include peak/resident memory, commit,
  available RAM, hard faults/evictions, energy/task, thermals, UI responsiveness,
  sustained behavior and failures. Mark unavailable counters; use scoped energy
  measurement rather than utilization as a proxy.
- [ ] Require correctness, creative quality, headroom and sustained comfort.
  Preserve [standard LoRA TPS/quality gates](swift-lora.md#required-standard-profile-qualification)
  within the GPU engine, plus CPU/GPU comparisons at the same adapter setting.
  Choose interactive/battery profiles from measured tradeoffs under
  [project promotion criteria](../PLAN.md#promotion-criteria).
- [ ] Publish eligible GPU entries for the [hardware selector](hardware-profiles.md)
  with capability predicates, memory envelope, phase/state identity and evidence
  scope. Check unknown devices, disabled/unavailable GPU, stale driver/runtime
  records and memory pressure. Keep requested/effective selection and fallback
  reasons visible without exposing prompts.
- [ ] Integrate admission and actionable fallback reasons. Check allocation
  failure, device loss, driver changes and cancellation teardown. Fail damaged
  requests safely; admit CPU fallback explicitly rather than switching mid-
  conversation. Retain rollback receipts and requalify affected upgrades.

**Exit:** profile-specific verdict, limits, evidence and reproducible CPU fallback,
or a documented no-go.

## Validation and implementation order

G0/G1 allow local builds and focused untimed capability/correctness checks.
G2 precedes timing. Full suites, benchmarks and timing-profile captures run only
in CI, on each target X1/X2 device for Adreno evidence. Follow
[validation policy](validation.md) and
[device CI policy](../PLAN.md#10-validation-and-performance-evidence).
Diagnostics use absolute system-temp working/output paths and preserve inherited
Windows system/profile variables.

Complete Prism G0–G3 before investing in custom shaders/APIs. Carry capabilities,
fixtures and placement evidence into Rust R6, then requalify Rust coverage and
safety. Moving source links below are research leads; pin/recheck dependencies
during implementation.

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
