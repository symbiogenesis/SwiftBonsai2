# Hardware discovery and profile policy

Runner uses CPU profiles. Implemented discovery supplies host/runtime inventory and checked
compatibility/evidence records; **automatic selection is not implemented**. The optional verified,
model-free runtime probe uses the immutable [release lock](../config/prism-inference.lock.json). The [selection contract](#automatic-profile-selection)
below defines future eligibility; [PLAN.md](../PLAN.md#hardware-selection) owns pending work.

## Host inventory

`./scripts/Doctor.ps1 -PassThru` preserves its v1 outer report and adds `Capabilities`
(`schema_version: 1`, `kind: runner-host-capabilities`). The pure `Get-BonsaiHostCapability`
projection in `scripts/Bonsai.Capabilities.psm1` also serves fixture checks.

| Section | Observations | Unknowns |
| --- | --- | --- |
| `operating_system`, `process` | Native OS/current-process architectures, available OS version/build, emulation | Unavailable facts |
| `cpu.identities` | CIM names/vendors/core/logical counts | Groups/efficiency classes |
| `cpu.isa` | .NET AdvSimd/dot product for current process architecture | Other ISA/native eligibility from emulation |
| `memory` | CIM total, OS-usable/available physical bytes, available commit limit/current usage | Installed capacity from CIM alone |
| `gpu` | CIM device/vendor/driver | API features/device budgets |
| `npu` | ComputeAccelerator signed drivers | Architecture/firmware/backend readiness |

Each section retains source and `observed`/`partial`/`unknown` status; unknown values are null.
Successful empty lists differ from failed queries. CIM operations time out at ten seconds;
missing/denied classes stay unknown. Capabilities exclude hostnames, serials, user/network
inventory, but outer Doctor tool paths require review before sharing. CIM
`memory.reported_total_physical_bytes` can omit hardware-reserved RAM. Live memory never replaces
request admission. Family names/driver signatures do not prove ISA, execution or deployment trust;
emulated-process ISA cannot qualify a native runtime.

### Native CPU observations

`Doctor.ps1 -ToolPath <existing-bonsai-tools.exe>` adds Rust `host-cpu`'s v1
`runner-native-cpu-observations`. Doctor never builds the tool; without it, the property is null.
Existing host projection and CPU digest retain their original scope.

| Observation | Contract |
| --- | --- |
| CPU sets | Group-relative topology, raw efficiency/scheduling classes and current-process allocation/parking; static topology/live availability remain separate |
| Bounds/failure | Bounded allocation, records and retries; malformed/unavailable results stay unknown, without guessed core counts |
| `power_snapshot` | `GetSystemPowerStatus` AC/battery, battery saver and charge with unknown sentinels |
| `power_policy_snapshot` | [Active scheme GUID][scheme], [effective mode][power-mode] via version-two notification, one-second initial wait, registration removed before successful return; unknowns suppress X2 comparisons |
| `isa` | Native ARM64 [Windows feature query][windows-features]: NEON/AdvSimd 19, dot product 43, **NEON I8MM 66**; SVE I8MM 57 is distinct |
| `host_identity` | Checked speech-build host query: fixed hardware-registry CPU name/architecture/family/model/revision/vendor, `RtlGetVersion` kernel and sorted available feature IDs in `0..256` |

Windows CPU sets use [group-relative indices/classes][cpu-sets]; IDs/classes establish neither
persistent/process-affinity eligibility, optional ISA nor core names. Power snapshots observe
settings, not energy; equal values cannot exclude intervening or in-scheme edits.
ISA observations are independent of CPU-set success/.NET digest; non-ARM64 processes report null
ISA even on ARM hardware. `dispatch_qualified` stays false. False Windows flags mean unsupported
**or HAL-undetected**. I8MM detection arrived in Windows 11 24H2 / Server 2025; the pinned bindings
lack its constant, so the native boundary explicitly retains SDK 26100 value 66. Discovery executes
no optional instruction and changes no dispatch/selection.

Malformed, missing or oversized registry values make the whole host identity unknown. Registry
name is an independent observation, not verified silicon SKU/firmware. Identity is separate from
live availability/power. The [X2 screen](simd-plan.md#x2-cpu-thread-screen) requires CIM/registry-name agreement
and retains exact identity in plans/receipts. Unknown/changed identity, including legacy receipts
lacking it, suppresses timing ratios. Kernel/feature changes require fresh captures; compare the
whole bounded feature list, not descriptive `isa` flags. No new digest/backend permission follows.

### Native Vulkan observations

The same `-ToolPath` adds `NativeGpuObservations` from `host-gpu`. A native ARM64 child queries
absolute system `vulkan-1.dll` with ordinary headless mitigations, 512 MiB one-process Job,
bounded output and ten-second deadline. Failure returns `status: unknown`, `adapters: null`;
successful zero-device enumeration is observed. `VK_*` overrides reject; custom loaders/layers/ICD
paths are unsupported. Installed graphics code remains a trust boundary: no signature/dependency
provenance verification is claimed.

Only an instance is created: no requested layers/extensions, logical device, submission, model,
surface or device allocation. Caps: 16 adapters, 64 queue families each, 1,024 device extensions;
incomplete/oversized enumeration rejects without unbounded allocation/retries. Reports retain
names/raw vendor/device/driver IDs, packed API versions, compute queues, extensions and heaps.
Driver encoding is vendor-specific, not an API version. CPU/virtual adapters get
`hardware_gpu: false`, excluding inference fallback.

Eligible Vulkan 1.1/core/extension queries also retain subgroup width/operations/stages, shader
FP16 and integer-dot features. Application API is loader API capped at 1.3; core eligibility uses
the lower application/device version. Unqueried fields stay null. Extension presence cannot
qualify cooperative-matrix arithmetic or Swift operations. Cooperative-matrix execution, live
budgets, exact SoC SKU, signed-driver readiness and complete dependencies remain unknown/unqualified.
Heap capacity is not free memory; shared physical storage counts once. `backend_ready` stays false;
CPU digest/launch policy are unchanged. No performance/energy is measured. Queries follow
[Khronos properties][properties], [properties2][properties2], [application API][application-api]
and locked [ash 0.38.0][ash].

Retained **2026-10-29 X2** observations remain distinct:

| Policy | Result | Retained evidence and limit |
| --- | --- | --- |
| Shipped headless child | Loader loaded; instance failed `VK_ERROR_INCOMPATIBLE_DRIVER` (`-9`), report unknown | Bounded stderr in external `SwiftBonsai-GpuInventory-15116-1793325174261883700`; cause/hardware support unresolved |
| Separate ten-second `Invoke-BonsaiNativeProcess` diagnostic | Loader 1.3.301, application 1.3.0, device 1.4.295; `Qualcomm(R) Adreno(TM) X2-90 GPU`; vendor `20803`, device `909329968`, driver `2151018496`; FP16/integer-dot true, subgroup 64 | External task-lane `gpu-tool-policy.json` / `.log`; differing mitigation, provenance and worker access unproven |

Both probes were limited to instance-level queries: no model, shader or benchmark ran. The shipped command
has no tool-policy retry; CPU fallback and worker mitigations remain required.

### Native OpenCL observations

The same `-ToolPath` adds `NativeOpenClObservations` from bounded `host-opencl`: GPU information
through the absolute system loader under unchanged headless policy, with no context, queue,
program, kernel or device allocation. It retains architecture/platform/device/driver/compiler
facts and bounded loader size/hash; provenance stays unverified, `backend_ready: false`.
CPU digests/launch policy ignore it. [OpenCL access and source audit](adreno-gpu-plan.md#opencl-access-and-source-audit)
owns exact bounds, identities and missing pinned PQ2/PTQ1 projections. Its pilot remains in PLAN;
OpenCL observations do not resolve Vulkan worker access.

## Runtime inventory

`prism-inference --capabilities` emits one v1 `prism-runtime-capabilities` record: package/version/
compiler target, scalar support, production NEON build predicate, exact graph identity suffixes
and unavailable Vulkan/Hexagon. `qualification` is always `unqualified`.

Provision/probe with
`./scripts/Get-Inference.ps1 -DataDirectory <absolute-path> -IncludeCapabilities`;
add `-VerifyOnly` to require existing files. Results add `Capabilities`/`CapabilityVerification`,
runtime file/archive identities and external diagnostics. Verification guards stay held through
exit. Custom locks reject; reported version/native target must match the compiled release pin.

The frozen example is identical in Prism's `tests/fixtures/runtime-capabilities-v1.json` and
Runner's `src/cpu-policy/tests/fixtures/runtime-capabilities-v1.json`; wire changes update both
copies/checks. Example version/target are sample facts, not pins. In `bonsai_cpu_policy::hardware`,
`RuntimeCapabilities::from_json` accepts at most 16 KiB, checking exact fields/types/schema,
bounded unique graph IDs and consistent target/build claims. Unknown versions/fields, accelerator
claims and asserted qualification reject. The parser does not duplicate eligibility rules;
graph suffixes prove neither artifact hash, portable state nor numerical/device qualification.

## Profile and evidence keys

`ProfileCompatibilityKey::from_json` requires every identity, rejecting case-insensitive reserved
`unknown` as concrete target/graph/state identity:

| Field | Binding |
| --- | --- |
| `objective` | `interactive` or `battery` |
| `model_sha256` | Complete exact weights, including derived export |
| `adapter` | Exactly `{"kind":"none"}` or `{"kind":"artifact","sha256":"<lowercase SHA-256>"}` |
| `runtime` | `archive_sha256`, full `source_revision`, exact `target` architecture/OS/environment |
| `capability_class_sha256`, `backend_driver_sha256` | Independently reviewed static capability/backend-driver records |
| `graph_contract`, `state_layout`, `settings_sha256` | Exact numerical/state identity and all settings: phase placement, threads, batching, precision/migration |
| `workload` | `context_tokens`, `minimum_prompt_tokens`, `maximum_prompt_tokens`, `maximum_output_tokens`, `state` (`fresh`/`reused`); maximum prompt plus full output fits context |

SHA-256 values are 64 lowercase hex digits; Git revisions are 40. Static capability records bind
observed facts, excluding timestamps, paths and live RAM/power; marketing family names are insufficient.

Doctor's `CpuCompatibilityIdentity` retains canonical UTF-8 JSON/lowercase SHA-256.
`Get-BonsaiCpuCompatibilityIdentity -Capabilities $report.Capabilities` reproduces it without probes.
Scope `native-windows-arm64-cpu-observations-v1` binds native architecture, ordinally sorted CPU
names/vendors/core counts and AdvSimd/dot-product flags; CPU backend binds OS version/build.
Duplicate processors retain multiplicity. Paths/timestamps/live RAM/power/accelerators do not
affect CPU digests. Missing/emulated/contradictory facts return `unknown-capability` and null
records/digests; qualification stays `unqualified`. These exclude full topology, unobserved ISA,
accelerator/driver identities, catalog admission and selection. Runtime archive/source/target
remain separate; hashing the whole Doctor report is not compatibility policy.

`ProfileEvidence::from_json` binds the full key to `tested_main_revision`, decimal-string
`ci_run_id`, retained `capture_sha256`, `verdict` and `energy_verdict`: `unknown`, `passed`,
`failed` or `no-go`. Battery pass requires energy pass. `applies_to` compares exact identity,
including objective/workload, without trusting asserted verdicts or authorizing execution.
Source ancestry/build provenance/run-capture existence/numerical/quality/resource/security
acceptance still need review.

Reserved value-free `FallbackReason` codes are `unknown-capability`, `unavailable-backend`,
`incompatible-runtime`, `incompatible-driver`, `stale-evidence`, `insufficient-memory` and
`strict-backend-unavailable`. [Profile policy](hardware-capabilities.md#automatic-profile-selection) defines their future use;
no selector emits them yet.

## Automatic profile selection

This is the selection contract, not enabled behavior. Runner owns discovery, selection,
verification and admission. [Prism Inference][prism] owns kernels, graph/device/state execution
and effective-placement reporting. Their versioned handshake keeps kernel eligibility rules
out of the broker.

### Device classes and observed identity

The recorded development target is an ASUS Zenbook A16, Snapdragon X2 Elite Extreme X2E94100.
The [X2 CPU screen](simd-plan.md#x2-cpu-thread-screen) retains its 2026-10-29 inventory and phase
thread controls; inventory establishes neither kernel eligibility nor performance/backend readiness.

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

### Eligibility, ranking and admission

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

Diagnostics report requested/effective profile, device class, phase placement, build/evidence
identity and a value-free fallback reason, excluding prompts/tensors. The [native Rust
UI](ui-architecture.md) presents a simple automatic choice; tuning stays in trusted configuration,
without arbitrary public arguments/paths or inherited environment overrides.

### Prefill, decode and complete state

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

Every route preserves the [broker contract](broker-policy.md): one active conversation,
authentication, exact counts, deadlines, cancellation, admission and daily isolation. Rust
acceleration also preserves the [runtime's safe device/replacement boundaries](native-rust-engine.md).

### Qualification scope

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
TPS/quality](swift-lora.md#standard-profile-qualification), installation/rollback and affected-upgrade requalification.
Qualified families may coexist with unsupported classes and qualified CPU fallbacks.
[Validation policy](validation.md) requires main-only matching-device CI for full suites,
benchmarks and performance captures; local builds/focused untimed checks are allowed.

## Focused checks

From an external temp working directory, use absolute checkout/build paths:

```text
pwsh -NoProfile -File <repo>/tests/Test-Foundation.ps1 -Case Capabilities
pwsh -NoProfile -File <repo>/tests/Test-Foundation.ps1 -Case Doctor
cargo test --manifest-path <repo>/Cargo.toml --target-dir <temp>/target --locked --offline --release -p bonsai-cpu-policy hardware::tests
```

Coverage: malformed/missing probes, emulation, failed/empty inventories, memory units/overflow,
strict wire parsing, identity invalidation and output reservation. Full suites/benchmarks remain
[CI-only](validation.md).

[cpu-sets]: https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-system_cpu_set_information
[scheme]: https://learn.microsoft.com/en-us/windows/win32/api/powersetting/nf-powersetting-powergetactivescheme
[power-mode]: https://learn.microsoft.com/en-us/windows/win32/api/powersetting/nf-powersetting-powerregisterforeffectivepowermodenotifications
[windows-features]: https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-isprocessorfeaturepresent
[properties]: https://docs.vulkan.org/refpages/latest/refpages/source/VkPhysicalDeviceProperties.html
[properties2]: https://docs.vulkan.org/refpages/latest/refpages/source/VkPhysicalDeviceProperties2.html
[application-api]: https://docs.vulkan.org/refpages/latest/refpages/source/VkApplicationInfo.html
[ash]: https://docs.rs/ash/0.38.0+1.3.281/ash/struct.Entry.html
[x2-specs]: https://www.qualcomm.com/content/dam/qcomm-martech/dm-assets/documents/Snapdragon-X2-Elite-Product-Brief.pdf
[dma64]: https://github.com/ggml-org/llama.cpp/pull/29197
[q2-hadamard]: https://github.com/ggml-org/llama.cpp/pull/30627
[prism]: https://github.com/symbiogenesis/prism-inference
