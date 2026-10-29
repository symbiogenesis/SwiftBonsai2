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

CPU-set IDs do not establish persistent affinity, process affinity eligibility,
optional ISA or Snapdragon core names. The [X2 CPU screen](x2-cpu-optimization.md)
retains this native inventory alongside its processor observation and fixed plan.

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
