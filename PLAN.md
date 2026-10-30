# Swift Bonsai 2 Runner plan

This is the single backlog. [Technical docs](docs/README.md) own current
behavior, contracts, commands and evidence; completed work belongs in Git
history.

Prioritize actual-model application acceptance, daily-service isolation,
creative quality and comfort. Preserve the qualified Swift CPU fallback. Within
research, measure the batch-one bottleneck and PTQ1 suitability, then improve
packed CPU work and prefill, exact prefix reuse and short-chain speculation
before optional precision or compatible drafters. Bounded GPU/NPU access
discovery can proceed in parallel; automatic profile promotion requires device
qualification. Broader Prism work waits for a concrete Swift dependency or
chosen milestone.

## Research scope and practical budgets

Use the [research register](docs/research-register.md) for **existing Snapdragon
X Elite or X2 Elite hardware on native Windows ARM64**. Qualify each family
independently; missing X2 hardware must not block an X Elite result. Record
supported SKUs, drivers and ISAs. WSL, x64 emulation, cloud GPUs, unavailable
proprietary kernels and future instructions are outside scope.

Prefer existing weights and compatible published auxiliaries. Exclude full
retraining, pretraining and new architectures. Bounded statistics, calibration,
layerwise transforms and tiny auxiliary fits need a native implementation and
measured laptop time and RAM estimate; “training-free” alone does not prove
feasibility.

Before each pilot, declare candidate count, shapes, prompt and token sample,
peak RAM and deadline. Default to **30 minutes for feasibility and two hours of
initial compute per idea**. Reduce the sample or record a no-go if it cannot
fit. Process one candidate or layer at a time; avoid whole-model expansion and
open-ended sweeps. Preserve admission, physical reserves, context and output
reservations and quality gates. Exploration budgets do not substitute for
qualification.

