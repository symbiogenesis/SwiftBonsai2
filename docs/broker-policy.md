# Broker policy and development service

`src/broker/` implements authenticated text-chat admission, bounded HTTP I/O,
exact-count context admission, single-slot scheduling and a development runner.
A separate explicit Windows launcher owns the pinned Qwen reference or Swift
assessment backend.
The PowerShell preflight and daily profile remain launch-disabled. Development
controls do not establish daily security or real-model qualification.

The first-party `bonsai-chat` interface is a separate pure-Rust native Windows
client. The broker serves the HTTP API; it supplies no browser UI or WebView.
The permanent [UI architecture](ui-architecture.md) keeps presentation native
while preserving the language-independent local LLM protocol boundary.

## Public request contract

Only HTTP/1.1 origin-form `POST /v1/chat/completions` is accepted. Host must be
`127.0.0.1:<port>`; when supplied, Origin must be exactly
`http://127.0.0.1:<port>`. Native clients may omit Origin but still need the bearer
credential. Keys must contain exactly 64 lowercase hexadecimal ASCII characters.
Comparison uses `subtle`; keys and prompt-bearing types have no secret-bearing
Debug/Serialize diagnostics. Format checking does not establish key randomness
or access control.

Headers are validated before body allocation. Critical duplicates, forwarding
headers, transfer/content encodings, trailers, upgrades and Expect are rejected.
A single decimal Content-Length from 1 byte through 1 MiB must match the body.
Content-Type must be `application/json`, optionally with `; charset=utf-8`.
Policy-only callers must bound their own prior allocations.

| Bound | Limit |
| --- | --- |
| Parsed header names/values/separators; complete raw request/response head | 16 KiB each |
| Header values | 64 |
| Public body / complete canonical backend body | 1 MiB each |
| Messages | 128 |
| Completion tokens | 1–4096, default 4096 |
| Decoded backend response | 4 MiB |
| Exact-count response | 4 KiB |
| Header intake / complete intake | 5 seconds / 30 seconds from intake start |
| Connect / complete write operation | 5 seconds each |
| Queue wait / total request | 30 seconds / 10 minutes from acceptance |

All deadlines are absolute, shortened by the supervisor deadline and never
refreshed by progress. Expiry is checked around I/O and after bounded synchronous
parsing; it is not a preemptive CPU limit. Errors expose value-free categories.

## Text-chat admission

The API uses familiar local LLM chat JSON and SSE conventions, with a strict
supported subset. Generic OpenAI-style clients need an adapter if they send
fields such as `model`, sampling controls or tool definitions. Native Rust UI
implementation does not widen that public schema or claim unrestricted SDK
compatibility.

The entire public JSON schema is:

```json
{
  "messages": [{"role": "user", "content": "Draft a scene."}],
  "max_completion_tokens": 1024,
  "stream": true
}
```

`stream` defaults to true. Null, wrong types, unknown fields and duplicate fields
are rejected at every schema level. An optional first system message may precede
alternating user/assistant turns ending on user. Content must be a string. Ending
on user avoids the pinned runtime's automatic assistant-prefill behavior.
Assistant messages may also contain a string `reasoning_content` to replay
previous reasoning returned by the backend. Present null/non-string values and
reasoning fields on user/system messages are rejected. Omitted reasoning stays
omitted; supplied reasoning is preserved and counted within the same body and
context bounds.

Model selection, token-cap aliases, tools, media, template/thinking/sampling
controls, LoRA, slots and timing overrides are unsupported. URLs in strings stay
plain text; the broker fetches no remote content.

Input is rebuilt as immutable canonical backend JSON with one completion,
cache reuse disabled, EOS honored and the reference sampler: temperature 1,
top-p .95, top-k 20, min-p 0, presence/frequency penalties 0, repetition penalty 1.
The canonical body explicitly requests `enable_thinking: true` and
`reasoning_effort: "xhigh"` and `preserve_thinking: true` through
`chat_template_kwargs`, matching the pinned template's defaults. Counting and
generation receive those same settings. The
launcher also fixes thinking on, effort `xhigh` and no separate reasoning cutoff
(`--reasoning-budget -1`); the total completion cap still includes reasoning and
does not promise that many visible prose tokens. See
[inference defaults and evidence](inference-defaults.md).
Public headers, credentials and original JSON are never forwarded.
The complete canonical body must also fit the backend's 1 MiB request cap.
Preparation rejects expansion past that cap before exact counting or a backend
connection; the fixed policy fields count toward the limit.

