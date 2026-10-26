# Pure-Rust native UI and local API compatibility

The first-party Swift Bonsai 2 UI must remain a **pure-Rust native application**.
This is a lasting architecture requirement for the MVP and subsequent product
work. UI application code, state, event handling, rendering integration and
network clients belong in Rust. Native operating-system APIs and audited Rust
bindings to those APIs are permitted.

Do not introduce a JavaScript or TypeScript frontend, HTML/CSS application UI,
browser shell, WebView, Electron, or a Tauri HTML frontend. Do not adopt the
inference server's browser UI as the first-party interface. A future UI toolkit
must preserve the Rust application boundary and must not require a browser
runtime or a second frontend language. PowerShell may orchestrate builds and
startup; it does not implement the UI.

## Native MVP

`src/chat-ui/` provides the `bonsai-chat` Windows application. Its presentation
uses `egui` and `eframe`, with a native window and `wgpu` rendering; application
state, widgets and the HTTP client are Rust. It has no browser runtime or web
frontend. The UI runs separately from the broker and inference process;
network work runs off the native event loop. The toolkit supports other desktop
platforms, but the broker ownership, credentials and speech integrations remain
Windows-specific; this migration does not establish a cross-platform runner.

The window uses a dark palette, maps Windows high-contrast settings to its
Rust-drawn widgets and keeps an in-memory workspace of up to 16 conversations.
Each conversation retains its draft, response settings, visible transcript and
separate exact committed model history. Switching while idle never promotes an
interrupted turn to model history. Each conversation row has a labelled actions
menu for rename, copying and deletion. Rename and delete use centered native
modals; deletion names its target, blocks background input and shows completion
feedback. Cancel and Escape preserve the conversation and its draft.
Optional recovery uses Windows user-scoped DPAPI through the audited credentials
boundary, with encrypted atomic file replacement outside Git and a single-writer
file lock. Saving is off by default; disabling it removes the saved copy. Only
completed history is restored to a fresh broker, with no automatic request replay
or microphone activation. Disk and encryption work run on a separate bounded worker.
The encrypted workspace is limited to 8 MiB; unreadable recovery remains untouched
until an explicit user saving choice. `eframe` handles
window input and per-monitor display scaling. Rust-drawn controls expose labels
through AccessKit and support keyboard navigation.
The conversation contains only user and assistant content. Startup, retry,
transport and speech status use separate controls; bounded application
diagnostics have their own collapsible view rather than appearing as chat
messages. The transcript supports selection and explicit copying without
interpreting model output as HTML. Find highlights Unicode case-insensitive
message-body matches, with previous/next navigation and jump-to-latest controls
outside the transcript scroller. Reasoning is excluded from search. Focused
interaction tests cover streaming selection, search and startup drafting.
Toolkit support for non-Latin glyphs does not imply full
bidirectional text editing or complete native-control accessibility parity.
An insufficient-memory startup failure offers **Retry model** after RAM is
freed. Retrying creates a fresh broker attempt with new diagnostic storage and
repeats the ordinary artifact, identity and 16 GiB admission checks. There is no
automatic retry or reduced-memory profile.

Stop and uncertain request failures also leave the window available for an
explicit **Retry model**. The client tears down the owned backend before a fresh
attempt, retains completed conversation turns, and never automatically replays
an interrupted request. Stop immediately freezes queued streaming updates;
network waits wake on the cancellation command. Requested stops use neutral
status messages while the backend is released, with Retry available only after
teardown. An interrupted prompt returns to an empty composer without replacing
a newly typed draft. If a fully completed turn wins the stop race, its visible
answer stays consistent with committed history. Local input-limit rejections restore the composer.
Response settings expose the fixed 8,192-token context bound and select a
reasoning-inclusive completion cap up to 4,096 tokens. Thinking is off by default;
the Response settings checkbox enables it for the next response and every Extra
effort pass. Each conversation preserves its choice, including optional encrypted
recovery; older saved conversations default to off. Response controls lock during
generation. Standard remains the
default single-call workflow; opt-in Extra effort performs at most three
sequential plan/draft/review calls with a shared deadline, stage labels and
retained drafts. Its quality gains remain unproven.

Exact context-limit rejections return a bounded, framed local error before
generation begins. The UI restores the prompt with specific output-cap/context
guidance and leaves the model ready. Unexpected disconnects describe the local
connection and do not guess at a token or memory cause. HTTP communication here
uses numeric loopback on the same computer, rather than an Internet service.

