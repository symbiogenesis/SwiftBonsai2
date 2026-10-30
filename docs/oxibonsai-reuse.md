# OxiBonsai source reuse review

Reviewed **2026-10-29** against OxiBonsai
[`1d10c48e14754ff6999158f8fa13c7c92b40b0dc`](https://github.com/cool-japan/oxibonsai/tree/1d10c48e14754ff6999158f8fa13c7c92b40b0dc)
(v0.2.3). The downloaded source ZIP has SHA-256
`dbdf127a1fe8e98d753fdbbad5045190e684ec08a4247a88105ccebadaca2967`.
The comparison control is Runner's
[pinned Prism 0.1.6 source and release](../config/prism-inference.lock.json),
revision `02c8a24b31ac1c35f2ffd3530f6a753ce440bdc0`.

## License and ownership

OxiBonsai's [LICENSE](https://github.com/cool-japan/oxibonsai/blob/1d10c48e14754ff6999158f8fa13c7c92b40b0dc/LICENSE)
and workspace manifest declare **Apache-2.0**, copyright 2026 COOLJAPAN OU
(Team Kitasan). Runner's original code has the same license, so the declared
terms permit copying and modification without relicensing Runner. The reviewed
archive contains no `NOTICE` file or additional license file. This review covers
the selected GGUF/kernel sources; it does not approve every transitive dependency,
downloaded model or optional backend.

For a source import, retain applicable copyright/attribution notices, distribute
Apache-2.0, and prominently identify modified files. Preserve an upstream NOTICE
if one is introduced in a later source revision. See
[Apache-2.0 section 4](https://www.apache.org/licenses/LICENSE-2.0#redistribution).
Engine algorithms and the production loader remain owned by
[Prism Inference](native-rust-engine.md); Runner consumes an immutable released
runtime. A source port there and a reviewed release update here are separate work.

## GGUF: adopt regression scenarios

Useful source references are the
[metadata parser](https://github.com/cool-japan/oxibonsai/blob/1d10c48e14754ff6999158f8fa13c7c92b40b0dc/crates/oxibonsai-core/src/gguf/metadata.rs),
[tensor descriptors](https://github.com/cool-japan/oxibonsai/blob/1d10c48e14754ff6999158f8fa13c7c92b40b0dc/crates/oxibonsai-core/src/gguf/tensor_info.rs),
[streaming parser](https://github.com/cool-japan/oxibonsai/blob/1d10c48e14754ff6999158f8fa13c7c92b40b0dc/crates/oxibonsai-core/src/gguf/streaming.rs)
and [edge-case tests](https://github.com/cool-japan/oxibonsai/blob/1d10c48e14754ff6999158f8fa13c7c92b40b0dc/crates/oxibonsai-core/tests/gguf_edge_cases.rs).
They make hostile declared lengths/counts, duplicate metadata/tensor names,
bounded allocation before input arrives, and nesting depth explicit review topics.

Runner already delegates inspection to `bonsai-gguf.exe`. Its released-tool
checks exercise the actual provisioned binary, independent serialized fixtures,
artifact hashing, bounded string/array inspection and malformed tensor extents.
Keep the current little-endian v3 and explicit Prism-dialect boundary. Adding a
second parser would introduce another format authority without improving this
release boundary. The initial adoption adds the `HostileDescriptors` case in
[`tests/Test-Gguf.ps1`](../tests/Test-Gguf.ps1), automatically included in the
existing Foundation `-Case All` invocation. Independently written fixtures cover
over-limit and `u64::MAX` tensor/metadata counts, duplicate keys/names and
overlapping tensor extents. Positive controls retain distinct metadata keys and
valid out-of-order nonoverlapping tensors. Each hostile probe has a 15-second
deadline and requires the expected parser diagnostic, ordinary error exit and
empty standard output, so a crash or setup failure cannot satisfy rejection.
This protects future runtime updates; it does not change parsing speed or
establish that a previously accepted file is now rejected.

The Oxi parser is not a strict drop-in replacement. Its
[compatibility probe](https://github.com/cool-japan/oxibonsai/blob/1d10c48e14754ff6999158f8fa13c7c92b40b0dc/crates/oxibonsai-core/src/gguf/reader.rs#L146)
documents that a v1 file can be reported loadable although the strict parser
rejects it. Metadata array materialization and default memory mapping also need
an independent allocation and file-lifetime review against Runner's existing
bounded inspection and retained-handle contracts.

## NEON: retain concepts, preserve Swift arithmetic

The [type definitions](https://github.com/cool-japan/oxibonsai/blob/1d10c48e14754ff6999158f8fa13c7c92b40b0dc/crates/oxibonsai-core/src/gguf/types.rs)
and [ternary kernel](https://github.com/cool-japan/oxibonsai/blob/1d10c48e14754ff6999158f8fa13c7c92b40b0dc/crates/oxibonsai-kernels/src/gemv_ternary.rs#L15)
establish different format identities:

| OxiBonsai | Swift/Prism boundary |
| --- | --- |
| `Q1_0_g128`, type 41: one sign bit per weight | PTQ1 uses interleaved base-three digits, not sign bits. |
| `TQ2_0_g128`, type 42: two-bit codes; code 3 maps to zero | PQ2 is type 142 in the pinned Prism dialect; its fourth code means +2. |
| FP32 input with FP32 FMA and horizontal reductions | Swift packed matvecs quantize activations to Q8_K/Q8_0, compute integer dots and apply ordered scales. |
| Qwen3 full-attention graph | Swift's `qwen35` has full-attention plus GDN/convolution state. |

Do not rename types or substitute these kernels for PQ2/PTQ1. Even Swift's
ternary-only census does not make the activation arithmetic interchangeable.
The existing [SIMD contract](simd-plan.md) remains authoritative.

Potential implementation references are
[fused packed decode/dot and independent accumulators](https://github.com/cool-japan/oxibonsai/blob/1d10c48e14754ff6999158f8fa13c7c92b40b0dc/crates/oxibonsai-kernels/src/simd_neon.rs),
[row/cache tiling](https://github.com/cool-japan/oxibonsai/blob/1d10c48e14754ff6999158f8fa13c7c92b40b0dc/crates/oxibonsai-kernels/src/parallel_tiled.rs)
and [prefetch controls](https://github.com/cool-japan/oxibonsai/blob/1d10c48e14754ff6999158f8fa13c7c92b40b0dc/crates/oxibonsai-kernels/src/prefetch.rs).
Prism already has packed scalar/NEON paths. Compare these ideas only when a
measured hotspot justifies a bounded Prism-owned pilot, including original
activation groups, scale order, fourth-symbol behavior and scratch accounting.
Oxi's cache sizes and dispatch thresholds are heuristics, not Snapdragon evidence.

The [float SIMD module](https://github.com/cool-japan/oxibonsai/blob/1d10c48e14754ff6999158f8fa13c7c92b40b0dc/crates/oxibonsai-kernels/src/simd_float_ops.rs#L64)
also needs safety repairs before any direct reuse: public safe operations check
some slice lengths only with `debug_assert!`, then execute raw NEON loads/stores.
Those guards disappear in ordinary optimized builds. Its reciprocal estimates,
FMA and reassociated reductions would need a separately declared numerical path.

One concrete candidate adds detail to the existing CPU work: Oxi's
[two-accumulator loop](https://github.com/cool-japan/oxibonsai/blob/1d10c48e14754ff6999158f8fa13c7c92b40b0dc/crates/oxibonsai-kernels/src/simd_neon.rs#L750)
breaks a serial accumulation dependency. Pinned Prism's
[integer dot loop](https://github.com/symbiogenesis/prism-inference/blob/02c8a24b31ac1c35f2ffd3530f6a753ce440bdc0/src/kernels/neon.rs#L71)
adds eight `dot16` results through one `i32x4` accumulator. A pilot could split
those exact integer contributions across two accumulators and combine before
the final reduction, retaining signed widening and every floating scale/add.
All partial sums fit i32 under the existing 128-weight bounds, so this change
can preserve exact results without importing Oxi's floating-point arithmetic.
Compare only control versus two accumulators on three real Swift shapes,
checking generated assembly for spills and target-device CI for useful benefit.
Oxi's AArch64 prefetch helper is a no-op without its nightly compiler cfg;
the presence of a prefetch API alone does not establish executed hints.

The initial review imported no Oxi kernel or parser dependency. Subsequent
implementation is recorded under [implementation status](#implementation-status);
outstanding qualification stays in [PLAN.md](../PLAN.md#cpu-kernels).

## Additional optimization and data-structure review

The upstream head was rechecked on 2026-10-29 and remains `1d10c48`.
These are source comparisons, without local benchmark execution or measured
Snapdragon gains.

| Candidate | Source evidence and decision |
| --- | --- |
| Parallel projection rows over shared slices | Oxi's live [linear layers](https://github.com/cool-japan/oxibonsai/blob/1d10c48e14754ff6999158f8fa13c7c92b40b0dc/crates/oxibonsai-model/src/layers/linear.rs#L110) call Rayon row-parallel kernels. Pinned Prism's actual [prepared projection loop](https://github.com/symbiogenesis/prism-inference/blob/02c8a24b31ac1c35f2ffd3530f6a753ce440bdc0/src/kernels.rs#L1659) is serial. This is the strongest additional pilot: partition output rows, share packed weights and already prepared activations, preserve each row's arithmetic and use explicit worker budgets. |
| Immutable owner plus byte-range tensor views | Oxi's [parallel kernels](https://github.com/cool-japan/oxibonsai/blob/1d10c48e14754ff6999158f8fa13c7c92b40b0dc/crates/oxibonsai-kernels/src/parallel.rs#L97) borrow weight slices. Prism already owns shared `Arc<[u8]>` tensors, but its separate [generic threadpool](https://github.com/symbiogenesis/prism-inference/blob/02c8a24b31ac1c35f2ffd3530f6a753ce440bdc0/src/backend/threadpool.rs#L188) creates copied per-worker matrix slices and is not the Swift projection path. Extend existing shared ownership when implementing row tasks; do not copy matrix payloads into tasks. |
| Work-sized contiguous row tasks | Oxi's live [Q1 tiled path](https://github.com/cool-japan/oxibonsai/blob/1d10c48e14754ff6999158f8fa13c7c92b40b0dc/crates/oxibonsai-kernels/src/parallel_tiled.rs#L123) amortizes scheduling. Its selector ignores reduction width and guesses AArch64 cache sizes. Choose candidate task sizes from rows, row bytes, available workers and measured work. Its ternary dispatch routes both parallel strategies to flat row parallelism. |
| Borrowed file-backed weight storage | Oxi's [weight loaders](https://github.com/cool-japan/oxibonsai/blob/1d10c48e14754ff6999158f8fa13c7c92b40b0dc/crates/oxibonsai-model/src/model/weight_loaders.rs#L132) borrow checked tensor blocks. This may reduce cold-load copying and private allocations through file-backed storage, but its [engine ownership](https://github.com/cool-japan/oxibonsai/blob/1d10c48e14754ff6999158f8fa13c7c92b40b0dc/crates/oxibonsai-runtime/src/engine.rs#L1455) deliberately leaks containers with `Box::leak`. Any Prism pilot must instead own the mapping and retained verified file normally, account for residency and release storage on failure/shutdown. |
| Small n-gram draft table | Oxi's [bigram/trigram table](https://github.com/cool-japan/oxibonsai/blob/1d10c48e14754ff6999158f8fa13c7c92b40b0dc/crates/oxibonsai-runtime/src/ngram_cache.rs) is used by opt-in Metal/macOS greedy speculation. Useful for the existing model-free drafter plan only after Swift's complete-state verifier. Adapt with a total key/byte cap and only the last two context tokens; Oxi caps successors per key but not total keys, and clones the full context during drafting. |
| Small released-page recycler | Oxi's [standalone page pool](https://github.com/cool-japan/oxibonsai/blob/1d10c48e14754ff6999158f8fa13c7c92b40b0dc/crates/oxibonsai-model/src/paged_kv_cache.rs#L99) illustrates constant-time free-list allocation. Only investigate a small, charged cap on uniquely owned released FP16 pages if Prism reset/replay allocation churn is material. Its full preallocated FP32 pool is a poor fit for one memory-bounded conversation. |

No broadly superior cache structure was found. Prism already shares sealed
64-token FP16 `Arc` pages and keeps private mutable tails. Oxi's
[prefix cache](https://github.com/cool-japan/oxibonsai/blob/1d10c48e14754ff6999158f8fa13c7c92b40b0dc/crates/oxibonsai-model/src/prefix_cache.rs)
is a block-hash trie; its wrapper clones FP32 KV before copying into dense live
storage, has no CLI/server construction call site, and does not include Swift's
recurrent/convolution state. Pinned entries can prevent eviction while insertion
continues; trie nodes are not pruned and reported payload bytes omit trie storage.
Importing it would weaken the existing cache accounting/ownership fit.

The advertised interleaved packing and aligned-buffer helpers have only their
own test callers, not production model callers. Oxi's paged/FP16/quantized KV
helpers are also distinct from its shipping dense CPU cache. Its reusable
sampler buffers and partial top-k selection are worthwhile techniques already
present in pinned Prism; they do not establish another missing optimization.

## Security features and transfer boundaries

The review checked actual serving code as well as helper modules. Runner's
[broker policy](broker-policy.md), [artifact verification](../src/artifacts/README.md)
and [worker containment](sandboxing.md) remain authoritative; source inspection
does not close outstanding actual-model or broker-only isolation qualification.

| Oxi feature | Transfer decision |
| --- | --- |
| [ChatML marker neutralization](https://github.com/cool-japan/oxibonsai/blob/1d10c48e14754ff6999158f8fa13c7c92b40b0dc/crates/oxibonsai-runtime/src/server.rs#L1217) in user-supplied message content, including nested reveal-by-removal tests | Useful regression scenarios for forged turn/role delimiters. Swift renders the verified template then parses special tokens, so qualify content-versus-control handling at that exact boundary. Oxi removes marker text repeatedly; copying it would alter legitimate literal content and counting/template parity. Declare a reject/escape/literal policy before changing behavior. This is not a general prompt-injection defense or an OS security boundary. |
| [Optional per-client/global token buckets](https://github.com/cool-japan/oxibonsai/blob/1d10c48e14754ff6999158f8fa13c7c92b40b0dc/crates/oxibonsai-runtime/src/rate_limiter.rs), trusted-proxy identity and `Retry-After` | Potential additional availability control for rapid request churn, separate from concurrency and queue limits. Disabled in default middleware. For Runner's one local service, prefer fixed-size state and measured need; forwarding headers remain rejected. Oxi's `max_clients` only triggers stale-entry removal, then inserts even if all entries are fresh: it is not a hard cardinality cap. Do not copy that allocation policy. |
| [Process-wide admission and timeout layers](https://github.com/cool-japan/oxibonsai/blob/1d10c48e14754ff6999158f8fa13c7c92b40b0dc/crates/oxibonsai-serve/src/main.rs#L238) | Useful integration-test principle: routes share one budget. Runner already bounds pending accepts, active requests, queue time, I/O and generation lifetime. Preserve cancellation and streaming-lifetime accounting rather than adopting response-header-only middleware as equivalent. |
| Bearer checks, request/output bounds and malformed-model rejection | Already implemented in Runner. Oxi's hand-written XOR-fold comparison is not an improvement over Runner's `subtle` comparison and fixed-format keys. The adopted hostile-GGUF tests cover useful additional release-regression scenarios. |

Oxi's standalone server defaults to `0.0.0.0`, optional bearer authentication,
default wildcard CORS, and unauthenticated health/metrics. Its exported circuit
breaker has no serving-path caller. These settings/helpers do not add Windows
worker isolation, exact artifact identity or stronger default access control to
Runner. Retain the current numeric-loopback, mandatory credentials, exact
Host/Origin/routes, value-free diagnostics and fail-closed worker launch.

## Implementation status

The follow-up implementation used separate Git worktrees and individual commits.
Runner now implements exact rejection of the 33 control/user-defined spellings
from the verified Swift tokenizer, after decoding and concatenating text parts.
The inventory is bound to model and tokenizer metadata identities. Ordinary
lookalikes remain unchanged. The owned service returns a fixed framed HTTP 400
before contacting either backend endpoint and remains reusable.

One fixed-size service-wide budget now admits a burst of eight authenticated
requests and refills one request per monotonic second. Its HTTP 429 response
includes `Retry-After: 1`; an allocation-free, bounded drain preserves complete
delivery to clients still writing the rejected body. Native chat recognizes only
the exact bounded, fully framed errors, restores the draft and retains committed
history without automatic resend. See the current [broker policy](broker-policy.md)
and [native client contract](ui-architecture.md).

Prism's row-parallel implementation is delivered as a
[reviewable source candidate](runtime-candidates/README.md), with its exact
[patch/base manifest](runtime-candidates/oxibonsai-row-parallel.json). It uses
private persistent workers over shared packed weights/prepared activations,
explicit 1-through-16 worker selection and serial fallback/default. The source
commit is based on newer Prism main `ff3d54d`, rather than Runner's older released
pin. The preserved patch was applied in another worktree and its entire Git tree
matched the tested source commit. The runtime source feature is not an activation
or a release promotion in Runner.

Focused delimiter/inventory/parser-parity, burst/refill/concurrency/deadline,
real-loopback error/recovery and native-client framing/history checks passed,
along with affected Release Clippy. The runtime's focused bitwise, borrowed-row,
malformed-payload, graph-state/checkpoint and feature-policy checks, formatting,
Release Clippy and rustdoc passed. Full suites, performance/actual-model device
qualification and the reviewed runtime release remain pending in `PLAN.md`.
