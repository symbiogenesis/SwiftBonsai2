# Hexagon NPU execution plan

This expands [M5](../PLAN.md) for Snapdragon X1 and X2 under the planned
[hardware profile selector](hardware-profiles.md). The recorded X1E-78-100 is
the initial device; X2 Elite Extreme is an additional qualification target.
**All N0–N7 exits are pending.** Current builds/launch policies use CPU execution,
including scalar/NEON in the [Rust reference](https://github.com/symbiogenesis/prism-inference/blob/main/README.md). No NPU
performance, deployment or automatic-selection qualification is established on
either family.

## Scope and route selection

Qualify Hexagon only where it improves completed-response latency or energy
within memory, quality and sustained-comfort limits. Compare the same Swift
artifact against CPU and qualified Adreno profiles. Retain CPU fallback; device
utilization/TOPS and simultaneous CPU/GPU/NPU use are not acceptance criteria.
The intended result is automatic selection of a qualified configuration for the
observed device, workload and interactive/battery objective. X1 and X2 may select
different backends, phase placements, batches, worker counts and power modes.

### X1 and X2 qualification lanes

| Lane | Starting hypothesis | Required evidence |
| --- | --- | --- |
| Recorded X1E-78-100 / Hexagon v73 | Compare packed HVX execution with CPU NEON and Adreno; bound mapping-window and dispatch costs | Actual runtime/driver access, signed kernels, exact Swift graph/state and useful device benefit |
| X2 / Hexagon v81, initially X2 Elite Extreme | Prioritize batched HMX prefill and packed HVX decode with extended DMA mappings; retain CPU/Adreno comparisons | Verified extended-mapping path, arithmetic and signed package at the selected revision, separate prefill/decode and sustained evidence |

These are experiment priorities, not default placements or family-wide results.
Query actual Hexagon architecture, runtime and driver capabilities; a processor
name cannot enable an unverified path. Qualification receipts must identify the
exact SKU/device capabilities, driver/runtime/kernel package and resource limits
accepted by the selector. Missing, changed or unsupported capabilities exclude
that candidate and retain a qualified CPU fallback.

Upstream [64-bit DMA support][hexagon-dma64], merged September 21, 2026, targets
v81 and newer and avoids dynamic weight-buffer mappings above the old address
window through DMA/VTCM execution. [Q1/Q2 Hadamard matmul and row reads][hexagon-q2],
merged October 18, 2026, provide a useful implementation lead. Neither change
establishes support for this project's PQ2/PTQ1 artifacts, complete graph or
signed Windows distribution. Pin and audit source before reuse.

Upstream Q2_0 has 64 weights per scale; Prism PQ2 has 128. A candidate bridge
splits each PQ2 block and duplicates its scale, retaining decoded weights at
approximately 5.9% more packed-weight storage. Verify symbol layout, raw scale
bits and transform metadata. Activation quantization, reduction/scale order and
logits/state still require separate qualification against the PQ2/Q8_K contract.
Keep [upstream layouts][hexagon-design] and
[Prism numerical contracts](simd-plan.md) explicit in the operation ledger.

Preserve weights, tokenizer/template, sampler, transforms, attention and declared
activation/cache precision. Pin PQ2 through the [assessment lock](../config/swift-assessment.lock.json)
and identify [current-weight PTQ1](swift-memory.md) separately. PQ2's Q8_K (or
odd-group Q8_0) and PTQ1's Q8_0 arithmetic need independent qualification. Use
[SIMD contracts](simd-plan.md) for operation fixtures and pinned Prism as the
whole-model oracle until [Rust parity](native-rust-engine.md) passes. Requantization,
approximation, training and speculation remain separate experiments.

N0/N1 can proceed alongside CPU work without waiting for Vulkan or Rust replacement.
Budget about two engineering days for discovery, then estimate the actual port.
N2 onward requires a trusted operation oracle. Every prototype already needs
bounded process lifetime, deadlines, memory supervision and safe device teardown;
N5 adds production broker integration.

| Route | Role | Required proof |
| --- | --- | --- |
| Prism/ggml Hexagon | First source audit/packed-kernel prototype; retain pinned graph | PQ2/PTQ1, transforms, hybrid graph, adapter and signed deployment together |
| GenieX `llama_cpp` | Device-access control/possible conventional-GGUF reference | Exact plugin/package, supported format and actual placement |
| GenieX `qairt` or ONNX Runtime GenAI/QNN | Conditional compiled-model route | Faithful export, chipset package, precision/shapes/cache and bounded memory |
| Native Rust device boundary | Later M7 integration | Safe ownership API, audited driver boundary and no C/C++ inference-library dependency |

