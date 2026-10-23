# M1 request admission, bounded HTTP I/O and development orchestration

The Rust library in `src/broker/` implements request admission, bounded
reception, backend HTTP exchange/exact-count admission, response writing and
explicit development orchestration with numeric-loopback transport. The
executable supports an opt-in runner against an already supplied backend and a
separate explicit Windows launcher for the pinned reference CPU backend. Model
launch remains disabled in the PowerShell preflight and daily reference profile.
These are development components, not daily-service qualification.

The policy accepts only HTTP/1.1 origin-form `POST /v1/chat/completions`, numeric
Host `127.0.0.1:<port>`, and the exact `http://127.0.0.1:<port>` Origin when supplied.
Native clients may omit Origin but still require the bearer credential. The
constructor requires 64 lowercase hexadecimal key characters. The separate
`New-DevelopmentKeys.ps1` helper creates distinct 32-byte random keys in new files
with protected owner/LocalSystem ACLs; the development runner explicitly does not
verify those ACLs or ancestor ownership. Key format alone proves neither
randomness nor access control. Credential comparison uses
`subtle`; keys and prompt-bearing types have no Debug/Serialize diagnostics.

Headers are checked before accepting the body. Limits are 64 header values and
16 KiB of parsed header names/values/separators. Critical duplicate fields,
forwarding headers, transfer/content encodings, trailers, upgrades and Expect are
rejected. A single decimal Content-Length is required, from 1 byte to 1 MiB, and
must equal actual body bytes. Content-Type must be `application/json`, optionally
with `; charset=utf-8`. These are deliberately narrow adapter requirements. The
policy API alone does not bound allocations already made by its caller; the
`reception` adapter supplies bounds during reading, as described below.

## One-request receiver

`reception::receive_chat(stream, &policy, supervisor_deadline)` consumes one
owned `AsyncRead + Unpin + 'static` stream and requires a Tokio time driver.
The caller must supply a stream whose Drop closes the connection; the lifetime
bound prevents ordinary borrowed-stream arguments but cannot prove a wrapper's
Drop behavior. The adapter creates no socket, filesystem object or task. Future
listener integration must bound accepted connections before allocating a receiver.

The complete raw head, including request line, whitespace and terminator, is
limited to 16 KiB, with 64 header values. The fixed buffer can contain a coalesced
body prefix; no more than 16 KiB is read before head policy validation. Strict
CRLF and no leading blank lines are required. httparse 1.10.1 parses syntax; all
header values are appended without deduplication. Raw method/target are checked
before conversion to HTTP types, preventing URI fragment normalization from
turning an unsupported wire route into the admitted one. Malformed, folded,
ambiguous or unsupported framing fails closed.

Only after successful head validation is the declared body allocated, at most
1 MiB. Reads fill only its remaining length; completion does not wait for peer
EOF. EOF before completion rejects. Already-buffered bytes beyond Content-Length
reject as unsupported surplus; bytes arriving later are deliberately left unread
and must be discarded on close. The existing strict JSON policy then returns a
`PendingChat`; exact context admission still must occur before generation.

Header reception has a five-second absolute deadline. The entire intake,
including JSON preparation, has a thirty-second absolute deadline from the same
start. Both are shortened by the supervisor's total request deadline. Progress
never resets either timer; explicit expiry checks surround reads and successful
return. Parsing is bounded synchronous work, with expiry checked afterwards,
not a preemptive CPU deadline. Errors reveal only value-free categories and drop
the owned stream; cancellation drops it through ordinary future ownership.

On success, `ReceivedChat::into_parts()` returns the stream and `PendingChat` for
the response adapter. Send one response with `Connection: close`, then
drop the stream. Connection reuse is unsupported. Response writing, streaming
and total response deadlines are implemented separately below; backend transport
and generation cancellation remain integration work.

## One-response writer

