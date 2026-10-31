# Swift Bonsai 2 Runner plan

This is the single backlog. [Technical docs](docs/README.md) own current behavior,
contracts, commands and evidence; completed work belongs in Git history.

Prioritize actual-model application acceptance, daily-service isolation, creative
quality and comfort while preserving the qualified Swift CPU fallback. Research order:
measure batch-one bottlenecks and PTQ1 suitability; improve packed CPU projections and
prefill; qualify exact prefix reuse and short-chain speculation; then consider precision
changes or compatible drafters. Bounded GPU/NPU access discovery can run in parallel.
Automatic profiles require device qualification; broader Prism work needs a Swift
dependency or chosen milestone.

The chosen parallel accelerator milestone is native Rust Prism Hexagon support:
signed device access followed by a same-artifact packed X2 projection, then
measured CPU/GPU/NPU selection. Qualify X1 independently when competitive; neither
family names nor nominal TOPS choose a backend or displace the CPU gates below.

The long-term migration target is complete C++ retirement in Runner and Prism,
including reference tests, generated code, external inference dependencies and
compiler/build plumbing. Follow the [file-by-file contract and inventory](docs/cpp-retirement.md);
native reference controls are temporary, with independent evidence retained as data.

The next delivery milestone is an accepted daily-use Swift PQ2 CPU profile on the
owner-managed hardware. Close the application, isolation, creative-quality, resource
and installation gates below with reproducible evidence before expanding that profile.

## Research scope and practical budgets