Qualcomm's [GGUF/QAIRT plugins][geniex-sdk] and [Q4_0 recommendation][geniex-models]
do not establish Swift PQ2/PTQ1 support. Prefer a small supported-type operator
fixture for device access; use a separately pinned small model only if necessary.
It cannot qualify Swift or change model selection. Do not provision another large
model or keep the control resident with Swift. Wrapping external engines in Rust
does not satisfy replacement requirements.

[GenieX v0.8.0][geniex-signed] reports a signed HTP catalog for its bundled revision.
Verify that the selected v73/v81 binary and catalog cover the required kernels;
the bundled signature does not cover later source changes or rebuilt DSP code.
Custom DSP binaries need their own supported signing/distribution route. The
[upstream Windows guide][hexagon-windows] also describes test-signing. Changes to
Secure Boot, boot/signing policy or certificate trust require a separate owner
decision. Missing signed deployment is a blocker.

[ONNX Runtime GenAI/QNN][qnn-genai] requires a prepared model package, not a GGUF
provider switch. Expanding 27B weights to one/two bytes each costs roughly
27/54 GB before scales, state and duplicate buffers. Demonstrate packed execution
or another bounded representation before exporting for a 32 GB device.

## Delivery sequence and exits

| Stage | Output and exit |
| --- | --- |
| N0 — Native access | Host/runtime/signing manifest and bounded device fixture; prove native ARM64 Hexagon execution and a signed deployable route, or record the blocker |
| N1 — Swift compatibility | Per-operation/type/shape placement matrix; assign treatment for every numerical, memory and fallback gap |
| N2 — Packed kernels/transforms | PQ2 first, PTQ1 separately; codec, activation, scale/transform and operator fixtures pass before timing |
| N3 — Hybrid graph/state | Contiguous graph regions, ownership and memory ledger; whole-model state correctness plus bounded cancellation/recovery pass |
| N4 — Placement/tuning | Separate X1/X2 CPU/Adreno/Hexagon screens with phase and objective costs; demonstrate reproducible useful benefit or no-go |
| N5 — Broker/resources | Versioned experimental profile compatible with hardware selection and guarded lifecycle/fallback; preserve authentication, exact count, process and resource contracts |
| N6 — Device qualification | Per-device quality/correctness/latency/memory/energy evidence; required CI and predeclared metric criteria pass on settled main |
| N7 — Promotion/upkeep | Qualified selector entries, fallback/rollback and upgrade checklist; unresolved family/SKU gaps remain explicit |

### N0/N1: Inventory and compatibility

- [ ] Record host/device architecture, observable driver/firmware, Hexagon/SDK/
  compiler, DLL/DSP hashes, signatures, ABI, licenses and source commits. Recheck
  sources against pins. Keep separate X1/v73 and X2/v81 records and verify the
  actual mapping and execution capabilities exposed on each. Use absolute temp
  working/output paths and preserve
  inherited Windows system/profile environment variables.
- [ ] Run a bounded supported-type correctness fixture with dispatch/trace proof
  of Hexagon placement. Enumeration, loading and utilization alone are insufficient.
  Keep timing/profile captures in CI, including single operators.
- [ ] Map the captured Swift graph's operations, types, strides/shapes, precision,
  transforms/basis, support/fallback and resident/scratch bytes. Include embeddings,
  packed/floating projections, normalization/RoPE, full attention, GDN/convolution,
  feed-forward/output, adapters and caches/checkpoints for prefill and decode.
  Generic Q2/Q4/Q8 support does not prove PQ2/PTQ1 or graph coverage.
- [ ] Select one bounded prototype region, with expected costs and owned gaps.
  Audit the [HTP ternary prototype][htp-prototype] at an immutable revision for
  license, codec and transforms. End discovery with proceed, narrower scope or
  blocked, including conditions for reopening.

### N2/N3: Packed execution, state and memory

