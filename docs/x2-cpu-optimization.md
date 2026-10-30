# X2 Elite Extreme CPU thread screen

The **2026-10-29 model-free inventory** identifies the development target, not a qualified speedup
or GPU/NPU profile. Native ISA semantics live in [capability contracts](hardware-capabilities.md).

| Observation | Retained value |
| --- | --- |
| CIM/independent hardware-registry name | `Snapdragon(R) X2 Elite Extreme - X2E94100 - Qualcomm Oryon(TM) CPU` |
| Native toolchain/target/kernel | Pinned Rust; `aarch64-pc-windows-msvc`; Windows `10.0.26300` (`10.0.26300.0` probe display) |
| CPU topology | 18 physical/logical cores, group 0, efficiency/scheduling classes `[0, 1]`; class numbers do not name core types |
| Native Windows ISA | Positive NEON, dot-product and **NEON I8MM** flags; dispatched kernels still need codegen/numerical/device qualification |
| GPU inventory | Adreno X2-90, driver `32.0.149.0` |
| Power policy | Effective `balanced`; scheme `381b4222-f694-41f0-9685-ff5bb260df2e` |
| Planned screen | `d6-p6`, `d4-p12`, `d8-p18`; 24 samples; `ModelProcessStarted: false` |

Built CLI checks accepted unchanged native identity/power observations and rejected injected AC,
effective-mode and kernel-build changes plus legacy observations lacking host identity. The checked
speech-host query agreed on CPU/kernel. Reports, plans and comparison receipts remain in external
`Bonsai-PerfNext3-20261029/x2-evidence`. These checks loaded no model and measured no timing.

The manual [X2 CPU workflow](../.github/workflows/x2-cpu-evaluation.yml) supplies a bounded first
thread screen on the owner-managed native ARM64 runner, on settled `main`. It shares the existing
model-capture concurrency group. All performance execution remains CI-only under
[validation](validation.md).

## Controls and cost

The Rust maintenance planner selects at most three candidates from the reviewed decode/prefill
thread budgets `6/6`, `8/8`, `4/12`, `8/12`, `8/18`, pruning budgets larger than the observed
logical processor count. If more than three remain, it selects the first, middle and last. On this
18-core machine those are `6/6`, `4/12` and `8/18`. These are thread counts without affinity
pinning; Windows chooses their placement.

Each candidate is paired with the unchanged `4/4` control using two fixed synthetic creative-writing
prompts of different lengths. The second repetition reverses pair order. At most 24 fresh processes
run, with 128 generated tokens and a 180-second deadline each: a maximum 72 minutes of model
execution. Artifact verification and workflow setup are additional; the job has a two-hour ceiling.
The OS file cache is not flushed, so fresh process does not mean cold storage.

The screen fixes the pinned original PQ2 model, runtime, adapter at scale 1.3, publisher sampler,
seed 42, 8,192-token context, FP16 KV, 128/64 prompt batches, CPU-only placement, no repacking and
zero polling. The legacy completion executable uses its own template defaults; these differ from the
chat server's explicit thinking policy. This is a CPU thread screen, not chat qualification.

Artifacts must already exist. Each sample retains the shared 15.5 GiB admission, 11.5 GiB Job commit
ceiling, 4 GiB physical reserve, one-process ownership, caller lifetime and bounded logs. The batch
supervisor verifies and holds exact model, adapter and runtime files. No model download or
runtime/default promotion occurs during the screen.

Before verifying model bytes or starting thread trials, execution runs the exact verified completion
executable with only `--version` under the ordinary headless worker policy. This model-free
preflight retains a separate `worker-preflight/` receipt and bounded logs, with a 512 MiB Job and
ten-second deadline. Failure stops the screen and leaves `ModelTrialsRequested: false` in its
report. The shared supervisor's `ModelProcessStarted` field means an executable was started; the
preflight's explicit `ModelLoaded: false` distinguishes it from model trials. PlanOnly remains
nonexecuting. The relaxed inspection launch cannot satisfy this worker check.

## Inspecting the plan

From native ARM64 PowerShell 7.6+, use an existing checked maintenance executable and an absolute
data directory outside Git:

```powershell
./scripts/Invoke-X2CpuEvaluation.ps1 -DataDirectory $data -ToolPath $tool -PlanOnly
./scripts/Doctor.ps1 -ToolPath $tool -PassThru
```

Plan inspection probes hardware and reads compiled policy without loading a model. It requires an
observed X2 Elite Extreme name and agreement between CIM and the native CPU-set count, plus
agreement between the CIM processor name and the bounded native registry name. Missing native
processor/kernel/feature identity rejects planning. The plan retains `HostIdentity`, separate from
live power or CPU availability. The planner never treats a name as kernel eligibility. Unknown
topology or a foreign device rejects the screen.

Doctor's optional `NativeCpuObservations` uses the same Rust `host-cpu` command. Without
`-ToolPath`, Doctor retains its build-independent inventory. Native CPU observations keep static
topology separate from current parking/allocation flags. CPU-set IDs and class numbers are
observations, not persistent affinity settings or additions to the existing CPU compatibility
digest. Windows defines group-relative core/cache/NUMA indices and relative efficiency classes in
the [CPU-set structure
contract](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-system_cpu_set_information).

