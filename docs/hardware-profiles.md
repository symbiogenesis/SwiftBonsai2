# Automatic Snapdragon X1/X2 selection contract

Current released policies are CPU-only: no automatic selector or qualified accelerator default is
established. [PLAN.md](../PLAN.md#hardware-selection) tracks outstanding work; [capability
contracts](hardware-capabilities.md) describe implemented discovery and evidence APIs. Runner owns
discovery, selection, verification and admission. [Prism Inference][prism] owns kernels,
graph/device/state execution and effective-placement reporting; the versioned handshake keeps kernel
eligibility rules out of the broker.

## Device classes and observed identity

The 2026-10-29 development inventory reports an ASUS Zenbook A16, Snapdragon X2 Elite Extreme
X2E94100, 18 physical/logical cores and Adreno X2-90 driver 32.0.149.0. This identifies a target,
not optional ISA, performance or backend readiness. The [X2 CPU screen](x2-cpu-optimization.md)
retains its phase-specific thread controls and exact device observations.

| Target | Candidates | Required independent evidence |
| --- | --- | --- |
| X1: initially X1E-78-100 / Adreno X1-85 / Hexagon v73 | Packed scalar/NEON CPU, Vulkan, viable packed Hexagon regions | Windows ISA/driver, signed NPU access, mapping costs, exact Swift graph/state and device results |
| X2: initially Elite Extreme / Adreno X2-90 / Hexagon v81 | Packed CPU/Vulkan, batched HMX prefill, packed HVX decode | Actual SKU/capabilities, signed package, extended DMA and separate phase/sustained results |
| Unknown SKU or invalidated/unavailable accelerator | Reviewed compatible CPU fallback | Native ARM64, baseline ISA, verified artifacts and ordinary memory/context admission; otherwise refuse execution |

Family rules cover only independently qualified SKU/capability/driver classes. The
[X2E-94-100/X2E-96-100 product brief][x2-specs] advertises up to 228 GB/s system bandwidth and 80
INT8 TOPS; neither predicts Swift throughput or exclusive bandwidth, and other SKUs differ.

[DMA64][dma64] permits v81+ buffers beyond the older 32-bit mapping window only with the actual
supported Windows runtime/driver/DMA path; X1 mapping/tiling remains separate. Address windows are
not physical RAM budgets. [Q1/Q2 Hadamard support][q2-hadamard] does not establish PQ2/PTQ1
activation arithmetic, grouped recurrent transforms, adapters or signed distribution. A catalog for
existing binaries cannot sign rebuilt code.

Discovery reports, scoped CPU digests and verified runtime capability probes identify limited
observations, not accepted profiles. CPU records exclude accelerator API/firmware/readiness and
broader optional ISA/topology. Failed probes remain unknown. Live RAM/power affects admission, not
immutable compatibility identity.

## Eligibility, ranking and admission

Selection is a bounded deterministic lookup/ranking over versioned qualified profiles; ordinary
startup neither benchmarks nor tunes. Explicit CPU controls remain reproducible.

A profile binds exact model/derived artifact, adapter, runtime/build/backend, static hardware/driver
compatibility, numerical/state/settings contract, shapes, context/full output reservation, objective
and fresh/reused workload bucket. Required unknown, mismatched or stale facts exclude it. Discovery,
family matches and asserted receipts cannot authorize execution; signed backend readiness is
independent.

| Objective | Ranking basis |
| --- | --- |
| Interactive | Complete-task latency within qualified quality/resource/security constraints |
| Battery | Measured energy per useful task within accepted latency/quality/resource limits; battery operation alone proves no saving |

Retained device evidence, versioned thresholds and deterministic tie-breaking include losing/no-go
entries. Placement, phase threads/waits, batches/tiles, sustained power policy and state layout
belong to profile identity. Four threads and all cores are controls, not universal optima.

Recheck current RAM/commit/device budgets and backend readiness immediately before admission. An
unavailable preferred accelerator may yield an eligible qualified CPU fallback; an explicit
strict-backend diagnostic fails. Never silently change model, sampler, adapter, precision, context
or full output reservation to fit a profile.

Report requested/effective profile, device class, phase placement, build/evidence identity and a
value-free fallback reason; exclude prompts/tensors. The [native Rust UI](ui-architecture.md)
presents a simple automatic choice. Tuning remains trusted configuration, excluding arbitrary public
arguments, paths and inherited environment overrides.

## Prefill, decode and complete state

Batched prefill reuses weight tiles across tokens; sequential blocks alone do not. Packed decode has
little reuse and pays bandwidth, unpacking, transforms and dispatch/submission costs. [Algorithm
contracts](inference-algorithms.md) separately govern tiled projections and chunkwise GDN.

Keep PQ2/PTQ1 packed storage and bounded local tiles as the control. Charge repacked/expanded
weights, duplicate CPU/GPU/NPU storage, scratch, load/repack/compile, synchronization, migrations
and cache misses. Residency, contiguous regions and reusable submissions help only with correct
ownership. Shared bandwidth/power makes simultaneous execution a measured candidate.

Plans remain stable during a request. Reevaluate power, memory, runtime or driver changes only at
safe boundaries. Backend/phase transitions require qualified complete KV/recurrent/convolution/
checkpoint migration with declared numerical/layout identity and cost, or verified teardown/
rebuild. Otherwise retain the eligible plan or return a bounded recovery result; never continue
partial work on another backend. Driver changes invalidate affected accelerator evidence/caches, not
an independently compatible CPU control.

Every route preserves one active conversation, authentication, exact token counts, deadlines,
cancellation, admission and daily isolation. Rust acceleration also preserves the audited safe
device and replacement boundaries in the [runtime contract](native-rust-engine.md).

## Qualification scope

Receipts bind actual SKU/capabilities, OS/driver/backend, artifacts/profile, settled main revision,
CI run, raw captures and uncertainty. Workloads distinguish cold/warm/reused/edited state, short/
long prefill, fixed/natural outputs, AC/battery, pressure and sustained use. Report first useful
token, complete task, decode, peak memory, responsiveness, thermals/comfort and observable energy.
One family/device does not qualify another; missing valid power telemetry excludes energy ranking.

Selector checks cover unknown/misidentified devices, missing ISA/signed backend, stale runtime/
driver/evidence/cache, budgets/workload boundaries, deterministic ranking, objective changes, CPU
fallback, strict rejection and complete-state handoff/failure/cancellation. Simulate negative ISA
cases; execute optional instructions/accelerators only on eligible hardware.

Promotion follows [PLAN.md's criteria](../PLAN.md#promotion-criteria), including independent [LoRA
TPS/quality](swift-lora.md), installation/rollback and affected-upgrade requalification. Qualified
families can coexist with unsupported classes or qualified CPU fallbacks.
[Validation](validation.md) permits local builds/focused untimed checks; full suites, benchmarks and
performance captures remain main-only CI on matching target devices. Documentation changes neither
enable profiles nor update pins.

[x2-specs]: https://www.qualcomm.com/content/dam/qcomm-martech/dm-assets/documents/Snapdragon-X2-Elite-Product-Brief.pdf
[dma64]: https://github.com/ggml-org/llama.cpp/pull/29197
[q2-hadamard]: https://github.com/ggml-org/llama.cpp/pull/30627
[prism]: https://github.com/symbiogenesis/prism-inference