- [ ] Implement checked lossless tiling and fused packed matvec. Preserve PQ2
  symbols, activation quantization, integer sums and scale order. Qualify PTQ1
  canonicality/Q8_0 rounding separately. Use [S0–S3](simd-plan.md#delivery-sequence-and-exits)
  fixtures for fourth symbols, extremes/zeros, malformed bytes, unaligned blocks,
  tails and awkward dimensions. Initial operator parity is bitwise; later FMA/
  reassociation needs declared tolerances and quality acceptance.
- [ ] Evaluate HVX packed work and HMX prefill only where the selected hardware/API
  supports the arithmetic. A different precision contract is a separate profile.
  Bound VTCM tiles, DMA staging, padding/alignment and repack scratch; add overlap/
  fusion only after dependencies and numerical checks pass.
  Keep weights packed in shared RAM and expand bounded tiles in VTCM; include
  persistent expansion as a separately budgeted candidate. HMX prefill needs
  genuine batched matrix work and compatible hybrid-state scheduling; sequential
  matvec block scheduling alone does not provide weight reuse.
- [ ] Preserve transform normalization/signs/axes and grouped recurrent projection
  basis. Resolve [adapter basis blockers](swift-lora.md), including grouped SSM,
  before approving LoRA. Compare no-LoRA/approved-LoRA lanes separately; fusion
  cannot change the standard adapter artifact or bypass its basis gate.
- [ ] Prefer contiguous regions, immutable resident weights, reusable scratch and
  owned KV/convolution/GDN state. Specify checkpoint and phase-change representation.
  Compare fresh/reused/edited prefixes, reset, cancellation, failed evaluation and
  restored continuation using logits/state and deterministic-stream rules.
- [ ] Account for host weights/mappings, device tiles/repacked copies, RPC/shared
  buffers, state/checkpoints, compilation/graph caches and peak load overlap.
  Count unique physical bytes once; record address-space budgets separately.
  Shared memory does not prove zero-copy. Include live CPU/GPU/NPU and adapter copies.
- [ ] Start with one NPU session. For the X1/v73 path, verify the approximately
  3.5 GB mapping window and dynamic mapping described by the
  [upstream design][hexagon-design]. For X2/v81, verify the selected signed
  runtime's [64-bit DMA mappings][hexagon-dma64] and actual DMA/VTCM-only access
  constraints. Address-space windows are separate from physical RAM budgets.
  Add layer-partitioned sessions only when evidence warrants overhead/state
  duplication.
- [ ] Retain the 12 GiB Swift service allowance and 4 GiB physical reserve.
  Qualify larger limits separately; include allocations outside Job commit in
  admission/watchdogs. Refuse or explicitly reduce placement if the ledger cannot
  fit; do not borrow Qwen's 18 GiB allowance or silently shorten context.

[Chunkwise WY/UT GDN](inference-algorithms.md#chunkwise-gated-deltanet-prefill)
needs faithful scheduling/fixtures and separate floating-order qualification.
Qualcomm's [multiplication-only inverse](https://arxiv.org/abs/2606.06034) uses
truncation/residual correction and belongs in an approximation lane. Phone NPU
results do not qualify Windows X Elite; strict triangularity alone does not
bound low-order error. [Low-bit LUT KV](inference-algorithms.md#compression-bound-and-deferred-experiments)
also differs from lossless physical FP16 storage.

### N4: Select prefill and decode placement

Hold artifact, adapter setting, sampler, context and precision fixed. Admit only
correct configurations. Separate load/repack/compile, prefill, first-token wait,
decode and completed-task time.
Run the screen separately for X1 and X2, including interactive and battery
objectives. Prefill and decode may use different kernels or devices only when
the qualified phase transition includes state/layout conversion and peak memory.
Prefer contiguous graph regions and batched submissions over individual matmul
RPCs. Shared system bandwidth and power contention remain part of the cost.

| Candidate | Question |
| --- | --- |
| CPU control / qualified Adreno | Does Hexagon beat the supported baseline? |
| Largest contiguous Hexagon regions | Does resident packed work offset dispatch/mapping costs? |
| NPU prefill, CPU/Adreno decode | Does prompt benefit exceed state/layout migration? |
| CPU/Adreno prefill, NPU decode | Does packed decoding improve sustained latency/energy? |
| Selected CPU/GPU/NPU regions | Does parallelism offset crossings, bandwidth/power contention? |

- [ ] Capture placement/fallbacks, transfers, RPC/submission/sync, mapping churn,
  repack/compile, traffic and supported counters. Identify compute, bandwidth or
  dispatch limits before tuning; mark unavailable counters. Separate instrumented
  profiles from clean performance captures.
- [ ] Screen bounded batches/tiles, HVX workers, power modes and CPU threads/waits.
  Reuse graphs/buffers only with valid shape/state ownership. Check effective
  defaults: the [upstream guide][hexagon-guide] describes burst aliasing and warning
  fallback for unknown modes. Validate settings; compare supported sustained modes.
- [ ] Include migrations, cache misses and startup in request cost. Retain only
  gains surviving sustained creative workloads. Screen all-three-device execution
  last, rather than maximizing utilization.

### N5: Integrate profile and failure contracts

- [ ] Add a separate experimental profile/build lock after N3. Extend the fixed
  [CPU policy](../src/broker/src/launch_policy.rs) with reviewed libraries, arguments
  and child-only environment settings. Keep rejection of arbitrary overrides.
  Check effective placement/resources before readiness. Strict NPU requests fail
  if unmet; preferred-NPU profiles may choose a qualified fallback before generation,
  recording the effective profile and reason.
  Feed the same eligibility, memory and phase-transition contracts into the
  [hardware selector](hardware-profiles.md); automatic mode cannot bypass them.
  Reconsider candidates at safe request boundaries after capability or resource
  changes, preserving state identity and the existing failure contract.
- [ ] Preserve authentication, Host/Origin/routes, exact counts, canonical requests,
  one active conversation, caps and cancellation. Helpers beyond the one-process
  Job limit need owned lifetime, parent-death teardown, resource and isolation
  qualification; drivers/services are not automatically Job-contained.
- [ ] Bound waits and test timeout, allocation/mapping failure, device loss,
  unsupported shapes and cancellation in submitted work. Never release buffers
  still in device use. Restore all committed state or close admission until
  verified teardown/reset. Fail unrecoverable requests without hidden replay or
  continuation from partial state on CPU.
- [ ] Bound private compiled/repacked caches and key by artifacts, kernel/runtime/
  ABI, observable driver/firmware, shape, precision and placement. Validate metadata,
  invalidate stale entries and account for cold/warm costs. Exclude payloads/prompts
  from technical logs and ordinary telemetry.

### N6/N7: Qualify, promote and retain rollback

- [ ] Predeclare fixtures, tolerances and failure criteria. Use admitted 8K prompts
  at 512, 2K and near-8K tokens with required output reserve; include fixed 128/512-
  token captures and natural creative completions. Cover cold/warm caches,
  reused/edited/occupied contexts, cancellation and failed recovery. Extend context
  only after separate memory/state acceptance.
- [ ] Repeat/interleave CPU, qualified Vulkan and viable NPU configurations.
  Retain median/p95/variance, prefill/first-token/decode, time/tokens/retries to
  acceptable output, memory, energy/task and sustained behavior. Compare AC/battery
  separately, including 15–30-minute runs, responsiveness and thermals/fan observations.
  No valid power telemetry means no energy verdict.
  Retain distinct X1/X2 device receipts and selector decisions. Test unknown
  processors, absent NPU, unsupported mappings, stale driver/runtime entries and
  insufficient memory; actual NPU execution requires a matching device lane.
- [ ] Require full CI correctness, daily security/resources and no material quality
  regression. Resolve adapter basis blockers and apply [LoRA TPS/quality gates](swift-lora.md#required-standard-profile-qualification)
  within NPU, separately from fixed-adapter CPU/GPU comparisons.
- [ ] Apply [promotion criteria](../PLAN.md#promotion-criteria): at least 10% lower
  end-to-end latency or a predeclared capacity/energy benefit within interactive
  limits. Set benefit/regression thresholds before capture. Missing evidence or
  unresolved critical failure keeps the profile experimental.
- [ ] Retain results under `docs/results/` with CI run/tested main revision,
  artifacts/build/driver, placement, raw captures and metric verdicts. Promote only
  qualified interactive/battery profiles or record no-go. Preserve CPU rollback,
  invalidate stale caches and requalify affected upgrades. Device absence needs
  bounded visible fallback/failure.

Retrieval embeddings and draft models are separately qualified features, not
Swift decode acceleration. Reopen blocked work only when its specific signed-
deployment, graph, memory or performance limitation changes.

## Validation and sources

Follow [validation policy](validation.md): local builds, assembly inspection and
focused untimed correctness checks are allowed. Full suites, benchmarks and
profiling/timing captures are CI-only. Device qualification needs reviewed manual
main-only CI on each target X1/X2 device; hosted ARM64 cannot supply Hexagon evidence.

Moving source links are research leads. N0 must pin/recheck them. Matrices,
fixtures, manifests, memory ledgers and device reports above are required future
outputs, not implemented files or satisfied gates.

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