## Reading evidence

The workflow retains plans, CIM/native processor/kernel/feature/topology observations, ISA,
effective arguments, response hashes, raw synthetic captures and per-sample supervision.
Prompt/decode timings and process wall time remain separate. `WallMilliseconds` includes load,
but excludes prior artifact verification and CPU/power observations before creation/after owned
teardown. Fresh processes retain OS file cache; they are not cold-storage trials.

| Memory observation | Meaning and limitation |
| --- | --- |
| `ProcessMemory` | Existing nominal one-second supervisor queries plus final exit when available; successful count retained, including failed/over-ceiling samples; missing data is null with zero samples |
| `SampledPeakPrivateBytes`, `SampledPeakWorkingSetBytes` | Maxima of current process counters; brief allocations may escape sampling |
| `LifetimePeakWorkingSetBytes` | Windows lifetime resident peak, including mapped pages |
| Job commit / system available RAM | Job commit omits mapped residency; one-second physical observations can miss brief pressure |

Private commit, working set, Job commit and available RAM overlap: never add them. These
aggregates identify neither phase allocations/lifetimes, complete model storage nor energy.

Timing ratios require **both** gates below; invalid conditions preserve raw samples and
independent token-count comparability:

| Gate | Required comparison |
| --- | --- |
| Batch `ExecutionConditions` | Known unchanged native topology, AC, battery saver, active scheme/effective mode and CPU/kernel/full-feature identity at initial/final boundaries |
| `PairExecutionConditions` | Both samples' `CpuConditions.Before`/`After`/`Comparison` plus the transition between them, captured by optional `observeCpuConditions` outside process wall time without model polling |

The pair gate catches a mode change that reverses before batch end. Missing/malformed sample
observations or legacy receipts without `host_identity` remain raw-only evidence. CIM/registry
name agreement does not prove physical SKU or optional-kernel parity. Power notifications are
sequential with a bounded one-second initial wait. Charge percentage and transient parking/
allocation do not gate comparisons; equal boundaries cannot exclude within-sample changes or
settings edits within one scheme. These are power conditions, not energy measurements.

All repository device workflows, including TTS/sandbox trials, share one non-cancelling physical-
runner concurrency group; unrelated applications may still contend. Emit candidate/control
ratios only when both reach 128 tokens with matching prompt/decode counts. Natural early
completion stays visible without a fixed-length ratio; response equality is separate from quality.
Two pairs per workload give an initial screen, not significance/sustained qualification.
Retain losses/failures; promising phases need complete-task, quality, responsiveness and comfort
follow-up. The screen supplies no energy verdict, accelerator enablement or new default.

### CI availability and headless startup

The **2026-10-29** GitHub Actions inventory reported zero self-hosted runners and no X2 screen
runs. Restore/register native ARM64 `swift-bonsai-snapdragon`, run settled `main`, and retain CI
evidence; local model execution is not a substitute.

A model-free legacy `llama-completion.exe --version` check on that date returned `3221225794`
(`0xC0000142`, DLL initialization failure) under ordinary completion supervision: ten-second
limit, 512 MiB Job, one process, no selected/loaded model. A repeated probe held all 63 verified
runtime files through exit, source `73bfe6cc485554822f80c473a3c0c4f25e80f41b`, archive SHA-256
`cc808b33d35c72a3bd5c69a0d609d422426feb9dce23868e77992b6c33b8ddb3`.
Read-only PE inspection found `llama-completion.exe` → `llama-completion-impl.dll` →
`llama-common.dll` → `SHELL32.dll`, consistent with the shell concern but not proof of the failing
DLL. External `Bonsai-X2-HeadlessProbe-dcbb58f9ed3440ff809d03673989ec31` retains
`runtime-verification.json` and `verified-headless/supervision.json`. Resolve compatibility and
repeat bounded preflight before model trials; inspection-only help/version cannot qualify a
worker or authorize weaker Win32k denial.

The [UCRT argv source candidate](runtime-candidates/headless-ucrt-argv.md) removes the shell
import in an exact-source CPU build. Its paired control reproduces `0xC0000142`; candidate version
and seven Unicode/quoting/ownership cases pass unchanged worker policy on X2. Source/compiler/
imports/hashes/supervision are retained there. This model-free source result leaves provisioned
runtime/thread defaults unchanged pending upstream release review and device CI.

## Device evidence separation

Native speech builds use a separate checked host identity for build-cache reuse. CPU registry
identity, Windows kernel version, native target and OS feature observations partition the entire
Cargo/native output directory and its receipt. Changing from X1 to X2 repeats native compilation and
CMake's ISA probes. Speech CI retains the same host identity in every compiled-cache restore prefix
and verifies it in the static-library receipt. This prevents an older machine's native output from
silently bypassing X2 compilation; it does not select new thread counts or qualify a speech
performance gain.

New LoRA and PTQ1 captures retain the observed processor name and distinguish `Snapdragon X Elite`,
`Snapdragon X2 Elite` and `Snapdragon X2 Elite Extreme`. X2 receipts require a matching processor
observation; relabeling historical X1 evidence rejects. Device-dependent runtime promotion gates
cannot combine different families. Existing frozen X1 contract hashes and legacy receipt shapes
remain readable; the added identity observations do not qualify any X2 profile.
