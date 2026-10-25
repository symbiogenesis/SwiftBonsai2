# Development guide

Run these examples from the repository root in native Windows ARM64 PowerShell
7.6+. Use [validation](validation.md) for language standards and focused checks.

## Current state

Swift Bonsai 2 PQ2_0 is the priority low-memory CPU candidate. Its
[assessment](swift-memory.md) uses separate immutable model/runtime pins from the
Qwen `reference-cpu` profile. Swift CPU load/decode, optional pretrained LoRA and
weight-lossless PTQ1 export work as development checks. Quality, performance and
daily-service acceptance remain pending. See [LoRA qualification](lora-qualification.md),
[storage sharing](prism-storage-overlay.md) and [memory options](swift-memory-options.md).
No new training is required.

The repository provides inventory, verified provisioning, source builds, GGUF
inspection, bounded CPU checks, a strict Rust broker and an explicit Windows
launcher for the Qwen reference or Swift assessment backend. The experimental
[native Rust CPU reference](../src/prism-runtime/README.md) is separate from that launcher.
[Adreno](adreno-gpu-plan.md) and [Hexagon](hexagon-npu-plan.md) integration remain
planned. Current launch policies select CPU execution.

Daily model chat and an offline UI are not enabled. Development ownership and
memory controls do not qualify restricted inference identity, filesystem/egress
isolation or same-user backend bypass prevention.

## Inventory and provisioning

Choose an absolute local data directory outside Git on a hard-link-capable
filesystem such as NTFS. Provisioning rejects junctions and device paths.

```powershell
./scripts/Doctor.ps1
# Optional existing data directory and new report file:
./scripts/Doctor.ps1 -DataPath D:\BonsaiData -OutputPath D:\Reports\doctor.json

$data = Join-Path $env:LOCALAPPDATA 'SwiftBonsai2'
./scripts/Get-Engine.ps1 -DataDirectory $data
./scripts/Get-Model.ps1 -DataDirectory $data
./scripts/Get-Engine.ps1 -DataDirectory $data -VerifyOnly
./scripts/Get-Model.ps1 -DataDirectory $data -VerifyOnly
```

These defaults provision the Qwen reference lock: about 13.15 GB of model data
and a 12.78 MB CPU runtime archive. To provision Swift instead, pass
`-LockPath ./config/swift-assessment.lock.json` to both provisioning commands.
Neither path downloads fallback, projector or full-precision weights.

Model bytes live under `models/<revision>/`, archives under `downloads/`, and
installed runtimes under `runtime/<commit>/`. Downloads resume immutable URLs
through a retained `.partial` handle; ignored Range requests restart. Size/hash
checks precede exclusive hard-link publication. Invalid partials and failed
extraction staging directories remain for inspection. Scripts never substitute
another model/runtime. Runtime verification compares installed files to the
verified archive, ARM64 PE architecture and reported build identity; publisher
digests do not attest source-to-binary builds.

The shared Rust verifier owns installed-runtime checks and retains verified handles
through bounded version/help probes. CPU and paired completion re-verify the runtime
inside their Rust supervisor and hold the guards until execution ends. Provisioning
therefore uses the packaged maintenance tool or builds it from the pinned sources.

Doctor marks unavailable facts when inventory queries are denied. Reports include
local paths; review them before sharing.

For bounded GGUF v3 metadata inspection after full size/SHA-256 verification:

```powershell
./scripts/Read-Gguf.ps1 -ModelPath (Join-Path $data 'models/4ca720788d1e01f1bff70c033e0d0028fd02e502/Qwen3.8-27B-UD-Q3_K_XL.gguf')
```

The [GGUF tool](../tools/gguf/README.md) also supports explicit Prism census and
streaming PQ2-to-PTQ1 export. `-GgufToolPath` selects a prebuilt executable;
otherwise the wrapper builds pinned Rust sources in external temp storage.
Metadata inspection skips tensor payloads; census and export read them.

## Bounded CPU load/decode check

```powershell
./scripts/Invoke-CpuCheck.ps1 -DataDirectory $data -CheckOnly
./scripts/Invoke-CpuCheck.ps1 -DataDirectory $data
# Explicit Swift assessment, after provisioning its separate lock:
./scripts/Invoke-CpuCheck.ps1 -DataDirectory $data -Profile swift-pq2-assessment
# Optional pinned pretrained adapter:
./scripts/Get-Adapter.ps1 -DataDirectory $data
./scripts/Invoke-CpuCheck.ps1 -DataDirectory $data -Profile swift-pq2-assessment -UseAbliterateLora
```

`-CheckOnly` reports admission without starting a model. Execution uses in-process
`llama-completion`, a 16-token cap, 8K context, FP16 K/V, F32 recurrent/convolution
state, no prompt-cache reuse and a ten-minute deadline. Swift uses batch
128/microbatch 64. The embedded template's
thinking behavior is requested explicitly with `--reasoning on` and
`--reasoning-effort xhigh`, alongside the publisher sampler. No extra system
persona is injected. This is not a finished-answer quality check; see
[inference defaults](inference-defaults.md) for the pinned completion renderer's
template-argument limitation. Modern `llama-cli` starts a server and is not used
here.

