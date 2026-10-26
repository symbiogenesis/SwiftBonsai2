# Automatic Snapdragon X1/X2 selection contract

Outstanding hardware-selection work is tracked only in
[PLAN.md](../PLAN.md#hardware-selection). This document defines the eligibility,
ranking and state boundaries for native Windows ARM64 X1/X2 execution. Current
released launch policies are CPU-only; no automatic selector or qualified
accelerator default is established. The recorded development device is the
32 GB Snapdragon X1E-78-100.

## Device classes and observed identity

| Target | Relevant candidates | Evidence boundary |
| --- | --- | --- |
| X1, initially X1E-78-100 / Adreno X1-85 / Hexagon v73 | Packed scalar/NEON CPU, Vulkan, viable packed Hexagon regions | Actual Windows ISA/driver, signed NPU access, mapping costs, exact Swift graph/state and device results |
| X2, initially Elite Extreme / Adreno X2-90 / Hexagon v81 | Packed CPU/Vulkan, batched NPU prefill and packed decode | Actual SKU/capabilities, signed package and extended DMA, arithmetic/graph/state and independent phase/sustained results |
| Unknown SKU or unavailable/invalidated accelerator | Compatible reviewed CPU fallback | Native ARM64, baseline ISA, verified artifacts and ordinary memory/context admission; otherwise unsupported execution |

A family rule covers only independently qualified SKU/capability/driver classes.
Marketing names cannot establish ISA, GPU features, NPU architecture or bandwidth.
The Extreme X2E-94-100/X2E-96-100 specifications reach 228 GB/s system bandwidth and
80 INT8 TOPS; these are platform specifications, not Swift throughput or exclusive
device bandwidth. Other SKUs differ. See the [Qualcomm product brief][x2-specs].

[64-bit DMA support][dma64] permits v81+ buffers beyond the older 32-bit mapping
window, subject to the actual Windows runtime/driver and DMA path. X1 mapping/
tiling constraints remain independent. Neither address window is a physical RAM
budget. [Q1/Q2 Hadamard support][q2-hadamard] is a source lead; it does not establish
pinned PQ2/PTQ1 activation arithmetic, grouped recurrent transforms, approved
adapter or signed distribution. A catalog for older binaries cannot sign new code.

[H0 capability contracts](hardware-capabilities.md) provide versioned Doctor host
facts, model-free runtime reporting, checked profile/evidence keys and explicit
unknown/fallback vocabulary. Scoped CPU observation digests and verified `0.1.3`
prebuilt reporting identify limited observations, not accepted profiles. GPU/NPU
API/firmware/readiness and broader optional ISA/topology remain outside those CPU
records. Failed/unsupported probes are explicit unknowns. Live memory/power is a
request-admission input, not an immutable compatibility identity.

## Eligibility, ranking and admission

Automatic policy is a bounded lookup/ranking operation over versioned qualified
profiles. It does not benchmark or tune at ordinary startup. Existing explicit
CPU controls remain reproducible. Runner owns discovery, policy selection,
artifact verification and admission; [Prism Inference][prism] owns kernels, graph
execution, device/state ownership and effective-placement reporting. The shared
versioned handshake avoids duplicated kernel eligibility rules in the broker.

A profile's concrete identity includes exact model/derived artifact and adapter,
runtime/build/backend, static hardware/driver compatibility, numerical/state and
settings contract, shapes/context/output reservation, objective and workload/
fresh-or-reused bucket. Required unknown, mismatched or stale facts exclude a
candidate. A family match, discovery report or asserted evidence receipt cannot
authorize execution. Signed backend readiness is an independent requirement.

Interactive ranking concerns complete-task latency within qualified constraints.
Battery ranking concerns measured energy per useful task within acceptable
latency, quality and resource bounds; battery operation alone cannot establish
an energy benefit. Ranking uses retained device evidence, versioned thresholds
and deterministic tie-breaking, including losing/no-go entries. Placement,
phase-specific threads/waits, batches/tiles, sustained power policy and state
layout belong to the complete profile identity. Four threads or all cores are
controls rather than universal optima.

Current RAM/commit/device budgets and backend readiness are rechecked immediately
before admission. A preferred unavailable accelerator can yield an eligible
qualified CPU fallback; an explicit strict-backend diagnostic requirement fails.
The model, sampler, adapter, precision, context and complete output reservation
cannot silently change to make a profile fit. Discovery does not activate an
unqualified implementation.

Requested/effective profile, device class, phase placement, build/evidence identity
and a value-free fallback reason are technical outputs. Prompts and tensor payloads
stay out of diagnostics. Presentation remains a simple automatic choice in the
[pure-Rust native UI](ui-architecture.md); tuning is trusted configuration, not
public arbitrary arguments, paths or inherited environment overrides.

## Prefill, decode and complete state

Batched prefill can reuse weight tiles across prompt tokens and can suit HMX;
sequential block scheduling alone does not provide that reuse. Tiled projections
and chunkwise GDN retain their separate [algorithm contracts](inference-algorithms.md).
Decode is packed matrix-vector work with little reuse. HVX, eligible CPU and Adreno
costs include bandwidth, unpacking, transforms, dispatch and submission; INT8 TOPS
cannot forecast token rate.

Packed PQ2/PTQ1 shared storage and bounded local tiles avoid an implicit persistent
INT8/FP16 expansion contract. Repacked/expanded weights, duplicate CPU/GPU/NPU
copies and scratch have separate memory ledgers. Contiguous regions, resident
attention/convolution/GDN state, reusable submissions and buffers are beneficial
only under correct ownership. CPU/GPU/NPU share bandwidth and power; simultaneous
execution remains a measured candidate rather than an objective. Complete-task
cost includes load/repack/compile, synchronization, migration and cache misses.

An execution plan is stable within a request. Power-source/policy, memory,
runtime and driver changes permit reevaluation only at safe request boundaries.
Backend or prefill/decode transitions require qualified complete KV/recurrent/
convolution/checkpoint migration with declared layout/numerical identity and cost,
or verified teardown/rebuild. Otherwise the existing eligible plan or a bounded
recovery result applies. Partially executed work cannot continue on another backend.
A changed driver invalidates affected accelerator evidence/caches without
invalidating an independently compatible CPU control.

Every route preserves one active conversation, authentication, exact token counts,
deadlines, cancellation, memory admission and the daily isolation boundary.
Native Rust acceleration also retains the safe audited device boundary and
replacement contract in the [runtime reference](native-rust-engine.md).

## Qualification scope

Per-device acceptance binds actual SKU/capabilities, OS/driver/backend,
artifacts/profile, settled main revision/run, retained raw captures and uncertainty.
Cold/warm/reused/edited state, short/long prefill, fixed/natural outputs, AC/battery,
memory pressure and sustained use have explicit workload scope. Receipts distinguish
first useful token, complete task, decode, peak memory, energy when observable,
responsiveness and thermal/comfort evidence. One device/family's result cannot
qualify another. Missing valid power telemetry means no energy verdict or
energy-based ranking.

Selector correctness includes unknown/misidentified devices, missing optional ISA
or signed backend, stale runtime/driver/evidence/cache, insufficient budgets,
workload bucket boundaries, deterministic ranking, objective changes, qualified
CPU fallback, strict rejection and complete-state handoff/failure/cancellation.
Negative optional-ISA cases are simulated; actual optional/accelerator execution
requires matching eligible hardware.

Promotion includes correctness/security, creative quality, resource/headroom and
sustained comfort, [standard LoRA TPS/quality requirements](swift-lora.md), useful
paired device benefit, reproducible installation/rollback and affected-upgrade
requalification. A qualified family can coexist with explicit unsupported classes
or a qualified CPU fallback elsewhere. Local builds and focused untimed checks
follow the [validation policy](validation.md); full suites, all benchmarks and
performance captures remain reviewed main-only CI, with target devices providing
their own acceptance evidence. Documentation does not enable profiles or alter pins.

[x2-specs]: https://www.qualcomm.com/content/dam/qcomm-martech/dm-assets/documents/Snapdragon-X2-Elite-Product-Brief.pdf
[dma64]: https://github.com/ggml-org/llama.cpp/pull/29197
[q2-hadamard]: https://github.com/ggml-org/llama.cpp/pull/30627
[prism]: https://github.com/symbiogenesis/prism-inference
