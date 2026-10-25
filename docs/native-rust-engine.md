# Native Rust Prism engine

The experimental [Rust CPU reference](https://github.com/symbiogenesis/prism-inference/blob/main/README.md) implements
verified loading, the Qwen35 hybrid graph and transactional token generation
from token IDs, without a C/C++ inference library. Eligible Windows ARM64
PQ2/PTQ1 operations use baseline NEON automatically; scalar remains the reference
and fallback. The pinned Swift chat/tokenizer, native sampler and experimental
owned broker path are connected. **Bounded Swift full-model numerical parity
and owned HTTP lifecycle are qualified through the recorded source diagnostics;
reviewed prebuilt consumption remains pending.** Pinned Prism remains the
comparison and fallback engine. The [active MVP checklist](prism-upstream-todo.md)
records the narrower Swift evidence; the historical R0–R7 entries below retain
their broader scope.

## Scope

Preserve model weights, decoded scales, activation/cache precision, transforms,
attention topology, sampler and ordinary decoding. Lossless layouts, SIMD,
tiling, buffer reuse, fusion and scheduling are in scope. Floating-point
reassociation needs declared numerical tolerances. Precision reduction,
approximate attention, pruning, retraining and speculation are separate experiments.

The performance goal is roughly 2× end-to-end throughput, subject to profiling;
3–5× is a stretch case requiring major baseline inefficiencies. Neither Rust nor
another engine's results imply a speedup. Measure prefill, decode and completed
task time separately, accounting for bandwidth and the measured critical path.

## Ordered work

For the current Swift-only engine MVP, follow the
[active Prism migration TODO](prism-upstream-todo.md). Only the parts needed to
load, execute and serve the pinned Swift PQ2 model are active; the broader
profiling, optimization, adapter, accelerator and compatibility work below is
deferred until after that MVP. Existing implementation evidence and separate
profile-promotion requirements remain unchanged.

The ultimate deployment goal is [automatic X1/X2 configuration](hardware-profiles.md):
one runtime with safe CPU fallback and qualified per-device CPU/GPU/NPU plans,
selected for the workload and interactive/battery objective. Hardware discovery
design can proceed now; implementation and performance promotion follow the
reviewed CPU MVP handoff. Existing kernel-level `Auto` is not that selector.

- [ ] **R0 — Pin the comparison and audit reusable techniques.** Record exact
  model/adapter identities, tensor types, dimensions, transforms and adapter
  contracts. Audit Camelid, Orangu and packed-kernel sources at immutable
  revisions, including licenses, notices, hardware assumptions and correctness
  evidence. Keep PQ2, verified current-weight PTQ1 and the earlier Qwen reference
  as distinct lanes; lossless weights do not imply identical activation arithmetic.
  Exit with a reproducible baseline and implementation map.

- [ ] **R1 — Profile the ARM64 baseline in CI.** Pin runtime/compiler/options,
  driver, context, batches, sampler, threads and power mode. Separate cold load,
  prefill and decode. Attribute packed operations, transforms, hybrid graph work,
  allocations/copies, synchronization and memory traffic to the critical path.
  Exit with a first kernel target and an Amdahl/bandwidth headroom estimate.

- [ ] **R2 — Complete the Rust correctness reference.** The bounded loader,
  packed scalar operations and Qwen35 graph are implemented. Complete the selected
  artifact's tensor inventory, full-model tokenizer parity, chat template,
  special/stop tokens, sampler/RNG and adapter forward path. Reuse bounded GGUF/codec components and
  Rust-owned LoRA storage; ggml pointer bindings are not replacement tensors.
  Retain malformed-input, allocation and unsupported-contract checks. Exit with
  exact weight/metadata fixtures, scalar parity and bounded full-model comparisons
  against Prism.

  - [x] **R2.1 — Honor declared generation stop tokens.** The token driver
    validates the verified model's EOS and optional EOT/EOM IDs against the graph
    vocabulary, then combines them with additional caller stop IDs under the
    existing 256-token bound. BOS/padding IDs and insertion flags are validated;
    token-ID input remains exact and receives no automatic BOS/EOS insertion.
    Declared stops terminate without emission or entry into conversation state.
    Focused metadata and serialized-model driver regressions cover the contract.
  - [x] **R2.2 — Detokenize generated GPT2-byte tokens.** A bounded vocabulary
    validates retained `gpt2`/`qwen35` metadata and graph vocabulary size. Explicit
    driver text output retains exact token bytes and incrementally renders UTF-8
    across token boundaries, including literal control-token spellings. Invalid
    or truncated UTF-8 uses replacement characters only in the display field;
    original bytes remain available. Synthetic mapping, boundary, malformed-input
    and serialized-model checks cover this output path. Plain-text input is
    implemented below; chat templates and full-model tokenizer parity remain open.

  - [x] **R2.3 — Add an explicit seeded reference sampler.** Bounded
    temperature/top-k/top-p selection, stable token-ID ties and documented RNG
    draws have focused golden/boundary coverage. Explicit driver selection
    reports effective settings and seed. Greedy remains the default; invalid
    options reject before model loading. A serialized-model test checks wiring.
    This reference contract is separate from required Prism sampler/RNG parity.
  - [x] **R2.4 — Implement the bounded byte-pair merge core.** Normal GPT-2 byte
    vocabulary and ordered merge rules are validated. Already pretokenized pieces
    encode by rank with leftmost ties, bounded heap work and exact byte fallback.
    Focused checks cover byte preservation, supplied boundaries, overlapping
    merges, malformed metadata and input/output limits.
    The following items add ordinary-text splitting and explicit special input;
    templates and full-model tokenizer parity remain separate R2 exits.

  - [x] **R2.5 — Implement pinned Qwen35 text boundaries.** The bounded scanner
    follows the pinned Prism implementation and Unicode category data, including
    combining marks, single-number pieces, contractions and whitespace behavior.
    It returns borrowed UTF-8 slices without normalization or dropped bytes;
    oversized matches reject rather than acquire invented merge boundaries.
    Focused fixtures and an independent category-data comparison cover the port.
    A native pinned-source differential also matched boundary contexts containing
    every Unicode scalar and 20,000 deterministic mixed cases; this checks
    boundaries rather than full-model tokenization.
  - [x] **R2.6 — Recognize special input tokens explicitly.** Validated control
    and user-defined spellings use bounded earliest-start/longest-match scanning.
    Literal and parse modes are explicit; ambiguous spellings reject, and no
    BOS/EOS is inserted by recognition. Overlap-oracle, Unicode and resource-bound regressions
    cover recognition independently of ordinary text tokenization.
  - [x] **R2.7 — Connect bounded text encoding to the driver.** `Tokenizer`
    composes special handling, pinned boundaries and ranked BPE with whole-input
    byte, piece and output bounds. `--prompt` accepts plain text instead of
    `--tokens`, keeps special spellings literal unless explicitly enabled, and
    reserves the complete requested output allowance before graph execution.
    Serialized-model checks compare text prompts with exact token-ID input.
    Chat templates and full-model parity remain open; R2.8 adds explicit insertion.

  - [x] **R2.8 — Apply declared BOS/EOS insertion explicitly.** The opt-in text
    encoder honors verified `add_bos_token`/`add_eos_token` flags and charges
    inserted IDs against the complete prompt allowance. Absent/false flags add
    nothing; enabled boundaries are inserted even when the input already has
    them. Existing raw encoding and token-ID input retain their exact semantics.
    The driver exposes `--insert-special none|model`, defaulting to none.
    Focused flag, empty-input, boundary and serialized-driver checks cover the
    contract; chat templates and full-model tokenizer parity remain open.

  - [x] **R2.9 — Share checked generation-stop policy with library callers.**
    `SpecialTokens::generation` combines verified EOS/EOT/EOM with caller stops,
    preserves their order and validates vocabulary, output bounds and the
    256-distinct-stop limit. The driver uses this same API. Focused boundary,
    collision and insertion-policy checks cover the shared contract; full-model
    stop-token and sampler parity remain separate R2 qualification.

- [ ] **R3 — Qualify packed CPU kernels.** Follow [SIMD S0–S4/S6](simd-plan.md).
  Baseline NEON PQ2/PTQ1, automatic selection, scalar forcing and focused exact
  regressions are implemented using safe core SIMD APIs. Profiling, fuller
  per-path memory evidence, optional ISAs and performance acceptance
  remain open. Preserve quantizers, scale order, generic PQ2's fourth symbol,
  tails and malformed-input rejection. Compare [exact ternary LUTs](inference-algorithms.md#exact-ternary-tables-for-decode)
  under the same contract; INT8 table quantization is a separate approximation.
  Exit with focused correctness plus target-device kernel and end-to-end CI evidence.

- [ ] **R4 — Optimize memory and prompt processing.** Use R1/R3 evidence to
  select tiling, lossless packing, scratch reuse, persistent workers and fusion.
  Count repacking, retained copies and peak memory. Preserve immutable LoRA B
  sharing, tensor metadata, adapter basis and forward arithmetic. Changes to
  reductions/FMA require separate numerical qualification; see [SIMD S5](simd-plan.md#delivery-sequence-and-exits).

  - [x] **R4.1 — Reuse logits and graph scratch.** `Decoder::evaluate_into`
    reuses caller-owned output; Qwen35 retains graph/kernel workspace and stages
    state deltas before commit. Checkpoints own independent state/logits.
    Synthetic fixtures cover output validation, reuse, cancellation and rollback.
    A [source follow-up](inference-algorithms.md#scratch-lifetimes) shares
    phase-disjoint attention/FFN projections and gates and convolves QKV in place,
    removing 152 KiB of nominal Swift scratch payload. The provisioned `0.1.1`
    binary awaits a reviewed release/lock update; R4.2 remains unqualified.
  - [ ] **R4.2 — Qualify allocation and memory benefit.** Compare owned-output
    and reusable-storage paths with the same model/workload. Capture prefill/decode
    allocations, peak memory, logits/tokens, prefix restoration and failure recovery.
    Synthetic capacity reuse does not establish process-memory or throughput gains.
  - [x] **R4.3 — Ingest intermediate prompt tokens without logits.** Qwen35
    skips output-only normalization/projection while validating computed state.
    Final uncached prompt tokens and decode steps produce logits; exact prefix
    hits reuse saved output. The compatibility fallback still validates logits.
  - [x] **R4.4 — Share compatible activation preparation.** Scoped prepared
    matvecs reuse exact activation quantization across compatible sibling
    projections. Qwen35 groups FFN up/gate, full-attention Q/K/V and recurrent
    projections by input, basis/permutation, width and activation contract.
    Mixed Q8_K/Q8_0 or transformed/plain groups prepare separately; nothing is
    cached across calls. Synthetic exact-output and preparation-count checks
    cover sharing and recovery. Full-model memory and speed evidence remain R4.2.
  - [x] **R4.5 — Store already-rounded KV as FP16.** Full-attention caches retain
    exact binary16 bits in `u16` vectors and widen before unchanged scalar products.
    Convolution/GDN state stays FP32; state/checkpoint budgets charge actual
    mixed-size capacities. Only KV element payload is halved; device gains remain open.
  - [ ] **R4.6 — Add bounded block prefill.** Prove causal/state scheduling,
    atomic commit and cancellation before comparing [Vec-LUT and chunkwise WY/UT GDN](inference-algorithms.md#chunkwise-gated-deltanet-prefill).
    Declare numerical tolerances and qualify prefill separately from decode.
    - [x] **R4.6a — Establish the bounded reference scheduler.** Explicit
      1–256-token blocks use the decoder's checked block-ingestion boundary.
      The sequential fallback preserves exact per-token arithmetic and checks
      cancellation/deadlines between tokens. Whole-request rollback includes
      failures inside blocks; the final uncached prompt token produces logits.
      Tiled graph kernels and measured prefill benefit remain R4.6 work.

- [ ] **R5 — Complete runtime and broker integration.** Bounded conversation
  ownership, checkpoints, prefix restoration, streaming, cancellation, reset and
  rollback are implemented at token level. Absolute generation deadlines use
  the same transaction rollback; checks surround synchronous decoder work and
  delivery, so an in-progress operation must return before expiry is detected.
  Integrate broker count/completion/
  readiness, authentication, Host/Origin/routes, process ownership and resource
  admission. Retain strict Rust standards; keep OS/device or future unsafe SIMD
  calls behind audited safe boundaries. Exit with native Windows ARM64 builds,
  focused regressions and full CI correctness/security coverage, without a C/C++
  inference-library dependency.

  - [x] **R5.1 — Audit borrowing against the pinned nightly.** The decoder's
    reusable output and scoped `PreparedMatvec` guard already express their
    ownership without new solver behavior. Keep these interfaces and complete
    checkpoint ownership. Prefix restoration now retains the runtime's token
    allocation, an optimization also expressible on stable. In the broker,
    Polonius permits one helper to lend buffered bytes on success and close the
    owned reader on failure, centralizing duplicate I/O/deadline/EOF checks.
    The [compiler probes](https://github.com/symbiogenesis/prism-inference/blob/main/tools/rust-trial/README.md) retain higher-ranked
    associated-view normalization as a next-solver capability; no current
    production backend needs that abstraction. Reassess when a concrete new
    backend or stream interface requires it.
  - [x] **R5.2 — Share immutable KV checkpoint pages.** Sealed 64-token FP16
    pages are shared immutably; the mutable tail and exact convolution/recurrent
    state are copied. Page-boundary and edited-prefix regressions prove snapshot
    isolation and recovery. Snapshot admission conservatively charges every
    referenced page per snapshot, including page tables, private tail capacity
    and recurrent storage, without relying on sharing to meet the bound.
    Full-model process-memory benefit remains R4.2.

  - [x] **R5.3 — Roll back caller-owned sampling state.** Independently
    cloneable samplers participate in generation transactions. Focused
    deterministic tests verify sampling-state restoration on
    admission, sampling, decoder, cancellation, deadline or delivery errors,
    including failed graph recovery. Successful stop draws advance sampling
    state without entering conversation history; existing greedy APIs remain.

  - [x] **R5.4 — Expose verified tokenization without graph allocation.** The
    driver's explicit `--mode tokenize` verifies artifact length/SHA-256 and
    bounded metadata, then returns exact prompt IDs/count and the reserved
    output allowance. It retains no weight payload and constructs no decoder;
    full-file hashing still reads the artifact. Focused malformed-identity,
    metadata-only and context-bound checks cover this command. Persistent broker
    count/completion/readiness integration remains open.

  - [x] **R5.5 — Reuse verified tokenization and exact accounting.** A library
    session verifies an artifact once, builds matching tokenizer/special-token
    declarations, then releases the metadata and file handle. Explicit text
    policies and checked raw IDs share prompt/context/output accounting; failed
    requests leave the session reusable. The text-tokenization command uses this
    path while raw-ID mode preserves its metadata-only contract. Focused
    verified-artifact, repeated-request and reservation checks cover this piece;
    persistent broker transport and chat-template rendering remain open.

  - [x] **R5.6 — Share incremental token text output with service callers.** The
    public decoder and token driver use the same bounded UTF-8 display path.
    At most three incomplete bytes survive a push; finish flushes truncation,
    and reset discards provisional suffixes after cancellation. Failed pushes
    preserve prior state. Focused byte-partition checks match standard lossy
    decoding while vocabulary callers retain original lossless token bytes.

  - [x] **R5.7 — Read bounded text prompts outside process arguments.** The
    driver accepts an absolute UTF-8 prompt file or stdin through an explicit
    `--prompt-file` option. Reads stop at the 1 MiB limit plus one overflow byte;
    malformed/empty input rejects before model verification. Newlines and BOM
    are preserved, input forms are exclusive, and text policies/accounting match
    inline prompts. Focused parser, reader and serialized-driver checks cover
    the contract. Stdin reads remain synchronous and have no input deadline.

  - [x] **R5.8 — Expose a persistent bounded tokenization worker.** An explicit
    `bonsai-token-worker` verifies one artifact, flushes readiness with identity
    and fixed policy, then serves strict JSONL count/encode requests without
    model I/O or graph allocation. Framing, context and response contents are
    bounded; malformed requests have value-free errors and leave the session
    reusable, while oversized frames and transport failures terminate. Focused
    protocol and executable checks cover readiness, artifact release, recovery
    and exact counts/IDs. Chat-template rendering and broker wiring remain open.

  - [x] **R5.9 — Share bounded prompt reading with library callers.** The
    driver and public `prompt_input::read_prompt` use the same 1 MiB UTF-8 input
    contract, consuming at most one overflow byte. Valid bytes remain exact;
    empty, malformed and failed reads return no partial prompt. Sanitized I/O
    errors retain their kind and return immediately, including interruption.
    Focused exact-limit, short-read, infinite-input and privacy checks cover
    this synchronous boundary; callers still own cancellation and input deadlines.

  - [x] **R5.10 — Count and validate exact token IDs through the worker.**
    Strict `count_tokens` and `encode_tokens` requests reuse the verified
    session's vocabulary and complete context/output accounting. Raw sequences
    retain order and duplicates without text parsing or BOS/EOS insertion.
    Focused protocol and executable regressions cover empty/boundary input,
    malformed and mixed request shapes, overflow and recovery after rejection.
    This completes the worker operation; broker wiring remains separate R5 work.

  - [x] **R5.11 — Reuse verified output pieces without graph loading.**
    `TextOutputSession` verifies one complete artifact, builds its bounded output
    vocabulary, then releases model metadata and the file handle. Borrowed
    lossless bytes and declared control status compose with independent
    caller-owned UTF-8 display streams. Focused checks cover artifact release,
    identity/metadata rejection, split UTF-8, controls, independent streams and
    failure preservation. Generation and full-model parity remain separate exits.

  - [x] **R5.12 — Check the persistent worker's client boundary.** A bounded
    synchronous `TokenClient` pins readiness identity and every fixed policy
    field before text count/encode exchanges. Response shape, correlation,
    counts and vocabulary IDs are checked; protocol and transport faults close
    admission, while rejected input remains recoverable. Focused fault checks
    and actual-worker/output-session composition cover the boundary. Callers
    still own process lifetime, I/O deadlines, cancellation and broker wiring.

  - [x] **R5.13 — Complete checked raw-ID worker client operations.** The
    reusable client exposes raw count/encode, validates vocabulary and full
    context/output reservation before transport, and checks returned counts and
    exact sequences. Focused fault and actual-worker checks cover bounded
    framing, correlation and recovery without text parsing or insertion.

  - [x] **R5.14 — Read bounded token-ID files and stdin.** The driver and public
    `token_input::read_tokens` accept strict JSON token arrays outside process
    arguments. Independent byte/token bounds, private failures and pre-read
    option checks preserve exact raw-ID accounting without text insertion.
    Focused reader and serialized-driver checks cover file/stdin parity,
    malformed/empty driver input and rejection before artifact loading.

- [ ] **R6 — Add qualified Adreno/Hexagon paths after CPU parity.** Follow
  [G0–G5](adreno-gpu-plan.md) and [N0–N7](hexagon-npu-plan.md), implementing the
  same packed types, transforms and complete hybrid graph through safe audited
  device boundaries. Investigate X2 packed NPU prefill/decode alongside CPU and
  Vulkan controls; discovery of either accelerator need not wait for the other.
  Reuse Prism capability reports/fixtures, but qualify Rust coverage and safety
  separately for actual X1/X2 device and driver classes. Evaluate resident
  state/scratch, fusion, submission reuse and phase-specific placement, including
  migration, signed runtime compatibility and shared-memory costs.
  Preserve [attention rounding constraints](inference-algorithms.md#attention-locality-and-the-io-bound).
  Exit per backend/device with correct whole-model ARM64 execution and
  target-device CI benefit, or a retained no-go and CPU fallback. Neither
  accelerator bypasses R5 ownership, native Rust replacement requirements or R7
  qualification.

- [ ] **R7 — Qualify each replacement profile.** Run paired, repeated/interleaved
  target-laptop CI comparisons with fixed artifacts/workloads and predeclared
  numerical/performance tolerances. Retain startup/repack, prefill, first-token,
  decode, completed-task, memory, energy, thermal, failure and sustained results.
  Compare logits/recurrent state, deterministic streams where applicable and
  creative quality. Require the [standard LoRA TPS/quality gates](swift-lora.md#required-standard-profile-qualification)
  within Rust, plus Prism/Rust comparisons at the same adapter setting. Accept no
  material correctness, quality, security or resource regression. Publish limits
  and verdict, promote only qualified profiles and retain reproducible Prism
  fallback. Retire old build/overlay/FFI paths only after replacement verification.

  - [ ] **R7.1 — Supply qualified X1/X2 automatic profiles.** Follow
    [H0–H5](hardware-profiles.md#delivery-sequence). Export checked capabilities,
    effective placement and state/numerical compatibility to Runner; keep kernel
    eligibility in the runtime. Qualify separate prefill/decode, thread/batch and
    power choices, deterministic profile selection, stale-driver invalidation,
    memory admission and CPU fallback. Demonstrate each promoted device class
    independently, including complete-state phase handoff and request-boundary
    reevaluation. Automatic selection consumes reviewed evidence; ordinary
    startup does not run a performance search.

R0/R1 scope the first kernel experiment; synthetic R3 work can precede complete
R2 graph parity. R2/R3 precede broader optimization, R4/R5 establish the CPU
replacement, and R7 qualifies each CPU or accelerator profile. Failed experiments
should narrow the next target while retaining useful evidence.

## Research and validation

The [algorithm map](inference-algorithms.md) covers exact LUTs, block prefill,
chunkwise GDN and checkpoint sharing. Audit sources during R0 before reuse.
[Orangu's reported gains](https://mnemosyne-systems.github.io/orangu/doc/server.html#scope)
on CIX P1/Mali use [different activation/repacking choices](https://mnemosyne-systems.github.io/orangu/doc/server-internals.html);
[Camelid's published gains](https://github.com/timtoole02/Camelid/blob/main/docs/BONSAI2.md)
include speculation. Neither is an unchanged-arithmetic Snapdragon forecast.

Follow [validation policy](validation.md): local builds, analyzers, assembly
inspection and focused correctness checks are allowed. Full suites and every
benchmark remain CI-only. Device promotion needs reviewed manual main-only CI
on the target laptop; generic hosted ARM64 establishes no device-performance verdict.
