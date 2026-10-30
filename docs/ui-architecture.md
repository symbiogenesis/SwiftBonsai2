# Pure-Rust native UI and local API compatibility

The first-party UI must remain a **pure-Rust native application**. Application
code, state, events, rendering integration and network clients belong in Rust;
native OS APIs and audited Rust bindings are permitted. PowerShell may provision,
build and launch it.

Do not add a JavaScript/TypeScript frontend, HTML/CSS UI, browser shell, WebView,
Electron or web-rendered Tauri frontend, or adopt the inference server's browser
UI. A replacement toolkit must preserve this boundary without a browser runtime
or second frontend language. Interoperability belongs at the HTTP API boundary.

## Native application

`src/chat-ui/` provides the Windows `bonsai-chat` application using `egui`,
`eframe`, a native window and `wgpu`. State, widgets and HTTP clients are Rust.
The production manifest selects DX12, Vulkan and OpenGL/GLES through eframe's
`wgpu_no_default_features`, retaining upstream adapter selection and
`WGPU_BACKEND`. Browser/Metal backends are not requested; upstream test features
and installed drivers can affect backend availability. Toolkit portability does
not make Runner cross-platform: process ownership, credentials and speech remain
Windows-specific.

The dark UI runs separately from the broker/inference worker. Network, disk,
DPAPI and speech stay off the event loop, with bounded queues, cancellation and
joined shutdown. Windows high contrast, per-monitor scaling, labelled
AccessKit controls and keyboard navigation are supported. Full bidirectional
editing and native-control accessibility parity still require qualification.

### Launch and ownership

PowerShell bootstraps a trusted maintenance executable; Rust owns build reuse,
immutable tool snapshots and the per-user launch guard through teardown.
The [development guide](development-guide.md#source-launch-caches-and-runtime-selection)
owns source-launch receipts/cache rules; the [package contract](chat-packages.md#launch-without-a-checkout)
owns verified installed launches.

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

| Component | Contract |
| --- | --- |
| Workspace | Up to 16 conversations, each with draft, response settings, visible transcript and separate exact committed history. Transcript count/byte eviction (at most 512 messages) never changes model history. |
| Presentation | Only user/assistant content enters the transcript. Setup/retry/transport/speech status use separate controls; bounded diagnostics are collapsible. Output is selectable plain text, never interpreted HTML. Rename/delete use centered native modals; deletion names its target and blocks background input. Cancel/Escape preserves the conversation/draft. |
| Recovery | Off by default; user-scoped DPAPI, 8 MiB bound, atomic replacement outside Git and one writer/data directory. A fresh broker restores only completed history, without request replay, microphone or playback. Interrupted visible text stays separate. [Recovery operation](development-guide.md#optional-recovery-and-preview) owns saved fields, unreadable-file preservation, explicit saving/deletion choices and same-user protection limits. |
| Stop/retry | Stop freezes queued stream updates and wakes network waits. **Retry model** waits for owned-backend teardown, creates fresh credentials/diagnostics and repeats artifact/identity/15.5 GiB admission checks. Completed turns survive; no automatic resend or reduced profile. An interrupted prompt returns only to an empty composer, preserving newer drafts. If completion wins the Stop race, visible answer and committed history agree. |
| Rejections/failures | Recognized local input rejections restore the draft and retain readiness. Unexpected disconnects require retry and do not identify a token/memory cause. [Broker policy](broker-policy.md#native-client-completion-and-recovery) owns the exact error/completion rules. |
| Response settings | The explicit Thinking boolean, output limit and adapter scale apply to the next response and every Extra effort pass; controls lock during generation. [Settings](development-guide.md#conversations-and-response-settings) owns bounds, defaults and Standard/experimental Extra effort execution. Extra effort quality gains remain unproven. |

Find controls follow the [development guide](development-guide.md#conversations-and-response-settings).
[Streaming resources](streaming-resources.md#native-chat-and-transcript) owns
incremental matching, highlight/underline cache invalidation and work bounds.

`bonsai-chat.exe --preview` uses the same presentation with marked sample content
and no broker, credentials, microphone or speech service. It checks layout and
interaction, not model/speech behavior.

## Interoperability belongs at the API boundary

Use local LLM HTTP/JSON/SSE conventions: numeric loopback, bearer authorization,
`POST /v1/chat/completions`, text messages and `text/event-stream`. Preserve supplied
assistant reasoning separately as `reasoning_content`. [Broker policy](broker-policy.md#public-request-contract)
owns the supported subset, bounds, errors and native completion semantics.
Route names alone establish neither arbitrary-server nor OpenAI SDK compatibility;
clients emitting extra fields need an explicit adapter.

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
transcription results cannot reactivate it. [Speech operation](development-guide.md#speech-setup-and-operation)
owns in-memory clip format, capture/transcription lifecycle and explicit review
before send. Audio uses authenticated numeric loopback, never a cloud service.

Speech uses only visible assistant answers; reasoning/private drafts are excluded.
The development guide owns fixed profile conditioning, mutually exclusive talkers,
external `consistent_voice: true` capabilities, complete-WAV fallback and missing-service
guidance. Service failure never silently substitutes another recognizer or a Windows
voice. Images, video and image generation remain unavailable in the pinned Swift profile.

## Review and validation

Review native Rust presentation and protocol preservation through native builds,
focused UI/protocol checks and actual-model acceptance as appropriate. Full suites
and benchmarks stay CI-only under [validation](validation.md). Window launch and
synthetic work counts establish neither model quality, speech behavior, sustained
performance nor daily-service isolation. [PLAN.md](../PLAN.md) owns pending qualification.
