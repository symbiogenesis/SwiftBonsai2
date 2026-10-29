# Bounded streaming work and response storage

Runner bounds the work between model output, local HTTP forwarding and native
presentation. These storage and lifetime changes preserve the model,
sampler, numerical contract and speech PCM conversion.

| Boundary | Storage and work contract |
| --- | --- |
| Broker canonical requests | Serializes directly through a bounded writer, rejecting the first write that would exceed 1 MiB and capping requested vector growth at that limit. Counting and generation use identical canonical bytes. The production generation exchange releases those bytes after the complete write and flush, before waiting for response headers. |
| Broker SSE forwarding | Reuses its 8 KiB payload buffer. After the first read, combines only immediately ready payload, with at most 32 reads per group. A pending read releases the available prefix immediately; there is no batching timer. |
| Native chat HTTP decoding | Releases each available payload read without waiting to fill the 8 KiB read buffer or complete a declared HTTP chunk. Chunk endings are checked before the next payload; successful generation still requires complete HTTP framing and terminal SSE events. Chunk-size metadata uses fixed stack storage. |
| Native chat request lifetime | Transfers ownership of serialized request bytes into Standard generation and each Extra effort pass. Releases the allocation after the full write and flush, before reading response headers. The original request used for local validation is released before constructing Extra effort stage bodies. Request bytes, limits, shared stage deadlines and failure policy stay the same. |
| Native chat notifications | Admits streaming notifications below 64 queued entries and at most 1 MiB of queued incremental text/reasoning payload. Adjacent deltas of the same kind combine up to 16 KiB without crossing lifecycle boundaries. Reserves 16 further entries for ordered lifecycle notifications; the window processes at most 64 events per frame. Backpressure observes cancellation and the request deadline. |
| Native transcript accounting | Maintains text-byte totals as messages change instead of scanning the whole conversation for each delta. Byte-budget eviction removes one accumulated prefix, preserving the existing trailing-message policy and recovery format. |
| Native transcript find | Streams scalar-lowercased UTF-8 through a linear matcher with two query-byte-sized offset/fallback arrays. Retains no whole-message folded string or per-message-byte offset table. Preserves original UTF-8 highlights, nonoverlapping folded matches and the existing 10,000-result cap with one-match lookahead. Completed-message search caches remain reusable; a changed streaming body still requires a fresh scan. |
| Native committed history | Shares immutable content and reasoning allocations across worker, UI, queued commit events, request snapshots and restored conversations. History vectors remain independent. Extra effort allocates its augmented final user task without copying every preceding message body. Recovery and HTTP serialization retain the same JSON strings. |
| Native JSON serialization | Recovery and outgoing chat requests stop serialization at their existing 8 MiB and 1 MiB encoded-byte bounds, including JSON escaping. Extra effort's temporary encoded working material is also bounded by the request limit. Requested vector growth is capped at the corresponding bound; oversized workspaces are rejected before constructing their complete encoded representation. |
| Native recovery admission | Reserves one plaintext snapshot slot before serialization and retains it through encryption and disk completion. An occupied worker leaves the workspace dirty without constructing another encoded snapshot. Final-save serialization waits for the slot; explicit erase and shutdown retain command ordering. The encrypted format and 8 MiB plaintext bound stay the same. |
| Startup tool captures | Valid UTF-8 output transfers its bounded byte allocation into the returned string; invalid UTF-8 retains the existing replacement-character decoding. The log writes stdout then stderr directly, without constructing a combined output string. Each raw stream retains its 16 MiB cap. |
| Speech request intake | Multipart fields and header lines are borrowed lazily; the fourth field is rejected before field parsing without collecting all delimiter matches. The authorized HTTP intake allocation is reused for the body, with exact fallible growth and direct reads into final storage. PCM normalization, supported fields and request limits stay the same. |
| Native speech commands | Bounds queued commands to 64 entries and 1 MiB of payload, with room reserved for control commands. Adjacent same-generation text combines up to 16 KiB. Enforces the existing 32,768 UTF-16-unit stream lifetime before enqueueing, including text already consumed by the worker. Overload invalidates synthesis and reports an explicit speech failure; Stop and shutdown remain nonblocking. |
| Incremental speech segmentation | Scans segments using byte offsets and compacts the final unconsumed text tail at most once per call, preserving Unicode boundaries. Sentence boundaries and the 256-character limit release text during continuous generation; the 700 ms word fallback waits for input to idle. Completion flushes the final tail. |
| Native speech playback | Combines immediately queued text up to 256 characters per request. Normalizes PCM to 100 ms frames, with 700 ms startup pre-roll and 16 seconds of producer lookahead. Allows the codec's 640 ms burst interval when checking supply speed. Slow supply or a playback underrun switches to buffering validated complete segments. Application staging is bounded to 5,760,000 PCM bytes, plus the 768,000-byte producer queue, 38,400-byte native queue and two 4,800-byte frame buffers. Short completed segments drain without waiting for more text. |
| Complete speech WAV response | Validates the complete generated clip before success headers. Converts samples through one reusable 32 KiB buffer plus a 44-byte WAV header instead of retaining an additional encoded clip of up to 5,760,044 bytes. |
| Streaming speech PCM | Reuses one 32 KiB payload buffer with fixed spare space for HTTP chunk framing. Each encoded chunk uses one write operation instead of allocating a vector and writing its prefix, payload and suffix separately. |
| Native speech PCM intake | Reuses one fixed 16 KiB buffer for HTTP headers, chunk framing and PCM payload. Metadata parsing consumes buffered bytes rather than issuing byte-sized socket reads; chunk-size lines use ten fixed bytes. Delivers each available even-byte PCM prefix immediately, retaining at most one split sample in two stack bytes. Checks cancellation/deadlines even for buffered data and requires exact chunk endings, terminal framing and EOF. |
| Native TTS decode graphs | The private pinned-source overlay retains at most one talker decode graph context and native allocator workspace, instead of caching up to sixteen attention windows. It frees obsolete native workspaces before building a replacement. CPU graph execution completes before eviction; model weights, KV and the hidden-state bridge have separate persistent ownership. Small host staging-vector capacities remain cached. A request returning to an earlier window rebuilds its workspace. |

Extra effort retained drafts/answers use explicit lifecycle events, each bounded
by the existing 4 MiB response cap, rather than bypassing the incremental stream
limit. Committed history retains its existing request/response bounds. Unexpected
lifecycle-capacity exhaustion closes the worker without blocking teardown.
Speech overload ends that readout and keeps text chat available. A new explicit
readout or the next response can begin a fresh stream; queued text is never
silently replayed after failure. Superseding speech commands discard obsolete
queued work using the existing generation-cancellation boundary.

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

Focused regressions check byte identity, PCM clipping and rounding, bounded
storage reuse, operation counts, pending reads, cancellation and failures. Such
checks measure neither elapsed time nor device throughput. The removed encoded
WAV vector is a storage-lifetime fact, not a measured reduction in process peak
memory. Actual-model latency, allocation/peak-memory captures, combined speech
and chat resource use, accessibility and sustained comfort remain subject to the
[validation policy](validation.md) and the outstanding acceptance work in
[PLAN.md](../PLAN.md#application-and-daily-service).
TTS graph eviction trades cross-window reuse for bounded retained scratch;
the latency cost of rebuilding windows and the complete-model memory benefit
still need target-device qualification. The storage changes do not qualify a
lower model admission threshold. Separately, the owner-selected Swift development
budget uses 15.5 GiB admission and an 11.5 GiB Job cap, retaining the 4 GiB
physical reserve, memory-pressure shutdown and full context/output reservations.