`response::send_chat_response(client, backend_response, &admitted_chat,
supervisor_deadline)` owns a client writer and an `http::Response` whose owned
body reader **must already decode HTTP framing**. A trusted backend
transport must bound upstream headers, reject ambiguous/unsupported framing,
and report short Content-Length or incomplete chunked bodies as read errors.
Passing a raw backend socket would defeat the completion contract. The `backend`
module below supplies a compatible decoder; neither module opens a connection.

Before writing anything, the adapter requires status 200, exactly one
Content-Type matching the admitted request's `stream` flag, and no
Content-Encoding. Accepted media types are `application/json` and
`text/event-stream`, optionally followed by `; charset=utf-8`, case-insensitively.
The stream flag is retained alongside the identical immutable canonical request;
backend headers cannot select a different response mode. Every downstream header
is rebuilt: `Connection: close`, `Cache-Control: no-store`,
`X-Content-Type-Options: nosniff`, the fixed media type, and computed framing.
No upstream header, cookie, redirect, CORS policy or credential is forwarded.

JSON is collected up to 4 MiB, checked for UTF-8 and JSON object syntax, and sent
unchanged with its computed Content-Length. Invalid or oversized JSON fails
before any client write. This syntax check does not validate the completion
schema, duplicate response keys or model behavior. SSE uses an 8 KiB read buffer
with the same 4 MiB total decoded-byte limit. Each buffer becomes a downstream
HTTP chunk and is flushed; SSE bytes, event boundaries, pings and error events
remain unchanged. The adapter does not wait for event boundaries or retain the
full stream. At the byte
limit, one further byte is read to distinguish exact-limit EOF from surplus;
no payload beyond the limit is forwarded.

Only clean decoded backend EOF permits the terminating HTTP zero chunk. Backend
read errors, byte-limit violations and observed expiry abandon the stream without
attempting that terminator or inventing an SSE `[DONE]` event. A final write,
flush or shutdown can still fail after the terminator has been transmitted; an
error return does not prove that no complete HTTP body reached the peer.
Clean HTTP completion is a transport verdict,
not a successful-generation verdict: the pinned server can finish after an SSE
error event. Clients still need completion/error interpretation and safe rendering
of model text.

