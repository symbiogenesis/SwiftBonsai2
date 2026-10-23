# Hexagon NPU execution plan

Planning decision, October 23, 2026. This expands M5 in [PLAN.md](../PLAN.md)
into an implementation and qualification sequence for the Snapdragon X Elite
X1E-78-100. All N0–N7 exits below are pending. The current engine builds and
launch policies use CPU execution; the [native Rust reference](../src/prism-runtime/README.md)
still uses scalar kernels. This document enables no backend, installs no SDK or
driver, and establishes no NPU performance result.

## Objective and scope

Use Hexagon where it improves useful completed-response latency or energy within
the laptop's memory, quality and sustained-comfort limits. Select among measured
CPU, Adreno and Hexagon configurations for the same Swift artifact. Maximum NPU
utilization, advertised TOPS and simultaneous use of all processors are not
acceptance criteria. Retain a reproducible CPU fallback and qualify each profile
independently; a finite tuning pass cannot prove a global optimum.

Keep the exact Swift weights, tokenizer/template, sampler, transforms, attention
topology and declared activation/cache precision. Pin PQ2 through the
[assessment lock](../config/swift-assessment.lock.json) and identify the verified
current-weight PTQ1 export separately. PQ2 uses Q8_K activation arithmetic and
PTQ1 uses Q8_0; lossless weight conversion does not make their execution
contracts identical. Use the [SIMD numerical contracts](simd-plan.md) and
[native engine roadmap](native-rust-engine.md) as the common CPU/NPU oracle work.
Use pinned Prism as the whole-model oracle until the Rust parity exits pass;
the experimental Rust graph is not an accepted whole-model substitute.
Requantization, approximate arithmetic, training and speculative decoding remain
separate experiments, with separate quality and memory decisions.

Start N0/N1 alongside the established bounded CPU work; neither requires the
complete Vulkan qualification or Rust replacement first. N2 onward needs a
trusted oracle for the operation being moved. Continue usable CPU/Adreno delivery
while closing NPU gaps. Budget approximately two engineering days for discovery
only, then estimate the kernel/graph port from its actual gaps. A substantial
ternary port can take weeks; the discovery budget is not its delivery estimate.
N0–N4 prototypes already require bounded owned-process lifetimes, deadlines,
memory supervision and safe device-buffer teardown. N5 adds the production
broker/profile integration; early experiments do not bypass those safeguards.

## Runtime routes and evidence

| Route | Planned role | Gate before relying on it |
| --- | --- | --- |
| Prism/ggml Hexagon backend | First source-audit and Swift packed-kernel prototype route; preserve the pinned Prism graph | Exact PQ2/PTQ1, transforms, hybrid operations, adapter path and signed deployment must work together |
| GenieX `llama_cpp` plugin | Supported Windows device-access control and possible conventional-GGUF reference | Pin plugin/package, exact format and actual placement; generic GGUF loading is insufficient |
| GenieX `qairt` or ONNX Runtime GenAI/QNN | Alternative compiled-model route if discovery establishes a concrete advantage | Explicit faithful export, chipset-specific package, quantization/shape/cache contract and full memory accounting |
| Native Rust device boundary | Later integration with M7 after the chosen kernel/graph contract passes | Safe Rust ownership API, audited driver boundary and the existing no-C/C++-inference-library replacement requirement |

Qualcomm documents separate [GGUF and QAIRT plugins][geniex-sdk] and recommends
`Q4_0` for its [GGUF NPU route][geniex-models]. Neither establishes support for
Swift PQ2/PTQ1. A small supported-type operator fixture is the preferred access
control; use a separately pinned small conventional model only if needed.
It validates that runtime/device combination, never Swift support or a new model
choice. Do not provision a second large model or keep a control model resident
with Swift. GenieX/Prism controls remain external reference engines; wrapping
them in Rust does not satisfy M7's inference-library replacement requirement.