Follow the [research scope, budgets and experiment
checklist](docs/research-register.md#experiment-scope-and-budgets): existing native
Windows ARM64 X Elite/X2 Elite hardware, independent family qualification and a
five-minute total ceiling for owner-device diagnostics, model trials and benchmarks,
including self-hosted CI. Prefer smaller trials; longer qualification remains pending.
Apply the [promotion
criteria](#promotion-criteria) before changing defaults or admission. Full suites,
benchmarks and performance captures stay in owning CI.

## Application and daily service

### Actual-model acceptance

Before further owner-device trials, pass the focused hosted Windows ARM64 Rust
regressions for optional acceptance receipt retention, Find replacement/Unicode
matching and speech-runner policy. Local compilation exhausted its bounded
allowance before those cases could execute; formatting and the focused PowerShell
speech-policy check do not replace them.

The Rust speech tokenizer and GGUF loading/ownership path, greedy sampler and
decode-workspace ownership and compute leases, together with captured
Philox/sampling/tokenizer controls and source identity guards, need the hosted
full Qwen build/link and owning-crate checks. Local model-free token/bit
parity does not close those integration gates. Preserve the captured Windows
ARM64 MSVC exceptional-float behavior when qualifying other compiler versions.

Run the hosted owning-crate Release analyzer, documentation and integration checks
after the standard-library decoding/owned-buffer, signedness, typed-block,
C-string URI and HTTP-framing changes, including the diagnostic/transcript
collection updates. Focused local decoding/ownership, signed seed/thread bounds,
packed replay/rejection, session metadata and exact rejection-header checks pass,
alongside the retained captured Philox/tokenizer and header-fragment controls.
These source-bound checks do not replace full native linking, owning UI tests
or the application acceptance gates above. The CPU-policy, broker and speech-service
Release analyzers pass; the broader tooling analyzer remains pending after its
bounded local attempt timed out draining output.

Run hosted owning-UI Release analyzer/documentation checks and native ASR/TTS
build/link checks after the lending-cache, in-place Find, borrowed HTTP/SSE/PCM,
bounded speech scratch, concrete waveform-owner and source-owned packed-payload
changes. Focused source-bound HTTP/SSE/PCM, speech-cache, speech-request/storage/
waveform-owner, packed replay and broker borrowed-payload/header regressions pass
locally, and the transcript/Find test
sources type-check. Native owning UI and linked broker/service test runs remain
pending after bounded local compilation. Retain hosted
coverage for layout/Find invalidation and speech scratch/error/owner release;
measure adaptive broker forwarding and retained idle speech scratch before
claiming a latency, throughput or process-memory gain. These checks do not close
the actual-model gates above.

The Rust PTQ1 DLL collector also needs the hosted source-bound build/test bundle
and main-only numerical workflow. Its pinned-DLL probe and complete 369-record
ABI/default/configuration comparison pass locally without weights; this does not
close actual-model logits/state comparison, writer-basis or promotion gates.

- Qualify startup, supplied-history streaming, full 8K context and 4K output, long
  conversations, speech, offline restart and recovery. Cover Stop, abandonment,
  deadlines, memory pressure, owned-process cleanup and explicit retry without replay.
  Qualify the opt-in bounded supplied-history and `stalled_consumer` workloads on
  X2 within the shared five-minute total budget under the [acceptance
  contract](docs/validation.md#actual-model-application-acceptance), charging checkout,
  hosted-tool intake and verification against the actual remaining case allowance.
  Skipped requested workloads remain pending; bounded history does not close
  long-history acceptance. Retain context
  rejection, same-instance reuse, termination and usage, Stop, Job and listener cleanup
  and fresh-owner evidence, including completed optional substeps when a later
  request or nonreading pause is cancelled. Fixture size and output reservations
  do not prove exact occupancy or generation; stalled forwarding does not prove
  socket saturation or an active backend at Stop.
- Qualify [transcript and Find](docs/streaming-resources.md#native-chat-and-transcript)
  through appends, replacements, conversation switches and the 512-message limit,
  including retry-heavy sessions and saturated results, cached matches, highlights,
  active underlines, Unicode wrapping, query, width and scale changes, selection
  preservation and recovery. Include queued Find navigation after shortened or
  same-length response replacement and eviction, and identical Unicode query/body
  matching through streamed appends.
- Qualify speech after X1-to-X2 data-directory moves and CPU-feature or Windows changes;
  model-free cache checks are insufficient. Prepare a reduced actual-model workload
  on hosted CI with one aggregate owner-device setup/loading/comparison/cleanup
  allowance; the full TTS comparison remains hosted-only. Review Windows assistive
  technology, keyboard and focus, scaling, high contrast, minimum-window layout
  and multilingual input.
- Complete [signed distribution acceptance](docs/chat-packages.md): binary provenance,
  dependency and license notices, clean-machine ARM64 Visual C++ prerequisites,
  installation, upgrades and rollback.

### Isolation, transport and recovery

- Establish a [restricted inference identity](docs/sandboxing.md#worker-authority):
  read-only approved artifacts, private bounded cache and temp storage, no unrelated
  file or credential access, denied egress and no same-user backend bypass. Qualify MXC
  native loading, private storage and broker-only communication without broader host
  loopback authority. Preserve atomic Job limits, exact worker identity and [native
  integration](docs/sandboxing.md#native-integration-contract), with no weaker fallback.
- Bound worker scratch, PSEC-private temporary storage and stdout/stderr disk usage;
  define retention for diagnostics and credentials across retries. Qualify storage
  exhaustion and owner-loss cleanup while preserving evidence needed for review.
- Qualify the [broker security and transport contract](docs/broker-policy.md): numeric
  loopback, exact Host, Origin and routes, private credentials, rejected tools/MCP,
  dynamic loading and capability overrides, bounds on queues, bytes, tokens and
  duration, cancellation on disconnect or stalled consumption. Exercise the global
  8-request burst, 1/second refill, absolute deadlines, complete HTTP 429 responses
  during writes, constant-space draining, reuse and explicit resend without replay.
- Extend [streaming resource contracts](docs/streaming-resources.md) to actual-model
  device trials: ready-prefix delivery, long transcripts, speech backlog and chat
  overlap, PCM/WinMM reuse, borrowed-PCM uploads, native-owned WAV synthesis and direct
  final-storage HTTP intake. Cover exact fragmented-header limits, maximum WAV fallback,
  buffered prefixes and framing tails, declared lengths, cap overflow, truncation or
  partial delivery, invalid output, failed writes, segment changes, driver refusal,
  deadlines, Stop and recovery. Measure task latency and peak memory; byte and pointer
  identity and work counts alone prove no gain.
- Qualify [control-token
  rejection](docs/oxibonsai-reuse.md#security-features-and-transfer-boundaries) in
  actual template and tokenizer sessions: all 33 Swift spellings, user and system
  content, assistant history and reasoning, split parts, JSON escapes and lookalikes.
  Verify fixed HTTP 400 recovery and identical count/generation requests; qualify the
  reference vocabulary separately.
- Qualify load, allocation-failure and backend-loss recovery: bounded restore attempts,
  unchanged workspace on failure, no restart loops, silent profile changes, corruption
  or automatic cloud fallback.

### Durable qualification evidence

- Establish durable owner-managed archival for the credential-free qualification
  selections under [evidence retention](docs/validation.md#evidence-retention). Bind
  exact files and SHA-256 digests to source/build/device identities, run and artifact
  references and independent reviews, retaining losing/no-go results. Automate capture
  before temporary Actions artifacts expire and rehearse digest-verified retrieval
  without the original runner. Missing archived evidence keeps promotion pending.

## Runtime and correctness

Use the [native Rust contract](docs/native-rust-engine.md). [Runtime
promotion](docs/native-rust-engine.md#default-runtime-promotion) retains the standard
adapter at scale 1.3, binds independent gates to exact source, release and app
identities and leaves missing evidence pending.

- Release the reviewed Windows read-consumer helper API through the registry,
  replace Prism server's exact Git dependency with that reviewed registry release,
  and validate packaging before re-enabling server crate publication. Preserve
  the explicit GitHub binary/source delivery boundary until those gates pass.

- Audit immutable comparison and reuse revisions: model, adapter, tensor and transform
  identity, hardware, numerics, licenses and notices. Keep PQ2, current-weight PTQ1 and
  Qwen as separate controls; respect [OxiBonsai reuse
  boundaries](docs/oxibonsai-reuse.md). Implement justified kernels in Prism.
- Qualify pinned Rust on full-context, long-output Swift: scalar/NEON logits, complete
  KV, recurrent and convolution state, exact prompts and token IDs, sampler, termination
  and lifecycle recovery at `1e-3 + 1e-3 * abs(reference)` tolerance.
- Qualify the [rank-one Rust
  graph](https://github.com/symbiogenesis/prism-inference/blob/c192575cb70be3aa2c4b6e2a5c313c6d4bd68525/docs/rank-one-adapter.md):
  native writer-basis and full-model zero/nonzero logits and state. Integrate
  private-service scale changes and lifecycle before adapter admission. Synthetic or
  source-only execution is insufficient; qualify PTQ1 and other formats separately.
- Produce batch-one attribution for projections, transforms and quantization, attention,
  GDN/convolution, copies, output, sampler, transport and speech. Measure complete-model
  allocations, scratch lifetimes and peaks for reusable logits, state-only ingestion,
  shared activations, physical FP16 KV, checkpoint pages and sampler candidates. Audit
  full-cache replacement, broadcast and transpose materialization and repeated
  preparation; remove the largest avoidable copy or allocation while preserving
  independent snapshots. Qualify long-context tiled attention-value traversal under the
  [overhead
  review](docs/native-rust-engine.md#2026-literature-runtime-overhead-and-work-placement).
- After [packed projections](#cpu-kernels), implement bounded CPU chunkwise GDN prefill
  with a small chunk-size set, declared long-prefix and restore tolerances, causal
  scheduling, bounded scratch, cancellation and whole-request rollback. Preserve
  transforms, normalization, convolution and FP32 state under the [prefill
  contract](docs/inference-algorithms.md#chunkwise-gated-deltanet-prefill); sequential
  blocks alone do not reuse matrices.
- If attention is material, compare grouped-head KV reuse and bounded score/value tiles,
  retaining versus recomputing scores. Preserve globally normalized FP16 probabilities
  before V; online-softmax/Flash changes require separate numerical and continuation
  qualification under the [attention
  contract](docs/inference-algorithms.md#attention-locality-and-the-io-bound).
- If recurrent-state traffic is material, compare short FP32 KVBuffer-style factor
  buffering with sequential updates. Charge reads, materialization and flushes;
  checkpoints and cancellation must include pending updates and convolution. Keep
  writeback deferral separate from precision changes and stop without a complete-cycle
  benefit; see [state-traffic
  research](docs/inference-algorithms.md#2026-state-traffic-and-verification-research).
- Qualify exact prompt reuse across edits, switches, cancellation and restore: matching
  model, template, tokens and complete state, with bounded private snapshots and
  eviction. First qualify latest committed same-session state at bit-identical adapter
  scale: the pinned serving wrapper currently resets every request and defeats the
  generic runtime's live-prefix matcher, as recorded in the [source
  audit](docs/native-rust-engine.md#bounded-parallel-row-qualification). Require
  fresh-recompute parity, switched/edited histories, cancellation/rollback,
  scale-change/change-back and fault/explicit-reset recovery before changing that
  serving contract. Start with immutable KV pages, exact suffix replay
  and simple lookup and eviction before radix trees or sparse placement. Resolve total
  versus incremental accounting: two 149.625 MiB recurrent/convolution snapshots already
  cost **299.25 MiB before KV/metadata**, exceeding a 256 MiB total cache. Keep that
  budget and conservative per-snapshot page charges.
- Build a short-chain CPU block verifier before proposals: complete state, partial
  acceptance, EOS and output limits, Stop, failed commit and rollback. Establish greedy
  parity, then conditional acceptance and residual sampling for actual temperature,
  top-k, top-p and penalties. Distribution preservation does not promise identical RNG
  use or seeded output.
- After verifier qualification, try bounded prompt/history suffix proposals with a few
  short windows and no second model. Compare repetitive and creative held-out prompts,
  charging drafting, verification, commit and recovery together. Retain ordinary
  decoding; stop if acceptance cannot repay the cycle.
- If chain recovery is material, compare snapshot/replay, saved factors and delayed
  commit using
  [SpecLA/TreeWY/RecoverSSM](docs/inference-algorithms.md#trained-parallel-proposals-and-full-state-verification).
  Preserve complete state and declared float identity; wide trees must beat chains.
- Only after a verifier leaves a feasible measured break-even, screen a published small
  drafter for exact tokenizer and vocabulary, Swift/LoRA and native ARM64 compatibility;
  check target-feature interfaces when consumed. Charge both models' weights, KV,
  features, scratch, startup and prefill before provisioning. Suffix failure does not
  rule out learned drafters; Qwen naming does not prove compatibility. Record no-go if
  none fits; exclude general training and DSpark.
- Qualify replacements against matching pinned-Prism artifacts, adapters and settings
  and all promotion gates, retaining deterministic streams where applicable. Retire old
  build, overlay and FFI paths only after acceptance.

## Complete C++ retirement

Use the [retirement contract](docs/cpp-retirement.md) to track each file's behavior,
replacement evidence, source identity and remaining consumers. Source deletion does
not close pending hosted integration or full-model acceptance.

- Replace the remaining native stochastic sampling pipeline and CodePredictor's
  separate GGML sampling graph with qualified Rust execution. Preserve target-specific
  top-p tie ordering, sequential `expf` arithmetic and accumulation, Philox draws,
  exceptional floats and the captured masking/penalty/greedy controls. Qualify actual
  pinned tokenizer vocabularies as well as the complete model-free scanner/ID captures.
  Qualify Rust graph-cache resource ownership and synchronized compute leases in
  the full hosted Qwen consumer, including native graph-build failure, cancellation
  and model lifecycle recovery. Qualify the Rust tokenizer loading/ownership
  boundary in that consumer and actual-model speech lifecycle; model-free metadata,
  callback and lifetime checks do not close those integration gates.
- Retire Prism's native reference families file by file after matching hosted replay
  of their source-level assertions. Preserve all approved green IDs, full geometry
  schedules, corrections, unsupported outcomes and numerical/target boundaries.
  Remove dispatch, native ABI probes, build/cache/source-download paths and linkage
  only when their retained obligations have replacements.
- Replace Qwen/GGML TTS and Canary ASR's `transcribe-cpp` inference dependency with
  native Rust model/audio execution, qualifying exact artifacts, token/PCM/state,
  cancellation, lifecycle, quality and resource contracts. Then remove compiler,
  CMake, static/native library and source provisioning paths. Retire publisher and
  native DLL reference execution after immutable controls, Rust acceptance and
  fallback/promotion gates cover their consumers; moving C++ behind FFI does not
  complete retirement.
  The checked-in C++ collector is retired, but its Rust replacement still invokes
  the pinned native DLL engine. Preserve the complete source-bound ABI preflight
  and independent full-state numerical comparison until that engine is retired.

## Creative quality, adapters and memory

### Quality and speech

- Establish an independent writing and art-direction rubric: drafting, constrained
  revision, continuity, dialogue, voice and tone, composition, instruction following,
  coherence, repetition, unwanted refusals and useful completed answers. Qualify
  Thinking, output settings and Extra effort; changed text or benchmark averages are
  insufficient.
- Complete [standard LoRA
  qualification](docs/swift-lora.md#standard-profile-qualification): SSM writer-basis,
  loaded-at-zero response and logits identity, nonzero application, paired device trials
  and independent human reviews. Require both same-engine decode TPS and quality parity,
  bounded RAM premium and visible regressions. Qualify app scale 1.3 and other intended
  scales separately from the scale-one, 512-token contract; one response cannot choose a
  scale.
- Compare Q8_0/Q4_K_M and bounded speech thread choices through [paired device CI and
  independent listening](docs/validation.md#paired-speech-comparison). Retain quality,
  startup, first-audio, synthesis latency and resources; attribute bootstrap, warm Base
  prefill, residual-code prediction, codec, transport and playback. Distinguish
  generated PCM from audible output, supply gaps and underruns; use [existing resource
  evidence](docs/performance-expectations.md#q4-speech-resource-comparison) and reuse
  contracts to choose remaining work.
- Qualify [native-owned VoiceDesign
  references](docs/streaming-resources.md#native-speech-generation-and-streaming)
  through Design teardown and Base extraction on first use, profile changes and
  bootstrap failure. Include Rust parameter defaults, CPU thread selection,
  reserved-token suppression, repetition penalties, Philox semantic and predictor
  sampling and Rust-owned speech tokenization in actual-model, listening and resource
  acceptance. Cover tokenizer initialization/rejection, worker teardown, exact utterance,
  instruction and Base reference token IDs; model-free token/vector/C++ field and bit
  comparisons are insufficient.
- Qualify chat alone, warm TTS and overlap with a small shared thread-budget set: UI
  frames, audio continuity, useful-answer time, sustained memory and heat. Check cold
  idle separately from active, warm and Canary monitoring, including wake and Stop
  during the 30-second TTS lease. Preserve [ASR/TTS exclusion and
  reserves](docs/streaming-resources.md#speech-scheduling-and-playback).
- Defer [speech
  speculation](docs/performance-expectations.md#2026-speech-inference-research) until a
  compatible native artifact or laptop-feasible tiny fit exists for selected 1.7B (not
  paper 0.6B). Before fitting, require residual-code verification, sampler correction,
  accepted-state recovery and bounded scratch; stop if costs erase complete-readout
  benefit.

### Memory and profiles

- Investigate lossless adapter reservation and rank-one accumulation with allocator and
  strict rounding and order checks. Resolve counter discrepancies and preserve dynamic
  scale and load-without-apply under [allocation
  rules](docs/swift-lora.md#allocation-semantics).
- Complete [PTQ1 release qualification](docs/swift-memory.md#release-decision) beyond
  the bounded Auto numerical and native-service smokes. Qualify full-model
  scalar/explicit-NEON logits and live state, no-LoRA/loaded-zero identity and nonzero
  application, immutable adapter writer-basis provenance, then stable-condition
  paired complete responses at app scale 1.3, context/output and recovery, quality
  and comfort with PQ2 rollback. Promote or reduce admission only after suitability,
  using the verified
  **1,259,520,000-byte** packing difference. Account for mapped residency separately
  from private commit and preserve reserves.
- Capture prefill, first useful answer, decode TPS, task time, retries and truncation,
  working, private, Job, system and device memory peaks, hard faults, observable energy,
  responsiveness and sustained heat and fan behavior. Retain X2 `ProcessMemory`
  aggregates and qualify complete-model coverage and model-phase attribution beyond
  observed stdout boundaries. Follow the [measurement
  rules](docs/performance-expectations.md#2026-literature-measurement-and-comparison):
  repeated interleaved pairs with uncertainty, independent X1 and X2 results,
  instrumented attribution apart from timing, idle versus active power and consistent
  token boundaries, energy per acceptable task with output and retry counts. Overlapping
  counters are not additive; sampled maxima can miss brief allocations.
- Qualify creative, daily, long-context and battery profiles within total budgets.
  Larger context, Q8/Q4 KV, Flash Attention and recurrent precision need separate
  numerical, state and quality gates; preserve canon and output reservations.
- Attribute mappings, microbatch 32/64, persistent output rows, post-prefill trimming,
  prefetch, rotation/sign metadata and stacks before [lossless
  reductions](docs/swift-memory.md#mapping-and-startup). Review immutable-B sharing
  against the intended runtime, preserving rounding and distinguishing overlapping and
  disjoint allocation lifetimes.
- Qualify Qwen loading and service control independently when its 18 GiB allowance plus
  4 GiB reserve is admissible.
- Add source-referenced scene packets and author-approved context. Manuscripts and
  edited project bible stay authoritative; start with manual excerpts or lexical
  retrieval, embeddings only for demonstrated need. Summaries require source versions
  and author approval; vision, image critique or generation needs a matching model and
  shared-memory budget.

## Bounded calibration and optional precision

After exact CPU controls, use small fixed calibration samples, disjoint held-out
prompts, bounded statistics and [research
budgets](#research-scope-and-practical-budgets). Retain full-context, continuation and
LoRA gates; calibration proves neither quality nor reduced admission.

- Estimate process savings and traffic against PTQ1 first: 8K FP16 attention KV is 512
  MiB, so INT8 saves at most ~256 MiB before scales and scratch; recurrent/convolution
  state is separately 149.625 MiB. Cache ratios do not apply to weights.
- Screen native Q8 KV, then at most one justified asymmetric K/V or rotation-plus-scalar
  candidate. Keep FP16 controls, explicit numerical and cache identity and exact
  checkpoints; charge metadata, conversion, rotations, dequantization and retained
  copies. Defer SVD or index schemes without an 8K need; follow [compression
  constraints](docs/inference-algorithms.md#compression-and-approximation-constraints).
- Only for material state traffic or storage, compare FP16 recurrence with FP32, then at
  most one DAMP-inspired FP16/INT8 split. Keep convolution separate; charge scales,
  rounding and RNG, restore and trajectory error. Add low-rank bypass or directional
  rounding only if update loss and net benefit justify factors, merges and rollback.
- Check packed entropy and correlation including scales, indexes, random access and
  decoding. Try Shannonic-inspired streams only if executable bytes and traffic beat
  PTQ1 without retained expansion; current-census BITCOS remains rejected. New
  codebooks, rotations or scales need native layerwise cost and benefit preflight, not
  whole-model fitting from GPU results; see [weight/state
  research](docs/swift-memory.md#2026-entropy-coding-and-alternative-weight-artifacts).

## Hardware selection

- Extend [shared Doctor and launch
  discovery](docs/hardware-capabilities.md#host-inventory) with checked SoC/SKU, GPU/NPU
  APIs, features, architecture, drivers, runtime and firmware, signed readiness and
  static identity. Keep unknowns explicit and live budgets separate from compatibility.
- Register a run-specific ephemeral owner-managed ARM64 runner before the bounded
  [X2 thread screen](docs/cpu-kernels.md#x2-cpu-thread-screen); the current inventory
  has none. Qualify the complete short screen within one shared setup, verification,
  loading, comparison and cleanup budget; leave broader thread choices pending.
  Resolve legacy `0xC0000142` startup with shell DLL audits and bounded `--version`
  probes under unchanged mitigations. Extend the [exact-candidate X2
  model smoke](docs/runtime-candidates/README.md#explicit-x2-candidate-model-smoke)
  to Unicode model paths, effective thinking control, continuation/state, adapters
  and cancellation qualification. Integrate and release the [headless UCRT
  candidate](docs/runtime-candidates/README.md#headless-ucrt-argv-candidate) upstream;
  qualify its exact binary before repinning Runner.
- Qualify scalar/NEON, optional ISA and phase-specific thread, batch, wait and power
  choices independently on X1/X2. Screen settled `main`; extend only for measured
  opportunity. Suppress comparisons if CPU, kernel, features, topology or power change
  or are unknown, including between pairs, under [screen
  controls](docs/cpu-kernels.md#controls-and-cost).
- Compare event waits with polling for idle CPU, wake latency, decode and cancellation.
  Inventory Windows sampling limits for timing, energy, heat and memory; require valid
  telemetry for energy verdicts. Core counts and family names do not choose profiles;
  bare-metal gang scheduling stays outside the app.
- Build the bounded reviewed [profile
  catalog](docs/hardware-capabilities.md#profile-and-evidence-keys): compatibility,
  workload, reuse and objective keys, paired evidence, thresholds and losing/no-go
  entries. Implement deterministic [eligibility, ranking and
  admission](docs/hardware-capabilities.md#eligibility-ranking-and-admission) for task
  latency and battery energy, charging startup, repacking, compilation, migration and
  sustained costs. Use stable ties without startup benchmark search.
- Recheck budgets and readiness before admission; use qualified CPU fallback or strict
  rejection without changing model, adapter, sampler, precision, context or output.
  Report requested/effective profile, placement, evidence and value-free fallback
  reasons in diagnostics and a simple native Rust Auto control.
- Reevaluate safely after power, memory, runtime or driver changes: invalidate evidence
  and caches, migrate complete state or tear down and rebuild, never partial requests.
  Cover unknown devices, absent ISA/accelerators, stale identity, pressure, workload
  boundaries, ties, objective changes, strict rejection and failed or cancelled handoff.
- Demonstrate [automatic interactive and battery
  operation](docs/hardware-capabilities.md#automatic-profile-selection) independently on
  X1/X2, including installation and rollback, sustained use and upgrade requalification.

## CPU kernels

Follow [CPU arithmetic, dispatch and acceptance](docs/cpu-kernels.md). Pin artifacts,
runtime, compiler, format, shapes, ISA and numerics; choose work from CI hotspot,
bandwidth, packing and dispatch evidence.

- Qualify actual prompt/response latency and complete application lifecycle for the
  [shared-slice row-parallel candidate](docs/oxibonsai-reuse.md#implementation-status),
  beyond the retained [fixed-step and short service evidence](docs/native-rust-engine.md#bounded-parallel-row-qualification).
  Preserve shared packed weights and prepared Q8 activations, complete state,
  cancellation, pool/task accounting and whole-request cost. Leave further shapes,
  formats and worker budgets pending; do not split a longer matrix across short jobs.
  Review exact source and released bytes before new selections; fixed-step timing
  and one short service stream do not qualify prefill, decode TPS or defaults.
- Qualify full-model and memory behavior of NEON PQ2, odd-group PQ2 and PTQ1. Preserve
  scalar forcing, diagnostics, fourth-symbol support and [intermediate LUT and
  correction bounds](docs/cpu-kernels.md#packed-arithmetic) before narrowing. Prototype
  direct PTQ1-by-Q8_K scalar on a few shapes, then NEON, preserving grouping, raw
  scales, quantized activations, float order and odd-group fallback. Keep its numerics
  separate from PTQ1-by-Q8_0 through full-model and adapter qualification.
- Compare at most four NEON layouts on three shapes, one matrix at a time, unpacking
  into consumption order without VNNI. Compare at most two exact TL1_1/TL2_1-style LUTs
  with ternary eligibility and generic PQ2 fallback; charge preparation, overlapping
  source and repacked storage, tables, cache and startup.
- Implement one tiled projection-prefill family, initially valid token tiles 8/16/32,
  shared activation preparation and bounded LUT/weight tiles without persistent
  INT8/FP16 expansion; stop if construction or scratch erases gains.
- Establish installed Windows CPU/OS NEON/dotprod/I8MM eligibility before dispatch.
  Inspect Release code generation and forbidden-ISA leakage and require detection,
  parity and benefit under the [dispatch
  contract](docs/cpu-kernels.md#execution-and-dispatch).
- Optimize measured transforms, elementwise work, quantizers, reductions and prefill
  through safe APIs or audited isolated unsafe code. Cover edges, malformed input,
  rollback, non-ARM and absent ISA; recheck dependencies, licenses and codegen.
- During planned tiled prefill, attention and chunkwise GDN work, check whether
  signed padded/relative index mapping introduces repeated floor division by a fixed
  non-power-of-two constant. Consider [Joffe's algorithm](docs/cpu-kernels.md#signed-constant-floor-division)
  within that measured parent hotspot; existing 8/16/32 tiles already permit shifts.
  Choose tile sizes for parent-kernel benefit; preserve unsigned PTQ1 decoding,
  floating quantizer rounding and runtime-stride semantics. The current audit found
  no eligible Runner caller;
  implement a justified consumer in Prism with full-range integer parity, pinned-Rust
  ARM64 codegen and paired complete-request CI evidence before repinning. Keep this
  conditional on useful parent work, without a standalone utility or benchmark project.
- If integer-dot dependencies are material, screen [two
  accumulators](docs/oxibonsai-reuse.md#neon-retain-concepts-preserve-swift-arithmetic)
  in Prism's `i32x4` loop on three shapes. Preserve widening, exact dots, Q8 groups and
  float order; inspect spills and require device CI benefit before repinning.
- Compare scalar Rust, optimized Rust and Prism in paired device CI, separating
  hot-cache and streaming weights, preparation, dispatch, prefill, decode and task
  costs, allocations and sustained energy and heat. Scope profiles by device, format,
  shape, phase and objective.

## Adreno Vulkan

Follow [Adreno contracts](docs/adreno.md); discovery alone changes no CPU profile.

- Capture native GPU, driver and API features, limits and [memory
  telemetry](docs/hardware-capabilities.md#native-vulkan-observations), rejecting
  emulation and preserving unknowns and conservative admission. Resolve X2 `host-gpu`
  headless-worker `instance-creation` failure (`VK_ERROR_INCOMPATIBLE_DRIVER`), using
  retained stage/result diagnostics and bounded current-worker module snapshots;
  resolve protected GDI32 reference lookup error `126` despite module-snapshot
  presence; the count-only graphics query was not reached. Do not treat that
  lookup failure as proof of the Vulkan failure cause or ICD identity;
  retain bounded on-disk loader/new-module hashes for dependency review. Module names
  and on-disk hashes do not identify failed DLLs, bind mapped-image bytes or establish
  signature and dependency provenance.
  Tool-policy X2-90 observations neither explain it
  nor qualify worker access.
- Pilot one native ARM64 OpenCL packed projection candidate and Swift shape in Prism:
  scalar PQ2-by-Q8_K control, exact grouping, raw scales and float order, separate PTQ1
  lane. Prove query and workload access under unchanged worker mitigations; refusal is a
  no-go. Follow [access/source
  boundaries](docs/adreno.md#opencl-access-and-source-audit), hard 30-minute feasibility
  and two-hour initial-CI caps. Charge preparation, scratch and transfers; preserve
  reserves. Exclude whole-model expansion and profile selection.
- Build a separately locked pinned-Prism `GGML_VULKAN=ON` candidate with reviewed ARM64
  SDK, shader, compiler and library identities, unrelated accelerators disabled and
  bounded experimental profile preserving service policy and CPU lane.
- Complete the PQ2/PTQ1 operation, placement, fallback and transfer ledger and
  descriptor checks without limit overrides. Qualify packed operations, full logits, KV,
  GDN and lifecycle, awkward shapes, Vulkan Q8_1 versus CPU Q8_K/Q8_0 and separate
  no-LoRA and approved-LoRA basis and forward lanes under [arithmetic/placement
  rules](docs/adreno.md#swift-arithmetic-and-placement).
- Compare CPU, partial and maximum feasible offload and eligible Hexagon, separating
  prefill and decode, batches, threads, waits, preparation, packing, submission,
  transfers, GPU service, sampling and migration; distinguish slow kernels from host
  starvation. Qualify attention, cache and Flash changes separately.
- Optimize measured kernels, layouts, residency, descriptors, pipelines, allocations,
  scheduling and caches: packed and coalesced layouts, vector unpacking, shape tiles and
  supported replay; textures require native support. Charge cold preparation and copies,
  count physical memory once, bound caches and preserve asynchronous lifetimes. Rerun
  numerical, state, adapter and lifecycle gates.
- Qualify device benefit, quality, comfort, memory, admission and effective-plan
  reporting, absence or loss, pressure, stale evidence, cancellation and upgrades and
  committed-state CPU rollback under [resource/failure
  rules](docs/adreno.md#resources-failure-and-acceptance). Qualify Rust GPU coverage and
  device safety separately after Rust CPU parity.

## Hexagon NPU

Follow [Hexagon contracts](docs/hexagon.md).

- Complete independent X1/v73 access and X2/v81 native compute, deployment,
  license/resource and effective-placement qualification. Bind the
  [driver-bound generic X2 dispatch](docs/evidence/hexagon-x2-driver-bound-dispatch.json)
  and [native FD cycle](docs/evidence/hexagon-x2-native-fd-cycle.json) to independently
  observed selected DSP bytes, mapping modes and teardown; neither identifies the
  complete device tuple. Verify DMA64 and effective VTCM against matching signed
  binaries. Trust/Secure Boot changes need a separate owner decision.
- Qualify the separate [newer signed Q2 candidate](docs/hexagon-signed-q2-candidate.md)
  before choosing a packed route. Its v0.10.0 archive, seventeen runtime files and
  five catalog members are [verified as data](docs/evidence/hexagon-signed-q2-package.json);
  its changed seven-argument start ABI, queue ownership, actual operation support
  and Q8_1 numerical path remain pending. Compare the bounded guarded integer-dot
  composition with a directly fused packed kernel; ordinary Q2/Q8_1 substitution
  does not preserve Swift Q8_K/per-128 FMA. Custom kernels still require a supported
  signing/distribution operator, but they are not the only signed route to research.
- Review and release the experimental native Rust capability/terminal-buffer boundary
  prepared in Prism without changing its publication boundary. After the successful
  fixed module-open/4 KiB FD map/unmap/free/close cycle, qualify native computation,
  delayed/extended mapping, asynchronous ownership and complete vendor teardown.
  The explicit reference initializer requests an unsigned DSP process class while
  retaining Windows signed-package/catalog checks; it does not establish effective
  process class or authorize OS trust changes. Preserve the independent STATIC512,
  default-open14 and old mutable-input guard observations without assigning causes.
  Bind generated QAIC host marshalling/wire controls to exact SDK/tool and imported
  IDL identities under the [queue provenance gate](docs/hexagon-signed-q2-candidate.md#native-rust-computation-requires-another-reviewed-boundary).
  The [hosted SDK recipe review](docs/evidence/hexagon-sdk-inventory-41168872514/provenance.json)
  retains the host command template. The [third data review](docs/evidence/hexagon-sdk-functional-41171339745/provenance.json)
  observes matching normalized remote/AEEStdDef IDL tokens and selected queue
  declarations, while whole queue/remote headers differ. Bind the SDK's unreviewed
  `remote_work.h` and effective Windows conditionals/layout/calling conventions
  and import closure before Rust invocation; the public Windows `remote_ext.h`
  input also remains unpinned. Establish applicable SDK tool/output terms; the uncertain NOTICE
  decoding and keyword observations establish no grant. The [public generated-C/Rust
  wire control](docs/evidence/hexagon-public-qaic-41171859264/provenance.json) passed
  for one fixed 48-byte start frame and null-argument stop on hosted Linux LP64,
  replaying pinned dependency inputs. Preserve its exact source/compiler identities
  when extending coverage; it does not qualify SDK/public equivalence, Windows
  ARM64 ABI, queue ownership, signed device computation or whole-model behavior.
  Preserve the reviewed Windows explicit-zero/read-noblock route and symbolic
  timeout default. Compare public infinite-wait, FD/cache/callback/import-close
  contracts and authoritative u64 close against the matching SDK; the reference
  wrapper's narrowing pointer typedef is not an ABI to reuse.
  Implement the reviewed queue/IDL, correlated responses, bounded waits, in-use
  buffer retention and stop/drain contract before submitting a device kernel.
- Start with `blk.0.ffn_up.weight`, K=5120/M=17408/N=1, keeping transforms, adapter
  corrections and committed state on CPU. Consume the [verified original projection
  and signs](docs/evidence/hexagon-swift-projection-binding.json) through the
  [CPU operation/wire controls](tools/hexagon/packed-projection.md), then bind signed
  device outputs, actual activations, logits and complete state before timing or
  expanding placement. Preserve Q8_K preparation and original per-128 FMA; qualify
  PTQ1 canonicality/Q8_0 independently. The model-free PQ2/Q2 weight-layout control
  adds 419,840,000 bytes for a whole packed copy and is not a production bridge.
  Extend actual placement/fallback/transfer accounting, including the guarded
  primitive's additional weights, activations, returned limb dots and CPU reduction.
  Effective VTCM, DMA descriptors, session/queue/mapping costs, all host scratch and
  resources outside Job commit remain admission charges; the proposed 18,752-byte
  fused double-buffer reserve covers only its declared tiles.
- Evaluate HVX packed work and HMX batched prefill with bounded VTCM, DMA and repacking
  scratch, exact fusion/recurrent basis and independent adapters. Cover ring saturation,
  partial tiles, double buffering, requested and effective VTCM, allocation refusal and
  transfer cancellation. Use the documented DMA-ring bug as a regression pattern without
  claiming a Swift defect.
- Qualify complete ownership and restore for fresh, reused and edited prefixes,
  cancellation, reset and failed continuation; qualify chunkwise scheduling separately.
  Account for host/device mappings and copies, RPC, state, caches, load overlap and
  resources outside Job commit. Verify X1 mapping windows and signed X2 DMA64; start
  with one session and justify partitions within reserves.
- Compare CPU, qualified Adreno, contiguous Hexagon and mixed phases at fixed artifacts
  and settings on X1/X2, prioritizing packed X2 prefill/decode when available. Charge
  startup, migration, traffic, RPC, synchronization, contention and sustained power;
  attribute queues, quantization, DMA, HVX/HMX, wake and waits. Phone rankings and
  unavailable root/DVFS controls do not choose Windows profiles.
- Connect the offline Rust comparison policy to independently verified archived
  captures and safe request-boundary selection only after qualification. Bind metrics,
  effective phase placement and common comparison digest to the same trusted evidence;
  recheck signed readiness and complete resource admission. Use device/workload and
  interactive/battery results to choose CPU, GPU or NPU dynamically, with independent
  X1/X2 scope, strict-backend rejection and unchanged CPU fallback. Ordinary startup
  must not benchmark, infer eligibility from TOPS or stitch unrelated phase timings.
- Integrate a locked experimental profile with checked placement and readiness,
  child-only environment and helper teardown, authentication, counting, one-slot,
  deadline and cancellation policy. Reject overrides and qualify isolation and resources
  beyond one process Job.
- Exercise unsupported shapes, bounded waits, mapping or allocation failure, device
  loss, post-submit cancellation and stale caches under [failure
  rules](docs/hexagon.md#memory-lifecycle-and-failure). Preserve in-use buffers and
  complete committed state or close admission through verified teardown, without secret
  replay or partial CPU continuation.
- Publish scoped profiles/no-go results from paired repeated CI: short and near-limit
  prompts, full output reserve, fixed and natural completions, edited and reused state,
  AC/battery, pressure and 15–30-minute sustained use. Retain raw evidence, uncertainty,
  LoRA parity, CPU rollback and [upgrade gates](docs/hexagon.md#qualification-evidence).
  Qualify Rust NPU coverage and device safety after Rust CPU parity, preserving signed
  deployment and full state.

## Survey ideas outside the current laptop backlog

The [research catalog](docs/research-register.md#research-catalog) owns excluded
literature: unavailable ISAs, datapaths and proprietary kernels, Android-root/QPT/DVFS,
new architectures/readers, large drafter training, full-Hessian pipelines and wholesale
lattice/rotation refits. GPU results alone justify neither a laptop job nor replacing
already-ternary Swift; bounded native preflights need the research budgets. Existing
Thinking/effort controls remain the baseline for methods requiring training.

Defer DASC/Tail-Replay checkpoint omission until exact reuse and suffix recomputation
cannot meet a real need. Wide trees, SVD caches and sparse-attention indexes need a
measured batch-one/8K bottleneck and bounded native benefit. Multi-tier or multi-request
serving is not a total-RAM saving here. [Withdrawn PolarQuant/unresolved
Litespark](docs/research-register.md#withdrawn-or-unresolved-reports) remain negative
evidence, not tasks.

## Deferred Prism migration

Reopen broader [Prism](https://github.com/symbiogenesis/prism-inference) work only for a
Swift dependency or chosen milestone. The [conformance
contract](docs/prism-upstream-tests.md) owns exact inventories, assertions and
exclusions. Remaining areas:

- Reference provenance and qualification: all backend partitions, 22,817 ordered
  evaluation and support geometries, 1,530 separate performance workloads and 70
  Windows-applicable source results, excluding Apple. Ratchet complete reviewed evidence
  only; retain required green IDs and full diagnostic handoffs.
- [Numerical/codec
  APIs](docs/prism-upstream-tests.md#numerical-operations-and-codec-contracts):
  operations, custom/map callbacks, backward graphs, remaining importance encoders,
  codec qualification and owned `qb-*` APIs.
- [Chat/parser/tokenizer
  behavior](docs/prism-upstream-tests.md#chat-parser-and-tokenizer-contracts): general
  templates and parsers, capability execution, server conversions and every tokenizer
  source.
- [Acquisition/model/session
  APIs](docs/prism-upstream-tests.md#acquisition-model-and-session-contracts):
  production CLI, retrieval/resolution, generic model/session APIs, architecture graphs
  and original model, state and concurrency sources.
- [DSpark/adapters/grammar/ABI](docs/prism-upstream-tests.md#dspark-adapters-grammar-and-abi):
  DSpark, actual LoRA conversion/merge/inference, LLGuidance and safe public C ABI.
  Preserve approved corrections and compile-only `test-c.c` scope.

## Deferred dependency experiments

- Follow the [audit](docs/dependency-audit.md) and [MXC capability
  issue](https://github.com/microsoft/mxc/issues/1579) before supervisor changes. Retain
  three Windows wgpu backends; removal needs support, device and fallback acceptance.
  Split archive APIs only for demonstrated independent-speech build or size benefit;
  preserve containment, accessibility and native UI.
- Qualify [SIMD-JSON tape
  reuse](docs/dependency-audit.md#current-candidate-and-pending-gate) in main-only CI:
  Unicode, duplicate/unknown fields, shapes, bounds and canonical bytes; at least 5%
  lower equal-weight geometric-mean time and no case over 5% slower in **both
  cold/reused screens**, including scales. Retain the current dependency decision until
  every gate passes.

## Optional agents

- Qualify creative chat first; evaluate OpenCode, then Aider, through a bounded
  authenticated inference bridge.
- Qualify MXC ProcessContainer isolation for agents, plugins, hooks and descendants:
  `allowDaclMutation:false`, no untrusted audit mode or unsandboxed fallback;
  filesystem/reparse, handle, subprocess and network bypass checks. A VM tier with a
  separate kernel needs separate acceptance.

## Promotion criteria

Apply [validation's promotion criteria](docs/validation.md#promotion-criteria):
independent correctness, security, full-state, quality and resource gates, sustained
comfort without sustained paging, paired target-device benefit and reproduction on
settled `main` with immutable identities and rollback. The initial performance gate is
**10% lower end-to-end latency**, or documented capacity/energy benefit within accepted
latency and quality limits.