`check_head -> prepare -> admit` retains a pending request until a trusted counter
returns the exact rendered prompt length. The counter receives the canonical
bytes for the private `/v1/chat/completions/input_tokens` route; generation must
use those identical bytes and immutable model/runtime/template/thinking mode.
Positive counts and checked arithmetic must satisfy:

```text
rendered prompt tokens + requested completion tokens + 64 reserve <= 8192
```

The reserve is conservative policy, not a measured tokenizer-error allowance.
Bytes/characters are never treated as tokens. Counter errors or identity drift
must reject admission.

## One-request receiver

`reception::receive_chat(stream, &policy, supervisor_deadline)` owns an
`AsyncRead + Unpin + 'static` stream. It reads strict CRLF HTTP with no leading
blank lines into a bounded raw-head buffer, preserving duplicate headers and
checking the raw method/target before URI normalization.

Only after head validation does it allocate/fill the declared body. Early EOF
or buffered surplus beyond Content-Length rejects; later surplus is left for
close, or detected by the supervisor's read monitor. Completion does not wait
for peer EOF. Success returns the stream plus `PendingChat` through
`ReceivedChat::into_parts()`. Errors and cancellation drop the stream.
One response followed by close is required; connection reuse is unsupported.

## Private backend HTTP exchange and exact counts

`backend::BackendPolicy::new(port, key)` fixes one numeric-loopback port and
private key. The supervisor must supply a separate random protected backend
credential; this policy alone cannot prove endpoint identity or distinctness.

`backend::count_and_admit(stream, pending_chat, &policy, deadline)` sends the
canonical bytes to `/v1/chat/completions/input_tokens`. It requires HTTP 200 JSON
with exactly positive integer `input_tokens` and string
`object: "response.input_tokens"`. Missing, duplicate, unknown, null, wrong-type
or alternate root/tag shapes reject. The count connection closes before return;
the original canonical allocation is retained for generation.

`backend::request_chat(stream, &admitted_chat, &policy, deadline)` sends those
identical bytes to `/v1/chat/completions`. Both functions rebuild Host,
Authorization, Content-Type, Content-Length, `Accept-Encoding: identity` and
`Connection: close`; caller routes, URLs and headers cannot override them.
There are no redirects or retries. One five-second deadline covers the whole
request write/flush; response headers may wait until the total deadline for
initial prefill.

Response parsing requires strict CRLF HTTP/1.1, status 200 and one matching
Content-Type. Content-Encoding, Trailer and Upgrade are rejected. Exactly one
decimal Content-Length or singleton exact `chunked` Transfer-Encoding is
required, including empty bodies. Interim, close-delimited or ambiguously framed
responses are unsupported. Coalesced body bytes remain in the buffered reader.

`BackendBody` implements `AsyncRead`, enforcing decoded-byte/deadline limits,
32-byte chunk-size lines including CRLF and 64 KiB cumulative chunk metadata.
Chunk extensions/trailers are rejected; the pinned server emits neither.
Immediately ready metadata work yields after bounded steps. Short bodies,
malformed delimiters or incomplete termination return sticky I/O errors and
drop the stream after any already-yielded payload. Framed completion does not
wait for socket EOF; surplus is discarded and the connection is never reused.

## One-response writer

`response::send_chat_response(client, backend_response, &admitted_chat, deadline)`
owns a client writer and backend response. Its body must already decode HTTP
framing and report incomplete framing as read errors; a raw backend socket is
not valid input. The backend adapter above supplies this contract.

Before writing, the adapter requires status 200, one Content-Type matching the
admitted `stream` flag and no Content-Encoding. Accepted types are
`application/json` or `text/event-stream`, optionally with `; charset=utf-8`,
case-insensitively. Downstream headers are rebuilt with `Connection: close`,
`Cache-Control: no-store`, `X-Content-Type-Options: nosniff`, fixed media type and
computed framing. No upstream cookie, redirect, credential or CORS policy passes
through.

JSON is buffered within 4 MiB, checked for UTF-8/object syntax, then sent unchanged
with computed Content-Length. Invalid JSON fails before client writes; this does
not validate completion schema or duplicate response keys. SSE forwards unchanged
bytes from an 8 KiB buffer as flushed HTTP chunks, without retaining the stream
or interpreting events. At the limit, one extra read distinguishes exact-limit
EOF from surplus; excess payload is never forwarded.

