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

The native window uses a dark palette and keeps conversation history in memory.
An insufficient-memory startup failure offers **Retry model** after RAM is
freed. Retrying creates a fresh broker attempt with new diagnostic storage and
repeats the ordinary artifact, identity and 16 GiB admission checks. There is no
automatic retry or reduced-memory profile.

The demo uses the existing pinned Swift PQ2 CPU backend through the broker.
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
request contains `messages`, `max_completion_tokens` and `stream`; it accepts no
`model` field, arbitrary sampling controls, tools, media or unknown fields.
Clients must use the exact loopback Host, preserve request framing, respect
bounds and distinguish framed transport completion from a successful terminal
generation event. See the [public request contract](broker-policy.md#public-request-contract).

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
