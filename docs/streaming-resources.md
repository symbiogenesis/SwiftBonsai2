# Bounded streaming work and response storage

Runner bounds work and storage from model output through local HTTP to native
presentation. These contracts preserve model/sampler numerics and speech PCM
conversion. Limits include JSON escaping and protocol framing where stated.

## Broker HTTP storage

| Boundary | Storage and work contract |
| --- | --- |
| Authorized request intake | Small requests keep coalesced bodies in the fixed 16 KiB header allocation. Larger requests compact only the received payload prefix, reuse the allocation and reserve fallibly to Content-Length, capped at 1 MiB. Reads fill uninitialized final spare capacity, avoiding a separately zeroed whole-body allocation. Read limits exclude trailing requests even when capacity exceeds Content-Length. Authorization, framing, deadlines and cancellation precede backend work. |
| Canonical requests | A bounded writer rejects the first write exceeding 1 MiB and caps requested vector growth there. Counting and generation use identical bytes. Generation releases them after the complete write/flush, before reading response headers. |
| Backend payload intake | Reads into the caller's final storage when staging is empty, at least 8 KiB remains in the destination and that space fits the current Content-Length/chunk payload. Coalesced prefixes and framing tails use buffered reads; chunk endings never enter payload. Direct reads enforce the same empty-read, truncation, sticky-failure and absolute-deadline rules. |
| SSE forwarding | Reuses an 8 KiB buffer, combining only immediately ready payload after the first read, at most 32 reads per group. A pending read immediately releases the prefix; no batching timer. |
| Buffered JSON responses | Reads into uninitialized final-vector spare capacity using fallible geometric reservations capped at 4 MiB, at most 8 KiB per read. A separate one-byte probe detects overflow at the cap. Complete HTTP framing, UTF-8 and JSON-object validation precede client writes. |

## Native chat and transcript

| Boundary | Storage and work contract |
| --- | --- |
| HTTP decoding | Releases available payload without waiting to fill the 8 KiB buffer or finish an HTTP chunk. Checks chunk endings before further payload; success requires complete HTTP framing and terminal SSE events. Chunk-size metadata uses fixed stack storage. |
| Request lifetime | Standard generation and each Extra effort pass own serialized request bytes, releasing them after full write/flush and before response headers. The locally validated original is released before constructing Extra effort stage bodies. Releasing storage preserves canonical bytes, limits, shared stage deadlines and failure policy. |
| Notifications | Admits streaming notifications below 64 queued entries and at most 1 MiB of queued incremental text/reasoning. Adjacent same-kind deltas combine up to 16 KiB within lifecycle boundaries. Another 16 entries are reserved for ordered lifecycle notifications; the window handles at most 64 events/frame. Backpressure observes cancellation and the request deadline. |
| Transcript accounting | Updates text-byte totals per change without scanning the conversation per delta. The 512-message live/recovery limit bounds per-frame traversal even for empty/rejected turns. Count and byte eviction remove one combined prefix, preserving the trailing-message byte policy and recovery format. |
| Native transcript find | Streams scalar-lowercased UTF-8 through a linear matcher with query-sized offset/fallback arrays. Only the current tail retains continuation scratch, so appending an answer scans only its new bytes, including matches spanning appends. Ordinary appends retain completed cache entries and results in place, copying only newly discovered results. Saturated result prefixes skip tail work until a structural/query change requires refresh. Retains no whole-message folded string or per-message-byte offset table. Preserves original UTF-8 highlights, nonoverlapping folded matches and the existing 10,000-result cap with one-match lookahead. Replacements invalidate their cached matches even when the byte length is unchanged. |
| Native transcript highlight layouts | Uses fixed-size body/query revisions and visible match counts instead of duplicating and comparing highlight-range vectors each frame. Find navigation reuses the highlighted text galley and caches only the active result's underline glyph runs, without rebuilding all highlight sections or copying the body. Geometry and theme remain separate cache keys. Appending another message preserves completed layouts; replacement, query changes and newly exposed results invalidate affected highlights. Width and display-scale changes refresh both layout and active underline geometry. |
| Committed history | Shares immutable content/reasoning allocations across worker, UI, queued commits, request snapshots and restored conversations; history vectors remain independent. Extra effort allocates its augmented final user task without copying preceding bodies. Shared ownership preserves recovery and HTTP JSON bytes. |

