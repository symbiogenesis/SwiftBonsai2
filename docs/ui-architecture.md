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

`src/chat-ui/` provides the `bonsai-chat` Windows application. Its application
source is Rust, using native Win32 controls through Rust bindings and a Rust
HTTP client. The UI runs separately from the broker and inference process;
network work runs off the native event loop. It adds no GUI framework or browser
runtime dependency.

The native window uses a dark palette, follows Windows high-contrast/system
color changes, exposes native control labels and keeps conversation history in
memory. Per-monitor DPI changes apply the system's suggested window rectangle.
An insufficient-memory startup failure offers **Retry model** after RAM is
freed. Retrying creates a fresh broker attempt with new diagnostic storage and
repeats the ordinary artifact, identity and 16 GiB admission checks. There is no
automatic retry or reduced-memory profile.

The demo uses the existing pinned Swift PQ2 CPU backend through the broker.
Its pinned abliteration LoRA is enabled by default, with an explicit
`-NoAbliterateLora` opt-out; the broker verifies and holds the selected adapter.
The UI does not load model weights, select arbitrary runtime arguments, bypass
artifact verification or change model, memory, context, sampler or thinking
policy. A native UI does not by itself qualify runtime correctness, LoRA,
creative quality, performance or daily-service isolation.

Use the [development guide](development-guide.md) for launch commands and
[broker policy](broker-policy.md) for the exact supported request contract.

## Interoperability belongs at the API boundary

A Rust-only UI remains compatible with local LLM conventions through the
language-independent HTTP API, rather than through a browser frontend.
The compatibility surface is an OpenAI-style `POST /v1/chat/completions`
endpoint, JSON text messages, bearer authorization and incremental
`text/event-stream` Server-Sent Events (SSE). Clients retain supplied assistant
reasoning separately from visible answer text and replay it using
`reasoning_content` when supported by the contract.

The present broker implements a deliberately strict subset. Its complete public
request contains `messages`, `max_completion_tokens`, `stream` and an optional
fixed `model: "bonsai"` alias. Content accepts strings or bounded standard text
parts; arbitrary model selection, sampling controls, tools, media and unknown
fields remain unsupported.
Clients must use the exact loopback Host, preserve request framing, respect
bounds and distinguish framed transport completion from a successful terminal
generation event. See the [public request contract](broker-policy.md#public-request-contract).

The native SSE client accepts UTF-8 BOM and CR/LF/CRLF line endings, including
fragmented input, and UTF-8 charset parameters on the event-stream media type.
It requires a terminal `finish_reason` plus `[DONE]` and completed HTTP framing.
A `length` result retains partial text and displays the token-limit outcome;
unsupported terminal reasons, error events and missing completion reject.

## Speech input and output

The PowerShell demo launcher provisions the pinned Canary, tokenizer and Qwen
VoiceDesign files by default, using immutable URLs and exact size/SHA-256
verification. `-SpeechModelsOnly` provisions files without building or launching;
`-NoSpeechModels` skips that step. Runtime installation and service startup remain
separate from artifact downloads and native presentation.

Speech input uses NVIDIA Canary Qwen 2.5B through an explicitly configured,
authenticated numeric-loopback [companion service](../tools/canary-asr/README.md).
The Rust UI requests the Windows `microphone` capability on its UI thread before
audio-only capture. Denied, cancelled or unavailable access fails closed. The
microphone is off initially; only the explicit microphone toggle can start it.
Disabling it, sending a prompt, starting read-aloud, creating a conversation or
closing the window cancels capture and invalidates outstanding session results.
Late permission/capture/transcription completions cannot reactivate capture.

Bounded mono 16 kHz PCM WAV clips stay in memory in the client. Capture pauses
while Canary transcribes a clip; recognized text returns to the composer for
review, with no automatic send. The companion uses the actual pinned Canary
weights and no recognition fallback; its runtime requirements are separate from
the Swift CPU backend. No audio is sent to a cloud service by this integration.

Read-aloud uses the separate, authenticated local
[Qwen3-TTS 1.7B VoiceDesign companion](../tools/qwen-tts/README.md). The native
**Voice profile** text setting describes the desired voice in ordinary language;
no preset, speaker file or reference recording is required. It applies to the
next explicit read-aloud request. Only visible assistant answers are synthesized,
as plain text; reasoning is excluded. Long answers are split into bounded text
segments and synthesized/played sequentially with the same description. Stop
speech cancels synthesis and native WAV playback. Missing/not-ready TTS prevents
output independently of Canary input, with no Windows-voice fallback. Idle
speech controls remain visible and show native setup/prerequisite guidance when
selected; a missing service never activates the microphone or substitutes output. Images,
video and generated images remain unavailable in the pinned Swift profile.

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
Use native builds, focused protocol/UI checks and an actual-model demo where
appropriate. Full suites and all benchmarks remain CI-only under the existing
[validation policy](validation.md). Do not use a successful window launch as
evidence of model quality or sustained inference performance.
