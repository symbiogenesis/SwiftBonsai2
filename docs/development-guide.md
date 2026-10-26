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
[Prism Inference runtime](https://github.com/symbiogenesis/prism-inference) is a
separate Rust project. Its initial supported model path is Swift/Bonsai; broader
Prism inference support remains future work.
[Adreno](adreno-gpu-plan.md) and [Hexagon](hexagon-npu-plan.md) integration remain
planned. Current launch policies select CPU execution.

A native Rust chat app is available through `Start-Chat.ps1`.
Daily-service acceptance remains pending. Development ownership and memory
controls do not qualify restricted inference identity, filesystem/egress
isolation or same-user backend bypass prevention.

## Native chat app

Use native Windows ARM64 PowerShell 7.6+, the pinned Rust toolchain, an existing
Swift PQ2 model/runtime installation and at least **16 GiB of available RAM**.
For a new installation, provision the model/runtime with Swift's assessment lock
and the pinned abliteration adapter:

```powershell
$data = Join-Path $env:LOCALAPPDATA 'SwiftBonsai2'
./scripts/Get-Engine.ps1 -DataDirectory $data -LockPath ./config/swift-assessment.lock.json
./scripts/Get-Model.ps1 -DataDirectory $data -LockPath ./config/swift-assessment.lock.json
./scripts/Get-Adapter.ps1 -DataDirectory $data
./scripts/Start-Chat.ps1 -DataDirectory $data
```

The native `egui`/`eframe` window follows the lasting
[UI architecture policy](ui-architecture.md): application UI code stays in Rust;
native system bindings are permitted, while WebViews, JavaScript, TypeScript,
HTML and CSS are excluded. `wgpu` renders the Rust-drawn controls. The app
starts and owns the development broker, uses a dark appearance, shows startup
status and streams responses. If startup
reports insufficient RAM, free memory and select **Retry model**. Each retry
starts a fresh owned broker and repeats artifact verification and the standard
16 GiB memory admission check. Conversation history stays in memory.
Stopping an active response closes backend admission and stops its owned broker
and inference process. Select **Retry model** to start a fresh owned session,
repeat artifact/memory admission, and continue with completed conversation turns
retained in memory. Retry never resends a prompt automatically. Uncertain
transport/protocol failures also require this explicit retry. Closing the window
stops its owned broker and inference process.

Thinking is off by default. Turn on **Settings → Response → Thinking** to enable
it for the next response and every Extra effort pass. Each conversation keeps
its own choice; saving conversations also saves that choice. Older saved
conversations and newly created conversations start with thinking off.

For a model-free visual check, build the chat executable into an external temp
directory and open its sample preview:

```powershell
$previewTarget = Join-Path ([IO.Path]::GetTempPath()) ('Bonsai-Preview-' + [guid]::NewGuid().ToString('N'))
cargo build --locked --release --package bonsai-chat --target-dir $previewTarget
Start-Process -FilePath (Join-Path $previewTarget 'release/bonsai-chat.exe') -ArgumentList '--preview' -WorkingDirectory ([IO.Path]::GetTempPath())
```

The preview is visibly marked and uses the same UI with sample content. It
starts no broker or speech services and does not access credentials or a
microphone. Use it to inspect resizing, display scaling, selection and keyboard
navigation. The preview does not establish real-model streaming or audio
behavior. Although the toolkit supports Windows, macOS and Linux, this runner's
process ownership, credentials and speech integrations remain Windows-specific.

The conversation shows only user and assistant content. Model readiness,
generation errors, retry guidance and speech status appear outside the chat;
application diagnostics are available in a separate collapsible view.

The sidebar keeps up to 16 conversations, each with its own unsent draft and
response settings. Start a **New conversation** without clearing the previous
one, select a chat to resume it, or open the **⋯** menu beside any conversation
to rename, copy or delete that conversation. The deletion confirmation names the
conversation; **Keep conversation** or Escape cancels. Deleting the last chat
leaves a fresh empty conversation ready to use.
Switching is available while the model is ready and idle. You can draft a
message while the model prepares; sending waits for readiness.

Long or pasted drafts scroll inside the composer, keeping the conversation and
Microphone and Send/Stop controls visible. Each Swift Bonsai response has its
own speaker button at the lower right of its bubble. **Ctrl+End** moves to the
end of a draft; **Shift+Enter** inserts a new line and **Enter** sends when ready.

Use **Find** or **Ctrl+F** to search visible message text, **F3** / **Shift+F3**
for the next / previous match, and **Latest** to return to the newest message.
While Find has focus, **Enter** / **Shift+Enter** moves to the next / previous
match. **Escape** closes Find and clears its highlights. A second **Escape**
stops an active response or speech.
Search is case-insensitive, supports Unicode, and excludes private thinking.
The search controls stay above the scrolling transcript.

Conversations remain in memory by default. In **Settings**, enable **Save
conversations on this device** for recovery across launches. The app encrypts
the workspace with Windows user-scoped DPAPI in `DataDirectory/chat-recovery.dpapi`;
no plaintext conversation is written to that file. Saving includes unsent drafts,
response settings, voice description, transcript and exact completed model history.
Interrupted text stays visible after recovery and is never promoted to completed
history or automatically sent. The microphone remains off. Turning saving off
removes the saved copy while keeping the current workspace open. Recovery is
bounded to 8 MiB and only one window can write it for each data folder. Corrupt or
unreadable recovery is preserved with a visible warning until you explicitly
enable saving to replace it. Windows user protection does not isolate data from
other processes running as that user, and deletion is not secure erasure or backup
removal. Keep the model data folder private and outside Git.

**Mode** defaults to **Standard**, which makes one completion request.
**Extra effort (experimental)** makes up to three sequential planning, drafting
and review requests to the same model, with visible stage labels and a shared
ten-minute deadline. It keeps complete drafts visible for comparison. A
truncated revision does not replace a complete draft. Extra passes take longer;
quality improvements remain unproven.

**Output cap** defaults to 4,096 tokens and offers 512, 1,024 and 2,048 tokens.
Every completion cap includes reasoning, within the fixed 8,192-token total
context. The broker also reserves 64 tokens for template accounting. These
controls do not estimate an exact local token count or discard history; the
broker still performs exact admission for every call. A conversation that exceeds
this combined allowance gets a specific message to reduce the output cap or start
a new conversation. The prompt returns to the composer for editing, and the model
stays ready without requiring a retry. Reaching the output cap during an otherwise
complete reply is a separate outcome and lets you ask Swift to continue.

HTTP is used only between the UI, broker and model server on this computer,
through numeric loopback (`127.0.0.1`); chat inference does not send the conversation
to a website. An unexpected local connection closure requires **Retry model** and
does not by itself establish a token-limit or memory failure. Completed turns stay
in memory and the interrupted prompt is restored for review.

The window maps Windows high-contrast colors to its controls, uses `eframe`
display scaling and exposes control labels through AccessKit. Conversation
text remains selectable and can be copied explicitly. Check selection while
streaming and scrolling long conversations; full bidirectional editing and
native-control accessibility parity are not established. The window
distinguishes an answer that reached its token limit from a naturally completed
answer. Select a response's **speaker button** to play that response using
Qwen3-TTS 1.7B VoiceDesign. The active button becomes **Stop** to cancel synthesis
and playback; selecting another response switches playback to it. Each button
becomes available when its response finishes; older responses remain available
while another response is generating. Reasoning is
not read aloud. Edit **Voice profile** to
describe a voice, for example "A warm, low-pitched narrator with a calm pace and
clear consonants." No voice preset or reference recording is required.

Enable **Read aloud as responses arrive** beside the composer to read new
responses while Swift writes them. The choice is off by default and belongs to
the current conversation. Speech starts in short sentence or word chunks and may
lag behind the text while Qwen loads or synthesizes. **Stop speech** cancels the
current reading without stopping text generation. Selecting any completed
response's speaker button takes over playback and pauses automatic reading until
the next prompt. Disable the checkbox to stop automatic reading; manual replay
remains available. Extra effort reads its visible stages and cancels queued audio
at each stage change. Optional conversation saving preserves the checkbox choice
without replaying anything on restore.

The app automatically provisions and configures both native ARM64 speech
companions. Canary uses the selected NVIDIA Canary Qwen 2.5B model and Qwen uses
the selected 1.7B VoiceDesign model. Rust implements the services, authentication,
request validation, WAV processing and process ownership. Reviewed native GGML
C++ engines perform inference. GPU/NPU acceleration is not enabled.

The three pinned Q8_0 GGUF files total approximately 4.8 GiB and live below
`$data/speech/native/` in revision-specific directories. Exact size and SHA-256
verification rejects altered caches and reuses valid files. Interrupted downloads
resume. Existing original snapshots and old private runtimes are preserved.
Download without compiling or launching:

```powershell
$speech = ./scripts/Start-Chat.ps1 -DataDirectory $data -SpeechModelsOnly
$speech.CanaryModelPath
$speech.TtsModelPath
$speech.TtsCodecPath
```

`Get-NativeSpeechModels.ps1 -DataDirectory $data -Model Canary` or `-Model QwenTts`
selects one engine's files. `Get-SpeechModels.ps1` remains an explicit utility for
original reference snapshots; the app no longer consumes those snapshots.

Native artifact planning uses `bonsai-tools` with the crates' compiled model
locks. Select a trusted packaged tool with `BONSAI_TOOLS_PATH` or the provisioning
script's `-ToolPath`; otherwise the shared resolver builds the maintenance CLI
in system temp. This also applies to model-only provisioning. It never loads a
speech model or builds the speech engines.

Native builds require the pinned Rust compiler and Visual Studio 18 ARM64 C++
tools/Windows SDK. The provisioner downloads pinned LLVM ARM64 and native engine
source archives into external storage. It changes only child compiler settings,
with no global installation or profile modification. Build outputs and evidence
stay outside Git. `-AsrPath` and `-TtsPath` can supply already built executables;
the service still verifies pinned model files when loading.

Canary loads after the first permitted microphone clip, stays warm while the
microphone generation remains enabled, and stops when input is disabled or
cancelled. Read Aloud releases input before loading Qwen. Qwen generates bounded
256-character segments sequentially, streaming PCM through bounded native playback
while generation continues. Older independently managed companions retain WAV
playback with bounded prefetch. After successful reading, an owned Qwen service
stays warm for up to 30 idle seconds. Stop speech, failure, transcription and
shutdown release it immediately; the memory monitor remains active while idle.
Both speech services use separate owned Windows Jobs. Admission requires 9 GiB
available; an independent monitor stops only speech below 5 GiB. The Job commit
ceiling is 16 GiB. Swift's 16 GiB startup admission and 4 GiB running reserve are
unchanged. Resource failures preserve text chat and the completed answer.

TTS uses native ARM64 CPU inference. Its default thread budget reserves two of
the available logical CPUs instead of assuming SMT and halving the count. Use
`-TtsThreads 6` (or another count from 1 through 256) to select an explicit
budget; zero selects automatic sizing. More threads do not guarantee higher
throughput. Compare thread counts with the CI evaluation workflow.

Q8_0 remains the default voice-design profile. To evaluate the smaller pinned
Q4_K_M talker, retaining the Q8_0 codec:

```powershell
./scripts/Start-Chat.ps1 -DataDirectory $data -TtsQuantization Q4_K_M
./scripts/Get-NativeSpeechModels.ps1 -DataDirectory $data -Model QwenTts -TtsQuantization Q4_K_M
```

Each profile has its own immutable artifact identity and the service verifies
the selected profile before loading. Q4 is an explicit quality/speed tradeoff;
it has not been promoted as equivalent to Q8. Streaming reduces time to first
sound, prefetch reduces segment gaps, and warm reuse avoids repeated model
verification/loading. These changes do not establish a throughput improvement.

`-NoTts` skips automatic TTS setup; `-NoSpeechModels` skips all automatic speech
setup and downloads. Neither downloading nor compiling opens a microphone.
For independently managed services, pass `-CanaryKeyFile C:\Speech\canary.key`
and/or `-TtsKeyFile C:\Speech\qwen-tts.key`. Ports default to 8766/8767 and must
be distinct from one another and the chat ports. Key-only services are not owned
by the app. Both authenticated capability responses are checked, and no fallback
recognizer or Windows voice is substituted. See the
[Canary](../tools/canary-asr/README.md) and [Qwen](../tools/qwen-tts/README.md)
protocol contracts. Voice descriptions guide generation without guaranteeing
persistent speaker identity between segments. Native actual-model smoke checks
establish one working path, not a measured performance gain.

The visible **Microphone: off** button explains speech setup when Canary is not
configured or ready. With Canary ready, selecting it requests Windows microphone
permission before recording and changes the button to **Microphone: on**.
The microphone remains off unless explicitly enabled, and capture pauses while
each bounded five-second clip is transcribed. While enabled, the initialized
recorder is reused after transcription instead of initializing the device for
every clip. Fixed clip boundaries can still divide words; this is not streaming
recognition or voice-activity detection. Selecting **Microphone: on** to turn it off, Send, New conversation,
read-aloud or closing the window stops capture and cancels pending input.
Transcripts appear in the composer for review; they are never sent automatically.
Permission denial or a missing/not-ready Canary service keeps capture off.
Each response's **speaker button** explains missing Qwen service setup when it
cannot start. Speech controls remain visible and actionable while
idle; they do not silently hide setup requirements behind greyed-out buttons.
The native UI does not download Canary weights or substitute another recognizer. The
companion's supported runtime and memory requirements are separate from Swift's
16 GiB admission check; native Windows ARM64 Canary inference is not yet
qualified. Qwen uses the isolated x64 environment because the official
torchaudio package has no Windows ARM64 wheel. UI/protocol tests do not establish
real-model audio quality or latency.

`Start-Chat.ps1` checks and builds the chat app and broker in a reusable external
Cargo cache under `DataDirectory/build/chat/`, separated by checkout and pinned
toolchain. Cargo checks source/dependency freshness on each launch; output hash
receipts detect missing or changed cached executables and force a rebuild.
Pass absolute `-ChatPath` and `-BrokerPath` values to select existing builds.
A fresh system-temp lane retains compiler output in `chat-build.log` and app
output in `chat.log`. `Start-Chat.ps1` is the supported launcher.
The pinned abliteration LoRA is enabled at scale **1.3** by default. In Settings,
adjust **Abliteration scale** from 0 to 2 in 0.1 steps; the choice applies to the
next response and every Extra effort pass without restarting the model. Zero
disables the correction for a response. Optional conversation saving preserves
the selected scale; older saved conversations use 1.3. This experimental default
does not establish effectiveness or output quality on Swift. Pass
`-NoAbliterateLora` to run without it; direct `bonsai-chat` launches accept
`--no-abliterate-lora`. Missing or altered adapter bytes fail startup with
provisioning guidance; there is no automatic download or fallback.
`-FrontendPort` and `-BackendPort` select distinct local API ports when the
defaults are occupied. The script provisions speech model files by default; the
Swift model and CPU runtime must already be provisioned. It
retains credentials and technical evidence in external temp storage. Do not
share those credentials or the complete evidence directory.

The app uses the broker's documented strict subset of OpenAI-style JSON/SSE
chat conventions. Unsupported fields remain rejected; this is not a claim of
drop-in compatibility with every OpenAI client.

The app uses the pinned Prism/llama CPU backend with original Swift PQ2 weights
and the pinned adapter unless opted out. This development default does not
qualify the [standard LoRA profile](swift-lora.md#required-standard-profile-qualification).
The separate native inference release `0.1.3` supports the selected thinking mode
and is available through the opt-in `--native-engine` path. Thinking is off by
default; enable it under **Settings → Response** for the next response.
Quality, throughput and daily-service qualification remain pending.

Pass `Start-Chat.ps1 -UseRepackedPtq1` to explicitly select only the verified
current-weight PTQ1 export described in [the memory assessment](swift-memory.md).
Direct `bonsai-chat` and development broker launches accept
`--use-repacked-ptq1`. Missing or changed PTQ1 bytes fail verification; the app
does not substitute the older publisher PTQ1 checkpoint or silently fall back.
The pinned adapter remains available with the same opt-out and scale setting.
The native Rust inference service rejects this option until its separate
end-to-end PTQ1 qualification is complete. See [PTQ1 readiness](ptq1-readiness.md)
for the small decision screen and remaining correctness/quality work.

## Planned X1/X2 configuration

The recorded development machine is a 32 GB Snapdragon X Elite X1E-78-100.
The ultimate target covers Snapdragon X1 and X2 Windows ARM64 systems with
[automatic hardware profiles](hardware-profiles.md). That selector is planned;
current released launch policies remain explicit CPU profiles and provide no
automatic generation detection or GPU/NPU placement.

The planned Auto mode will identify actual CPU/OS capabilities, GPU/NPU and
driver/runtime compatibility, then select a qualified bounded configuration for
the device, workload and interactive or battery objective. Prefill and decode
may use different approved placements and CPU settings when the complete
state-transfer cost and numerical contract are qualified. Admission uses actual
available RAM and device budgets, rather than assuming every X1 or X2 has the
recorded machine's capacity.

Unknown or unsupported hardware will select a supported baseline CPU profile
with a recorded reason, or refuse admission when its requirements cannot be
met. Hardware names and advertised TOPS alone will not enable acceleration.
The CPU MVP and existing model, context, LoRA and service acceptance requirements
remain the immediate delivery scope.

## Inventory and provisioning

Choose an absolute local data directory outside Git on a hard-link-capable
filesystem such as NTFS. Provisioning rejects junctions and device paths.

The default data directory keeps the `SwiftBonsai2` name for compatibility with
existing installations. It is separate from the `SwiftBonsai2Runner` checkout.

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
available-RAM admission including the 4 GiB reserve. Add `--use-abliterate-lora`
to load the pinned adapter and enable its bounded per-request scale with the
Prism CPU server (default 1.3); the demo
requests this automatically. Direct broker launches omit it by default.
The native inference backend does not support this option. The publisher Prism
CPU backend supports the pinned adapter with either original PQ2 or the explicit
`--use-repacked-ptq1` current-weight export.
Profile names are exact; arbitrary adapters,
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

Run the broker from an external native working directory as above. The
selection requires the original pinned Swift model and verified Prism Inference
release; it accepts no adapter, alternate model or inference overrides.
The separately installed server executable is verified against the release lock
and retained against replacement. Its child process
receives the existing 12 GiB Job, 16 GiB available-memory admission, 4 GiB
reserve watchdog, protected keys and listener-PID checks. The seed is fixed at
42. Native identity must match before the frontend opens. Interrupted frontend
generation closes admission and stops the owned backend; restart the launcher
to establish a fresh owned instance.

This is an experimental path. Bounded source-runtime checks qualify actual model
loading, native logit/state parity, prompt/tokenizer and seeded sampler parity,
and owned HTTP recovery. The provisioned `0.1.3` release includes those source
repairs and pins the reviewed binary and source identities. See the
[MVP checklist](prism-upstream-todo.md) for supported scope and remaining release
qualification. Full-context behavior, throughput and daily-service acceptance
remain open. The server's private child protocol alone does not establish Job
ownership or prevent same-user bypass.

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
    -ReceiptPath (Join-Path $lane 'prism-inference-0.1.3-aarch64-pc-windows-msvc.receipt.json') `
    -ArchivePath (Join-Path $lane 'prism-inference-0.1.3-aarch64-pc-windows-msvc.zip') `
    -SourceArchivePath (Join-Path $lane 'source.zip') `
    -SourceRevision $reviewedRevision -Version 0.1.3 `
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