The [GenieX v0.8.0 release][geniex-signed] reports a Microsoft-signed HTP catalog
for its bundled llama.cpp revision. Custom DSP kernels change that identity;
establish a supported signing/distribution route for the resulting binaries.
The [upstream Windows guide][hexagon-windows] also describes test-signing.
Changing Secure Boot, boot policy, driver signing or certificate trust requires
a separate explicit owner decision and is outside ordinary setup. A missing
signed deployment route is a recorded blocker, not permission to weaken policy.

[ONNX Runtime GenAI's QNN guide][qnn-genai] requires a prepared model package;
enabling a provider cannot execute the existing GGUF directly. Converting 27B
weights to one byte each is roughly 27 GB before scales, state, activations and
duplicate buffers; two bytes each is roughly 54 GB. These are order-of-magnitude
storage calculations, not feasible 32 GB deployment budgets. Retain efficient
packed execution or demonstrate a bounded alternative before undertaking export.

## Delivery sequence and exits

| Stage | Concrete output | Exit condition |
| --- | --- | --- |
| N0 — Native access and reproducibility | Host/runtime/signing manifest and bounded supported-type device check | Native Windows ARM64 execution on the actual Hexagon is proved, with a deployable signed route or an explicit blocker |
| N1 — Exact Swift compatibility | Per-operation/tensor/shape placement matrix and scoped port decision | Every unsupported operation, numerical contract, memory limit and fallback boundary has an owner and proposed treatment |
| N2 — Packed kernels and transforms | PQ2 kernel first, PTQ1 separately, with golden fixtures and bounded scratch | Codec, activation, scale/transform and operator correctness pass before timing |
| N3 — Hybrid graph and resident state | Executable contiguous graph regions, state ownership and memory ledger | Whole-model numerical/state correctness and bounded cancellation/recovery pass for the proposed placement |
| N4 — Phase placement and tuning | Small measured CPU/Adreno/Hexagon comparison with bottleneck traces | At least one placement has a reproducible useful benefit, or an explicit no-go identifies the limiting cost |
| N5 — Broker and resource integration | Separate named experimental profile, guarded loader and lifecycle/fallback policy | Existing authentication, exact count, process ownership and resource contracts pass with the device boundary |
| N6 — Device qualification | Paired correctness, quality, latency, memory and sustained-energy report | Required CI gates and predeclared per-metric criteria pass on settled main |
| N7 — Profile promotion and upkeep | Reproducible accepted profile, rollback and upgrade checklist | Only the qualified configuration becomes selectable for daily use; remaining gaps stay visible |

### N0/N1: Inventory and compatibility

- [ ] Record native host/device architecture, installed driver/firmware identities
  where observable, Hexagon architecture, SDK/compiler versions, host DLL and
  DSP library hashes, catalog signatures, ABI compatibility, redistribution
  terms and exact source commits. Recheck moving documentation against those
  pins. Preserve inherited Windows system/profile environment variables; use
  explicit absolute temporary working/output directories for diagnostics.
- [ ] Run a bounded supported-type correctness fixture with placement evidence.
  Inspect backend dispatch and device traces to distinguish Hexagon execution
  from CPU fallback. Device enumeration, a successful model load or an NPU
  utilization percentage alone does not establish placement. Keep timing and
  profiling captures in CI, including single-operator experiments.
- [ ] Build a matrix from the exact captured Swift graph: operation, tensor
  type, dimensions/strides, activation/cache precision, transformation/basis,
  supported backend, fallback and resident/scratch bytes. Include embeddings,
  PQ2/PTQ1 and floating projections, Hadamard/sign operations, normalization,
  RoPE, full attention, Gated Delta Net/convolution, gated feed-forward, output
  projection, adapter operations and the configured cache/checkpoint path.
  Check both single-token decode and prefill shapes. Generic Q2/Q4/Q8 support
  is not proof of PQ2/PTQ1 support or complete graph coverage.
- [ ] Choose one bounded prototype region using the matrix and existing CPU
  evidence. Record the expected limiting costs and missing work; audit the
  [HTP ternary prototype][htp-prototype] only at an immutable revision with
  license, codec and transform review. Its existence is not golden correctness
  or target-device performance evidence. End discovery with proceed, narrow
  scope, or blocked, including the exact reason and condition for reopening.

### N2/N3: Packed execution, graph state and memory

- [ ] Implement checked lossless host tiling and fused packed matvec first.
  Preserve PQ2 symbol semantics, scales, Q8_K quantization, integer sums and
  scale application order. Qualify PTQ1's canonical encoding and distinct Q8_0
  rounding separately. Follow [S0–S3](simd-plan.md#delivery-sequence-and-exits)
  for zero/extreme inputs, fourth-symbol handling, malformed data, unaligned
  blocks, tails, awkward dimensions and initial bitwise operator parity.
  Later reassociation/FMA or precision changes need their own declared
  tolerances and quality decision.
- [ ] Evaluate HVX vector execution for packed decode and HMX matrix tiles for
  prefill where the selected hardware/API supports the required arithmetic.
  Neither unit earns an assumed speedup. If HMX requires a different precision
  contract, retain the faithful path and label the alternative separately.
  Bound VTCM tiles, DMA staging, alignment/padding and host repack scratch;
  prototype fusion/overlap only after dependencies and numerical behavior pass.
- [ ] Preserve Hadamard normalization, sign placement, layer-specific axes and
  grouped recurrent projections. Follow the [adapter basis and qualification
  blockers](swift-lora.md): adapter branches must consume their specified
  original/transformed input, including the unresolved grouped-SSM basis.
  Compare no-LoRA and approved-LoRA lanes separately; no merge or fusion may
  silently change the standard adapter artifact or bypass its basis gate.
- [ ] Prefer contiguous supported regions with persistent immutable weights,
  reusable scratch and resident state. Specify ownership and representation of
  every KV, convolution and recurrent-state buffer, including checkpoints and
  phase changes. Compare fresh versus reused/edited prefixes, reset,
  cancellation, failed evaluation and restored continuations. Selected logits,
  intermediate states and deterministic streams need declared comparison
  rules; generated text alone is insufficient proof of equivalence.
- [ ] Produce a memory ledger for host weights/mappings, NPU tiles/repacked
  copies, RPC/shared buffers, active/pinned mappings, KV/recurrent state,
  checkpoints, compilation, graph caches and peak load/repack overlap. Charge
  unique physical allocations once and separately record mapping/address-space
  budgets; shared memory does not imply one copy or zero-copy handoff.
  Include adapter storage and all live CPU/GPU/NPU copies.
- [ ] Start with one NPU session. The upstream [backend design][hexagon-design]
  describes an approximately 3.5 GB virtual mapping window, dynamic mapping of
  weights and multiple-session alternatives. Verify actual Windows behavior at
  the chosen pin; this is not a total-RAM ceiling or a universal device limit.
  Compare bounded dynamic mapping with layer-partitioned virtual sessions only
  when mapping overhead or capacity warrants it. Virtual sessions do not add
  physical NPUs. Account for per-session state, duplicated buffers and sync.
- [ ] Retain Swift's current 12 GiB service allowance and 4 GiB physical reserve
  during experiments; a larger allowance needs separate memory qualification.
  Include shared/device allocations outside Windows Job commit accounting in
  admission/watchdogs. Refuse launch or reduce the explicitly requested
  experimental placement when the full bounded ledger cannot fit; do not
  quietly borrow the Qwen reference's 18 GiB allowance or shorten context.

### N4: Select placement separately for prefill and decode

Use a small successive screen, holding model, adapter setting, sampler, context
and precision fixed. Admit only configurations whose component/state correctness
has passed. Separate load/repack/compile, prompt processing, first-token wait,
steady decode and whole finished-task time.

| Configuration to investigate | Question it resolves |
| --- | --- |
| CPU control and the qualified Adreno profile when available | Does the NPU improve the best supported baseline, rather than an intentionally slow control? |
| Largest contiguous supported Hexagon regions in both phases | Does resident packed execution overcome dispatch and memory costs? |
| NPU prefill, CPU or Adreno decode | Does matrix-heavy prompt processing justify the state/layout migration? |
| CPU or Adreno prefill, NPU decode | Does low-overhead packed decoding improve sustained energy or latency? |
| Selected CPU/GPU/NPU regions | Does added parallelism beat extra crossings, shared-bandwidth contention and power competition? |

- [ ] Capture actual per-operation placement, fallback count, transferred bytes,
  RPC/submission/synchronization time, mapping churn, repack/compile time,
  memory traffic and device counters where supported. Identify compute,
  bandwidth or dispatch limits before adding fusion, tiling or DMA overlap.
  Report unavailable counters. Run profiling separately from clean performance
  captures so instrumentation cannot manufacture the claimed result.
- [ ] Screen batch/microbatch and tile sizes, HVX workers, supported power modes,
  CPU generation/prefill threads and waiting policy. Reuse buffers and command
  graphs only where shape/state ownership permits it. Review effective backend
  defaults; the [current upstream guide][hexagon-guide] documents a default
  alias to burst and warning-based fallback for unknown power modes. Validate
  requested settings before launch; compare balanced/sustained modes supported
  by the actual pin instead of accepting maximum clocks as optimal.
- [ ] Include phase migration, cache misses and startup costs in request time.
  Prefer fewer device crossings when equivalent arithmetic allows them; do not
  move all operations to Hexagon merely to increase utilization. Keep a
  configuration only if the measured gain survives sustained runs and the
  ordinary creative workflow. Screen all-three-device execution last.

### N5: Integrate an explicit profile and failure contract

- [ ] Add a separately versioned experimental NPU profile and matching build/
  artifact lock after N3. The fixed [CPU launch policy](../src/broker/src/launch_policy.rs)
  remains the reference. Explicitly allow reviewed device libraries, backend
  arguments and child-only environment settings; retain rejection of arbitrary
  inherited overrides. Validate requested versus effective placement and
  resource limits before readiness. A strict NPU request fails if its declared
  placement cannot be satisfied; a preferred-NPU profile may select an already
  qualified fallback before generation, recording the effective profile/reason.
- [ ] Keep authentication, Host/Origin/routes, exact token counts, canonical
  request settings, one active conversation, output/context caps and cancellation.
  Extend the audited process/device boundary rather than relaxing policy for a
  plugin. Any runtime service/helper beyond the current one-process Job limit
  needs explicit ownership, parent-death teardown, resource and isolation
  qualification. A device driver/service is not automatically contained by the
  inference Job. New device access does not establish daily isolation.
- [ ] Bound waits and test NPU timeout, allocation failure, device loss/reset,
  mapping failure, unsupported shape and cancellation during submitted work.
  Do not free or reuse buffers while the device may still access them. Restore
  complete committed KV/recurrent/checkpoint state or close admission until
  verified teardown/reset recreates it. Fail the in-flight request after an
  unrecoverable device error; do not resume from partial state on CPU or replay
  already streamed tokens as a hidden retry. Exercise restart and recovery.
- [ ] Bound private compiled-graph/repacked-weight caches. Key them by model and
  adapter digests, runtime/kernel/ABI versions, driver/firmware where observable,
  graph shape, precision and placement settings. Validate metadata before
  reuse, invalidate incompatible entries and account for warm/cold behavior.
  Keep payloads/prompts out of technical logs and ordinary telemetry.

### N6/N7: Qualification, promotion and rollback

All full suites and every benchmark, including microkernels, profiling, smoke
and dry runs, execute only in CI under [the validation policy](validation.md).
Local builds, source/assembly inspection and focused untimed correctness checks
are allowed. Device qualification requires reviewed manual main-only self-hosted
CI on this laptop; generic hosted ARM64 runners cannot provide Hexagon evidence.
This documentation update requires no runner setup, dispatch, push or CI polling.

- [ ] Predeclare fixtures, metric tolerances and failure criteria. Start within
  the admitted 8K profile with 512-token, 2K-token and near-8K prompts bounded
  by the required output reserve, fixed 128/512-token captures and separate
  natural creative completions. Include warm/cold model and compiled caches, reused/edited
  prefixes, occupied-context decode, cancellation and failed recovery. Extend
  context only after its separate state/memory acceptance.
- [ ] Repeat/interleave CPU, qualified Vulkan and viable NPU configurations;
  record median/p95, variance, prefill, time to first token, decode TPS, time/
  tokens/retries to acceptable output, peak/resident/shared memory, energy per
  acceptable task and sustained behavior. Compare AC and battery independently,
  including 15–30-minute runs, OS responsiveness and heat/fan observations.
  Missing valid power telemetry means no energy-efficiency verdict.
- [ ] Require numerical/state correctness, required full CI suites, daily
  security/resource acceptance and no material quality regression. Apply the
  mandatory [LoRA TPS and quality parity gates](swift-lora.md#required-standard-profile-qualification)
  against the same NPU engine without LoRA, separately from NPU-versus-CPU/GPU
  comparisons at a fixed adapter setting. Resolve basis blockers first.
- [ ] Use [the project promotion criteria](../PLAN.md#promotion-criteria): target
  at least 10% lower end-to-end latency, or a predeclared capacity/energy benefit
  within accepted interactive limits. Set the minimum energy/capacity benefit
  and allowed per-metric regressions before capture. Uncertainty, missing
  evidence or an unresolved critical failure leaves the profile experimental.
- [ ] Retain a result under `docs/results/` when evidence exists, linking CI run,
  tested main revision, artifact/runtime/driver identities, effective placement,
  raw captures and per-metric verdicts. Promote only the winning interactive or
  battery profile; record an explicit no-go if Hexagon offers no useful gain.
  Preserve the pinned CPU fallback, invalidate stale caches and requalify
  affected gates after runtime/kernel/driver updates. Device absence must have
  a bounded, visible fallback or failure path.

A later small NPU embedding model belongs to a qualified retrieval feature;
an NPU draft model belongs to the separate speculation/rollback work. Neither
counts as accelerating Swift decoding. Reopen a blocked Swift lane when its
specific signed-kernel, graph, memory or performance limitation changes.

## Sources and implementation handoff

Primary documentation was checked October 23, 2026. Moving branches describe
research leads; N0 must pin the exact implementation and recheck each claim.
The operator matrix, manifests, kernel fixtures, memory ledger, profile and
device reports above are required future outputs, not files or gates already
implemented by this plan.

[geniex-sdk]: https://github.com/qualcomm/GenieX/blob/main/sdk/README.md
[geniex-models]: https://geniex.aihub.qualcomm.com/en/models/supported
[geniex-signed]: https://github.com/qualcomm/GenieX/releases/tag/v0.8.0
[hexagon-windows]: https://github.com/ggml-org/llama.cpp/blob/master/docs/backend/snapdragon/windows.md
[hexagon-guide]: https://github.com/ggml-org/llama.cpp/blob/master/docs/backend/snapdragon/README.md
[hexagon-design]: https://github.com/ggml-org/llama.cpp/blob/master/docs/backend/snapdragon/developer.md
[qnn-genai]: https://github.com/microsoft/onnxruntime-genai/blob/main/docs/qnn.md
[htp-prototype]: https://github.com/aakar-mutha/llama.cpp/tree/ternary-bonsai-htp