Backend reads use the supervisor's absolute deadline, permitting slow initial
prefill without an arbitrary short idle timeout. Each entire client operation
(headers, one chunk's prefix/payload/suffix, or final flush/shutdown) is shortened
to five seconds. Partial writes do not restart it. JSON's complete write shares
one such deadline. Expiry is checked before and after I/O and after synchronous
JSON parsing; this is not a preemptive CPU limit. No automatic retry occurs.
Any error may follow a partial write, so callers must close and never append a
second response. Shutdown runs only on success and is itself bounded.

Both supplied endpoints must close on Drop; `'static` bounds discourage borrowed
streams but cannot prove wrapper behavior. Dropping the future or observing an
I/O failure drops both endpoints. An `AsyncWrite` client alone cannot detect a
read-side disconnect while waiting for backend data. Actual disconnect monitoring,
backend cancellation, process ownership and the total generation lifecycle remain
supervisor/transport gates. The adapter creates no task, listener or process.

## Text-chat admission

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
error allowance or a substitute for matching rendering. The supplied-stream
count helper below implements this exchange; bytes/characters are never treated as tokens. An admitted body is
immutable. Parser failures return value-free error categories to keep private
input out of diagnostics.

## Private backend HTTP exchange and exact counts

`backend::BackendPolicy::new(port, key)` accepts a numeric loopback port and the
same 64-lowercase-hex key format as the frontend policy. The supervisor must
supply a separate random private backend credential and protect it. The policy
does not prove randomness, distinctness, endpoint identity or access control.

`backend::count_and_admit(stream, pending_chat, &backend_policy, deadline)` owns
one already-connected full-duplex stream and sends the pending canonical bytes
to the fixed `/v1/chat/completions/input_tokens` route. It requires HTTP 200 JSON
with at most 4 KiB and exactly two fields: positive integer `input_tokens` and
the string `object: "response.input_tokens"`. Missing, duplicate, unknown, null,
wrong-type and alternate root/tag shapes fail closed. The count then passes
through the existing checked 8K admission rule. The original canonical allocation
is retained unchanged, and the count connection is dropped before returning.

`backend::request_chat(stream, &admitted_chat, &backend_policy, deadline)` sends
those identical bytes to `/v1/chat/completions`. Both operations rebuild Host,
Authorization, Content-Type, Content-Length, `Accept-Encoding: identity` and
`Connection: close`. They accept no caller route, URL or forwarded header, never
retry or follow redirects, and use one five-second limit for the entire request
write/flush, shortened by the supervisor deadline. Response headers may wait
until that total deadline, accommodating initial prefill. Every ready operation
also checks expiry explicitly.

Response heads are bounded to 16 KiB/64 fields and parsed with httparse while
retaining duplicate headers. Strict CRLF, HTTP/1.1, status 200 and a singleton
matching Content-Type are required. Content-Encoding, Trailer and Upgrade are
rejected. Exactly one decimal Content-Length or singleton exact `chunked`
Transfer-Encoding is required, even for empty bodies. Conflicting or repeated
framing fails; interim responses, close-delimited bodies and encodings are
unsupported. Coalesced body bytes stay in the owned buffered reader.

The returned `BackendBody` implements `AsyncRead` and can be passed directly to
`response::send_chat_response`. It enforces the same absolute supervisor deadline,
a 4 MiB decoded limit, 32-byte chunk-size lines including CRLF, and 64 KiB of
cumulative chunk metadata including data delimiters. Chunk extensions and
trailers are deliberately rejected; the pinned server's provider emits neither.
Immediately ready metadata work yields after a bounded number of steps. A short
Content-Length, malformed/missing chunk delimiter or incomplete terminating
empty trailer returns an I/O error after any yielded payload. Errors are sticky
and immediately drop the stream. Framed completion does not wait for socket
EOF; surplus is discarded on drop and the connection is never reused.

These generic APIs require owned `AsyncRead + AsyncWrite + Unpin + 'static`
streams whose Drop closes the connection, plus a Tokio time driver. They create
no task, socket, listener or process. Dropping an exchange or returned body drops
the supplied endpoint. The supervisor still must own one immutable verified
backend, connect only to it for both operations, prevent bypass/replacement and
monitor real frontend disconnects to cancel generation. Supplied streams and
synthetic count responses cannot establish those properties or qualify actual
model tokenization.

## Development orchestration and loopback transport

`scheduler::RequestGate::new(max_waiting)` permits one active operation plus
zero through eight waiting reservations. `reserve()` is nonblocking and holds
capacity before any socket acceptance or body allocation. `Reservation::enter`
waits fairly for the active slot under an absolute deadline; FIFO begins when
that future is first polled, not when reservation handles are created. Dropping
reservations, queued futures or active guards releases their permits. `close()`
permanently refuses admission and wakes all queue/shutdown waiters. The active
guard alone does not cancel work; the service integrates that signal below.

`supervisor::serve_chat` receives one owned stream, splits its read/write sides,
and runs counting, context admission, generation and response forwarding with
the same connector and immutable canonical body. No generation connection is
attempted after a rejected count. A single absolute deadline covers the whole
operation and is shortened to ten minutes from entry. Header, intake, connect
and write limits may shorten it further; progress never refreshes the total.
The generic supervisor supports local/non-Send streams and spawns no task.

After reception, a one-byte client read races the entire backend pipeline.
EOF, read failure or additional input wins over simultaneous backend progress
and drops both endpoints. A TCP write-half-close is deliberately treated as
abandonment: clients must keep their sending side open until the response ends.
This policy rejects pipelining; it does not distinguish full departure from a
half-close or detect a silently unreachable peer before TCP reports it. Outer
future cancellation also drops owned streams. No second response or automatic
retry is attempted after failure.

`transport::LoopbackBackend` owns one immutable `SocketAddrV4` and private HTTP
policy. It connects only to numeric `127.0.0.1`, with no DNS, redirects or retry,
and a five-second connect limit shortened by the total deadline. The private
credential authenticates requests to a server; it does not authenticate that
server's process or prove its model/template identity.

`service::LocalChatService::bind(frontend_port, frontend_key, backend_port,
backend_key, max_waiting)` explicitly opens a development listener on numeric
IPv4 loopback. Frontend port zero permits OS assignment. Other ports must be
unprivileged; ports and credentials must be distinct. Every input is validated
before binding; no key/file/process/UI is created. `accept()` reserves capacity
before socket acceptance; pending accepts and returned unpolled handles count
toward the bound. The OS TCP backlog is separate. Full capacity returns a
value-free error immediately; callers must avoid busy retry loops. This API
does not spawn an unbounded accept/task loop.

`AcceptedChat::serve()` allows thirty seconds from acceptance to acquire the
active slot, with queue time included in the ten-minute total. Shutdown races
accepting and active work; queued entries wake through the gate. `close()`
leaves the listener bound until the service itself is dropped. All operations
must still be polled for asynchronous cleanup; dropping them closes their
sockets and releases reservations synchronously.

Once a generation connection is attempted, failed delivery, disconnect,
deadline or external cancellation permanently closes the service's admission
gate before the active permit is released. Clean framed response completion
disarms this guard. The latch prevents the next request following an
unacknowledged cancellation; it does not prove the old model task stopped.
There is no automatic reset/restart. A future launcher must establish a stopped
or freshly owned backend before recreating the service. Failures before a
generation attempt release their slots without this latch.

Real loopback regressions exercise synthetic counting/generation, private
headers, capacity, disconnect, caller cancellation and shutdown. Supplied
stream regressions cover FIFO, queue expiry, total deadlines and malformed
counts. These establish broker behavior, not model execution or daily security.
Before enabling daily chat, qualify immutable owned backend identity, bypass
prevention, real tokenization, cancellation acknowledgement/process termination,
OS memory limits and restricted inference identity/filesystem/egress. Offline
UI assets and creative/comfort qualification follow. The existing PowerShell
preflight retains these gates and does not call these development APIs.

## Executable runner and key files

`runtime::run` drives at most `LocalChatService::capacity()` owned serving
futures, including queued sockets, and does not spawn request tasks. It accepts
only while below that validated bound. Shutdown is polled before acceptance;
Ctrl-C closes admission, cancels active work and drains all owned futures before
returning. Signal failure, accept failure and a generation-cancellation latch
also stop the runner. Dropping the runner drops its outstanding sockets. It does
not acknowledge backend model cancellation or terminate an externally supplied
backend. No automatic restart or latch reset occurs.

The executable requires `--development --allow-unverified-key-files`, distinct
unprivileged frontend/backend ports (frontend zero permits OS assignment), and
absolute key-file paths. Credentials cannot be supplied as argument values or
environment variables. Each file is read with an exact 64-byte bound and must
contain lowercase hexadecimal ASCII without a newline. Nonregular files and
reparse components are rejected. On Windows the opened files allow read sharing
only, and their handles remain alive during serving to refuse write/delete
opens. ACLs and ancestor ownership/replacement remain explicitly unverified.
The only routine output is the numeric frontend endpoint and value-free status;
no prompts, keys, arguments or raw I/O errors are logged.

`scripts/New-DevelopmentKeys.ps1` creates a new GUID private directory below an
existing owner-controlled external local parent. It creates protected owner and
LocalSystem ACLs, checks the resulting directory owner/DACL before generating
secrets, and creates both files with explicit protected security and CreateNew.
It returns paths only, supports WhatIf, preserves the environment and never
reuses existing credentials. Same-user access, privileged access and directory
replacement by the parent owner are outside its assurance. A partial directory
may remain on setup failure; no automatic destructive cleanup is attempted.
See the [development commands](development-guide.md#explicit-development-broker).

## Owned Windows development launcher

`--launch-development --allow-unisolated-backend` selects the separate
`launcher::LaunchConfig` path. Both acknowledgements are required. The only
accepted model/runtime is the compiled-in `reference-cpu` lock; users cannot
substitute executables, model files, profile JSON or extra server arguments.
Appending `--check-only` performs read-only policy/artifact verification and
reports current memory admission without creating credentials or a process.
The Rust launch policy checks the fixed profile and rejects inherited
LLAMA/LLAMACPP/GGML/AIP/MTMD and model-endpoint/token overrides. Host Windows
system and profile environment variables are preserved for the child.

`artifacts::VerifiedArtifacts` size/hash-checks the complete pinned model and
ZIP, validates archive paths and bounded expansion, compares the installed
runtime tree with the verified archive, and validates ARM64 PE images. The
Windows handles for artifacts and their ancestor directories deny write/delete
access while the owner lives. This protects the verified files during use; it
does not attest a restricted execution environment or a source-to-binary build.

The broker stays safe Rust. `bonsai-windows-process` owns the narrowly audited
Win32 process/Job calls and `bonsai-windows-credentials` owns RNG, security
descriptors and protected creation. Their unsafe blocks document pointer,
buffer, handle and lifetime obligations. Both crates expose owned safe APIs;
their manifests and focused native regressions run in Foundation.

The launcher creates a fresh protected evidence directory, distinct frontend
and backend keys, and new technical stdout/stderr files. It requires 22 GiB
available physical memory before launching the fixed 18 GiB reference service.
The Job has kill-on-close, an 18 GiB committed-memory cap and one-process limit,
without breakaway. Job-list and standard-handle-list attributes are applied
at suspended process creation before any child code can run. The Job handle
is noninheritable, so abrupt broker death also ends the backend. The process
working directory is the absolute external protected evidence directory.

No frontend listener opens before a ten-minute startup deadline, complete
bounded HTTP health response, a protected-endpoint authentication check, and
the Windows listener PID matching the owned process. The pinned `/health`
endpoint is public, so readiness additionally requires `/v1/models` to reject
a missing credential and accept the generated backend key. Each probe has a
short absolute I/O timeout, bounded headers/body and strict response framing.
Backend count/generation connections recheck the listener PID before and after
connecting. PID observations complement pinned artifacts and the retained
process handle; same-user/admin interference remains outside this development
assurance.

During startup and serving the launcher observes process exit, private bytes,
working set and the 4 GiB available-physical-memory reserve. Serving additionally
checks listener ownership. A guard failure closes admission and drains sockets.
Every normal serving exit, including the generation-cancellation latch, kills
the complete owned Job and waits up to ten seconds for it to become empty.
Startup failures and owner drop also kill the Job. No automatic restart or
admission reset is supplied. A Job commit limit does not cover every mapped
resident model page, so the physical-memory watchdog remains necessary.

The data/evidence paths must be existing external local directories with no
reparse components. Credentials and diagnostic evidence remain retained for
inspection; no recursive cleanup is attempted. Never share the whole evidence
directory because it contains `frontend.key` and `backend.key`. The existing
PowerShell CPU/LoRA checks retain their separate C# process helper and pins.

This mode supplies ownership and resource limits for development. Restricted
tokens, filesystem access, OS egress denial, prevention of same-user backend
bypass, daily chat/UI and real-model quality/performance remain unqualified.
The Swift assessment and approved LoRA are not silently substituted into the
reference launch mode. See the [launch command](development-guide.md#owned-windows-development-backend).

Dependencies, including httparse 1.10.1 and Tokio 1.50.0, are exact-pinned with a committed Cargo lockfile. Transport enables Tokio's net/sync features and the executable additionally enables runtime/signal support. Native ARM64 Rust
1.99.1 is the checked toolchain. Local work may build and run focused
regressions; the full broker suite runs only in the main-only Foundation workflow.
See [pre-step research](implementation-research.md) for sources and sanity checks.