Extra effort retains drafts/answers through ordered lifecycle events, each bounded
by the 4 MiB response cap. They do not bypass the incremental stream limit.
Committed history keeps its request/response bounds. Unexpected lifecycle-capacity
exhaustion closes the worker without blocking teardown.

## Recovery and startup

| Boundary | Storage and work contract |
| --- | --- |
| JSON serialization | Recovery/outgoing requests stop at 8 MiB/1 MiB encoded bytes, including JSON escapes. Extra effort's temporary encoded material also fits the request limit. Requested vector growth cannot exceed the bound; oversized workspaces reject before complete encoding. |
| Recovery admission | One plaintext snapshot slot is reserved before serialization through encryption/disk completion. A busy worker leaves the workspace dirty without encoding another snapshot. Final save waits for the slot; erase/shutdown retain command ordering. The encrypted format and 8 MiB plaintext bound are unchanged. |
| Recovery decoding | Enforces 16-conversation, 128-committed-message and 512-visible-message limits during deserialization, including inactive snapshots. Excess elements reject before payload decoding. Invalid recovery preserves the live workspace and saved file. |
| Startup tool captures | Valid UTF-8 transfers its bounded allocation into the returned string; invalid UTF-8 keeps replacement-character decoding. Logs write stdout then stderr without a combined string. Each raw stream is capped at 16 MiB. |

## Speech HTTP storage

| Boundary | Storage and work contract |
| --- | --- |
| Request intake | Borrows multipart fields/header lines lazily; rejects the fourth field before parsing without collecting delimiter matches. Reuses authorized HTTP intake storage for the body with exact fallible growth and direct final-storage reads. PCM normalization, supported fields and request limits are unchanged. |
| Microphone upload | Validates WAV framing without copying PCM. Canonicalization builds a 44-byte header and borrows original samples, including extensible WAV. Writes multipart fields, header, PCM and ending separately with checked aggregate Content-Length and cancellation between partial writes. Boundary checks cover both source slices and their fixed-size join. Upload framing leaves scope before inference response reads. |
| Buffered responses | Reuses validated HTTP header storage for JSON/complete-WAV bodies, moves only received payload, grows fallibly to checked Content-Length and reads directly into final storage. Retains 64 KiB JSON/5,760,044-byte WAV limits, framing, deadlines and cancellation. Streaming PCM uses its separate fixed buffer. |
| Fragmented headers | Buffered responses and companion intake scan each delimiter position once, retaining three overlap bytes. Split delimiters/coalesced bodies preserve the first header ending, 16 KiB header bound, framing, deadlines and cancellation. |

## Speech scheduling and playback

| Boundary | Storage and work contract |
| --- | --- |
| Commands | At most 64 queued entries/1 MiB payload, with control-command reserve. Adjacent same-generation text combines up to 16 KiB. The 32,768 UTF-16-unit lifetime bound applies before enqueueing, including text already consumed. Overload invalidates synthesis and reports failure; Stop/shutdown remain nonblocking. |
| Waits | A cold worker with no output, warm service or retained input generation/service blocks on the queue condition variable. Commands/sender closure wake it under the same lock. Active output, warm TTS and retained Canary input use a 50 ms timeout for streaming, expiry, cancellation and health checks. Owned-service memory monitoring is independent. |
| Segmentation | Scans byte offsets and compacts the unconsumed tail at most once/call, preserving Unicode boundaries. Sentences and the 256-character limit release text during continuous generation; the 700 ms word fallback waits for idle input. Completion flushes the tail. |
| Playback | Combines immediately queued text up to 256 characters/request. PCM uses 100 ms frames, 700 ms startup pre-roll and 16 seconds of producer lookahead; supply-speed checks allow the codec's 640 ms bursts. Slow supply/underrun switches to buffering validated complete segments. Staging is capped at 5,760,000 PCM bytes, plus 768,000-byte producer/38,400-byte native queues and two 4,800-byte frame buffers. Short completed segments drain without more text. |
| PCM reuse | Successful WinMM unprepare returns 4,800-byte packets to synthesis through a nonblocking queue. Reuse spans segments without retaining old audio. At most eight spares (38,400 bytes) are cached beyond playback bounds; full/disconnected queues discard excess without delaying Stop/playback. |
| Playback descriptors | At most eight boxed WinMM descriptors circulate between the active queue and an empty spare pool, avoiding per-packet boxes. Successful unprepare releases PCM for recycling and clears every descriptor field. Still-playing/failed unprepare retains both active allocations; teardown keeps bounded quarantine for driver refusal. Spares retain no PCM. |