Only clean decoded EOF permits the HTTP zero chunk. Backend errors, excess bytes
or observed expiry abandon streaming without inventing `[DONE]`. Final write,
flush or shutdown can fail after a terminator reached the peer. HTTP completion
does not establish successful generation: SSE can contain an error event.
Clients must interpret completion/errors and render model text safely.

Backend reads use the total deadline. Each complete client operation (headers,
one chunk, or final flush/shutdown) has at most five seconds; partial writes do
not restart it. JSON's whole write shares one such deadline. Errors can follow
partial output, so close without appending a second response. Shutdown runs only
on success. Cancellation drops both endpoints.

## Development orchestration and loopback transport

`scheduler::RequestGate::new(max_waiting)` permits one active operation and zero
through eight waiting reservations. `reserve()` acquires capacity before socket
acceptance/body allocation. `Reservation::enter` waits fairly under an absolute
deadline; FIFO begins at first polling, not reservation creation. Dropping queued
futures, reservations or active guards releases permits. `close()` permanently
refuses admission and wakes waiters; the active guard alone does not cancel work.

`supervisor::serve_chat` owns one client stream and runs reception, exact counting,
admission, generation and forwarding with the same connector/canonical body.
Rejected counts never start generation. Its total deadline is at most ten
minutes from entry. It supports local/non-Send streams and spawns no task.
After intake, a one-byte client read races the backend pipeline. EOF, read error
or extra input wins ties and drops both endpoints. TCP half-close counts as
abandonment: clients must keep their sending side open until response completion.
This rejects pipelining but cannot detect unreachable peers before TCP reports
failure. There is no second response or automatic retry.

`transport::LoopbackBackend` connects only to fixed `127.0.0.1`, without DNS,
redirects or retry. A private HTTP key authenticates requests to the server;
it does not authenticate that server's process/model identity.

`service::LocalChatService::bind(frontend_port, frontend_key, backend_port,
backend_key, max_waiting)` validates distinct ports/keys before opening numeric
IPv4 loopback. Frontend zero permits OS assignment; other ports must be
unprivileged. Windows binding requests `SO_EXCLUSIVEADDRUSE`. `accept()` reserves
capacity before accepting; pending accepts and unpolled returned handles count.
The OS backlog is separate. Full capacity returns immediately, so callers must
avoid busy retry loops.

`AcceptedChat::serve()` includes its thirty-second queue wait in the ten-minute
total from acceptance. Shutdown races accepting/active work and wakes queued
entries. `close()` retains the listener until service drop. Async cleanup still
requires polling; dropping operations closes sockets/releases permits directly.

Once generation connection is attempted, failed delivery, disconnect, expiry or
external cancellation permanently closes admission before releasing the active
permit. Clean framed completion disarms the latch. Closing sockets does not
acknowledge model cancellation; a stopped or freshly owned backend is required
before recreating the service. Failures before generation do not latch it.

Generic receiver/backend/response APIs create no listener, task or process.
Their owned `'static` streams must close on Drop; lifetime bounds cannot prove
wrapper behavior. Backend exchange needs `AsyncRead + AsyncWrite + Unpin`; all
timed APIs need Tokio's time driver. A write-only response adapter cannot itself
monitor read-side disconnects; the supervisor supplies that monitoring.

## Executable runner and key files

`runtime::run` polls at most the service capacity of owned request futures;
it spawns no detached tasks and prioritizes shutdown over acceptance. Ctrl-C,
signal/accept failure or the generation latch closes admission and drains work.
Runner drop closes outstanding sockets. It cannot terminate an externally
supplied backend or acknowledge its model cancellation.

External-backend mode requires `--development --allow-unverified-key-files` and
absolute key paths. Keys cannot be passed as values or environment variables.
Files must be regular, free of reparse components and exactly 64 lowercase-hex
bytes without a newline. Windows read-only/share-read handles remain open to
deny write/delete opens. ACLs and ancestor ownership are explicitly unverified.
Routine output contains the numeric endpoint and value-free status, never keys,
prompts, arguments or raw I/O errors.

