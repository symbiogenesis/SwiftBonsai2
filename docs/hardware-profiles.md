# Automatic Snapdragon X1 and X2 configuration plan

The ultimate hardware goal is one Windows ARM64 runner that detects Snapdragon
X1 and X2 systems and automatically selects the best qualified configuration
for the actual device and task. Optimize time to a useful answer for interactive
use, and energy per useful task within acceptable latency for battery use.
These objectives can select different profiles on the same processor.

**This is planned behavior.** Current released launch policies are CPU-only;
there is no hardware profile selector or qualified X1/X2 accelerator default.
The recorded development machine remains the 32 GB X1E-78-100. Preserve the
[CPU MVP delivery boundary](prism-upstream-todo.md) while preparing this later
work. Family detection, source support and advertised TOPS do not qualify a path.

## Device families and starting hypotheses

| Target | Starting candidates | Evidence required before automatic selection |
| --- | --- | --- |
| Snapdragon X1, initially X1E-78-100 / Adreno X1-85 / Hexagon v73 | Packed scalar/NEON CPU control, Vulkan, then viable packed Hexagon regions | Actual Windows ISA/driver capabilities, signed NPU access, mapping costs, full Swift graph/state correctness and paired device results |
| Snapdragon X2, initially Elite Extreme / Adreno X2-90 / Hexagon v81 | Packed CPU and Vulkan controls; prioritize NPU batched prefill and packed decode experiments alongside them | Actual SKU/capabilities, working extended DMA, matching signed kernels, graph/state correctness and paired device results |
| Unknown SKU, unavailable accelerator or invalidated evidence | Compatible reviewed CPU fallback within its memory/context limits | Native ARM64 execution, safe baseline ISA, verified artifacts and ordinary admission; otherwise report unsupported execution |

Qualify individual SKUs and capability/driver classes before grouping them into
a family rule. Do not infer CPU extensions, GPU features, NPU architecture or
usable bandwidth from a marketing name. The Extreme X2E-94-100 and X2E-96-100
are specified at up to 228 GB/s system memory bandwidth and 80 INT8 TOPS;
these are platform specifications, not Swift throughput or device-exclusive
bandwidth. Other X2 SKUs have different specifications. [Qualcomm product brief][x2-specs]

X2 is a leading NPU candidate because upstream [64-bit DMA support][dma64]
allows Hexagon v81+ to access buffers beyond the old 32-bit mapping window.
Verify the Windows runtime/driver and effective DMA path on the selected device;
retain X1's mapping/tiling treatment where required. Neither the old window nor
extended DMA describes total available model RAM.

Upstream [Q1/Q2 Hadamard support][q2-hadamard], merged October 18, 2026, is a
useful implementation lead. It does not establish support for the pinned Swift
PQ2/PTQ1 artifact, its activation arithmetic, grouped recurrent transforms or
approved adapter. Pin and audit reusable code and its signed deployment package
before implementation. A catalog covering an older binary does not sign new code.

## Selection contract

Implement selection as a bounded policy over versioned, qualified profiles.
Keep the existing explicit CPU controls reproducible. Automatic mode must:

1. Discover native architecture, CPU/OS ISA support and topology, actual SoC/SKU,
   GPU/NPU identities and features, observable driver/runtime/firmware versions,
   signed backend availability, installed/available RAM, commit and device budgets.
   Extend Doctor inventory and use the same checked capability representation in
   launch selection. Unsupported or failed probes are explicit unknowns.
2. Filter profiles by immutable model/derived-artifact and adapter identity,
   numerical contract, runtime/build/backend compatibility, hardware capabilities,
   supported shapes/context and current memory/resource admission. A family match
   alone is insufficient. Discovery never enables an unqualified implementation.
3. Rank only eligible profiles using retained device evidence for the selected
   interactive or battery objective, prompt/context/output size buckets, and
   fresh/reused state where qualified. Candidate settings include CPU thread and
   wait policies, batch/tile sizes, prefill/decode placement, sustained power mode
   and resident state/layout. Use deterministic tie-breaking and retain no-go
   results. Do not run an open-ended tuner or benchmark at ordinary startup.
4. Recheck current headroom and backend readiness before admitting generation.
   Select a qualified CPU fallback when preferred acceleration is unavailable;
   explicit strict-backend diagnostic requests fail if their requirement is unmet.
   Do not alter the model, sampler, adapter, precision, context or output reservation
   silently to make a profile fit.
5. Report requested/effective profile, device class, phase placement, profile
   evidence/build identity and a value-free fallback reason. Keep prompts and
   tensor payloads out of technical logs. Expose a simple automatic choice in the
   eventual UI; hardware tuning remains trusted configuration, not public arbitrary
   backend arguments, paths or inherited environment overrides.