Speech overload ends the readout and leaves text chat available. Explicit readout
or the next response can start a fresh stream; failure never silently replays
queued text. Superseding commands discard obsolete work through generation
cancellation.

## Native speech generation and streaming

| Boundary | Storage and work contract |
| --- | --- |
| Complete WAV response | Borrows native float samples through their matching allocation owner during validation, PCM conversion and HTTP writes, without a full Rust waveform copy. Success, invalid output and write failure release the owner. The complete clip is validated before success headers. A reusable 32 KiB conversion buffer and 44-byte header avoid an encoded clip allocation of up to 5,760,044 bytes. The pinned 2,876,159-sample ceiling bounds avoided float-copy overlap at 11,504,636 bytes; neither figure is measured process peak memory. |
| Voice bootstrap | The independently owned VoiceDesign waveform survives Design teardown and synchronous Base extraction, releasing on success/failure. Design/Base remain mutually exclusive. The reference must contain 1,920–720,000 float samples. Native ownership avoids a Rust copy and temporary overlap of at most 2,880,000 bytes (2.75 MiB) per profile acquisition. Startup latency and process peak memory remain unmeasured. |
| Streaming PCM output | Reuses a 32 KiB payload buffer with fixed HTTP-framing spare space. Each encoded chunk uses one write, avoiding a vector allocation and separate prefix/payload/suffix writes. |
| PCM intake | Reuses a fixed 16 KiB buffer for HTTP headers, chunk framing and PCM. Parses buffered metadata without byte-sized socket reads; chunk-size lines use ten fixed bytes. Releases available even-byte prefixes immediately, retaining at most one split sample in two stack bytes. Checks cancellation/deadlines even for buffered data; requires exact chunk endings, terminal framing and EOF. |
| TTS decode graphs | The private pinned-source overlay retains at most one talker decode graph context/native allocator workspace rather than up to sixteen attention windows. Obsolete workspaces are freed before replacement, after CPU execution completes. Weights, KV and hidden-state bridge retain separate persistent ownership. Small host staging-vector capacities stay cached; revisiting an earlier window rebuilds its workspace. |

The TTS library documents [native waveform allocation and regression provenance](../src/speech-tts/README.md#native-waveform-ownership)
and [voice-conditioning lifetime evidence](../src/speech-tts/README.md#voice-conditioning).
The companion's [HTTP contract](../tools/qwen-tts/README.md#http-contract) owns
WAV/PCM schemas and wire completion rules.

## Completion and cancellation

HTTP byte limits, framing, absolute deadlines and failure handling still apply.
Clean decoded completion permits the broker's HTTP zero chunk; the client still
requires its terminal SSE reason and `[DONE]`. Speech emits terminal framing only
after validated complete generation. A valid prefix may already have reached the
client when a later backend or write failure closes the response. Stop and
shutdown remain available when presentation has stopped consuming output;
backpressure cannot authorize a retry or replay.

After complete native PCM generation and validation, the speech service disarms
and joins its cancellation watchdog before publishing the terminal zero chunk.
A stop received during a pending socket peek takes precedence over its EOF.
This prevents normal completion closure from terminating a warm speech worker;
disconnects, extra data and deadlines still terminate active inference. Final
framing keeps its bounded write timeout. Focused regressions cover a stop racing
with EOF and consecutive PCM/WAV responses followed by immediate client closure.

## Evidence limits

Focused regressions check byte identity, PCM clipping and rounding, bounded
storage reuse, operation counts, pending reads, cancellation and failures. Such
checks measure neither elapsed time nor device throughput. Avoiding a complete
encoded-WAV vector establishes a storage lifetime, not a measured reduction in
process peak memory. Actual-model latency, allocation/peak-memory captures, combined speech
and chat resource use, accessibility and sustained comfort remain subject to the
[validation policy](validation.md) and the outstanding acceptance work in
[PLAN.md](../PLAN.md#application-and-daily-service).
TTS graph eviction trades cross-window reuse for bounded retained scratch;
the latency cost of rebuilding windows and the complete-model memory benefit
still need target-device qualification. These storage contracts do not qualify a
lower model admission threshold. The Swift development profile uses 15.5 GiB
admission and an 11.5 GiB Job cap, retaining the 4 GiB
physical reserve, memory-pressure shutdown and full context/output reservations.
