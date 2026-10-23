# M1 request admission and bounded HTTP I/O

The Rust library in `src/broker/` implements request admission, bounded
reception and response writing on supplied streams. It has no listener, backend connection,
process launcher or production token counter. Chat remains disabled. These are
policy/I/O components, not daily-service qualification.

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
body reader **must already decode HTTP framing**. A future trusted backend
transport must bound upstream headers, reject ambiguous/unsupported framing,
and report short Content-Length or incomplete chunked bodies as read errors.
Passing a raw backend socket would defeat the completion contract. No such HTTP
client or network connection is supplied by this module.

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
error allowance or a substitute for matching rendering. No production callback
is supplied; bytes/characters are never treated as tokens. An admitted body is
immutable. Parser failures return value-free error categories to keep private
input out of diagnostics.

Before any listener is enabled, integrate and qualify the reception/response adapters, private
backend credential and bypass prevention, exact-count transport, single-slot
bounded queue, total deadlines, streaming cancellation, owned process lifecycle,
memory limits and restricted inference identity/filesystem/egress. Offline UI
assets and creative/comfort qualification follow. These controls remain pending
in the existing PowerShell preflight.

Dependencies, including httparse 1.10.1 and Tokio 1.50.0, are exact-pinned with a committed Cargo lockfile. The response adapter adds no dependency. Native ARM64 Rust
1.98.0 is the initial checked toolchain. Local work may build and run focused
regressions; the full broker suite runs only in the main-only Foundation workflow.
See [pre-step research](implementation-research.md) for sources and sanity checks.
