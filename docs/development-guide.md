# Development guide

Run these examples from the repository root in native Windows ARM64 PowerShell
7.6+. Use [validation](validation.md) for language standards and focused checks.

Start with [Native chat app](#native-chat-app), or [portable packages](chat-packages.md)
for a launch without a checkout. [Inventory/provisioning](#inventory-and-provisioning),
[CPU checks](#bounded-cpu-loaddecode-check) and the [development broker](#explicit-development-broker)
are separate developer workflows, not additional app-setup steps.

## Current state

Swift PQ2_0 CPU is the priority low-memory profile. Load/decode, optional pretrained
LoRA and current-weight lossless PTQ1 export have development evidence; quality,
performance and daily-service acceptance remain pending. Swift and Qwen
`reference-cpu` use separate immutable pins. See [memory](swift-memory.md),
[LoRA qualification](lora-qualification.md) and [memory options](swift-memory-options.md).
No new training is required. [Prism Inference](https://github.com/symbiogenesis/prism-inference)
is a separately published experimental Rust runtime, initially for Swift/Bonsai.

Current launches explicitly select CPU. [Adreno](adreno-gpu-plan.md),
[Hexagon](hexagon-npu-plan.md) and [automatic profiles](hardware-profiles.md) remain
planned. The [X2 CPU screen](x2-cpu-optimization.md) compares prefill/decode budgets
in target-device CI; local inventory/`-PlanOnly` loads no model and cannot promote
defaults. Owned UI model/speech workers require [native containment](sandboxing.md);
actual-model and broker-only/same-user isolation acceptance remain pending.

## Native chat app

The [portable package workflow](chat-packages.md) installs reviewed prebuilt app
files without a checkout/compiler. Source launch requires native Windows ARM64
PowerShell 7.6+, the pinned Rust toolchain, Swift model/runtime files and
**15.5 GiB available RAM**. Serviced Windows 11 must admit PSEC 1.1 and
container-hosted loopback; failed containment never falls back to caller authority.

Provision Swift and its pinned adapter, then launch:

```powershell
$data = Join-Path $env:LOCALAPPDATA 'SwiftBonsai2'
./scripts/Get-Engine.ps1 -DataDirectory $data -LockPath ./config/swift-assessment.lock.json
./scripts/Get-Model.ps1 -DataDirectory $data -LockPath ./config/swift-assessment.lock.json
./scripts/Get-Adapter.ps1 -DataDirectory $data
./scripts/Start-Chat.ps1 -DataDirectory $data
```

The native Rust `egui`/`eframe` app follows the [UI contract](ui-architecture.md).
The window opens before broker/speech preparation finishes, so drafting remains
available during setup. Setup errors stay visible; close and relaunch after
correcting them. Closing stops preparation, compiler descendants and owned model
workers. Artifact verification and memory admission still precede model use.

**Stop** tears down the owned broker/backend. After teardown, **Retry model**
starts a fresh owned instance, repeating identity, artifact and 15.5 GiB admission
checks while retaining completed turns. Use the same explicit retry after
uncertain transport/protocol failures or after freeing RAM for a failed startup.
There is no automatic replay or reduced-memory fallback. Interrupted prompts
return only to an empty composer, preserving any newly typed draft. A local
connection closure alone does not establish a token or memory failure.

### Conversations and response settings

The sidebar holds up to 16 conversations with independent drafts and settings.
Switch while ready/idle; use **New conversation** or the row's **actions** menu to
rename, copy or delete. Deletion names its target; **Keep conversation** or
Escape cancels, and deleting the last chat creates an empty one. Status and
collapsible diagnostics stay outside the user/assistant transcript.

The composer scrolls long drafts without hiding Microphone or Send/Stop.
**Enter** sends when ready, **Shift+Enter** inserts a newline and **Ctrl+End**
moves to the draft's end. **Find** / **Ctrl+F** searches visible message bodies
with Unicode case-insensitive matching, excluding reasoning. Its magnifying glass
sits beside Settings; previous/next controls appear below the header. **F3/Shift+F3**
(or **Enter/Shift+Enter** while Find is focused) moves between matches; **Latest**
or the bottom-center down arrow returns to the newest message. **Escape** closes Find and clears highlights;
a second Escape stops active generation or speech.

| Setting | Behavior |
| --- | --- |
| **Thinking** | Off initially and in older saved chats; enables reasoning for the next response and every Extra effort pass. |
| **Mode** | Standard makes one call. Experimental Extra effort makes at most three sequential plan/draft/review calls under one ten-minute deadline, shows stage labels and retains complete drafts. A truncated revision does not replace a complete draft; quality gains remain unproven. |
| **Output cap** | Defaults to 4,096; also offers 512/1,024/2,048 reasoning-inclusive tokens within the fixed 8,192-token context and 64-token template reserve. |
| **Abliteration scale** | Pinned adapter defaults to 1.3; choose 0-2 in 0.1 steps for the next response/every Extra effort pass without restart. Zero disables the correction. Older saved chats use 1.3; effectiveness and quality remain experimental. |

Settings belong to each conversation and are retained by optional saving;
response controls lock during generation. The broker counts the exact prompt
for every call: controls neither estimate tokens nor discard history. A framed
context rejection restores the prompt and keeps the model ready; lower the output
cap or start a conversation. Reaching an admitted response's cap is a separate
partial-answer outcome, after which you can explicitly ask Swift to continue.
Other recognized input/burst rejections follow the [broker error contract](broker-policy.md#native-client-completion-and-recovery).

### Optional recovery and preview

In **Settings**, enable **Save conversations on this device** to save drafts,
response settings, voice description, transcript and exact completed history in
`DataDirectory/chat-recovery.dpapi`. User-scoped DPAPI encrypts the bounded 8 MiB
workspace; a file lock permits one writer per data folder. Saving defaults off.
Interrupted text can remain visible after recovery but never becomes committed
history or an automatic request. Restore activates neither microphone nor playback.
Disabling saving deletes the saved copy and keeps the workspace open. Corrupt or
unreadable recovery is preserved with a warning until explicit saving replaces it.
Keep the data folder private and outside Git: DPAPI does not isolate other
same-user processes, and deletion is neither secure erasure nor backup removal.

The app supports Windows high contrast, display scaling, labelled AccessKit
controls and selectable/copyable text. Full bidirectional editing and native-control
accessibility parity remain unqualified. Check layout, streaming selection,
resizing and keyboard navigation with a marked model-free preview:

```powershell
$previewTarget = Join-Path ([IO.Path]::GetTempPath()) ('Bonsai-Preview-' + [guid]::NewGuid().ToString('N'))
cargo build --locked --release --package bonsai-chat --target-dir $previewTarget
Start-Process -FilePath (Join-Path $previewTarget 'release/bonsai-chat.exe') -ArgumentList '--preview' -WorkingDirectory ([IO.Path]::GetTempPath())
```

Preview uses the same UI and sample content, without broker, credentials,
microphone or speech services. It establishes neither real-model nor audio behavior.

### Speech setup and operation

The launcher provisions native ARM64 Canary Qwen 2.5B and Qwen3-TTS 1.7B
VoiceDesign/Base companions. Rust owns services, authentication, validation, WAV
processing and process ownership; reviewed native GGML C++ engines infer on CPU.
The four pinned Q8_0 GGUF files total about 6.7 GiB under
`$data/speech/native/<revision>/`. Downloads resume with exact size/SHA-256
verification; launch checks cached sizes, then each service fully hashes and holds
files before first load. Original snapshots/private runtimes are preserved.

Download and verify without compiling or launching:

```powershell
$speech = ./scripts/Start-Chat.ps1 -DataDirectory $data -SpeechModelsOnly
$speech.CanaryModelPath
$speech.TtsModelPath
$speech.TtsCodecPath
$speech.TtsVoiceModelPath
```

`Get-NativeSpeechModels.ps1 -DataDirectory $data -Model Canary` or `-Model QwenTts`
selects one engine. `Get-SpeechModels.ps1` explicitly provisions original reference
snapshots, which the app no longer uses. `-NoTts` skips automatic TTS setup;
`-NoSpeechModels` skips all automatic speech setup/downloads. Neither provisioning
nor compiling opens a microphone.

Native speech builds require the pinned Rust compiler, Visual Studio 18 ARM64 C++
tools and Windows SDK. Pinned LLVM ARM64/engine archives and all outputs stay
outside Git; only child compiler settings change. Downloads use Windows
`System32/curl.exe` 8.4+ with HTTPS-only redirects and resumable verified partials;
LLVM extraction uses `System32/tar.exe`. Cached launch invokes neither download.
`-AsrPath`/`-TtsPath` select built executables, whose services still verify models.

Only explicitly enabled, Windows-permitted microphone capture starts input.
Five-second mono 16 kHz WAV clips remain in memory; capture pauses for each
transcription and reuses the recorder only for the same enabled, permitted
generation. Disabling or revoking access closes the recorder.
Fixed clip boundaries can split words: this is neither streaming ASR nor VAD.
Transcripts return to the composer for review, never automatic send. Disabling,
revocation, Send, New conversation, read-aloud or closing cancels input; late
completions cannot reactivate it. Missing/not-ready Canary or denied permission
keeps capture off and exposes setup guidance.

Each completed response's lower-right **speaker button** reads it independently,
including while a newer response generates. The active button becomes **Stop**;
selecting another response switches playback. Empty/streaming responses cannot
start manual playback. Only visible answer text is spoken, excluding reasoning.
Describe the **Voice profile**, for example "A warm, low-pitched narrator with a
calm pace and clear consonants." No preset or reference recording is required.
VoiceDesign makes one fixed reference phrase with a fixed seed/profile, then
releases before Base extracts/reuses conditioning across segments and messages.
Text, paragraphs and request/conversation IDs never choose the reference.
Changing profile creates another; restarts regenerate the same fixed inputs.
This conditioning design does not guarantee perceived speaker identity.

**Read aloud as responses arrive** is off by default per conversation. It sends
visible answer deltas in bounded sentence/word chunks while Swift generates;
loading/synthesis can lag text. **Stop speech** or manual replay suppresses
automatic reading for that response; the next prompt re-enables it. Disabling the
checkbox stops automatic reading while preserving manual replay. Extra effort
reads visible stages and cancels queued audio on replacement. Conversation
switch, generation Stop or model failure cancels speech. Saving retains the
checkbox without replay on restore. Working drafts and reasoning are excluded.

Owned speech uses separate Windows Jobs and mutual exclusion: read-aloud releases
Canary before Qwen loads; transcription releases Qwen. Canary stays warm only for
the enabled input generation. Qwen synthesizes sequential 256-character segments
with bounded streaming playback/prefetch and stays warm at most 30 idle seconds
after success. Stop, failure, transcription or shutdown releases it immediately;
resource monitoring continues while warm. Each service requires 9 GiB available,
a 5 GiB physical reserve and a 16 GiB Job commit ceiling. Swift retains its
15.5 GiB admission/4 GiB reserve; speech failure preserves text chat and answers.
[Streaming resources](streaming-resources.md) owns queue, segmentation and playback bounds.

TTS automatically reserves two logical CPUs; `-TtsThreads 1` through `256` selects
an explicit budget and zero restores automatic sizing. More threads do not imply
better throughput. Q8_0 remains the default. Evaluate smaller Q4_K_M talkers with
the unchanged Q8 codec explicitly:

```powershell
./scripts/Start-Chat.ps1 -DataDirectory $data -TtsQuantization Q4_K_M
./scripts/Get-NativeSpeechModels.ps1 -DataDirectory $data -Model QwenTts -TtsQuantization Q4_K_M
./scripts/Invoke-TtsEvaluation.ps1 -DataDirectory $data -PlanOnly
```

Each immutable profile is verified before loading. Actual evaluation belongs to
the main-only TTS workflow; [speech memory](performance-expectations.md#q4-speech-resource-comparison) defines paired
fresh/warm resource and listening review. Q4 is not qualified as equivalent to Q8;
streaming, prefetch and warm reuse alone establish no throughput or peak-memory gain.

For external services, pass `-CanaryKeyFile C:\Speech\canary.key` and/or
`-TtsKeyFile C:\Speech\qwen-tts.key`. Default ports 8766/8767 must differ from
each other and chat ports. Key-only services remain externally owned and trigger
no unused provisioning/builds. Authenticated capabilities are checked; TTS requires
`consistent_voice: true` and can use complete-WAV playback without streaming.
No substitute recognizer or Windows voice is used. Idle controls expose missing
setup guidance. See [Canary](../tools/canary-asr/README.md) and
[Qwen](../tools/qwen-tts/README.md) for protocols. Native smoke checks establish a
working path, not full audio quality, latency or daily-use qualification.

### Source launch, caches and runtime selection

`Start-Chat.ps1` bridges to Rust `chat-launch`; an owned `chat-prepare` worker runs
after the first UI tick. Rust owns option validation, speech setup, build selection,
credentials and app launch. `BONSAI_TOOLS_PATH` or `-ToolPath` selects a trusted
packaged maintenance CLI. Otherwise its reusable cache is
`%LOCALAPPDATA%/SwiftBonsai2/build/tools/`, separated by checkout/compiler. An
immutable bootstrap copy permits rebuilding without replacing the running tool.
This resolver also serves model-only provisioning without loading models or
building speech engines.

Chat/broker outputs use `DataDirectory/build/chat/`; companions use
`DataDirectory/build/native-speech/`. Source/build fingerprints and SHA-256 executable
receipts skip Cargo, LLVM checks, source staging and VS setup when unchanged.
Speech receipts/output additionally bind CPU registry identity, Windows kernel
and OS processor-feature observations: moving X1 data to X2 rebuilds native
objects and CMake ISA probes. Missing mandatory host facts reject reuse/build.
Source, embedded/configuration pins, locks, local dependencies, build environment
or executable changes invalidate receipts; unknown embedded inputs disable reuse.
Documentation/CI/PowerShell helper changes alone do not rebuild Rust. Cargo's
content freshness handles preserved timestamps. Each variant has an atomic receipt,
short labels protect MSVC path space, and no startup cache cleanup is needed.
A version probe is reused only for the executable digest checked during that launch.

Build cancellation, owner exit or crash closes its Windows Job and waits for
descendants before releasing the build lock. Writes complete before source/receipt
publication; corrupt completed partials are preserved before one fresh attempt.
Session credentials and diagnostics stay fresh. `-ForceBuild` repeats Cargo/native
preparation, including after external compiler/cache changes. Absolute `-ChatPath`
and `-BrokerPath` must select matching current builds; older apps lack deferred
startup flags. Unique system-temp lanes retain `source-build-*.log` and `chat.log`.

One user-wide exclusive `%LOCALAPPDATA%/SwiftBonsai2/chat.lock` covers preparation
through app exit, including launches from other checkouts/packages/data/ports.
Repeated launches report already starting/running. The empty sentinel persists
after release; `-SpeechModelsOnly` is independent and direct executable launches
bypass the script guard. Installed launches use [package verification](chat-packages.md#launch-without-a-checkout).

### Runtime selection

| Runtime/model | Source and installed launch options | Boundary |
| --- | --- | --- |
| Publisher Prism CPU, original Swift PQ2 | Default; pinned adapter enabled | Adapter scale follows response settings. Missing/altered adapters fail with provisioning guidance, without download or fallback. |
| Publisher Prism CPU, current-weight PTQ1 export | `-UseRepackedPtq1` (app/broker `--use-repacked-ptq1`) | Same adapter opt-out/scale. Missing/changed bytes reject; no older publisher PTQ1 substitution or fallback. |
| Experimental native Rust 0.1.6, original Swift PQ2 | `-NativeEngine -NoAbliterateLora` (app/broker `--native-engine`) | Rejects PTQ1/adapters before preparation; no engine switch after failure. Separate [PTQ1 qualification](ptq1-readiness.md) is pending. |

`-NoAbliterateLora` (direct app `--no-abliterate-lora`) omits the adapter and disables
its control. Provision the native release before selecting it:

```powershell
./scripts/Get-Inference.ps1 -DataDirectory $data
./scripts/Start-Chat.ps1 -DataDirectory $data -NativeEngine -NoAbliterateLora
```

Publisher Prism stays default until [runtime promotion](runtime-promotion.md).
Thinking uses the same setting across engines; native quality, throughput and
daily-service qualification remain pending. Native HTTP remains numeric loopback
with the [broker's strict public subset](broker-policy.md).

`-FrontendPort`/`-BackendPort` select distinct local ports. Swift/runtime provisioning
is explicit; only speech downloads at launch. Keep credentials and complete
evidence directories private.

## X1/X2 configuration boundary

The current machine is Snapdragon X2 Elite Extreme X2E94100; the previous machine
was a 32 GB X Elite X1E-78-100. [Retained inventory](x2-cpu-optimization.md) records
observations. [Hardware profiles](hardware-profiles.md) owns planned automatic
capability/driver/workload/objective selection, qualified prefill/decode placement,
actual-budget admission and supported CPU fallback/refusal. Released policies
remain explicit CPU selections; SKU names or TOPS never authorize acceleration.
Outstanding work stays in [PLAN.md](../PLAN.md).

## Inventory and provisioning

Use an absolute local data directory outside Git on a hard-link-capable filesystem
such as NTFS; junctions/device paths reject. The default retains `SwiftBonsai2`
for installation compatibility, separate from the `SwiftBonsai2Runner` checkout.

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

`bonsai-artifacts` shares ZIP, SHA-256, installed-file and PE checks between the
broker and maintenance. Extraction rejects unsafe or duplicate paths and excessive
expansion. Guards hold the archive, verified files and ancestors through probes;
completion takes a fresh guard for its batch. GGUF inspection/export also holds
its verified source and validates output before publication. Receipts record
identity but cannot transfer live file ownership or prove parser isolation.

Download cancellation reaches streamed bodies; a header timeout alone does not
bound a body read. Publication holds the exclusive partial-file handle through
verification and a same-volume hard link, which cannot overwrite a destination.
Failure preserves the partial. See the API contracts for
[streamed HTTP](https://learn.microsoft.com/en-us/dotnet/api/system.net.http.httpcompletionoption),
[PE images](https://learn.microsoft.com/en-us/windows/win32/debug/pe-format),
[exclusive creation](https://doc.rust-lang.org/std/fs/struct.OpenOptions.html#method.create_new)
and [hard links](https://learn.microsoft.com/en-us/windows/win32/fileio/hard-links-and-junctions).

Doctor marks unavailable facts when inventory queries are denied. Reports include
local paths; review them before sharing.

For bounded GGUF v3 metadata inspection after full size/SHA-256 verification:

```powershell
$inference = ./scripts/Get-Inference.ps1 -DataDirectory $data
./scripts/Read-Gguf.ps1 -ModelPath (Join-Path $data 'models/4ca720788d1e01f1bff70c033e0d0028fd02e502/Qwen3.8-27B-UD-Q3_K_XL.gguf') -GgufToolPath $inference.Gguf
```

The [GGUF tool](https://github.com/symbiogenesis/prism-inference/blob/main/tools/gguf/README.md) also supports explicit Prism census and
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
`--reasoning-effort xhigh`, alongside the publisher sampler. No system-only
preamble is injected: Swift's verified template rejects it. This check preserves
that template and does not qualify another chat profile. It is not a
finished-answer quality check; see
[inference defaults](inference-defaults.md) for the pinned completion renderer's
template-argument limitation. Modern `llama-cli` starts a server and is not used
here.

| Profile | Service allowance | Required available RAM, including 4 GiB reserve |
| --- | --- | --- |
| `reference-cpu` (default Qwen) | 18 GiB | 22 GiB |
| `swift-pq2-assessment` | 11.5 GiB | 15.5 GiB |

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

Policy planning uses the maintenance tool without model/key storage, inference
or a listener. Resolution may compile pinned Rust and retain build/request evidence
in external temp; `-ToolPath`/`BONSAI_TOOLS_PATH` selects a trusted packaged binary.
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

The launcher defaults to the compiled-in Qwen `reference-cpu` pins; Swift and
its pinned adapter require the explicit selections below. It performs no download
and accepts no arbitrary arguments. Use existing external data/evidence directories:

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
That selection fixes batch/microbatch to 128/64, an 11.5 GiB Job cap and 15.5 GiB
available-RAM admission including the 4 GiB reserve. Add `--use-abliterate-lora`
to load the pinned adapter and enable its bounded per-request scale with the
Prism CPU server (default 1.3); the demo
requests this automatically. Direct broker launches omit it by default.
The native inference backend does not support this option. The publisher Prism
CPU backend supports the pinned adapter with either original PQ2 or the explicit
`--use-repacked-ptq1` current-weight export.
Profile names are exact; arbitrary adapters,
models, runtimes and inference overrides remain rejected.

The [launcher contract](broker-policy.md#owned-windows-development-launcher) owns
retained artifact handles, fresh protected credentials/evidence, health/authentication
and listener-PID checks, memory guards and whole-Job teardown. Reference uses an
18 GiB Job cap/22 GiB admission with the same 4 GiB physical reserve. Startup,
guard or backend failure, Ctrl-C, closed generation admission and owner loss all
stop the Job; every backend connection rechecks listener identity.

The printed evidence directory contains `frontend.key`, `backend.key` and technical
logs. A client may read `frontend.key`; never share either key or the whole
directory. This example explicitly retains caller authority. The native UI instead requires
[sandboxing](sandboxing.md); broker-only/same-user bypass prevention and actual-model
daily acceptance remain unqualified.

## Build the CPU server from source

### Experimental native Swift backend

Provision the independently released
[Prism Inference](https://github.com/symbiogenesis/prism-inference) Windows ARM64
executables, then add `--native-engine` to the owned launcher:

```powershell
$inference = ./scripts/Get-Inference.ps1 -DataDirectory $data
./scripts/Get-Inference.ps1 -DataDirectory $data -VerifyOnly
& $broker --launch-development --allow-unisolated-backend `
    --profile swift-pq2-assessment --native-engine `
    --data-directory $data --evidence-parent ([IO.Path]::GetTempPath()) `
    --frontend-port 5173 --backend-port 8081 --max-waiting 2
```

The [release lock](../config/prism-inference.lock.json) pins the source revision,
archive and every installed file by length and SHA-256. Provisioning checks the
ARM64 executable architecture and installs under
`inference/<commit>/aarch64-pc-windows-msvc/` in the external data directory.
`Get-Inference.ps1` and the launcher consume these binaries without invoking
Cargo. Updating the runtime means reviewing a new release lock and provisioning
that release. For GGUF commands using a custom data directory, set
`BONSAI_DATA_DIRECTORY` to the same absolute path, or supply `-GgufToolPath`.
The crates.io packages provide the Rust source distribution;
ordinary `cargo install` compiles that source.

Add `-IncludeCapabilities` to `Get-Inference.ps1` for a bounded model-free probe
of the verified release. It checks the reported version/native target and the
shared H0 schema, retains archive/file guards through exit and returns external
verification evidence. This optional probe uses `bonsai-tools`; supply `-ToolPath`
or a trusted `BONSAI_TOOLS_PATH` to avoid building the maintenance tool. See the
[capability contract](hardware-capabilities.md#runtime-inventory).

Run from an external native working directory. The selection accepts only the
original Swift model and verified release, with no adapter/alternate model/overrides.
It retains the executable against replacement, fixes seed 42 and requires matching
native identity before frontend readiness. Ordinary 11.5 GiB Job / 15.5 GiB admission,
4 GiB watchdog, protected keys and listener checks apply. Interrupted generation
closes admission and stops the backend; relaunch creates a fresh owned instance.

Release 0.1.6 includes reviewed source loading/logit/state/tokenizer/sampler/recovery
repairs. [The runtime contract](native-rust-engine.md) defines their exact supported
scope; [PLAN.md](../PLAN.md#runtime-and-correctness) tracks full-context, throughput
and daily-service qualification. Private child protocol alone proves neither Job
ownership nor same-user bypass prevention.

### Prepare a runtime lock update

Runtime promotion uses checked prebuilt files from Prism's `main` Foundation
workflow. Review the exact source revision, all required run results and the
Windows ARM64 bundle receipt before generating a candidate. A receipt records
identities; it does not prove that the run passed or authorize publication.

Download the ARM64 bundle ZIP and receipt from that run, and the source ZIP from
`https://github.com/symbiogenesis/prism-inference/archive/<full-commit>.zip`,
into a new system-temp lane outside Git. Use the independently reviewed revision
and run identity, rather than copying them from an unreviewed receipt:

```powershell
./scripts/New-InferenceLock.ps1 `
    -ReceiptPath (Join-Path $lane 'prism-inference-0.1.6-aarch64-pc-windows-msvc.receipt.json') `
    -ArchivePath (Join-Path $lane 'prism-inference-0.1.6-aarch64-pc-windows-msvc.zip') `
    -SourceArchivePath (Join-Path $lane 'source.zip') `
    -SourceRevision $reviewedRevision -Version 0.1.6 `
    -RunId $reviewedRunId -RunAttempt 1 `
    -OutputPath (Join-Path $lane 'prism-inference.lock.candidate.json')
```

The command uses `bonsai-tools`; supply a trusted build with `-ToolPath` or
`BONSAI_TOOLS_PATH`, or let the resolver build the maintenance CLI in system temp.
Production `Get-Inference.ps1` provisioning still requires no Rust toolchain.
The review checks receipt identities, archive and individual file hashes,
the complete executable/license inventory, ARM64 PE headers and the source
archive's commit root, package versions and compiler pin. It writes a new
external candidate and retains `candidate.lock.json` and `verification.json` in
its verification directory. It verifies ZIP contents directly without extracting
the runtime, executing the reviewed binaries, building Prism source, publishing a
release or replacing the repository's lock.

Publication requires separate authorization under the session's repository
rules. After the exact checked bundle is available at the candidate's immutable
release URL, verify those downloaded bytes and review the candidate before
replacing `config/prism-inference.lock.json`. Rebuild the broker, which embeds
that lock, provision through `Get-Inference.ps1`, and run the required checks on
the settled `main` revision. Preserve the prior release for rollback. Packaging
and hosted synthetic checks do not broaden the retained model/device evidence.

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
inference or benchmark runs; the native Rust UI is built separately from inference.

Default parallelism reserves two CPU threads and 2 GiB, budgets 1 GiB per compiler,
and caps project/shared compiler workers at eight. If memory is unavailable it
uses at most two. `-Parallel 1` through `-Parallel 12` overrides that budget.
Incremental outputs are reused only after source/configuration checks.
