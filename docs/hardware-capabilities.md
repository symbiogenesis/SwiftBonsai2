# H0 discovery and compatibility contracts

The first H0 implementation supplies host inventory, model-free runtime
inventory and checked profile/evidence structures. It does not select profiles.
Released runtime provisioning uses its immutable `0.1.5` lock. The optional
verified capability probe consumes that release without loading a model.

## Host inventory

`./scripts/Doctor.ps1 -PassThru` retains its version-one outer report and adds
`Capabilities`, with `schema_version: 1` and `kind: runner-host-capabilities`.
The pure `Get-BonsaiHostCapability` projection in
`scripts/Bonsai.Capabilities.psm1` is also used by focused fixture checks.

| Section | Observed scope and remaining unknowns |
| --- | --- |
| `operating_system`, `process` | Native OS and current process architectures; OS version/build where available; native versus emulated process |
| `cpu.identities` | CIM processor names, manufacturers and core/logical counts; processor groups and efficiency classes remain unknown |
| `cpu.isa` | .NET AdvSimd and dot-product support for the current process architecture only |
| `memory` | CIM-reported total, OS-usable/currently available physical bytes and commit limit/current committed bytes where available |
| `gpu` | CIM device/vendor/driver inventory; API features and device budgets remain unknown |
| `npu` | ComputeAccelerator-class signed-driver inventory; architecture, firmware and backend readiness remain unknown |

Each section identifies its source and `observed`, `partial` or `unknown`
status. Unknown values are JSON null. A successful empty device list differs
from a failed inventory query. CIM requests use a ten-second operation timeout;
missing classes or denied access leave explicit unknowns. The descriptor does
not contain hostnames, serial numbers, user identities or network inventory.
The outer Doctor tool inventory still contains local paths; review it before
sharing.

