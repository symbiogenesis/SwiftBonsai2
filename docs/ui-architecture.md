# Pure-Rust native UI and local API compatibility

The first-party UI must remain a **pure-Rust native application**, for the MVP
and later product work. Application code, state, events, rendering integration
and network clients belong in Rust; native OS APIs and audited Rust bindings
are permitted. PowerShell may provision, build and launch the application.

Do not add a JavaScript/TypeScript frontend, HTML/CSS UI, browser shell, WebView,
Electron or web-rendered Tauri frontend, or adopt the inference server's browser
UI. A replacement toolkit must preserve this boundary without a browser runtime
or second frontend language. Interoperability belongs at the HTTP API boundary.

## Native MVP

`src/chat-ui/` provides the Windows `bonsai-chat` application using `egui`,
`eframe`, a native window and `wgpu`. State, widgets and HTTP clients are Rust.
The production manifest selects DX12, Vulkan and OpenGL/GLES through eframe's
`wgpu_no_default_features`, retaining upstream adapter selection and
`WGPU_BACKEND`. Browser/Metal backends are not requested; upstream test features
and installed drivers can affect backend availability. Toolkit portability does
not make Runner cross-platform: process ownership, credentials and speech remain
Windows-specific.

The dark native UI runs separately from the broker and inference worker. Network, disk,
DPAPI and speech work stay off the event loop, with bounded queues, cancellation
and joined shutdown. Windows high contrast, per-monitor scaling, labelled
AccessKit controls and keyboard navigation are supported. Full bidirectional
editing and native-control accessibility parity still require qualification.

### Launch and ownership

PowerShell bootstraps a trusted maintenance executable; Rust owns build reuse,
immutable tool snapshots and the per-user launch guard through teardown. Warm
source launches reuse digest-checked capability receipts; installed launches
verify the selected package and open the app in one native operation. See the
[development guide](development-guide.md#native-chat-app) and
[package contract](chat-packages.md) for commands and cache rules.

The launcher checks/builds presentation first. After the first UI tick, an owned
preparation worker builds/verifies the broker and speech companions and creates
fresh credentials. Drafts and recovered conversations survive preparation;
failures leave the window available. Closing cancels preparation and waits for
owned descendants. Artifact verification and memory admission precede model use.
Owned inference and speech require [native containment](sandboxing.md), with no
uncontained fallback. Microphone consent is a separate UI capability check.

The UI cannot load weights, select arbitrary runtime arguments or bypass broker
artifact, model, memory, context or sampler policy. The development guide owns
explicit PQ2/PTQ1/native-engine and adapter selections; none establishes runtime
promotion or full-model/device qualification.

### Conversation and failure semantics

The workspace holds up to 16 conversations, each with a draft, response settings,
visible transcript and separate exact committed history. Visible transcripts
retain at most 512 messages; count/byte eviction never changes model history.
Rename/delete use centered native modals; deletion names its target and blocks
background input, while Cancel/Escape preserves the conversation and draft.
Only user/assistant content appears in the transcript. Setup, retry, transport
and speech status have separate controls; bounded diagnostics are collapsible.
Model output remains selectable plain text, never interpreted HTML.

Optional recovery is off by default: user-scoped DPAPI, an 8 MiB bound, atomic
replacement outside Git and one writer per data directory. Unreadable recovery
is preserved until an explicit saving choice; disabling saving removes its copy.
Only completed history is restored to a fresh broker, without request replay,
microphone activation or playback. Interrupted visible text remains separate.
See the development guide for saved fields and same-user protection limits.

Stop freezes queued streaming updates and wakes network waits. The owned
backend tears down before **Retry model** becomes available. Retry creates fresh
credentials/diagnostics and repeats artifact, identity and 15.5 GiB admission
checks; it preserves completed turns without automatically resending a request
or reducing the profile. An interrupted prompt returns only to an empty composer,
preserving newly typed drafts. If completion wins the Stop race, the visible
answer and committed history agree. Recognized local input rejections restore
the draft and keep the service ready; unexpected disconnects require retry and
do not imply a token/memory cause.

Thinking defaults off; its explicit boolean and the bounded output/adapter-scale
settings apply to the next response and every Extra effort pass. Controls lock
during generation. Standard makes one call; experimental Extra effort makes at
most three sequential plan/draft/review calls with one deadline, stage labels
and retained drafts. Quality gains remain unproven.

Find searches Unicode case-insensitive message bodies, excluding reasoning.
Its magnifying-glass control sits beside Settings, with previous/next navigation
below the header. A bottom-center down arrow resumes following newer messages.
[Streaming resources](streaming-resources.md) owns incremental matching,
highlight/underline cache invalidation and work bounds.

`bonsai-chat.exe --preview` uses the same presentation with marked sample content
and no broker, credentials, microphone or speech service. It checks layout and
interaction, not model/speech behavior.

## Interoperability belongs at the API boundary

Use the language-independent local LLM HTTP/JSON/SSE conventions: numeric
loopback, bearer authorization, `POST /v1/chat/completions`, text messages and
`text/event-stream`. Preserve supplied assistant reasoning separately as
`reasoning_content`. [Broker policy](broker-policy.md#public-request-contract)
defines the complete supported subset, bounds, errors and native completion
semantics. A route name alone establishes neither arbitrary-server nor OpenAI
SDK compatibility; clients emitting extra fields need an explicit adapter.

Protocol changes must preserve plain-text/streaming semantics and document
unsupported features. Keep backend policy outside presentation. Compatibility
cannot bypass ownership, admission, credentials or context limits.

## Speech input and output

Rust owns companion HTTP/authentication, schema validation, WAV processing and
verified model files; reviewed native GGML C++ cores perform inference in
separate executables, avoiding mixed GGML versions. The shared dependency-free
`bonsai-audio` codec borrows validated mono PCM16 WAV payloads and opens no
devices; callers retain their own sample-rate, duration and error policies.
The [development guide](development-guide.md#speech-setup-and-operation) owns
provisioning, controls, voice conditioning, resource budgets and service lifetime.
The [Canary](../tools/canary-asr/README.md) and
[Qwen](../tools/qwen-tts/README.md) READMEs own their protocols.

Microphone access is requested on the UI thread only after explicit action.
Denied/cancelled/unavailable access fails closed; stale permission, capture or
transcription results cannot reactivate it. Five-second mono 16 kHz clips stay
in memory; capture pauses during transcription and retains its recorder only
for the same enabled, permitted generation. Disabling/revoking closes it.
Transcripts return to the composer for review, never automatic send. Audio uses
authenticated numeric loopback and is not sent to a cloud service.

Only visible assistant answers become plain-text speech; reasoning and private
working drafts are excluded. Profile-based fixed reference conditioning is
reused across segments/messages, with only one talker resident at a time.
Independent companions must advertise `consistent_voice: true`; complete-WAV
fallback is allowed when streaming is unavailable. Missing services expose
setup guidance without substitute recognizers/Windows voices. Images, video
and image generation remain unavailable in the pinned Swift profile.

## Review and validation

Review both boundaries: presentation stays native Rust, and the adapter preserves
the documented protocol. Use native builds, focused UI/protocol checks and
actual-model acceptance as appropriate. Full suites and all benchmarks remain
CI-only under [validation](validation.md). Window launch or synthetic work-count
checks do not establish model quality, speech behavior, sustained performance
or daily-service isolation; outstanding qualification stays in [PLAN.md](../PLAN.md).