`scripts/New-DevelopmentKeys.ps1` creates a fresh random directory with distinct
random keys under an existing owner-controlled external local parent. Protected
owner/LocalSystem ACLs exist before secret writes and are verified. Existing
objects are never reused. It returns paths only; `-WhatIf` resolves no executable
or credentials. Same-user/admin access and parent-owner replacement remain
outside its assurance. Partial directories remain for inspection.
See [commands](development-guide.md#explicit-development-broker).

## Owned Windows development launcher

`--launch-development --allow-unisolated-backend` selects `launcher::LaunchConfig`.
Only compiled-in pins and fixed arguments are accepted: `reference-cpu` is the
default, and `--profile swift-pq2-assessment` explicitly selects Swift's separate
model/runtime lock. Arbitrary executables, models, profile JSON and overrides
are rejected. Swift uses the original PQ2 weights without PTQ1 substitution.
`--use-abliterate-lora` selects only the pinned adapter at scale 1 with the Prism
CPU backend; it is rejected for the reference profile and `--native-engine`.
The demo requests it by default and offers `-NoAbliterateLora`; direct broker
launches omit it by default. `--check-only`
verifies policy/artifacts and reports memory admission without credentials or a
process. Neither selection establishes real-model service qualification. Inherited
LLAMA/LLAMACPP/GGML/AIP/MTMD and model-endpoint/token overrides are rejected;
Windows/profile environment variables are preserved.

`artifacts::VerifiedArtifacts` verifies complete model/ZIP size/hash, safe archive
paths/expansion, exact installed runtime contents and ARM64 PE images. Retained
Windows handles deny write/delete access to artifacts and ancestors during use.
When selected, the adapter's pinned size/SHA-256 is also verified and its handle
retained through backend shutdown; missing or altered files fail startup.
This does not attest source-to-binary builds or restricted execution.

Safe broker code delegates Win32 ownership to the audited
`bonsai-windows-process` and protected credential creation to
`bonsai-windows-credentials`. A fresh protected evidence directory contains two
keys and new stdout/stderr files. The reference requires 22 GiB available RAM
for its 18 GiB service; Swift requires 16 GiB for its 12 GiB service. Both retain
a 4 GiB physical reserve. Swift fixes batch/microbatch at 128/64; the reference
retains 256/128. Suspended process creation atomically applies
Job/standard-handle attributes: kill-on-close, the selected job-wide commit cap,
one-process limit, no breakaway and noninheritable Job handle. The absolute
external evidence directory is the process working directory.

No frontend opens before a ten-minute startup deadline, complete bounded health
response, authenticated `/v1/models` checks and listener PID matching the owned
process. `/health` is public, so `/v1/models` must reject a missing key and accept
the generated key. Probes have bounded headers/body, strict framing and short
absolute I/O deadlines. Count/generation connections check the listener PID
before and after connecting. Same-user/admin interference remains outside this
development assurance.

Startup and serving monitor process exit, private bytes, working set and the
4 GiB physical reserve; serving also checks listener ownership. Guard failure
closes admission/drains sockets. Every serving exit kills the whole owned Job
and waits up to ten seconds for emptiness. Startup failure, owner drop and abrupt
broker death also kill it. There is no automatic restart or public reset route.
Commit limits do not cover
all resident file-backed mappings, so the physical watchdog remains necessary.

Data/evidence parents must exist outside Git on local storage with no reparse
components. Evidence and credentials remain retained; never share either key or
the whole directory. CPU/LoRA checks use the packaged Rust maintenance tool and
same Windows process boundary, with their separate fixed arguments/pins.

The [bounded source HTTP diagnostic](../config/swift-owned-http-mvp.evidence.json)
passed supplied-history streaming, private reset/replay, independently verified
context rejection/recovery, active abandonment with owned shutdown, and a fresh
owned restart followed by success. It binds exact source-server bytes through
explicit temporary diagnostic patches, including matching 13.65 GiB parent and
child startup checks. The 12 GiB Job cap and 4 GiB running reserve were retained.
These are recorded diagnostic binaries; ordinary release pins and 16 GiB Swift
startup admission remain unchanged.

Restricted tokens, filesystem/egress isolation, same-user backend bypass
prevention, everyday native UI acceptance and real-model quality/performance
remain unqualified.
Synthetic loopback/stream regressions establish broker behavior, not those gates.
See [launch commands](development-guide.md#owned-windows-development-backend) and
[validation](validation.md). Dependency/toolchain pins live in the Cargo workspace
and lockfile; full suites run only in main-only Foundation CI.