The app uses the existing pinned Swift PQ2 CPU backend through the broker.
`Start-Chat.ps1 -UseRepackedPtq1` explicitly selects the verified current-weight
PTQ1 artifact on that same backend. PQ2 remains the default. This selection
retains the memory admission, running reserve, conversation and retry policies;
it does not qualify equivalent outputs, speed or adapter correctness.
Its pinned abliteration LoRA is enabled by default at scale **1.3**, with an
explicit `-NoAbliterateLora` opt-out; the broker verifies and holds the selected
adapter. Settings exposes a scale from 0 to 2 in 0.1 steps, applied to the next
response without restarting the model. Zero disables the correction for that
response. The selected scale also applies to every Extra effort pass. Optional
conversation recovery preserves it; older saved conversations default to 1.3.
The launch-time opt-out disables the scale control and omits adapter requests.
The UI does not load model weights, select arbitrary runtime arguments, bypass
artifact verification or change model, memory, context or sampler policy.
Thinking selection uses the broker's explicit boolean request option.
Completion selection stays within the broker's existing request bound.
A native UI does not by itself qualify runtime correctness, LoRA,
creative quality, performance or daily-service isolation.

Use the [development guide](development-guide.md) for launch commands and
[broker policy](broker-policy.md) for the exact supported request contract.

`bonsai-chat.exe --preview` opens the same presentation with sample content and
an explicit preview indicator. It does not start a broker, access credentials,
initialize a microphone or start speech services. Preview checks establish UI
layout and interaction only; they do not qualify model or speech behavior.

## Interoperability belongs at the API boundary

A Rust-only UI remains compatible with local LLM conventions through the
language-independent HTTP API, rather than through a browser frontend.
The compatibility surface is an OpenAI-style `POST /v1/chat/completions`
endpoint, JSON text messages, bearer authorization and incremental
`text/event-stream` Server-Sent Events (SSE). Clients retain supplied assistant
reasoning separately from visible answer text and replay it using
`reasoning_content` when supported by the contract.

