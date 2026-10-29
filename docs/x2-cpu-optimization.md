# X2 Elite Extreme CPU thread screen

The current development machine reports `Snapdragon(R) X2 Elite Extreme -
X2E94100 - Qualcomm Oryon(TM) CPU`, 18 physical and logical processors, and
Adreno X2-90 driver `32.0.149.0` (2026-10-29 inventory). These observations
identify the next tuning target. They do not establish optional ISA support,
GPU/NPU readiness or a faster qualified profile.

An additional model-free native probe on 2026-10-29, using the pinned Rust
toolchain's `aarch64-pc-windows-msvc` maintenance executable on Windows
`10.0.26300.0`, reports 18 physical/logical cores in processor group 0,
efficiency and scheduling classes `[0, 1]`, and positive Windows NEON,
dot-product and **NEON I8MM** availability flags. The processor name was
independently read from `HKLM\HARDWARE\DESCRIPTION\System\CentralProcessor\0`.
Model-free plan inspection selected `d6-p6`, `d4-p12` and `d8-p18` with 24
scheduled samples and `ModelProcessStarted: false`. The built CLI accepted an
unchanged native observation and rejected a copy with changed AC state.
These observations identify eligible instruction experiments; separately
dispatched kernels still require code-generation, numerical and performance
qualification. Class values do not identify Snapdragon core names.
See [native ISA semantics](hardware-capabilities.md).

A follow-up model-free probe on the same date observed effective mode `balanced`
and active scheme `381b4222-f694-41f0-9685-ff5bb260df2e`. The combined CLI accepted
identical observed conditions and rejected an injected effective-mode change
while AC remained unchanged. The checked native speech host query also confirmed
X2E94100 and Windows `10.0.26300`; plan inspection still returned 24 samples
with no model process started. These are discovery/correctness observations.

The manual [X2 CPU workflow](../.github/workflows/x2-cpu-evaluation.yml) supplies
a bounded first thread screen on the owner-managed native ARM64 runner, on
settled `main`. It shares the existing model-capture concurrency group. All
performance execution remains CI-only under [validation](validation.md).

## Controls and cost

The Rust maintenance planner selects at most three candidates from the reviewed
decode/prefill thread budgets `6/6`, `8/8`, `4/12`, `8/12`, `8/18`, pruning
budgets larger than the observed logical processor count. If more than three
remain, it selects the first, middle and last. On this 18-core machine those
are `6/6`, `4/12` and `8/18`. These are thread counts without affinity pinning;
Windows chooses their placement.

Each candidate is paired with the unchanged `4/4` control using two fixed
synthetic creative-writing prompts of different lengths. The second repetition
reverses pair order. At most 24 fresh processes run, with 128 generated tokens
and a 180-second deadline each: a maximum 72 minutes of model execution. Artifact
verification and workflow setup are additional; the job has a two-hour ceiling.
The OS file cache is not flushed, so fresh process does not mean cold storage.

The screen fixes the pinned original PQ2 model, runtime, adapter at scale 1.3,
publisher sampler, seed 42, 8,192-token context, FP16 KV, 128/64 prompt batches,
CPU-only placement, no repacking and zero polling. The legacy completion
executable uses its own template defaults; these differ from the chat server's
explicit thinking policy. This is a CPU thread screen, not chat qualification.

Artifacts must already exist. Each sample retains the shared 15.5 GiB admission,
11.5 GiB Job commit ceiling, 4 GiB physical reserve, one-process ownership,
caller lifetime and bounded logs. The batch supervisor verifies and holds exact
model, adapter and runtime files. No model download or runtime/default promotion
occurs during the screen.

## Inspecting the plan

From native ARM64 PowerShell 7.6+, use an existing checked maintenance executable
and an absolute data directory outside Git:

```powershell
./scripts/Invoke-X2CpuEvaluation.ps1 -DataDirectory $data -ToolPath $tool -PlanOnly
./scripts/Doctor.ps1 -ToolPath $tool -PassThru
```

