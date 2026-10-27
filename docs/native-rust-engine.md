# Native Rust Prism engine contract

[Prism Inference](https://github.com/symbiogenesis/prism-inference) owns the
safe Rust CPU runtime, tokenizer, packed kernels and hybrid graph. Runner
consumes its immutable [release lock](../config/prism-inference.lock.json)
and owns application, broker, artifact provisioning and released-runtime
integration. The pinned native Prism engine is the comparison and fallback.
The canonical [runtime and correctness work](../PLAN.md#runtime-and-correctness)
is maintained only in `PLAN.md`.

The application UI is a separate [pure-Rust native client](ui-architecture.md)
of the broker's local HTTP/JSON/SSE contract. Native UI availability does not
expand runtime qualification. Server/browser UI parity is outside the engine
acceptance contract.

## Swift CPU supported subset

The bounded CPU contract targets the exact Swift Bonsai 2 model on Windows
ARM64, initially the 32 GB Snapdragon X Elite. Runner's pinned `0.1.4` release
contains the bounded source CPU implementation. Release identity and focused
integration do not establish full-context behavior, sustained performance,
creative quality, adapter correctness or daily-service isolation.

The pinned `0.1.4` bundle comes from reviewed main revision
`72a55f66cf395294d95b4e3393932ff2302047dc` and successful
[Foundation run 40405141588](https://github.com/symbiogenesis/prism-inference/actions/runs/40405141588).
It retains sampler candidate storage between tokens without copying scratch into
transaction clones, and reads attention values in 32-channel tiles with at most
2 KiB accumulator payload. Frozen sampler, graph and bitwise attention fixtures
preserve the prior arithmetic and recovery contracts. These changes establish
neither a measured TPS gain nor lower model-level peak RAM.

| Area | Contract |
| --- | --- |
| Model | Exact `Swift-Bonsai-2-PQ2_0.gguf` from the [assessment lock](../config/swift-assessment.lock.json), verified by length, SHA-256 and metadata |
| Weights | Dense `qwen35`; [851 tensors](../config/swift.metadata.json): 353 F32, 96 BF16 and 402 PQ2_0 |
| Execution | CPU only; scalar reference and eligible baseline NEON, with explicit unsupported-operation errors |
| State | One 8,192-token conversation; 16 full-attention and 48 recurrent blocks; FP16 attention K/V, F32 recurrent/convolution state and bounded allocation |
| Input/output | Verified embedded template, exact prompt tokens, supplied reasoning history, declared stops and incremental text |
| Generation | [Inference defaults](inference-defaults.md), optional thinking with `xhigh` effort, production MT19937 draw ordering and native sampler tie behavior |
| Lifecycle | Verified readiness, identical canonical count/generation requests, one active generation, authentication, streaming, cancellation, deadlines, reset and recovery |
| Admission | 15.75 GiB available physical memory, 11.75 GiB Job commit limit and 4 GiB running physical reserve; owner-selected development budget, with full-context resource qualification pending; diagnostic allowances are not launch profiles |

This engine contract uses PQ2 without LoRA, prompt-cache reuse, speculation or
accelerator offload. Current-weight PTQ1, other model families and adapter
execution have separate identities and qualification. The LoRA-enabled standard
profile retains its independent [TPS and quality requirements](swift-lora.md#required-standard-profile-qualification).
The hardware [capability contract](hardware-capabilities.md) reports inventory;
kernel eligibility stays in the runtime. Kernel-level `Auto` is not Runner's
planned qualified-device profile selector.

Bounded Swift numerical comparisons use the unchanged
`1e-3 + 1e-3 * abs(reference)` tolerance for logits, complete logical
recurrent/convolution state and live FP16 KV rows against pinned native
execution. Exact chat/tokenizer and sampler references belong to the source
project's [prompt fixtures](https://github.com/symbiogenesis/prism-inference/blob/main/tests/fixtures/swift-prompt-parity.json)
and [sampler fixtures](https://github.com/symbiogenesis/prism-inference/blob/main/tests/fixtures/swift-sampler.json).
Bounded supplied-history streaming, output-limit termination, seeded replay,
context rejection/recovery, abandonment, owned shutdown and fresh restart have
a narrower scope than full 8K-context/4K-output or daily acceptance.

## Numerical and replacement boundaries

Preserve exact model/adapter identities, decoded scales, activation/cache
precision, transforms, attention topology, sampler and ordinary decoding.
PQ2, verified current-weight PTQ1 and the Qwen reference are distinct lanes;
lossless weights do not imply identical activation arithmetic. Generic PQ2's
fourth symbol remains valid even when Swift's particular census is ternary.

Lossless layout changes, SIMD, tiling, buffer reuse, fusion and scheduling use
the same arithmetic contract. Reassociation and altered reductions/FMA need
declared numerical tolerances. Precision reduction, approximate attention,
pruning, retraining and speculation are separate numerical experiments.
The [algorithm reference](inference-algorithms.md) defines packed-lookup,
attention-rounding, chunkwise recurrence and state-recovery constraints.

Owned Rust model and LoRA tensors are replacement storage; ggml pointer
bindings are not. Device and operating-system calls stay behind audited safe
boundaries. A CPU replacement has no C/C++ inference-library dependency.
Source reuse requires immutable revisions, licensing/notices, hardware
assumptions and correctness evidence.

The performance objective is roughly 2× end-to-end throughput; 3–5× depends on
major baseline inefficiencies and is a stretch hypothesis. Prefill, decode and
completed-task time are separate measurements, with cold load/repacking,
allocation/copying, synchronization, memory traffic and the measured critical
path included. [Orangu's published results](https://mnemosyne-systems.github.io/orangu/doc/server.html#scope)
on CIX P1/Mali use [different activation/repacking choices](https://mnemosyne-systems.github.io/orangu/doc/server-internals.html);
[Camelid's results](https://github.com/timtoole02/Camelid/blob/main/docs/BONSAI2.md)
include speculation. Neither establishes unchanged-arithmetic Snapdragon gains.

## Token and worker API semantics

These generic token-driver interfaces are distinct from the pinned Swift chat
session and its native sampler. Their boundaries remain explicit:

| Interface | Supported behavior |
| --- | --- |
| Generation stops | Verified EOS/EOT/EOM plus ordered caller stops, at most 256 distinct IDs; vocabulary/BOS/padding/insertion declarations are checked. Stops are neither emitted nor committed to history. |
| Raw IDs | Exact order and duplicates, with complete context/output reservation; no text parsing or implicit BOS/EOS insertion. Strict JSON files/stdin have independent byte/token bounds. |
| Text input | Verified `gpt2`/`qwen35` vocabulary, ranked leftmost-tie BPE with exact byte fallback and bounded heap work, pinned Qwen35 Unicode boundaries; no normalization or dropped bytes and no invented boundaries for oversized pieces. |
| Special input | Explicit literal/parse policy; control/user-defined spellings use bounded earliest-start/longest-match scanning, ambiguous spellings reject, recognition inserts no BOS/EOS. |
| Insertion | `--insert-special none\|model`, default `none`; model mode honors declared flags, adds enabled boundaries even when supplied explicitly, and charges them against prompt allowance. |
| Prompt reader | Explicit absolute UTF-8 file or stdin, mutually exclusive with inline input; 1 MiB plus one overflow byte, preserving newlines/BOM. Empty/malformed/failed reads return no partial prompt; sanitized errors retain I/O kind and return immediately, including interruption. Synchronous readers do not own I/O deadlines or cancellation. |
| Text output | Exact original token bytes plus incremental lossy UTF-8 display; at most three incomplete bytes retained, truncation flushed on finish, provisional suffix discarded on reset. Failed pushes preserve prior state. |
| Reference sampler | Explicit seeded temperature/top-k/top-p selection with stable ID ties and defined draws; greedy driver default, checked options before model loading. This generic sampler is distinct from native Swift sampler parity. |
| Tokenization-only mode | Full artifact identity and bounded metadata verified without graph allocation or retained weight payload; whole-file hashing still reads the artifact. |
| Verified sessions | Fixed tokenizer/special-token declarations and exact text/raw-ID accounting; failed requests leave sessions reusable. Output-piece sessions retain a bounded vocabulary and declared control status, supplying independent display streams without retaining the model file. |
| JSONL worker | Fixed verified readiness identity/policy and bounded count/encode operations; malformed requests have value-free recoverable errors, oversized framing/transport failures terminate. |
| Worker client | Exact readiness identity, policy, response shape, correlation, counts and vocabulary/raw sequences checked; protocol/transport faults close admission. Callers own process lifetime, deadlines and cancellation. |

## Transactional state

Generation, graph work and caller-owned cloneable sampling state participate
in one transaction. Admission, sampling, decoder, delivery, cancellation and
deadline failures restore state, including failed graph recovery. Successful
stop draws advance sampling state without entering conversation history.
Absolute deadlines bracket synchronous decoder and delivery work; an in-flight
operation must return before expiry is detected.

Reusable logits and graph scratch never alias independently owned checkpoints.
State-only prompt ingestion omits unused output/logit work while validating
every computed state value; final uncached tokens and decode steps produce
logits, and exact prefix hits restore saved output. Explicit 1–256-token blocks
retain sequential arithmetic and whole-request rollback, with cancellation
and deadline checks between tokens. Sequential blocks do not imply tiled
matrix-matrix execution or measured prefill benefit.

Prepared matvecs share activation preparation only for matching input,
basis/permutation, width and quantizer contracts; mixed Q8_K/Q8_0 and
transformed/plain inputs remain separate. Preparation is scoped to a call.
Full-attention state stores exact binary16 bits and widens them before scalar
products; convolution/GDN stays FP32. Snapshot budgets conservatively charge
every referenced sealed 64-token KV page, page table, mutable-tail capacity
and recurrent state per snapshot, without depending on sharing for admission.

## Acceptance ownership

The [conformance contract](prism-upstream-tests.md) preserves original source
inputs, assertions, tolerances, unsupported results and production bridges.
Full source conformance, generic APIs and accelerator coverage are separate
from the bounded Swift subset. Future release promotions use the
[lock-update procedure](development-guide.md#prepare-a-runtime-lock-update);
reviewed source, immutable released bytes and Runner integration are distinct
acceptance inputs.

Under [validation policy](validation.md), local builds, analyzers, assembly
inspection and focused correctness checks are allowed. Full suites and every
benchmark run only in CI. Device promotion requires reviewed main-only CI on
the actual target; generic hosted ARM64 evidence supplies no Snapdragon
performance verdict. Qualified profiles retain reproducible native fallback,
standard LoRA TPS/quality gates and no material correctness, quality, security
or resource regression. Runtime/backend changes require complete-state phase
handoff and a compatible state/numerical identity.
