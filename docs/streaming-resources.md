# Bounded streaming work and response storage

Runner bounds the work between model output, local HTTP forwarding and native
presentation. These are application I/O changes; they preserve the model,
sampler, numerical contract and speech PCM conversion.

| Boundary | Storage and work contract |
| --- | --- |
| Broker SSE forwarding | Reuses its 8 KiB payload buffer. After the first read, combines only immediately ready payload, with at most 32 reads per group. A pending read releases the available prefix immediately; there is no batching timer. |
| Native chat HTTP decoding | Releases each available payload read without waiting to fill the 8 KiB read buffer or complete a declared HTTP chunk. Chunk endings are checked before the next payload; successful generation still requires complete HTTP framing and terminal SSE events. Chunk-size metadata uses fixed stack storage. |
| Native chat notifications | Admits streaming notifications below 64 queued entries and at most 1 MiB of queued incremental text/reasoning payload. Adjacent deltas of the same kind combine up to 16 KiB without crossing lifecycle boundaries. Reserves 16 further entries for ordered lifecycle notifications; the window processes at most 64 events per frame. Backpressure observes cancellation and the request deadline. |
| Native transcript accounting | Maintains text-byte totals as messages change instead of scanning the whole conversation for each delta. Byte-budget eviction removes one accumulated prefix, preserving the existing trailing-message policy and recovery format. |
| Native speech commands | Bounds queued commands to 64 entries and 1 MiB of payload, with room reserved for control commands. Adjacent same-generation text combines up to 16 KiB. Enforces the existing 32,768 UTF-16-unit stream lifetime before enqueueing, including text already consumed by the worker. Overload invalidates synthesis and reports an explicit speech failure; Stop and shutdown remain nonblocking. |
| Incremental speech segmentation | Scans segments using byte offsets and compacts the final unconsumed text tail at most once per call, preserving Unicode boundaries. Sentence boundaries and the 256-character limit release text during continuous generation; the 700 ms word fallback waits for input to idle. Completion flushes the final tail. |
| Complete speech WAV response | Validates the complete generated clip before success headers. Converts samples through one reusable 32 KiB buffer plus a 44-byte WAV header instead of retaining an additional encoded clip of up to 5,760,044 bytes. |
| Streaming speech PCM | Reuses one 32 KiB payload buffer with fixed spare space for HTTP chunk framing. Each encoded chunk uses one write operation instead of allocating a vector and writing its prefix, payload and suffix separately. |

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

Focused regressions check byte identity, PCM clipping and rounding, bounded
storage reuse, operation counts, pending reads, cancellation and failures. Such
checks measure neither elapsed time nor device throughput. The removed encoded
WAV vector is a storage-lifetime fact, not a measured reduction in process peak
memory. Actual-model latency, allocation/peak-memory captures, combined speech
and chat resource use, accessibility and sustained comfort remain subject to the
[validation policy](validation.md) and the outstanding acceptance work in
[PLAN.md](../PLAN.md#application-and-daily-service).