Plan inspection probes hardware and reads compiled policy without loading a
model. It requires an observed X2 Elite Extreme name and agreement between CIM
and the native CPU-set count. The planner never treats a name as kernel
eligibility. Unknown topology or a foreign device rejects the screen.

Doctor's optional `NativeCpuObservations` uses the same Rust `host-cpu` command.
Without `-ToolPath`, Doctor retains its build-independent inventory. Native
CPU observations keep static topology separate from current parking/allocation
flags. CPU-set IDs and class numbers are observations, not persistent affinity
settings or additions to the existing CPU compatibility digest. Windows defines
group-relative core/cache/NUMA indices and relative efficiency classes in the
[CPU-set structure contract](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-system_cpu_set_information).

## Reading evidence

The workflow retains the plan, observed processor name, OS build, process ISA
and native topology,
exact arguments, response hashes, raw synthetic captures and per-sample
supervision. Prompt evaluation, decode and full process wall time remain
separate. Process wall time includes loading and excludes prior artifact
verification. Job commit excludes mapped weight residency, and one-second
physical-memory observations can miss brief pressure.

Initial and final native observations include AC/battery, battery-saver and
charge snapshots where Windows supplies them, plus the active power-scheme GUID
and checked effective power mode from documented version-two notifications.
Scheme/mode observations are sequential and the initial notification has a
bounded one-second wait. These are power conditions,
not energy measurements; transitions within a trial are not sampled. A power
or topology change calls for a new controlled run before interpreting ratios.
The report's `ExecutionConditions` enforces that boundary: changed or unknown
native topology, AC state, battery-saver state, active power scheme or effective
mode suppress all ratios,
while preserving raw samples and token-count comparability separately. Charge
percentage and transient CPU parking/allocation flags do not determine this
check. Equal boundary snapshots cannot rule out changes within a sample or
edits to individual settings within the same power scheme. Older reports without
the policy snapshot remain readable but cannot establish stable conditions.

All device capture/qualification workflows, including TTS and sandbox trials,
share one non-cancelling physical-runner concurrency group. This prevents other
repository CI trials from overlapping the screen on the laptop; unrelated
applications can still compete for its resources.

Candidate/control ratios are emitted only when both samples reach the 128-token
cap with matching prompt/decode counts. Early natural completion remains visible
without a fixed-length ratio. Exact response equality is recorded separately;
it is not a quality evaluation. Two pairs per workload are an initial screen,
not a significance or sustained-performance verdict.

Retain losing candidates and failures. Use promising phase results to choose a
follow-up complete-task, quality, responsiveness and sustained-comfort study.
The screen supplies no energy verdict and does not enable GPU/NPU execution.
Changing the standard profile still requires independent device evidence under
the [hardware selection contract](hardware-profiles.md).

As of the 2026-10-29 repository CI inventory, the GitHub Actions API reports
zero registered self-hosted runners and no X2 screen runs. Register or restore
the owner-managed native ARM64 runner with `swift-bonsai-snapdragon`, then run
the workflow on settled `main` and retain its evidence. Local model execution
does not replace that CI capture. No new thread default has been selected.

## Device evidence separation

Native speech builds use a separate checked host identity for build-cache reuse.
CPU registry identity, Windows kernel version, native target and OS feature
observations partition the entire Cargo/native output directory and its receipt.
Changing from X1 to X2 repeats native compilation and CMake's ISA probes.
Speech CI retains the same host identity in every compiled-cache restore prefix
and verifies it in the static-library receipt. This prevents an older machine's
native output from silently bypassing X2 compilation; it does not select new
thread counts or qualify a speech performance gain.

New LoRA and PTQ1 captures retain the observed processor name and distinguish
`Snapdragon X Elite`, `Snapdragon X2 Elite` and `Snapdragon X2 Elite Extreme`.
X2 receipts require a matching processor observation; relabeling historical X1
evidence rejects. Device-dependent runtime promotion gates cannot combine
different families. Existing frozen X1 contract hashes and legacy receipt shapes
remain readable; the added identity observations do not qualify any X2 profile.