Runner owns discovery, policy selection, artifact verification and admission.
[Prism Inference](https://github.com/symbiogenesis/prism-inference) owns kernels,
graph execution, device/state ownership and effective-placement reporting.
Define the versioned capability/profile handshake before connecting these parts;
do not duplicate kernel eligibility rules in the broker. Native Rust acceleration
must retain the safe audited device boundary and replacement requirements in the
[runtime plan](native-rust-engine.md).

## Prefill, decode and state placement

Prefill can reuse weight tiles across prompt tokens, making batched matrix
multiplication a strong HMX candidate. Existing sequential block scheduling
does not by itself provide that reuse. Qualify tiled projections and, where
needed, chunkwise GDN separately under the [algorithm contracts](inference-algorithms.md).

Single-conversation decode is a packed matrix-vector workload with little weight
reuse. Compare HVX packed dots, NEON/optional eligible CPU kernels and Adreno
using their actual bandwidth, unpacking, transforms and submission costs. INT8
TOPS cannot be converted into a Swift token-rate forecast. Retain PQ2/PTQ1 packed
weights in shared memory; expand bounded local tiles only where required.
Persistent INT8/FP16 expansion, repacking and duplicate CPU/GPU/NPU copies need
their own memory ledger and numerical qualification.

Prefer contiguous graph regions and reusable submissions/buffers. Keep
attention, convolution and GDN state near the surrounding computation when
correct and beneficial. CPU/GPU/NPU share memory bandwidth and power limits;
using all three simultaneously is only another measured candidate. Include
load/repack/compile, synchronization, state migration and cache misses in the
complete request cost.

Select a stable execution plan before each request. Reevaluate eligibility after
power-source/policy, memory, runtime or driver changes at safe request boundaries.
Existing conversations/checkpoints may change backend only through a qualified
complete-state migration or verified teardown/rebuild; otherwise retain a still
eligible plan or return a bounded recovery result. Never continue partially
executed work on another backend. A new driver invalidates affected accelerator
evidence/caches without discarding an independently compatible CPU control.

Keep a declared state-layout/numerical identity for KV, recurrent/convolution
state and checkpoints. Different prefill/decode placements require explicit
migration correctness and cost evidence, even when weights are identical.
Preserve one active conversation, deadlines, cancellation, memory admission,
authentication and daily isolation across every selected route.

## Delivery sequence

All H0-H5 exits are pending. These are project hardware milestones, separate
from the CPU MVP checklist and backend-specific S/G/N stages.

| Stage | Deliverable and exit |
| --- | --- |
| H0 - Inventory and profile contract | Shared capability schema, immutable compatibility/evidence keys, interactive/battery objectives and explicit unknown/fallback behavior; focused discovery fixtures |
| H1 - CPU controls on X1 and X2 | Safe scalar/NEON fallback on both, eligible optional ISA checks, independent thread/batch/power evidence and bounded memory; no inherited four-thread or all-core optimum |
| H2 - Accelerator candidates | Per-device exits from the SIMD, Vulkan and Hexagon plans; prioritize packed X2 NPU investigation alongside CPU/GPU controls, with signed deployment and complete graph/state checks |
| H3 - Measured profile catalog | Paired workload results for each admitted device/driver class, separate prefill/decode and migration costs, quality/resource verdicts, losing entries and versioned selection thresholds |
| H4 - Automatic selection | Deterministic eligibility/ranking, readiness recheck, effective-plan reporting, CPU fallback and safe request-boundary reevaluation; focused policy and recovery regressions pass |
| H5 - Promotion and upkeep | Qualified automatic interactive/battery behavior independently demonstrated on X1 and X2, reproducible install/rollback, and compatibility/cache invalidation and requalification on affected upgrades |

H0 design and bounded untimed inventory/access probes can proceed during CPU MVP
delivery. H1-H5 implementation and promotion follow the reviewed CPU runtime
handoff. Vulkan and Hexagon investigation can proceed independently.
One family's accepted profile may ship while the other retains a qualified CPU
fallback; keep the remaining family/accelerator exits open and explicit.

## Qualification and completion criteria

Use the [project promotion criteria](../PLAN.md#promotion-criteria), standard
[LoRA gates](swift-lora.md#required-standard-profile-qualification) and existing
numerical/security/resource requirements for every promoted profile. The ideal
configuration is the best measured eligible choice for a declared objective,
not a promise of a universal mathematical optimum or a fixed family/backend map.

Retain exact SKU/capabilities, OS/driver/backend versions, artifacts, profile and
tested main revision, CI run, raw captures and uncertainty. Measure short/long
prefill, fixed/natural outputs, warm/cold/reused/edited contexts where supported,
AC/battery, memory pressure and sustained creative tasks. Record first useful
token, completed-task latency, decode, peak memory, energy when available,
responsiveness and thermals. Results from X1, hosted ARM64 or a different X2 SKU
do not establish a verdict for an untested class.
Unavailable power telemetry means no energy verdict; battery operation alone
does not qualify an energy-saving profile or energy-based ranking.

Focused selection checks must cover unknown/misidentified devices, absent optional
ISA, incompatible or missing signed accelerators, stale driver/evidence/cache
keys, insufficient RAM, workload bucket boundaries, deterministic ranking,
objective changes, valid CPU fallback, strict-backend rejection, and complete-state
handoff/failure/cancellation. Execute optional ISA variants only on eligible
hardware; simulate negative capability cases.

Follow [validation policy](validation.md): local builds and focused untimed
correctness checks are allowed; full suites, all benchmarks and performance
captures run only in reviewed main-only CI. Target-device evidence requires
separate X1 and X2 hardware lanes; no such automatic-selection qualification is
currently established. Documentation changes do not enable any profile or update
runtime/model locks.

[x2-specs]: https://www.qualcomm.com/content/dam/qcomm-martech/dm-assets/documents/Snapdragon-X2-Elite-Product-Brief.pdf
[dma64]: https://github.com/ggml-org/llama.cpp/pull/29197
[q2-hadamard]: https://github.com/ggml-org/llama.cpp/pull/30627