An optional `Doctor.ps1 -ToolPath <existing-bonsai-tools.exe>` adds
`NativeCpuObservations` from the Rust `host-cpu` command. Its separate v1
`runner-native-cpu-observations` report retains group-relative CPU-set topology,
raw efficiency/scheduling classes and current-process allocation/parking flags.
Static topology and live availability have separate fields. Query allocation,
record count and retries are bounded; malformed or unavailable observations
produce explicit unknowns rather than a core-count guess. Doctor never builds
a tool for this option. Without it, the property is null and the existing host
projection and CPU compatibility digest retain their original scope.
The separate `power_snapshot` uses `GetSystemPowerStatus` for AC/battery,
battery-saver and charge observations, preserving unknown AC/charge sentinels.
These live conditions do not enter a compatibility digest or measure energy.
The separate `power_policy_snapshot` reports the active scheme GUID and effective
power mode using [PowerGetActiveScheme](https://learn.microsoft.com/en-us/windows/win32/api/powersetting/nf-powersetting-powergetactivescheme)
and the initial notification from
[PowerRegisterForEffectivePowerModeNotifications](https://learn.microsoft.com/en-us/windows/win32/api/powersetting/nf-powersetting-powerregisterforeffectivepowermodenotifications)
version two. Registration uses
a bounded one-second initial wait and is removed before a successful return.
Missing/unknown policy observations remain explicit unknowns and suppress X2
screen comparisons. Equal snapshots neither capture edits within a scheme nor
exclude intervening changes. No Windows power setting is changed.

The additive `isa` section queries `IsProcessorFeaturePresent` from a native
ARM64 process for NEON/AdvSimd (19), dot product (43) and **NEON I8MM (66)**.
The separate SVE I8MM flag (57) is not a substitute. Non-ARM64 processes retain
unknown/null ISA fields even if Windows reports ARM hardware underneath them.
These observations remain independent of CPU-set query success and of Doctor's
existing .NET-based compatibility digest. `dispatch_qualified` remains false.
Under [Microsoft's API contract](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-isprocessorfeaturepresent),
a false result means unsupported **or undetected by the HAL**, not proof of
absent silicon support. Windows 11 24H2 / Server 2025 added I8MM detection;
older systems cannot provide positive eligibility through that flag. The
pinned Windows bindings lack the I8MM constant, so the native boundary retains
its documented SDK 26100 value explicitly. No optional instruction is executed
by discovery and no kernel dispatch or runtime selection changes.

CPU-set IDs do not establish persistent affinity, process affinity eligibility,
optional ISA or Snapdragon core names. The [X2 CPU screen](x2-cpu-optimization.md)
retains this native inventory alongside its processor observation and fixed plan.

With the same explicit `-ToolPath`, Doctor also adds `NativeGpuObservations`
from `bonsai-tools host-gpu`. Its separate v1
`runner-native-gpu-observations` report queries the absolute Windows system
`vulkan-1.dll` from a native ARM64 child. The child retains the ordinary headless
worker mitigations, a 512 MiB one-process Job, bounded output and a ten-second
query deadline; failure leaves `status: unknown` and `adapters: null`. Successful
enumeration of zero devices returns an observed empty list. Vulkan environment
overrides (`VK_*`) reject; custom loaders, layers and ICD paths cannot be requested.
The installed graphics stack is still a trust boundary, and a successful query
does not verify its DLL signatures or dependency provenance.

The probe creates only a Vulkan instance, with no requested layers/extensions,
logical device, queue submissions, model, surface or device-memory allocation.
Its fixed storage caps are 16 adapters, 64 queue families per adapter and 1,024
device extensions; incomplete/oversized enumeration rejects without unbounded
allocation or retry. It retains driver-reported names, raw vendor/device/driver
IDs, packed API versions, compute queue counts, extension names and memory-heap
capacities. Raw driver versions have vendor-specific encoding and are not
decoded as Vulkan API versions. CPU and virtual adapters are explicitly marked
`hardware_gpu: false`; discovery never enables them as inference fallbacks.

When Vulkan 1.1 queries and the corresponding core version or advertised
extensions allow it, the report additionally observes subgroup width/operation
and supported-stage flags, shader FP16 and integer-dot features. The declared
application API is the loader version capped at 1.3 and is retained separately;
core feature eligibility uses the lower application/device API version.
Unknown or unqueried observations stay null.
Extension presence alone does not qualify cooperative-matrix arithmetic,
Swift packed operations or an inference backend. Cooperative-matrix execution,
live memory budgets, exact SoC SKU, signed-driver readiness and full dependency
identities remain unqualified/unknown. Heap capacities are not available-memory
budgets; shared host/device memory must not be counted twice for admission.
`backend_ready` remains false, and the existing CPU compatibility digest and
launch policy retain their current scope. No performance or energy is measured.

The native query follows the [Khronos physical-device contract](https://docs.vulkan.org/refpages/latest/refpages/source/VkPhysicalDeviceProperties.html)
and [Vulkan properties2 contract](https://docs.vulkan.org/refpages/latest/refpages/source/VkPhysicalDeviceProperties2.html),
using the already locked [ash loader bindings](https://docs.rs/ash/0.38.0+1.3.281/ash/struct.Entry.html).
Core-version gating follows [the application API contract](https://docs.vulkan.org/refpages/latest/refpages/source/VkApplicationInfo.html).

A model-free native ARM64 probe on this X2 host on 2026-10-29 returned the
explicit unknown report: the system loader loaded, but instance creation failed
with `VK_ERROR_INCOMPATIBLE_DRIVER` (`-9`) under the ordinary headless child
policy. The bounded child stderr remains in the external
`SwiftBonsai-GpuInventory-15116-1793325174261883700` temp evidence directory.
This result does not identify the cause or establish absent hardware support.
Do not enable a Vulkan profile from this result; keep the CPU fallback and
investigate driver/policy compatibility separately without weakening inference
worker mitigations. No model was loaded and no shader or benchmark was run.

A separately labelled ten-second diagnostic under the existing ordinary
`Invoke-BonsaiNativeProcess` tool policy succeeded on that date. It observed
loader API 1.3.301, declared application API 1.3.0 and device API 1.4.295;
the driver reported `Qualcomm(R) Adreno(TM) X2-90 GPU`, vendor ID `20803`,
device ID `909329968` and raw driver version `2151018496`. FP16 and integer-dot
shader feature queries returned true, and subgroup width was 64. This separate
tool-policy result is retained in the external task lane's
`gpu-tool-policy.json` and `gpu-tool-policy.log`. It does not prove which
headless mitigation causes the difference, qualify driver provenance or establish
model-worker access. The shipped inventory command keeps its ordinary headless
policy and returns unknown; it has no tool-policy retry. Both probes created
only a Vulkan instance, with no logical device or shader execution.

The same explicit `-ToolPath` now adds `NativeOpenClObservations` from the
bounded `host-opencl` command. It observes only GPU information through the
absolute system OpenCL loader under unchanged headless worker policy, with
no context, queue, program, kernel or device allocation. Its separate report
retains native architecture, platform/device/driver/compiler capabilities and a
bounded loader file size/hash observation, while leaving provenance unverified
and `backend_ready: false`. CPU compatibility digests and launch policy do not
consume it. See the [bounded OpenCL feasibility screen](opencl-feasibility.md)
for exact bounds, immutable source identities and the unchanged pinned backend's
missing Swift PQ2/PTQ1 packed-projection support. The separate bounded kernel
implementation pilot remains in PLAN. This does not resolve Vulkan worker access.

Names such as X1/X2 and a signed driver do not establish optional ISA, GPU/NPU
execution or deployment trust. An emulated process's ISA result cannot qualify
a native runtime. Current memory is a diagnostic snapshot, never a substitute
for admission immediately before a request.
`memory.reported_total_physical_bytes` is CIM's `TotalPhysicalMemory`; it can
exclude hardware-reserved RAM and does not establish installed capacity.

## Runtime inventory

Prism's standalone `prism-inference --capabilities` emits one JSON record with
`schema_version: 1` and `kind: prism-runtime-capabilities`. It reports the runtime
package/version/compiler target, scalar support, the production NEON build
eligibility predicate, exact supported graph identity suffixes and unavailable
Vulkan/Hexagon backends. Its `qualification` is always `unqualified`.

Use `./scripts/Get-Inference.ps1 -DataDirectory <absolute-path> -IncludeCapabilities`
to provision the pinned release and run the bounded model-free probe. Add
`-VerifyOnly` to require an existing installation. The result adds `Capabilities`
and `CapabilityVerification`, with retained runtime file/archive identities and
external diagnostic evidence. Verification guards remain held through process
exit. The probe rejects a custom lock and checks the reported version and native
target against the compiled release pin; it never establishes device qualification.

The frozen wire example is identical in Prism's
`tests/fixtures/runtime-capabilities-v1.json` and Runner's
`src/cpu-policy/tests/fixtures/runtime-capabilities-v1.json`. Deliberate wire
changes must update both copies and their checks. The example's target and
version are sample build facts, not a provisioning lock or device result.

Runner's `bonsai_cpu_policy::hardware::RuntimeCapabilities::from_json` accepts
at most 16 KiB and checks exact fields/types/schema, bounded unique graph IDs
and consistent target/build claims. It never reconstructs kernel eligibility
rules. Unknown versions, fields, accelerator claims and asserted qualification
reject. Reporting a graph suffix does not establish the matching model hash,
portable state compatibility or a successful numerical/device qualification.

## Profile and evidence keys

`ProfileCompatibilityKey::from_json` defines a concrete future evidence key.
Every identity must be present; the reserved `unknown` sentinel (in any case)
cannot become a concrete target, graph or state identity. It binds:

- `objective`: `interactive` or `battery`.
- `model_sha256`: the exact complete weight artifact, including derived exports.
- `adapter`: exactly `{"kind":"none"}` or
  `{"kind":"artifact","sha256":"<lowercase SHA-256>"}`.
- `runtime`: `archive_sha256`, full `source_revision` and exact compiler
  `target` (`architecture`, `os`, `environment`).
- `capability_class_sha256` and `backend_driver_sha256`: independently reviewed
  static capability and backend/driver compatibility records.
- `graph_contract`, `state_layout` and `settings_sha256`: exact numerical/state
  identity and the complete profile settings, including phase placement,
  threads, batching, precision and migration treatment.
- `workload`: `context_tokens`, `minimum_prompt_tokens`, `maximum_prompt_tokens`,
  `maximum_output_tokens` and `state` (`fresh` or `reused`). The maximum prompt
  plus the entire output reservation must fit the declared context.

SHA-256 fields use exactly 64 lowercase hexadecimal digits; Git revisions use
40. Capability-class records must exclude timestamps, tool paths and changing
free-memory/power values. They must bind actual observed capabilities rather
than a marketing family name.

Doctor's `CpuCompatibilityIdentity` now retains canonical UTF-8 JSON strings
and their lowercase SHA-256 digests. The exported
`Get-BonsaiCpuCompatibilityIdentity -Capabilities $report.Capabilities` helper
produces the same result without new probes. Its explicit scope is
`native-windows-arm64-cpu-observations-v1`: the capability-class record binds
native architecture, ordinally sorted CPU names/vendors/core counts and observed
AdvSimd/dot-product flags; the CPU backend record binds OS version/build. Repeated
processor entries retain their multiplicity. Changing live memory, timestamps,
tool paths or accelerator inventory does not change these CPU digests.
Missing, emulated or contradictory observations return `unknown-capability`
and null records/digests. The result remains `unqualified`.

These records identify retained observations for review. They do not establish
complete topology, unobserved optional ISA, GPU/NPU features or accelerator
driver compatibility. Runtime archive/source/target remain separate profile
fields. This scope excludes accelerator identities, catalog admission and
selection; their tasks live in the [central TODO](../PLAN.md#hardware-selection).
Hashing the complete Doctor report is not a compatibility policy.

`ProfileEvidence::from_json` binds the complete compatibility object to a
`tested_main_revision`, decimal-string `ci_run_id`, retained `capture_sha256`,
`verdict` and `energy_verdict`. Verdicts are `unknown`, `passed`, `failed` or
`no-go`. A battery pass requires an energy pass. `applies_to` performs exact
identity comparison, including objective and workload; it does not trust a
receipt's asserted verdict or authorize execution. Source ancestry, build
provenance, run/capture existence and numerical/quality/resource/security
qualification still require their existing reviews.

`FallbackReason` reserves value-free codes: `unknown-capability`,
`unavailable-backend`, `incompatible-runtime`, `incompatible-driver`,
`stale-evidence`, `insufficient-memory` and `strict-backend-unavailable`.
No selector currently emits these codes. The
[hardware profile contract](hardware-profiles.md) defines their policy boundary;
implementation tasks live in the [central TODO](../PLAN.md#hardware-selection).

## Focused checks

From an external temp working directory, use absolute checkout/build paths:

```text
pwsh -NoProfile -File <repo>/tests/Test-Foundation.ps1 -Case Capabilities
pwsh -NoProfile -File <repo>/tests/Test-Foundation.ps1 -Case Doctor
cargo test --manifest-path <repo>/Cargo.toml --target-dir <temp>/target --locked --offline --release -p bonsai-cpu-policy hardware::tests
```

These checks cover missing/malformed probes, emulation scope, failed versus
empty device inventories, memory units/overflow, strict runtime wire parsing,
identity invalidation and output reservation. Full suites and all benchmarks
retain their existing CI-only location.