The present broker implements a deliberately strict subset. Its complete public
request contains `messages`, `max_completion_tokens`, `stream`, optional
`enable_thinking` (default false), and an optional
fixed `model: "bonsai"` alias. An owned Swift backend with the verified pinned
adapter also accepts the bounded `abliteration_scale` scalar. Content accepts
strings or bounded standard text parts; arbitrary model selection, adapters,
sampling controls, tools, media and unknown fields remain unsupported.
Clients must use the exact loopback Host, preserve request framing, respect
bounds and distinguish framed transport completion from a successful terminal
generation event. See the [public request contract](broker-policy.md#public-request-contract).

The native SSE client accepts UTF-8 BOM and CR/LF/CRLF line endings, including
fragmented input, and UTF-8 charset parameters on the event-stream media type.
It requires a terminal `finish_reason` plus `[DONE]` and completed HTTP framing.
A `length` result retains partial text and displays the token-limit outcome;
unsupported terminal reasons, error events and missing completion reject.
Before generation, the broker may instead return HTTP 413 with the fixed JSON
code `context_length_exceeded`. Only a fully framed, bounded, recognized error
is treated as a recoverable context rejection; other failures require retry.

## Speech input and output

The PowerShell chat launcher provisions the pinned Canary Qwen 2.5B and Qwen3-TTS
1.7B VoiceDesign Q8_0 GGUF files using immutable URLs and exact size/SHA-256
verification. `-SpeechModelsOnly` provisions files without building or launching the app;
`-NoSpeechModels` skips automatic speech setup. The launcher builds native ARM64
Rust companions and creates separate protected credentials. Rust owns their
HTTP/authentication, schema validation, WAV processing and model-file verification.
Reviewed native GGML C++ cores implement inference. Separate executables avoid
mixing different native GGML versions in one process.

The UI owns speech through separate Windows Jobs. Canary loads only after an
explicitly enabled and permitted microphone captures its first clip, stays warm
for that enabled generation, and stops on input cancellation. Read Aloud releases
Canary before loading Qwen; transcription releases Qwen first. Output uses
sequential 256-character synthesis requests with bounded streaming playback and
prefetch while earlier audio plays. After successful reading, its owned process
stays warm for at most 30 idle seconds, retaining resource monitoring. Stop,
failure, microphone transcription and shutdown release it immediately. Both
services require 9 GiB available at startup and use an independent 5 GiB physical
reserve with a 16 GiB commit ceiling. Speech failure preserves text chat and the
completed answer. Swift's existing admission and running reserve remain unchanged.
`-NoTts` skips automatic TTS setup. Explicit `-CanaryKeyFile`/`-TtsKeyFile` select
independently managed companions instead. Runtime orchestration stays outside
presentation controls.

Speech input uses the authenticated numeric-loopback
[Canary companion](../tools/canary-asr/README.md). The Rust UI requests the Windows
`microphone` capability on its UI thread before
audio-only capture. Denied, cancelled or unavailable access fails closed. The
microphone is off initially; only the explicit microphone toggle can start it.
Disabling it, sending a prompt, starting read-aloud, creating a conversation or
closing the window cancels capture and invalidates outstanding session results.
Late permission/capture/transcription completions cannot reactivate capture.

Bounded five-second mono 16 kHz PCM WAV clips stay in memory in the client. Capture pauses
while Canary transcribes a clip, retaining the initialized recorder only for the
same enabled, permitted generation. Disabling or revoking access closes it.
Recognized text returns to the composer for
review, with no automatic send. The companion uses the actual pinned Canary
weights and no recognition fallback; its native engine is separate from
the Swift CPU backend. No audio is sent to a cloud service by this integration.

Each assistant bubble has a native speaker button in its lower-right corner.
It plays that response independently, changes to a stop control while active,
and replaces playback when another response is selected. Completed responses remain
available during generation of another response. Streaming and empty responses
cannot start manual playback; an active streaming response's stop button cancels it.
Read-aloud uses the separate, authenticated local
[Qwen3-TTS 1.7B VoiceDesign companion](../tools/qwen-tts/README.md). The
**Voice profile** text setting describes the desired voice in ordinary language;
no preset, speaker file or reference recording is required. It applies to the
next read-aloud request or streaming response. Only visible assistant answers are synthesized,
as plain text; reasoning is excluded. Long answers are split into bounded text
segments and synthesized sequentially with the same description. Native PCM
playback overlaps synthesis through a bounded queue; older independently managed
companions retain complete-WAV playback. Stop speech cancels synthesis and native
audio playback. Missing/not-ready TTS prevents
output independently of Canary input, with no Windows-voice fallback. Idle
speech controls remain visible and show native setup/prerequisite guidance when
selected; a missing service never activates the microphone or substitutes output. Images,
video and generated images remain unavailable in the pinned Swift profile.

The conversation-wide **Read aloud as responses arrive** checkbox is off by
default. It feeds visible answer deltas into a separate incremental speech
session while the model is still generating. Sentence boundaries and bounded
256-character chunks release text promptly; a short timeout releases accumulated
words during a pause, and completion flushes the final tail. Synthesis and playback
use bounded queues, so speech can lag behind text according to local model speed.
Reasoning and retained working drafts are never fed into speech. Extra effort
reads each visible stage, cancelling outstanding audio when the next stage replaces
it. Selecting a completed response's speaker button or **Stop speech** suppresses
automatic reading for the rest of that response; the mode applies again on the
next submitted prompt. Switching conversations, stopping generation or a model
failure cancels pending speech. Each conversation retains its mode choice,
including optional encrypted recovery; restoring never starts playback. Older
saved conversations default to off. The existing companion HTTP and model
request contracts remain unchanged.

This is not a claim of unrestricted OpenAI SDK or arbitrary local-server
compatibility. A generic client that emits extra fields needs an adapter to
the supported subset. Likewise, connecting `bonsai-chat` to another local server
requires checking that server's endpoint, authentication, JSON/SSE and reasoning
contracts; a matching route name alone is insufficient.

Future compatibility work should extend or adapt explicit protocol contracts
with focused tests. Keep backend-specific policy outside native presentation
code, preserve plain text and streaming semantics, and document unsupported
features. Interoperability does not authorize bypassing the broker's ownership,
admission, credential or context requirements.

## Review and validation

Review UI changes against both boundaries: first-party application code remains
Rust/native, and the HTTP adapter preserves the documented local LLM contract.
Use native builds, focused protocol/UI checks and an actual-model session where
appropriate. Full suites and all benchmarks remain CI-only under the existing
[validation policy](validation.md). Do not use a successful window launch as
evidence of model quality or sustained inference performance.
