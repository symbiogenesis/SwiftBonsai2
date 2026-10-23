# M1 request admission core

The Rust library in `src/broker/` implements request admission without a listener,
backend connection, process launcher or production token counter. Chat remains
disabled. This is a tested policy component, not daily-service qualification.

The policy accepts only HTTP/1.1 origin-form `POST /v1/chat/completions`, numeric
Host `127.0.0.1:<port>`, and the exact `http://127.0.0.1:<port>` Origin when supplied.
Native clients may omit Origin but still require the bearer credential. The
constructor requires 64 lowercase hexadecimal key characters; the future launcher
must generate 32 random bytes with the OS RNG and protect their storage. Key format
alone proves neither randomness nor access control. Credential comparison uses
`subtle`; keys and prompt-bearing types have no Debug/Serialize diagnostics.

Headers are checked before accepting the body. Limits are 64 header values and
16 KiB of parsed header names/values/separators. Critical duplicate fields,
forwarding headers, transfer/content encodings, trailers, upgrades and Expect are
rejected. A single decimal Content-Length is required, from 1 byte to 1 MiB, and
must equal actual body bytes. Content-Type must be `application/json`, optionally
with `; charset=utf-8`. These are deliberately narrow adapter requirements. The
future transport must also bound raw header/request-line bytes, header count,
body collection and read duration **during reception**, reject invalid wire
framing, preserve header multiplicity, and close rejected connections. This
library does not parse raw HTTP or bound allocations already made by its caller.

The entire public JSON schema is:

```json
{
  "messages": [{"role": "user", "content": "Draft a scene."}],
  "max_completion_tokens": 1024,
  "stream": true
}
```

`max_completion_tokens` defaults to 4096 and must be an integer from 1 through
4096; `stream` defaults to true. Explicit null and wrong types are rejected. At
most 128 messages are accepted, with an optional first system message followed
by alternating user/assistant turns ending on user. The final-user requirement
avoids the pinned runtime's automatic assistant-prefill mode. Message content
must be a string. All unknown and duplicate JSON fields are rejected, including
token-cap aliases, model selection, tools, media, arbitrary template settings,
thinking controls, sampling overrides, LoRA, slot controls and timing controls.
URLs within strings stay plain text. No remote content is fetched.

Accepted input is rebuilt as canonical backend JSON with one completion, cache
reuse disabled, EOS honored, and the reference sampler: temperature 1, top-p .95,
top-k 20, min-p 0, presence/frequency penalties 0, repetition penalty 1. The
embedded template's default thinking behavior is preserved. The completion cap
includes reasoning; it does not promise that many visible prose tokens. Public
headers, credentials and original JSON are never forwarded by this component.

`check_head -> prepare -> admit` keeps a request pending until a trusted callback
returns the exact rendered prompt token count. The callback receives the
canonical bytes intended for the pinned runtime's **internal**
`/v1/chat/completions/input_tokens` route. Generation must use those identical
bytes and the same immutable model/runtime/template and thinking mode. The
counter must reject absent/malformed counts, identity drift or backend errors.
The library requires a positive count and checked arithmetic satisfying:

```text
rendered prompt tokens + requested completion tokens + 64 reserve <= 8192
```

The 64-token reserve is a conservative initial policy, not a measured tokenizer
error allowance or a substitute for matching rendering. No production callback
is supplied; bytes/characters are never treated as tokens. An admitted body is
immutable. Parser failures return value-free error categories to keep private
input out of diagnostics.

Before any listener is enabled, implement and qualify the HTTP adapter, private
backend credential and bypass prevention, exact-count transport, single-slot
bounded queue, total deadlines, streaming cancellation, owned process lifecycle,
memory limits and restricted inference identity/filesystem/egress. Offline UI
assets and creative/comfort qualification follow. These controls remain pending
in the existing PowerShell preflight.

Dependencies are exact-pinned with a committed Cargo lockfile. Native ARM64 Rust
1.98.0 is the initial checked toolchain. Local work may build and run focused
regressions; the full broker suite runs only in the main-only Foundation workflow.
See [pre-step research](implementation-research.md) for sources and sanity checks.
