# Native Rust UI and local API contract

The first-party UI must remain **entirely native Rust**: application code, state,
events, rendering integration and network clients. Native OS APIs and audited
Rust bindings are permitted; PowerShell may provision, build and launch it.

Do not add a JavaScript/TypeScript frontend, HTML/CSS UI, browser shell, WebView,
Electron or web-rendered Tauri frontend, or the inference server's browser UI.
A replacement toolkit must preserve this boundary. Interoperability belongs at
the HTTP API boundary.

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

The UI cannot load weights, choose arbitrary runtime arguments or bypass broker
artifact, model, memory, context or sampler policy. Explicit
[runtime/format and adapter choices](development-guide.md#runtime-selection) do not
establish promotion or full-model/device qualification.

### Conversation and failure semantics

| Component | Contract |
| --- | --- |
| Workspace | At most 16 conversations, each with its own draft, response settings, visible transcript and exact committed history. Transcript count/byte eviction (at most 512 messages) never changes model history. |
| Presentation | Only user/assistant content enters the selectable plain-text transcript; never interpret output as HTML. Setup/retry/transport/speech status use separate controls and bounded collapsible diagnostics. Rename/delete use centered native modals; deletion names the target and blocks background input. Cancel/Escape preserves the conversation/draft. |
| Recovery | Off by default; user-scoped DPAPI, 8 MiB bound, atomic replacement outside Git and one writer/data directory. Restore completed history without request replay, microphone or playback; keep interrupted visible text separate. [Recovery operation](development-guide.md#optional-recovery-and-preview) defines saved fields, unreadable-file preservation, explicit saving/deletion and same-user limits. |
| Stop/retry | Stop freezes queued updates and wakes network waits. Retry waits for owned-backend teardown, creates fresh credentials/diagnostics and repeats artifact, identity and 15.5 GiB admission checks. Preserve completed turns and newer drafts; restore an interrupted prompt only to an empty composer. Never replay automatically or reduce the profile. If completion wins the Stop race, the visible answer and committed history agree. |
| Rejections/failures | Recognized local input rejections restore the draft and retain readiness. Other disconnects require retry and identify no token/memory cause. [Broker policy](broker-policy.md#native-client-completion-and-recovery) defines accepted errors and completion. |
| Response settings | Thinking, output cap and adapter scale apply to the next response and every Extra effort pass; lock controls during generation. [Settings](development-guide.md#conversations-and-response-settings) defines defaults, bounds and execution; Extra effort quality gains remain unproven. |

[Find controls](development-guide.md#conversations-and-response-settings) search
visible message bodies. [Streaming resources](streaming-resources.md#native-chat-and-transcript)
defines incremental matching, highlight/underline-cache invalidation and work bounds.
`bonsai-chat.exe --preview` uses the same presentation with marked sample content
and no broker, credentials, microphone or speech service; it checks layout and
interaction only.

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
verified model files. Reviewed native GGML C++ cores infer in separate executables
to avoid mixing GGML versions. The dependency-free `bonsai-audio` codec borrows
validated mono PCM16 WAV payloads and opens no devices; callers retain their own
sample-rate, duration and error policies.

Microphone access is requested on the UI thread only after explicit action.
Denied, cancelled or unavailable access fails closed; stale permission, capture
or transcription results cannot reactivate it. Audio stays on authenticated
numeric loopback. Transcripts require review before send; speech output uses only
visible assistant answers, excluding reasoning/private drafts.

[Speech operation](development-guide.md#speech-setup-and-operation) defines
provisioning, capture lifecycle, fixed voice conditioning, mutually exclusive
talkers, budgets, service lifetime and external-service requirements (including
`consistent_voice: true` and complete-WAV fallback). The [Canary](../tools/canary-asr/README.md)
and [Qwen](../tools/qwen-tts/README.md) READMEs define companion protocols.
Service failure exposes setup guidance without substituting a recognizer or
Windows voice. Images, video and image generation remain unavailable in the
pinned Swift profile.

## Review and validation

Review native Rust presentation and protocol preservation through native builds,
focused UI/protocol checks and actual-model acceptance as appropriate. Full suites
and benchmarks stay CI-only under [validation](validation.md). Window launch and
synthetic work counts establish neither model quality, speech behavior, sustained
performance nor daily-service isolation. [PLAN.md](../PLAN.md) owns pending qualification.