| Profile | Service allowance | Required available RAM, including 4 GiB reserve |
| --- | --- | --- |
| `reference-cpu` (default Qwen) | 18 GiB | 22 GiB |
| `swift-pq2-assessment` | 12 GiB | 16 GiB |

Admission fails closed without closing other applications. The packaged
[maintenance tool](../tools/bonsai/README.md) owns the process in a kill-on-close
Windows Job from creation, with a job-wide commit cap, one-process limit and no
breakaway. It monitors process memory, available physical memory, parent liveness,
deadlines and output bounds. A Job commit cap does not cover every resident
file-backed mapping, so the physical-memory watchdog remains necessary. Reported
Job peaks can include denied allocations; these diagnostics are not performance
or peak-memory acceptance.

Select a trusted Windows ARM64 `bonsai-tools` executable from the evaluated main
revision through `BONSAI_TOOLS_PATH` or `-ToolPath` to avoid compilation during
routine commands. Otherwise the bridge builds with pinned Rust in external temp.
Key creation with `-WhatIf` resolves no executable. Planning may resolve the tool
but never starts inference.

Swift and LoRA load/decode success does not establish finished-answer quality,
throughput, numerical basis or refusal behavior. The approved standard LoRA
requires [paired same-backend TPS and quality gates](swift-lora.md#required-standard-profile-qualification).

## Prepare the server policy

```powershell
./scripts/Get-ServerPlan.ps1 -DataDirectory $data
./scripts/Get-ServerPlan.ps1 -DataDirectory $data -VerifyRuntime
./scripts/Get-ServerPlan.ps1 -DataDirectory $data -VerifyArtifacts
```

The default uses the maintenance tool for policy planning; it creates no model or
key storage and starts no inference or listener. Tool resolution may compile the
pinned Rust sources and retain build/request evidence in external temp. Select
`-ToolPath` or `BONSAI_TOOLS_PATH` to use a trusted packaged binary.
`-VerifyRuntime` verifies installed runtime files and runs bounded version/help
probes; `-VerifyArtifacts` also hashes the existing model. Neither downloads nor
loads a model. The preflight rejects unknown profile fields, unsafe/network paths
and inherited runtime overrides, including `AIP_*`/`MTMD_*`, while preserving
Windows/profile environment variables.

The planned arguments select numeric loopback, one 8K FP16 CPU slot, a private
key-file path, no host tools/MCP/UI, no reuse/speculation and the reference sampler.
Help checks establish switch availability, not option enforcement. Native server
Host/Origin checks, control routes, per-request overrides and total duration need
the broker policy; an offline flag is not OS egress denial. The PowerShell
preflight remains launch-disabled even though the separate development broker
implements authentication, request bounds and lifecycle controls.

## Explicit development broker

The [broker contract](broker-policy.md) defines strict authenticated text chat,
exact token-count admission, bounded HTTP I/O and cancellation. It permits one
active request plus zero through eight waiting connections. Queue wait is at
most thirty seconds and the total request deadline is ten minutes. Clients must
keep their sending side open until the response finishes. Interrupted generation
permanently closes admission; there is no automatic restart or reset.

Build from an external native working directory:

```powershell
$repo = (Get-Location).Path
$scratch = Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-Broker-' + [guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($scratch)
$previous = [Environment]::CurrentDirectory
Push-Location $scratch
try {
    [Environment]::CurrentDirectory = $scratch
    cargo +nightly-2026-10-22-aarch64-pc-windows-msvc build --locked --release --manifest-path (Join-Path $repo 'src/broker/Cargo.toml') --target-dir (Join-Path $scratch 'build')
    if ($LASTEXITCODE) { throw 'Broker build failed.' }
} finally {
    [Environment]::CurrentDirectory = $previous
    Pop-Location
}
$broker = Join-Path $scratch 'build/release/bonsai-broker.exe'
```

### Connect an existing development backend

Create fresh protected keys under an existing owner-controlled external parent:

```powershell
$keys = ./scripts/New-DevelopmentKeys.ps1 -ParentDirectory $data
```

Configure the separate backend to use `$keys.BackendKeyFile`, then run the broker
from the external native working directory:

```powershell
$previous = [Environment]::CurrentDirectory
Push-Location $scratch
try {
    [Environment]::CurrentDirectory = $scratch
    & $broker --development --allow-unverified-key-files `
        --frontend-port 5173 --backend-port 8081 `
        --frontend-key-file $keys.FrontendKeyFile --backend-key-file $keys.BackendKeyFile --max-waiting 2
} finally {
    [Environment]::CurrentDirectory = $previous
    Pop-Location
}
```

This mode binds only `127.0.0.1`; frontend port zero requests an OS-assigned port.
It requires both opt-ins because it does not verify key ACLs, ancestor ownership
or backend process identity. The helper returns paths only and creates distinct
64-byte lowercase-hex credentials with protected owner/LocalSystem ACLs. Keys and
partial private directories remain for inspection. Same-user processes and the
parent-directory owner remain within the trust boundary. No UI route is supplied.

### Owned Windows development backend

The launcher supports only the compiled-in Qwen `reference-cpu` pins, without
LoRA or arbitrary arguments. It performs no download and does not select Swift.
Use existing external data/evidence directories:

```powershell
$previous = [Environment]::CurrentDirectory
Push-Location $scratch
try {
    [Environment]::CurrentDirectory = $scratch
    & $broker --launch-development --allow-unisolated-backend `
        --data-directory $data --evidence-parent ([IO.Path]::GetTempPath()) `
        --frontend-port 5173 --backend-port 8081 --max-waiting 2
} finally {
    [Environment]::CurrentDirectory = $previous
    Pop-Location
}
```

Append `--check-only` for read-only artifact/policy verification and memory
admission reporting, without credentials, a listener or a process.

The default selection remains `reference-cpu`. Pass
`--profile swift-pq2-assessment` to select Swift's separately pinned Prism runtime
and original PQ2 model, provisioned with `config/swift-assessment.lock.json`.
That selection fixes batch/microbatch to 128/64, a 12 GiB Job cap and 16 GiB
available-RAM admission including the 4 GiB reserve. Derived PTQ1 and adapters
are unavailable through this launcher. Profile names are exact; arbitrary
models, runtimes and inference overrides remain rejected.

The launcher retains verified model/runtime handles, creates fresh protected
credentials/evidence and applies the selected Job cap plus 4 GiB physical reserve
(18 GiB/22 GiB available RAM for the default reference).
The frontend opens only after health/authentication checks and listener-PID
verification. Backend connections recheck that PID. Ctrl-C, backend exit, guard
failure, startup failure or closed generation admission stops the whole Job;
owner drop or abrupt broker death also ends it. See the [launcher contract](broker-policy.md#owned-windows-development-launcher)
for detailed ownership and readiness limits.

The printed evidence directory contains `frontend.key`, `backend.key` and technical
logs. A client may read `frontend.key`; never share either key or the whole
directory. Restricted tokens, filesystem/egress isolation, same-user bypass
prevention and real-model daily acceptance remain unqualified.

## Build the CPU server from source

### Experimental native Swift backend

The Rust broker also has an opt-in `native-swift` build feature. Build
`bonsai-broker` with `--features native-swift` using the external working and
target directories shown above, then add `--native-engine` to the owned launcher:

```powershell
& $broker --launch-development --allow-unisolated-backend `
    --profile swift-pq2-assessment --native-engine `
    --data-directory $data --evidence-parent ([IO.Path]::GetTempPath()) `
    --frontend-port 5173 --backend-port 8081 --max-waiting 2
```

Run this executable from an external native working directory as above. The
selection requires the original pinned Swift artifact and its verified Prism
installation; it accepts no adapter, alternate model or inference overrides.
The current executable is retained against replacement and its native child
receives the existing 12 GiB Job, 16 GiB available-memory admission, 4 GiB
reserve watchdog, protected keys and listener-PID checks. The seed is fixed at
42. Native identity must match before the frontend opens. Interrupted frontend
generation closes admission and stops the owned backend; restart the launcher
to establish a fresh owned instance.

This is an experimental path. Actual model loading, prompt/tokenizer parity and
seeded sampler comparisons have evidence, but full-model numerical parity fails
the declared tolerance and complete device recovery remains pending. See the
[MVP checklist](prism-upstream-todo.md); no throughput or daily-service claim is
made. The internal `--native-engine-child` flag alone does not establish Job
ownership or prevent same-user bypass.

### Publisher Prism source build

Install Visual Studio 2026 native ARM64 C++ tools and CMake, or explicitly choose
the supported Visual Studio 2022 generator. The builder installs no toolchain.

```powershell
$doctor = ./scripts/Doctor.ps1 -PassThru
$gitPath = ($doctor.Tools | Where-Object Name -eq Git).Path
$cmakePath = ($doctor.Tools | Where-Object Name -eq CMake).Path
./scripts/Build-Engine.ps1 -DataDirectory $data -GitPath $gitPath -CMakePath $cmakePath -FetchSource
```

The builder fetches only the pinned commit, requires an exact clean checkout on
reuse, and builds Release `llama-server` under `builds/<commit>/cpu/`. External
receipts record configuration, compiler, hashes and version. Subprocess tools,
accelerators and automatic dependency/UI downloads are disabled. No test suite,
inference or benchmark runs; reviewed offline UI assets remain separate work.

Default parallelism reserves two CPU threads and 2 GiB, budgets 1 GiB per compiler,
and caps project/shared compiler workers at eight. If memory is unavailable it
uses at most two. `-Parallel 1` through `-Parallel 12` overrides that budget.
Incremental outputs are reused only after source/configuration checks.