Follow the [experiment
checklist](docs/research-register.md#turning-a-lead-into-an-experiment) and
[validation policy](docs/validation.md): full suites, benchmarks and performance
captures run only in their owning CI workflows; the owner-managed laptop runner
supplies device evidence. Retain results, including losing cases, with a next
step or no-go decision. Apply [promotion criteria](#promotion-criteria) before
changing defaults or admission.

## Application and daily service

### Actual-model acceptance

- Qualify startup, supplied-history streaming, requests with full 8K context and
  4K output, long conversations, speech, offline restart and recovery. Exercise
  Stop, abandonment, deadlines, memory pressure, process cleanup and explicit
  retry without replaying interrupted turns.
- Run optional `extended_history` and `stalled_consumer` workloads on the
  owner-managed X2 runner under the [native acceptance
  contract](docs/validation.md#actual-model-application-acceptance). Retain
  context rejection, same-instance reuse, termination and usage, explicit Stop,
  owned Job and listener cleanup and fresh-owner recovery evidence. Fixture size
  and output reservation do not prove exact 8K occupancy or 4K generation;
  stalled forwarding does not prove socket saturation or backend activity at
  Stop.
- Qualify [transcript and Find
  behavior](docs/streaming-resources.md#native-chat-and-transcript) across
  appends, replacements, conversation switches and the 512-message limit. Cover
  retry-heavy sessions, saturated results, cached matches, highlights and active
  underlines, Unicode wrapping, query, width and display-scale changes,
  selection preservation and recovery.
- Qualify speech startup after moving an X1 data directory to X2 and after
  CPU-feature or Windows changes; model-free cache checks do not qualify speech.
- Review Windows assistive technology, keyboard and focus, scaling, high
  contrast, minimum-window layout and multilingual input in the native app.
- Complete signed distribution acceptance under the [portable package
  contract](docs/chat-packages.md): binary provenance, dependency and license
  notices, clean-machine ARM64 Visual C++ prerequisites, installation, upgrades
  and rollback.

### Isolation, transport and recovery

- Establish a restricted inference identity: read-only approved artifacts,
  private bounded cache/temp storage, denied unrelated files, credentials and
  egress, and no same-user backend bypass. Qualify native MXC model loading and
  private storage. Close broader host loopback authority with broker-only
  communication. Preserve atomic Job limits and exact worker identity without
  weaker fallbacks. Follow [worker
  authority](docs/sandboxing.md#worker-authority), [native
  integration](docs/sandboxing.md#native-integration-contract) and the
  [daily-service boundary](docs/broker-policy.md).
- Qualify the [transport/security contract](docs/broker-policy.md): numeric
  loopback, exact Host, Origin and routes, private credentials, rejection of
  tools/MCP, dynamic loading and capability overrides, bounded queues, bytes,
  tokens and duration, and cancellation on disconnect or stalled consumption.
- Extend [bounded streaming contracts](docs/streaming-resources.md) to
  actual-model trials: ready-prefix delivery, long transcripts, speech backlog
  and chat overlap, PCM and WinMM reuse, borrowed-PCM uploads, native-owned WAV
  synthesis and direct final-storage HTTP intake. Cover fragmented headers at
  exact limits, maximum WAV fallback, buffered prefixes, framing tails, exact
  declared lengths, cap overflow, truncation/partial delivery, invalid output,
  failed writes, segment changes, driver refusal, deadlines, Stop and recovery.
  Capture task latency and peak memory in device CI; byte and pointer identity
  and work counts alone cannot prove gains.
- Extend [control-token
  rejection](docs/oxibonsai-reuse.md#security-features-and-transfer-boundaries)
  to actual template and tokenizer sessions: all 33 Swift spellings, user and
  system content, assistant history and reasoning, split parts, JSON escapes and
  lookalikes. Verify fixed HTTP 400 recovery and count/generation identity;
  qualify the reference vocabulary separately.
- Qualify the [global burst
  budget](docs/broker-policy.md#public-request-contract) (8 requests, refill
  1/second) under app request and recovery patterns: absolute deadlines,
  complete HTTP 429 during writes, constant-space draining, service reuse and
  explicit resend without replay.
- Qualify load, allocation-failure and backend-loss recovery, including bounded
  restore attempts and unchanged workspace on failure. Reject restart loops,
  silent profile changes, corruption and automatic cloud fallback.

## Runtime and correctness

Use the [native Rust contract](docs/native-rust-engine.md) and [promotion
evaluator](docs/native-rust-engine.md#default-runtime-promotion).
Default-runtime promotion retains the standard adapter at scale 1.3 and binds
independent gates to exact source, release and app identities; missing evidence
stays pending.

- Audit comparison and reuse sources at immutable revisions: model, adapter,
  tensor and transform identity, hardware assumptions, numerics, licenses and
  notices. Keep PQ2, current-weight PTQ1 and Qwen as separate controls. Respect
  the [OxiBonsai arithmetic/reuse boundaries](docs/oxibonsai-reuse.md);
  justified kernel pilots belong in Prism.
- Qualify pinned Rust on full-context, long-output Swift: scalar/NEON logits,
  complete KV, recurrent and convolution state, exact prompts and token IDs,
  sampler and termination and lifecycle recovery. Preserve `1e-3 + 1e-3 *
  abs(reference)` tolerance.
- Qualify the [rank-one Rust
  graph](https://github.com/symbiogenesis/prism-inference/blob/c192575cb70be3aa2c4b6e2a5c313c6d4bd68525/docs/rank-one-adapter.md)
  with native writer-basis and full-model zero/nonzero logits and state
  evidence. Integrate private-service scale changes and lifecycle before
  admitting adapters; synthetic or source-only execution is insufficient.
  Qualify PTQ1 and other formats separately.
- Produce a batch-one bottleneck receipt for projections,
  transforms/quantization, attention, GDN/convolution, copies, output, sampler,
  transport and speech. Measure complete-model allocations, scratch lifetimes
  and peaks for reusable logits, state-only prompt ingestion, shared
  activations, physical FP16 KV, checkpoint pages and sampler candidates. Audit
  full-cache replacement, broadcast/transpose materialization and repeated
  preparation; remove the largest demonstrated avoidable copy or allocation
  while preserving independent snapshots. Qualify tiled attention-value
  traversal on long contexts; see the [overhead
  review](docs/native-rust-engine.md#2026-literature-runtime-overhead-and-work-placement).
- After [packed projections](#cpu-kernels), implement one bounded CPU chunkwise
  GDN prefill path with a small chunk-size set. Preserve transforms,
  normalization, convolution and FP32 state; predeclare long-prefix and restore
  tolerances. Require causal scheduling, bounded scratch and cancellation and
  whole-request rollback under the [prefill
  contract](docs/inference-algorithms.md#chunkwise-gated-deltanet-prefill).
  Sequential blocks alone do not reuse matrices.
- If attention is material, compare grouped-head KV reuse and bounded
  score/value tiles, including retained versus recomputed scores. Preserve
  globally normalized FP16 probabilities before V. Online-softmax/Flash paths
  need separate numerical identity and continuation checks under the [attention
  contract](docs/inference-algorithms.md#attention-locality-and-the-io-bound).
- If recurrent-state traffic is material, test a short FP32 KVBuffer-style
  factor buffer against sequential updates. Charge reads, materialization and
  flushes and include every pending update and convolution value in checkpoints
  and cancellation recovery. Keep deferred writeback separate from precision
  changes; stop without complete-cycle benefit. See [state-traffic
  research](docs/inference-algorithms.md#2026-state-traffic-and-verification-research).
- Qualify exact prompt reuse: model, template and token identity, matching
  complete state, bounded snapshots, eviction and private storage, edits,
  switches, cancellation and restore. Start with latest committed state,
  immutable KV pages, exact suffix replay and simple lookup and eviction before
  radix trees or sparse placement. Resolve total versus incremental accounting:
  two 149.625 MiB recurrent/convolution snapshots cost **299.25 MiB before
  KV/metadata**, exceeding a 256 MiB total cache. Keep the budget and
  conservative per-snapshot page charges.
- Implement a short-chain CPU block verifier before proposals: complete state,
  partial acceptance, EOS and output limits, Stop, failed commit and rollback.
  Establish greedy parity, then conditional acceptance and residual sampling for
  the actual temperature/top-k/top-p/penalty policy. Distribution preservation
  does not imply identical RNG consumption or seeded output.
- After verifier qualification, add bounded prompt/history suffix proposals with
  a few short windows and no second model. Use repetitive and creative held-out
  prompts; measure drafting, verification, commit and recovery together. Keep
  ordinary decoding and stop when acceptance cannot repay the cycle.
- If measured chain recovery costs are material, compare snapshot/replay with
  saved-factor recovery and delayed commit using SpecLA/TreeWY/RecoverSSM
  references. Preserve complete state and declared float identity; wide trees
  need demonstrated benefit over chains. See [verification
  research](docs/inference-algorithms.md#trained-parallel-proposals-and-full-state-verification).
- After verifier qualification, if measured verification and recovery costs
  leave a feasible break-even, screen a published small drafter for exact
  tokenizer and vocabulary, Swift/LoRA and native ARM64 compatibility. Check
  target-feature interfaces only when consumed. Charge both models' weights, KV,
  features and scratch, plus startup/prefill costs, before provisioning. Suffix
  failure does not rule out learned drafters; a Qwen family name does not prove
  compatibility. Record a no-go if none fits; exclude general drafter training
  and deferred DSpark.
- Qualify replacements against pinned Prism at matching artifacts, adapters and
  settings and all promotion gates, retaining deterministic streams where
  applicable. Retire old build/overlay/FFI paths only after acceptance.

## Creative quality, adapters and memory

### Quality and speech

- Establish an independent writing/art-direction rubric: drafting, constrained
  revision, continuity, dialogue, voice and tone, composition, instruction
  following, coherence, repetition, unwanted refusals and useful completed
  answers. Qualify actual thinking and output settings and Extra effort; changed
  text or benchmark averages are insufficient.
- Complete [standard LoRA
  qualification](docs/swift-lora.md#standard-profile-qualification): SSM
  writer-basis, loaded-at-zero response/logits identity, nonzero application,
  paired device trials and independent human reviews. Require both same-engine
  decode TPS and quality parity, bounded RAM premium and visible regressions.
  Qualify app-scale 1.3 and other intended scales separately from the
  scale-one, 512-token contract; one response cannot choose a scale.
- Compare Q8_0/Q4_K_M speech and bounded thread choices through [paired device
  CI and independent listening](docs/validation.md#paired-speech-comparison).
  Retain voice quality, startup, first-audio, synthesis latency and resources;
  attribute bootstrap, warm Base prefill, residual-code prediction, codec,
  transport and playback separately. Distinguish generated PCM from audible
  output and record supply gaps and underruns. Use [resource
  evidence](docs/performance-expectations.md#q4-speech-resource-comparison) and
  existing reuse contracts to target measured remaining costs.
- Qualify native-owned VoiceDesign references through Design teardown and Base
  extraction on first use, profile changes and bootstrap failure under the
  [ownership
  contract](docs/streaming-resources.md#native-speech-generation-and-streaming).
  Include Rust Philox semantic/predictor sampling in actual-model acceptance;
  exact vectors/C++ bit comparisons do not qualify output, listening or
  resources.
- Qualify chat alone, warm TTS and chat/TTS together with a small set of shared
  thread budgets: UI frames, audio continuity, useful-answer time and sustained
  memory and heat. Check cold idle waits separately from active/warm/Canary
  monitoring, including wake and Stop during the 30-second TTS lease. Preserve
  ASR/TTS exclusion and independent reserves under the [scheduling
  contract](docs/streaming-resources.md#speech-scheduling-and-playback).
- Defer speech speculation unless a compatible native artifact or
  laptop-feasible tiny fit exists for the selected 1.7B path (not the paper's
  0.6B). Before a fit, require residual-code verification, sampler correction,
  accepted-state recovery and bounded scratch; stop if those costs erase
  complete-readout benefit. See [speech
  research](docs/performance-expectations.md#2026-speech-inference-research).

### Memory and profiles

- Investigate lossless adapter reservation and rank-one accumulation with
  allocator and strict rounding/order checks. Resolve counter discrepancies
  before claiming savings; preserve dynamic scale and load-without-apply under
  [allocation rules](docs/swift-lora.md#allocation-semantics).
- Qualify [current-weight PTQ1](docs/swift-memory.md#current-weight-ptq1-option)
  full-model logits and live state on scalar and explicit NEON with no-LoRA and
  LoRA basis and forward execution. Then screen paired complete responses,
  context/recovery, quality and comfort, retaining PQ2 rollback. Promote or
  lower admission only after suitability; base candidate reductions on the
  verified **1,259,520,000-byte** packing difference, separate mapped residency
  from private commit and preserve reserves.
- Capture prefill, first useful answer, decode TPS, task time,
  retries/truncation, working/private/Job/system/device peaks, hard faults,
  observable energy, UI responsiveness and sustained heat and fan behavior.
  Retain X2 `ProcessMemory` aggregates; qualify complete-model coverage and
  phase attribution. Overlapping counters are not additive; sampled maxima can
  miss brief allocations. Follow [measurement
  rules](docs/performance-expectations.md#2026-literature-measurement-and-comparison):
  repeated interleaved pairs, uncertainty, independent X1/X2 results,
  instrumented attribution separate from timing, idle versus active power,
  consistent token boundaries and energy per acceptable completed task with
  output and retry counts.
- Qualify creative, daily, long-context and battery profiles within total
  budgets. Larger context, Q8/Q4 KV, Flash Attention and recurrent precision
  changes need separate numerical, state and quality gates. Preserve canon and
  output reservations.
- Attribute mapped buffers, microbatch 32/64, persistent output rows,
  post-prefill trimming, prefetch, rotation/sign metadata and stacks before
  [lossless reductions](docs/swift-memory-options.md). Review immutable-B
  sharing against the intended runtime; preserve rounding and distinguish
  overlapping from disjoint allocation lifetimes.
- Independently qualify Qwen loading and service control when its 18 GiB
  allowance plus 4 GiB reserve can be admitted.
- Add source-referenced scene packets and author-approved context; manuscripts
  and the edited project bible stay authoritative. Start with manual
  excerpts/lexical retrieval; add embeddings only for a demonstrated need.
  Summaries need source versions and author approval; vision, image critique or
  generation needs a matching model and shared-memory budget.

## Bounded calibration and optional precision

Run separate native CPU experiments after exact controls work, using a small
fixed calibration sample, disjoint held-out prompts, bounded statistics and
[research budgets](#research-scope-and-practical-budgets). Calibration does not
qualify quality or reduced admission; retain full-context, continuation and LoRA
gates.

- Estimate savings and conversion traffic first: 8K attention KV is 512 MiB, so
  INT8 saves at most about 256 MiB before scales/scratch. Recurrent/convolution
  payload is a separate 149.625 MiB. Compare process benefit with PTQ1; cache
  compression ratios do not apply to weights.
- Screen native Q8 attention KV first, then at most one asymmetric key/value or
  rotation-plus-scalar candidate if justified. Keep FP16 controls, explicit
  cache and numerical identity and exact checkpoints. Charge metadata, conversion,
  rotations, dequantization and retained copies; defer SVD/index schemes without
  an 8K need. See [compression
  constraints](docs/inference-algorithms.md#compression-and-approximation-constraints).
- Only for material state traffic or snapshot storage, screen FP16 recurrent
  state against FP32, then at most one DAMP-inspired FP16/INT8 split. Keep
  convolution separate; account for scales, rounding/RNG, restore and trajectory
  error. Add low-rank bypass/directional rounding only if update loss and net
  benefit justify factor, merge and rollback complexity.
- Check packed-tensor entropy/correlation, including scales, indexes, random
  access and decode work. Prototype Shannonic-inspired streams only if realistic
  bytes and traffic beat PTQ1 without retained expansion. Keep current-census
  BITCOS rejected. New codebooks, rotations or scales need a native layerwise
  cost/benefit preflight, not whole-model refitting from GPU results. See
  [weight/state
  research](docs/swift-memory-options.md#2026-entropy-coding-and-alternative-weight-artifacts).

## Hardware selection

- Extend [discovery](docs/hardware-capabilities.md#host-inventory) with
  independently checked SoC/SKU, GPU/NPU APIs/features/architecture,
  drivers/runtime/firmware, signed readiness and static identities. Share
  checked data between Doctor and launch policy; keep unknowns explicit and live
  budgets separate from compatibility.
- Restore or register the owner-managed ARM64 runner before the [X2 CPU thread
  screen](docs/simd-plan.md#x2-cpu-thread-screen); the current inventory has
  none. Resolve legacy startup failure `0xC0000142` by auditing shell DLL
  dependencies and bounded `--version` probes under unchanged mitigations.
  Integrate and release the [headless UCRT argv
  candidate](docs/runtime-candidates/README.md#headless-ucrt-argv-candidate)
  upstream and qualify its exact binary before changing Runner's pin.
- Qualify scalar/NEON controls independently on X1/X2, optional ISA and
  phase-specific thread/batch/wait/power choices. Run the bounded screen on
  settled `main` before selecting defaults; extend only for a measured
  opportunity. Suppress comparisons for changed or unknown CPU, kernel,
  features, topology or power, including between pairs; follow [screen
  controls](docs/simd-plan.md#controls-and-cost).
- Compare event waits with polling for idle CPU, wake latency, decode and
  cancellation. Inventory Windows timing/energy/thermal/memory sampling limits;
  no energy verdict without valid telemetry. Four threads, all cores or a family
  name do not choose a profile; bare-metal gang scheduling remains outside the
  app.
- Build the bounded reviewed [profile
  catalog](docs/hardware-capabilities.md#profile-and-evidence-keys) with exact
  compatibility/workload/reuse/objective keys, paired evidence, thresholds and
  losing/no-go entries. Implement deterministic [eligibility, ranking and
  admission](docs/hardware-capabilities.md#eligibility-ranking-and-admission)
  for task latency and battery energy, charging startup, repacking, compilation,
  migrations and sustained costs. Use stable ties without startup benchmark
  search.
- Recheck budgets and readiness before admission. Choose qualified CPU fallback
  or strict rejection without changing model, adapter, sampler, precision,
  context or output. Report requested/effective profile, placement, evidence and
  value-free fallback reasons in diagnostics and a simple native Rust Auto
  control.
- Reevaluate at safe request boundaries after power/memory/runtime/driver
  changes; invalidate evidence and caches and prove complete-state migration or
  teardown and rebuild. Never move partial requests between backends. Cover
  unknown devices, absent ISA or accelerators, stale identity, pressure, workload
  boundaries, ties, objective changes, strict rejection and failed or cancelled
  handoff.
- Demonstrate automatic interactive and battery operation separately on X1/X2,
  including installation and rollback, sustained use and upgrade requalification
  under the [selection
  contract](docs/hardware-capabilities.md#automatic-profile-selection).

## CPU kernels

Follow [CPU arithmetic, dispatch and acceptance contracts](docs/simd-plan.md).
Pin artifacts, runtime, compiler, format, shapes, ISA and numerics. Select
optimizations from CI hotspot, bandwidth, packing and dispatch evidence.

- Qualify the [shared-slice row-parallel
  candidate](docs/oxibonsai-reuse.md#implementation-status) in Swift prepared
  projections: serial versus two small worker budgets on three shapes, sharing
  packed weights and prepared Q8 activations. Extend to complete state,
  cancellation, pool and task accounting and whole-request cost. Review source
  and a released binary before changing the pin or enabling workers.
- Complete full-model and memory qualification of NEON PQ2, odd-group PQ2 and
  PTQ1. Preserve scalar forcing, backend diagnostics and [packed
  arithmetic](docs/simd-plan.md#packed-arithmetic), including fourth symbols and
  bounds on every LUT sum and correction before narrowing.
- Prototype direct PTQ1-by-Q8_K, first scalar on a few Swift shapes, then NEON.
  Preserve activation grouping, raw scales, quantized activations, float order
  and odd-group fallback. Keep packing separate from PTQ1-by-Q8_0 numerics
  through full-model/adapter qualification.
- Compare at most four NEON layouts on three shapes, one matrix at a time,
  unpacking directly into consumption order. Adapt two-bit layout principles
  without VNNI instructions; compare at most two exact TL1_1/TL2_1-style LUT
  designs with ternary eligibility and generic PQ2 fallback. Charge preparation,
  source/repacked overlap, tables/cache and startup.
- Implement one tiled projection-prefill family, initially token tiles 8/16/32
  where valid, sharing activation preparation and bounded LUT/weight tiles.
  Reuse weights without persistent INT8/FP16 expansion; stop if construction or
  scratch erases the gain.
- Establish installed Windows CPU/OS NEON/dotprod/I8MM eligibility before
  separate dispatch. Inspect Release code generation and forbidden-ISA leakage;
  require detection, parity and benefit under the [dispatch
  contract](docs/simd-plan.md#execution-and-dispatch).
- Optimize measured transforms, elementwise work, quantizers, reductions and
  prefill through safe APIs or audited isolated unsafe code. Extend edge,
  malformed, rollback, non-ARM and absent-ISA fixtures; recheck dependencies,
  licenses and code generation after relevant changes.
- If integer-dot dependency chains are material, screen the [two-accumulator
  idea](docs/oxibonsai-reuse.md#neon-retain-concepts-preserve-swift-arithmetic)
  in Prism's `i32x4` loop on three shapes. Preserve widening, exact dots, Q8
  groups and float order; inspect spills and require device CI benefit before
  repinning.
- Compare scalar Rust, optimized Rust and Prism in paired device CI. Separate
  hot-cache and streaming weights, preparation, dispatch, prefill/decode/task
  costs, allocations and sustained energy/heat. Scope qualified profiles by
  device, format, shape, phase and objective.

## Adreno Vulkan

Follow the [Adreno contracts](docs/adreno-gpu-plan.md). Access discovery alone
does not qualify a worker backend or change the selected CPU profile.

- Capture native GPU/driver/API identities, features, limits and memory
  telemetry under the [inventory
  contract](docs/hardware-capabilities.md#native-vulkan-observations). Reject
  emulation; keep unknowns and conservative admission. Resolve X2 `host-gpu`
  instance-creation failure; tool-policy Adreno X2-90 observations neither
  explain that failure nor qualify worker access.
- Pilot native ARM64 OpenCL packed projection in a reviewed Prism candidate: one
  candidate and one Swift shape, scalar PQ2-by-Q8_K control, exact grouping,
  raw scales and float order, and a separate PTQ1 lane. Prove queries and workload access
  under unchanged worker mitigations before execution; retain refusal as a
  no-go. Follow [access/source
  boundaries](docs/adreno-gpu-plan.md#opencl-access-and-source-audit) with hard
  caps of 30 minutes for feasibility and two hours of initial CI compute. Charge
  preparation, scratch and transfers; preserve reserves and CPU fallback.
  Exclude whole-model expansion and profile selection from the pilot.
- Build a separately locked pinned-Prism `GGML_VULKAN=ON` candidate with
  reviewed ARM64 SDK/shader/compiler/library identities and unrelated
  accelerators disabled. Keep a bounded experimental profile and preserve
  service policy and the CPU lane.
- Complete the PQ2/PTQ1 operation/placement/fallback/transfer ledger and verify
  descriptors without limit overrides. Prove packed/operator and full-model
  logits/KV/GDN/lifecycle parity, awkward shapes, Vulkan Q8_1 versus CPU
  Q8_K/Q8_0 and separate no-LoRA/approved-LoRA basis/forward lanes under the
  [arithmetic/placement
  contract](docs/adreno-gpu-plan.md#swift-arithmetic-and-placement).
- Screen CPU, partial and maximum feasible offload against eligible Hexagon
  candidates. Separate prefill/decode, batches/threads/waits,
  preparation/packing, submission, transfers, GPU service, sampling and
  migration; distinguish slow kernels from host starvation. Qualify
  attention/cache/Flash changes separately.
- Optimize measured kernels, layouts, residency,
  descriptors/pipelines/allocations, scheduling and caches. Compare
  packed/coalesced layouts, vector unpacking, shape-specific tiles and supported
  replay; textures require native API support. Charge cold preparation/copies,
  count physical memory once, bound caches and preserve asynchronous lifetimes;
  rerun numerical/state/adapter/lifecycle gates.
- Qualify per-device benefit, quality, comfort, memory, admission and
  effective-plan reporting. Cover absence or loss, pressure, stale evidence,
  cancellation and upgrades with committed-state preservation and CPU rollback
  under [resource/failure
  rules](docs/adreno-gpu-plan.md#resources-failure-and-acceptance).
- After Rust CPU parity, separately qualify Rust GPU coverage and device safety
  against the same full-model, state and device gates.

## Hexagon NPU

Follow the [Hexagon contracts](docs/hexagon-npu-plan.md).

- Prove native signed access independently on X1/v73 and X2/v81: exact
  deployment, license and signature identities, actual mapping/execution and a small
  supported-type dispatch fixture. Audit DMA64, low-bit Hadamard and HTP leads;
  retain scoped no-go reasons. Trust/Secure Boot changes need a separate owner
  decision.
- Map Swift operations, layouts, transforms/basis, complete state, placement,
  fallback and bytes; choose one prototype region. Generic Q2/Q4/Q8 does not
  prove PQ2/PTQ1 support. Implement lossless PQ2 tiling/fused matvec and
  separately qualify PTQ1 canonicality/Q8_0. Audit any PQ2-to-Q2 bridge's
  metadata/storage premium before timing under [arithmetic/state
  rules](docs/hexagon-npu-plan.md#packed-arithmetic-and-graph-state).
- Evaluate HVX packed work and batched HMX prefill with bounded
  VTCM/DMA/repacking scratch, exact fusion/recurrent basis and independent
  adapters. Qualify ring saturation, partial tiles, double buffering,
  requested/effective VTCM, allocation refusal and cancellation during
  transfers. Use documented DMA-ring failure as a regression pattern, without
  assuming Swift has that defect.
- Prove complete ownership and restore across fresh, reused and edited prefixes,
  cancellation, reset and failed continuation; qualify chunkwise scheduling
  separately. Account for host/device mappings, copies, RPC, state/caches and
  load overlap, including resources outside Job commit. Verify X1 mapping
  windows and signed X2 DMA64; start with one session and justify partitions while
  preserving reserves.
- Compare CPU, qualified Adreno, contiguous Hexagon and mixed phases on X1/X2 at
  fixed artifacts and settings. Include startup, migration,
  traffic/RPC/synchronization, contention and sustained power; prioritize X2
  packed prefill/decode when available. Attribute queues, quantization, DMA,
  HVX/HMX, wake and waits; phone rankings and unavailable root/DVFS controls do
  not choose a Windows winner.
- Integrate a locked experimental profile with checked placement and readiness,
  child-only environment and helper teardown. Preserve authentication, counting,
  one-slot/deadline/cancellation policy; reject overrides and qualify isolation
  and resources beyond one process Job.
- Cover unsupported shapes, bounded waits, mapping/allocation failure, device
  loss, post-submit cancellation and stale caches under [lifecycle/failure
  rules](docs/hexagon-npu-plan.md#memory-lifecycle-and-failure). Keep in-use
  buffers alive; restore complete committed state or close admission through
  verified teardown, without secret replay or partial CPU continuation.
- Publish scoped profiles or no-go results from paired repeated CI: short and
  near-limit prompts, full output reserve, fixed and natural completions, edited
  and reused state, AC/battery, pressure and 15–30-minute sustained use. Apply
  [qualification gates](docs/hexagon-npu-plan.md#qualification-evidence),
  retaining raw evidence, uncertainty, LoRA parity, CPU rollback and upgrade
  rules.
- Qualify Rust NPU coverage and device safety after Rust CPU parity; retain
  signed deployment and full-state requirements.

## Survey ideas outside the current laptop backlog

The [research catalog](docs/research-register.md#research-catalog) records
excluded literature: unavailable ISA/datapath changes, proprietary kernels,
Android-root, QPT and DVFS controls, new architectures/readers, large drafter
training, full-Hessian pipelines and wholesale lattice/rotation refits.
Published GPU results alone do not justify a laptop job or replacement for
already-ternary Swift; bounded native preflights still need the research budgets
above. Existing thinking/effort controls remain the baseline for methods needing
compatible training.

Defer DASC/Tail-Replay checkpoint omission until exact reuse and suffix
recomputation cannot meet a real cache need. Wide trees, SVD caches and
sparse-attention indexes need a measured batch-one/8K bottleneck and bounded
native benefit. Multi-tier or multi-request serving is not a total-RAM saving
here. Keep [withdrawn PolarQuant and unresolved
Litespark](docs/research-register.md#withdrawn-or-unresolved-reports) as
negative evidence, not tasks.

## Deferred Prism migration

Broader implementation belongs in [Prism
Inference](https://github.com/symbiogenesis/prism-inference) and reopens only
for a Swift dependency or chosen milestone. The [conformance
contract](docs/prism-upstream-tests.md) owns exact inventories, assertions and
exclusions. Remaining areas:

- Complete reference qualification/provenance: all backend partitions, 22,817
  ordered evaluation/support geometries, 1,530 separate performance workloads
  and 70 Windows-applicable source results with Apple exclusion. Ratchet only
  complete reviewed evidence, retaining required green IDs/full diagnostic
  handoffs.
- Complete numerical operations, custom/map callbacks, backward graphs,
  remaining importance encoders, codec qualification and owned `qb-*` APIs under
  [numerical/codec
  contracts](docs/prism-upstream-tests.md#numerical-operations-and-codec-contracts).
- Complete general chat/template/parser behavior, capability execution, server
  conversions and every tokenizer source under [chat/parser/tokenizer
  contracts](docs/prism-upstream-tests.md#chat-parser-and-tokenizer-contracts).
- Complete production CLI, retrieval/resolution, generic model/session APIs,
  architecture-family graphs and original model/state/concurrency sources under
  [acquisition/model/session
  contracts](docs/prism-upstream-tests.md#acquisition-model-and-session-contracts).
- Complete DSpark, actual LoRA conversion/merge/inference, LLGuidance
  integration and safe public C ABI under [DSpark/adapter/grammar/ABI
  contracts](docs/prism-upstream-tests.md#dspark-adapters-grammar-and-abi).
  Preserve approved corrections and the compile-only scope of `test-c.c`.

## Deferred dependency experiments

- Follow the [dependency audit](docs/dependency-audit.md) and [MXC
  capability-feature issue](https://github.com/microsoft/mxc/issues/1579) before
  supervisor changes. Keep all three Windows wgpu backends; removal needs a
  support decision and device/fallback acceptance. Split archive APIs only for
  demonstrated build/size benefits in independent speech builds. Preserve
  containment, accessibility and native UI.
- Qualify the pinned [SIMD-JSON tape-reuse
  candidate](docs/dependency-audit.md#current-candidate-and-pending-gate) in
  main-only CI: preserve Unicode, duplicate/unknown-field handling, shape/bounds
  and canonical bytes. Require at least 5% lower equal-weight geometric-mean
  time and no case over 5% slower in both cold and reused screens, including
  scale-setting cases. Keep the
  current dependency decision until all gates pass.

## Optional agents

- Qualify creative chat first; evaluate OpenCode, then Aider, through a bounded
  authenticated inference bridge.
- Qualify MXC ProcessContainer containment for agents, plugins, hooks and descendants
  with `allowDaclMutation:false`, no untrusted audit mode or unsandboxed
  fallback. Exercise filesystem/reparse, handle, subprocess and network
  bypasses; a VM tier with a separate kernel needs its own acceptance.

## Promotion criteria

- Pass profile-specific correctness, security, full-state recovery, creative
  quality and resource gates, including independent LoRA TPS/quality
  requirements.
- Demonstrate bounded memory without sustained paging and owner-accepted
  responsiveness, heat and fan behavior during sustained use.
- Demonstrate paired target-device CI benefit: initially **10% lower end-to-end
  latency**, or documented capacity/energy benefit within accepted
  latency/quality limits. Predeclare tolerances and retain losing/no-go results.
- Reproduce on settled `main` with immutable artifact/build/device identities,
  run/capture references, limitations and rollback profile. Follow [validation
  policy](docs/validation.md): full suites and benchmarks stay in owning
  main-only CI; local builds, analyzers and focused correctness checks remain
  allowed.
