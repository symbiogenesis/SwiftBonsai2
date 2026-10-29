# Swift Bonsai 2 Runner TODO

This is the single list of outstanding project work. Current implementation
contracts, commands, evidence and supported-subset boundaries live in `docs/`.

Prioritize actual-model application acceptance, daily-service isolation and
creative/comfort qualification. Preserve the Swift CPU runtime boundary while
closing those exits. Hardware discovery and bounded Vulkan/Hexagon access
investigations can proceed independently; automatic profile promotion requires
qualified device evidence. Reopen broader Prism work only for a concrete Swift
dependency or an explicitly chosen later milestone.

## Research scope and practical budgets

Apply the [2026 research survey](docs/research-register.md) to **existing
Snapdragon X Elite or Snapdragon X2 Elite hardware, running native Windows
ARM64**. A candidate may qualify on either family independently; missing X2
hardware must not block an otherwise useful X Elite result. Record the supported
SKU/driver/ISA explicitly and preserve the qualified CPU fallback. WSL, x64
emulation, cloud GPUs, unavailable proprietary kernels and future CPU/GPU/NPU
instructions are not execution paths for these additions.

Full model retraining, pretraining and new model-architecture development are
out of scope. Bounded statistics, calibration, layerwise offline transforms and
very small auxiliary fits are eligible only when a native implementation and
measured laptop time/RAM estimate justify them. Prefer existing weights and
published compatible auxiliaries. A method being training-free does not establish
that its full-model Hessians, source weights, codebooks or preparation fit here.

For each new research pilot, declare candidate count, representative shapes,
prompt/token sample, peak-memory allowance and a hard deadline before execution.
Use **30 minutes for feasibility and two hours of initial compute per idea** as
planning defaults; reduce the sample or retain a no-go when the estimate fails.
These are exploration budgets, not claimed runtimes or substitutes for complete
qualification. Process one candidate/layer at a time where possible; avoid a
whole-model expanded copy or open-ended parameter sweep. Existing admission,
physical reserves, context/output reservations and quality gates remain binding.
Full suites, benchmarks and performance captures remain CI-only; the
owner-managed laptop runner supplies native device evidence under
[validation](docs/validation.md).

Keep existing application/correctness exits first in delivery priority. Within
research, establish the batch-one bottleneck and PTQ1 suitability; improve packed CPU work and genuine
prefill; qualify exact prefix reuse and short-chain speculation; then screen
optional precision or compatible drafters. Bounded GPU/NPU access discovery can
proceed in parallel. Every pilot ends with a retained result and a next-step or
no-go decision, including losing cases.

## Application and daily service

- Qualify actual-model startup, supplied-history streaming, full 8K-context/4K-output
  requests, long conversations, speech input/output, offline restart and recovery.
  Include speech startup after moving an existing X1 data directory to X2 and
  after CPU-feature/Windows changes; host-bound Cargo/native cache regressions
  remain model-free, while actual speech behavior requires device acceptance.
  Exercise Stop, abandonment, deadlines, memory pressure, process cleanup and
  explicit retry without replaying interrupted turns. Include Find across streamed
  appends, response replacements and conversation switches in long-conversation
  acceptance, including append-only result/cache reuse and saturated results.
  Exercise the 512-message live transcript boundary during retry-heavy sessions,
  Find navigation and recovery. Matching, cached-result and combined count/byte
  eviction regressions remain model-free. Qualify fixed-size highlight layout
  keys under saturated Find, navigation, query changes and response replacement;
  their invalidation and layout-work regressions also remain model-free.
- Review Windows assistive-technology support, keyboard/focus behavior, scaling,
  high contrast, minimum-window layout and multilingual text input with the
  actual native application.
- Complete signed installation/distribution acceptance: reviewed binary provenance,
  exact dependency/license notices, clean-machine ARM64 Visual C++ runtime
  prerequisites, installation/update/rollback and compatibility across upgrades.
  Use the [portable package contract](docs/chat-packages.md).
- Establish a restricted inference identity with read-only approved artifacts,
  private bounded cache/temp storage, denied unrelated files/credentials and
  denied egress. Prevent same-user backend bypass; Jobs, ACLs and loopback alone
  are insufficient. Qualify native MXC containment, including
  actual-model loading and private temporary storage. Close its broader host
  loopback authority with proven broker-only communication, preserving atomic
  Job limits and exact worker identity without weaker fallback tiers. Follow the
  [worker sandbox architecture](docs/sandboxing.md),
  [Windows integration review](docs/windows-integration.md) and qualify the
  [daily-service boundary](docs/broker-policy.md).
- Qualify the complete transport/security boundary: numeric loopback, exact
  Host/Origin/routes, private credentials, tools/MCP and dynamic-loading rejection,
  capability-override rejection, bounded queues/bytes/tokens/duration and reliable
  cancellation when clients disconnect or stop consuming output. Qualify linear
  speech-header scanning under fragmented delivery at the exact header limits;
  comparison counts and coalesced-body framing remain model-free checks. Extend
  the [bounded streaming regressions](docs/streaming-resources.md) to actual-model
  ready-prefix delivery, long-transcript updates, stalled-consumer speech backlog,
  speech/chat overlap with recycled PCM playback buffers and WinMM descriptors,
  including segment changes, Stop, driver refusal and recovery trials.
  Include borrowed-PCM microphone uploads without full canonical-WAV or multipart
  copies, retaining exact upload framing and cancellation. Qualify shared buffered
  speech response intake with direct final-storage reads, including maximum-size
  WAV fallback, partial responses and cancellation. Include broker nonstreaming
  JSON reads into bounded final storage, exact-cap overflow probing and truncated
  framing; their ownership and read-buffer regressions remain model-free. Capture
  complete-task latency and peak memory in target-device CI before claiming gains.
- Qualify recovery under load, allocation failure and backend loss without restart
  loops, silent profile changes, state corruption or automatic cloud fallback.
  Extend the bounded restore-count and unchanged-workspace regressions to these
  actual-application failure trials.

## Runtime and correctness

Default-runtime promotion retains the current standard profile's adapter at
scale 1.3. Use the [promotion contract and offline evidence evaluator](docs/runtime-promotion.md)
to bind each independent requirement to exact source/release/application
identities; missing reports remain pending.

- Audit comparison and reusable engine/kernel sources at immutable revisions,
  including model/adapter/tensor/transform identities, hardware assumptions,
  numerical evidence, licenses and notices. Keep PQ2, current-weight PTQ1 and
  the Qwen reference as separate controls.
- Qualify the pinned native Rust runtime over full-context/long-output Swift
  workloads, including scalar/NEON logits, complete live KV/recurrent/convolution
  state, exact prompts/token IDs, sampler/termination and lifecycle recovery.
  Preserve the declared `1e-3 + 1e-3 * abs(reference)` numerical tolerance.
- Qualify the [explicit rank-one Rust graph implementation](https://github.com/symbiogenesis/prism-inference/blob/c192575cb70be3aa2c4b6e2a5c313c6d4bd68525/docs/rank-one-adapter.md)
  with native writer-level basis and full-model zero/nonzero logits/state
  evidence, then integrate private-service scale changes and lifecycle before
  admitting adapter-enabled replacement profiles. Source-only numerical
  execution and synthetic regressions do not enable adapters in the service. Keep
  current-weight PTQ1 and other model/format support separately qualified.
- Measure complete model-level allocation, scratch lifetimes and peak memory for
  reusable logits, state-only prompt ingestion, activation sharing, physical FP16
  KV, checkpoint pages and retained sampler candidates. Produce a batch-one
  bottleneck receipt separating projections, transforms/quantization, attention,
  GDN/convolution, copying, output/sampler and transport/speech costs. Audit hot
  paths for full-cache replacement, broadcast/transpose materialization and
  repeated preparation; preallocated storage alone does not remove that work.
  Choose the largest demonstrated avoidable allocation/copy first, retaining
  independent snapshot ownership. Qualify tiled attention-value traversal on
  complete long-context workloads. See the
  [2026 overhead review](docs/implementation-research.md#2026-literature-runtime-overhead-and-work-placement).
- Qualify faithful chunkwise GDN prefill after the packed projection work in
  [CPU kernels](#cpu-kernels). Start with one bounded CPU chunk implementation and
  a small chunk-size set; preserve grouped/sign transforms, normalization,
  convolution and FP32 recurrent state. Declare floating tolerances before
  comparing long prefixes/restores. All prefill paths need causal scheduling,
  bounded scratch/cancellation and whole-request rollback; sequential blocks
  alone do not supply matrix reuse. See [algorithm constraints](docs/inference-algorithms.md).
- If full-attention work is material, compare grouped-query-head KV reuse and
  bounded score/value tiles on native CPU, building on the existing V traversal.
  Screen retained-score versus recomputed-score normalization where useful;
  preserve the reference's globally normalized FP16 probabilities before V.
  An online-softmax/Flash-style alternative needs a separate declared numerical
  path and continuation checks, rather than assumed equivalence from algebra.
- If measured recurrent-state traffic is material, prototype a short FP32
  KVBuffer-style factor buffer on native CPU, comparing base-state-plus-pending
  updates with the sequential control. Charge buffer reads, materialization and
  flush costs; checkpoints and cancellation must include every pending update
  and convolution value. Keep deferred writeback separate from any factor/state
  precision change, and stop if batch-one complete-cycle cost does not improve.
  See [state-traffic research](docs/inference-algorithms.md#2026-state-traffic-and-verification-research).
- Qualify full-model prompt reuse with exact model/template/token identity and
  matching KV plus recurrent/convolution state. Bound snapshots/eviction/private
  storage and cover edits, conversation switches, cancellation and restore.
  Start with reuse of the latest live committed state, immutable KV pages and
  exact suffix replay for edited histories. Resolve total versus incremental
  cache accounting:
  two independent 149.625 MiB recurrent/convolution snapshots already cost
  **299.25 MiB before KV/metadata**, so cannot fit a 256 MiB total cache. Admit only
  snapshots that fit the existing budget and conservative per-snapshot page
  charges; do not silently raise it. Compare simple lookup/eviction before a
  radix tree or optimized sparse checkpoint-placement policy.
- Implement a short-chain native CPU block verifier before speculative proposals.
  Cover complete KV/GDN/convolution state, partial acceptance, EOS/output limits,
  Stop, failed commit and whole-request rollback. Establish greedy parity first,
  then conditional acceptance/residual sampling for the actual temperature,
  top-k/top-p/penalty policy. Distinguish target-distribution preservation from
  identical RNG consumption and seeded output.
- Add bounded prompt/history suffix proposals after the verifier is qualified.
  Start with a few short draft windows, no second model, and both repetitive and
  creative held-out prompts. Measure draft/verify/commit/recovery together;
  retain ordinary decoding and stop when acceptance does not pay for the cycle.
- After measuring the simple chain verifier, compare complete snapshot/replay
  with compact saved-factor recovery and delayed commit if state-copy/replay
  costs are material. Use SpecLA/TreeWY/RecoverSSM algebra
  as references for a native Rust chain implementation; preserve complete state
  and declared float identity. Wide-tree machinery waits for a demonstrated
  benefit over short chains. See [verification research](docs/inference-algorithms.md#trained-parallel-proposals-and-full-state-verification).
- After the verifier is qualified and measured verification/recovery costs leave
  a feasible break-even, screen an already published small drafter for exact
  tokenizer/vocabulary, Swift/LoRA and native ARM64 runtime compatibility. Check
  target-feature compatibility where the drafter consumes those features; an
  independent small model does not require that interface. Charge both models'
  weights/KV/features/scratch and batch-one startup/prefill cost before provisioning
  a full candidate. Suffix failure on low-repetition prose does not rule out a
  compatible learned drafter. A Qwen family name alone is insufficient; record a
  no-go if no compatible artifact fits. Do not
  train a general replacement drafter or expand into the deferred DSpark migration.
- Qualify each Rust replacement profile against pinned Prism at matching artifacts,
  adapters and settings, retaining deterministic streams where applicable,
  creative quality, resource/security/comfort results and reproducible fallback.
  Retire old build/overlay/FFI paths only after replacement acceptance.

Reference: [native Rust runtime contract](docs/native-rust-engine.md).

## Creative quality, adapters and memory

- Establish an independent writing/art-direction rubric covering drafting,
  constrained revision, continuity, dialogue, voice, tone, composition, instruction
  following, coherence, repetition, unwanted refusals and useful completed answers.
  Qualify the actual thinking/output settings and experimental Extra effort mode;
  changed text or benchmark averages are insufficient.
- Complete the [standard LoRA qualification](docs/lora-qualification.md): verify SSM
  basis at residual writers, loaded-at-zero response/logits identity and nonzero
  application; capture paired target-device trials and genuine independent human
  reviews. Require both same-engine decode TPS and output quality parity, keeping
  the bounded RAM premium and every individual regression visible.
- Qualify app-scale 1.3 and other intended adapter settings explicitly. Do not
  relabel the scale-one/512-token contract or choose a scale from one response.
- Evaluate Q8_0/Q4_K_M speech profiles and bounded CPU thread choices through
  target-device CI and independent listening, retaining voice quality alongside
  startup, first-audio, synthesis latency and resource evidence. Use the
  [paired speech comparison contract](docs/validation.md) and
  [hosted resource evidence](docs/speech-memory.md). Attribute voice bootstrap,
  warm Base prefill, residual-code prediction, codec, transport and playback
  separately. Record first generated PCM versus first audible output and chunk
  supply gaps/underruns. Conditioning reuse, bounded graph retention and PCM
  recycling already exist; optimize measured remaining work, not those labels.
  Include the Rust Philox semantic/predictor draw path in actual-model speech
  acceptance; exact block/rounding vectors and pinned-C++ bit comparisons remain
  model-free checks, separate from output, listening and resource qualification.
- Qualify chat alone, warm TTS alone and concurrent chat/TTS under a small set of
  shared CPU-thread budgets. Include native UI frame delivery, audio continuity,
  useful-answer time and sustained memory/heat. Qualify cold speech command waits,
  which now block until notified, separately from the retained periodic monitoring
  of active output, warm TTS and retained Canary input. Check idle CPU/waits and
  wake/Stop responsiveness during the existing 30-second warm TTS lease. Preserve
  ASR/TTS mutual exclusion and independent reserves; postpone speech speculation unless
  an existing compatible native artifact or a measured laptop-feasible tiny fit
  is available for the selected 1.7B path, rather than the paper's 0.6B model.
  Before any such fit, require a native residual-code verifier, sampler correction,
  accepted-state recovery and bounded extra scratch. Stop if verification/recovery
  erases the complete-readout benefit.
- Investigate lossless adapter graph-reservation and rank-one accumulation changes
  only with allocator and strict rounding/order checks. Resolve unexplained memory
  counter differences before claiming saved RAM; preserve dynamic scale and
  load-without-apply behavior.
- Qualify current-weight PTQ1 full-model logits/live state on scalar and explicit
  NEON, including no-LoRA/LoRA basis and forward execution. After correctness,
  run the bounded paired device screen and review complete responses,
  context/recovery, quality and comfort. Keep PQ2 as rollback; use the
  [PTQ1 contract](docs/ptq1-readiness.md).
- Promote PTQ1 to the default only after suitability and lower admission thresholds
  are demonstrated. Use the exact verified 1,259,520,000-byte packing difference
  for a candidate reduction; account for mapped residency separately from private
  commit and preserve the physical reserve.
- Capture prefill, first useful answer, decode TPS, complete-task time,
  retries/truncation, peak working/private/Job/system/device memory, hard faults,
  energy where observable, UI responsiveness and sustained heat/fan behavior.
  Interleave repeated paired trials, report uncertainty and qualify X1/X2
  independently. Separate instrumented attribution from unprofiled request timing,
  and idle from active power. Use consistent first/last-token boundaries rather
  than averages of incomparable rates; retain output/retry counts and energy per
  acceptable completed task. See [measurement research](docs/performance-expectations.md#2026-literature-measurement-and-comparison).
- Qualify creative, daily, long-context and battery profiles within total resource
  budgets. Larger context, Q8/Q4 KV, Flash Attention and altered recurrent precision
  require separate numerical/state/quality gates; do not silently discard canon
  or weaken output reservation to fit.
- Attribute actual mapped buffers, microbatch 32/64 costs, persistent output rows,
  post-prefill workspace trimming, optional prefetch, rotation/sign metadata and
  thread stacks before pursuing lossless memory reductions. Review integration
  of the immutable-B-sharing overlay against the intended runtime; preserve
  exact rounding and distinguish overlapping from disjoint allocation lifetimes.
- Independently exercise the Qwen reference's actual-model loading and service
  control when its 18 GiB allowance plus 4 GiB reserve can be admitted.
- Add source-referenced scene packets and author-approved writing context while
  keeping manuscripts and the edited project bible authoritative. Start with
  manual excerpts/lexical retrieval; require evidence of need before embeddings.
  Generated summaries need source versions and author acceptance. Vision/image
  critique or image generation needs a matching model and shared-memory budget.

## Bounded calibration and optional precision

These are conditional, separately identified native CPU experiments after exact
controls work. Use a small fixed calibration sample and disjoint held-out prompts,
online/bounded statistics and the research budgets above. Calibration alone never
qualifies quality or reduced admission; surviving candidates still need the
existing full-context/continuation/LoRA gates.

- Estimate component savings and conversion traffic before implementing a codec.
  Full-attention KV is 512 MiB at 8K; INT8 can save at most about 256 MiB of that
  payload before scales/scratch. Live recurrent/convolution payload is a separate
  149.625 MiB. Compare actual complete-process benefit with the existing PTQ1
  packing gain; do not apply a component's compression ratio to model weights.
- Screen simple native Q8 full-attention KV first. If it earns further work,
  compare one bounded asymmetric key/value or rotation-plus-scalar candidate,
  retaining FP16 as the control. Charge metadata, conversion, rotations and
  dequantization; reject full-plus-compressed copies as a total-RAM saving.
  Retain explicit numerical/cache identity and exact checkpoint ownership;
  defer low-rank/SVD or large-index schemes without a measured need at 8K.
- Only if state traffic or admitted snapshot storage is material, screen FP16
  recurrent state against FP32, then at most one DAMP-inspired calibrated
  FP16/INT8 split. Keep convolution precision separate and account for scales,
  rounding/RNG, restore and long-trajectory error. Low-rank bypass/directional
  rounding is a follow-up only if a simple screen exposes update loss and enough
  net benefit to justify its factors/merge/rollback complexity.
- Run a bounded entropy/correlation feasibility check on existing packed tensors,
  including scales, block/stream indexes, random access and native decode work.
  Shannonic supplies lossless coding ideas; implement a stream prototype only if
  a realistic byte/traffic estimate beats current PTQ1 without a retained expanded
  model. Keep the current-census BITCOS layout rejected. New weight codebooks,
  rotations or scale selection require a separate native, layerwise cost/benefit
  preflight before any quantization work; do not start a whole-model refit from a
  paper's GPU result. See [weight and state research](docs/swift-memory-options.md).

## Hardware selection

- Extend shared discovery beyond scoped CPU observations, native CPU sets and
  native Windows NEON/dotprod/I8MM availability reports: independently checked
  SoC/SKU, GPU/NPU API/features/architecture, driver/runtime/firmware,
  signed backend readiness and static accelerator identities. Reuse the checked
  representation in Doctor and launch policy; keep unknowns explicit and live
  resource budgets separate from immutable compatibility evidence.
- Qualify safe scalar/NEON CPU controls independently on X1 and X2, including
  optional ISA eligibility and phase-specific thread/batch/wait/power choices.
  Run the bounded [X2 CPU thread screen](docs/x2-cpu-optimization.md) on settled
  main and retain results before selecting a new default. Restore/register the
  owner-managed ARM64 runner first; the 2026-10-29 CI inventory reports none.
  Resolve the provisioned legacy completion runtime's model-free headless-start
  failure (`0xC0000142`) before capture; audit its shell DLL dependency chain
  and repeat a bounded `--version` probe without relaxing worker mitigations.
  Retain the screen's verdict for topology, AC/battery saver, active power scheme
  and effective mode at batch and per-sample boundaries, including the transition
  between paired samples; suppress comparisons when observations change or are
  unknown. Extend the screen
  only when its phase measurements identify a useful next candidate.
  Compare supported event-based waits with polling using idle CPU, wake latency,
  active decode and cancellation responsiveness. Inventory native Windows timing,
  energy/thermal and memory observations plus their sampling limits; absent valid
  telemetry means no energy verdict. Do not assume four threads, all cores or a
  marketing family is sufficient, or port a bare-metal gang scheduler into the app.
- Build a bounded reviewed profile catalog keyed by exact artifact/adapter,
  runtime/build/backend/driver/capabilities, numerical/state/settings identity,
  context/shape/output/workload buckets, objective and fresh/reused state.
  Retain paired evidence, thresholds and losing/no-go entries.
- Implement deterministic eligibility/ranking for interactive complete-task latency
  and battery energy within accepted bounds. Include startup/repack/compile,
  phase/state migration and sustained costs; use stable tie-breaking and no
  startup benchmark search.
- Recheck current budgets and backend readiness before admission. Choose a
  qualified CPU fallback or explicit strict-backend rejection without silently
  changing model, adapter, sampler, precision, context or output reservation.
  Report requested/effective profile, placement, evidence identity and value-free
  fallback reasons in diagnostics and a simple native Rust Auto control.
- Reevaluate at safe request boundaries after power, memory, runtime or driver
  changes. Prove complete-state migration or verified teardown/rebuild; invalidate
  affected evidence/caches and never continue a partial request on another backend.
- Cover unknown/misidentified devices, absent optional ISA/signed accelerators,
  stale identities, budget pressure, workload boundaries, deterministic ties,
  objective changes, fallback/strict rejection and failed/cancelled state handoff.
- Demonstrate automatic interactive/battery operation separately on supported X1
  and X2 classes, including reproducible installation/rollback, sustained use and
  affected-upgrade requalification. No energy ranking without valid telemetry.

References: [selection contract](docs/hardware-profiles.md),
[capability identities](docs/hardware-capabilities.md).

## CPU kernels

- Freeze artifact/runtime/compiler/format/shape/ISA and numerical contracts, then
  use CI hotspot, bandwidth, packing and dispatch evidence to select optimizations.
- Complete full-model and memory qualification of baseline NEON PQ2, odd-group
  PQ2 and PTQ1. Retain scalar forcing, bounded loads/quantization, ordered scales,
  malformed-input rejection and actual-backend diagnostics; qualify the single
  bounded LUT candidate set below without changing fourth-symbol semantics. Bound
  table construction, unsigned-code sums and corrections independently; a final
  128-term generic PQ2 dot fitting i16 does not certify intermediate i16 sums.
- Prototype direct PTQ1-by-Q8_K execution for eligible current-weight projections,
  retaining PQ2's actual activation grouping and odd-group fallback route. Start
  with scalar Rust and a few real Swift shapes, then baseline NEON. Preserve raw
  scales, quantized activations and floating scale/add order so compact packing
  can be evaluated independently from PTQ1-by-Q8_0's numerical change. Keep this
  a distinct experimental path through full-model/adapter qualification.
- Compare a bounded set of NEON layouts that unpack directly into consumption
  order, avoiding unpack/shuffle/repack passes. Start with up to four layouts and
  three representative projection shapes, one matrix at a time. Borrow Intel's
  two-bit layout principle rather than its VNNI instructions; compare at most two
  exact TL1_1/TL2_1-style LUT designs with the current control, validated ternary
  eligibility and generic PQ2 fallback. Charge preparation, source/repacked overlap, table/cache
  pressure and startup before claiming a whole-model gain.
- Implement one genuinely tiled projection-prefill family before a broader port.
  Compare a few token tiles, initially 8/16/32 where shapes permit, sharing
  compatible activation preparation and bounded cache-sized LUT/weight tiles.
  Reuse weights across tokens without a persistent INT8/FP16 expansion. Count LUT
  construction and scratch; stop if those costs erase the reuse benefit.
- Use the audited native Windows NEON/dotprod/I8MM availability observations
  in [capability discovery](docs/hardware-capabilities.md) to establish the
  installed OS/process eligibility before separately dispatched variants.
  Inspect Release code generation and
  forbidden-ISA leakage; require reliable detection, parity and useful benefit.
- Optimize measured Hadamard/elementwise, activation/quantizer, reduction and tiled
  prefill bottlenecks through safe APIs or an audited isolated unsafe boundary.
  Expand boundary/tie/extreme/unaligned/malformed/rollback and non-ARM/absent-ISA
  fixtures; recheck dependency/license/code generation after relevant changes.
- Compare scalar Rust, optimized Rust and applicable Prism controls in paired
  target-device CI, separating hot/streaming weights, packing/dispatch, prefill,
  decode, task cost, allocations and sustained energy/thermal behavior. Admit
  only qualified per-device, format/shape, phase and objective entries.

Reference: [CPU arithmetic and dispatch contracts](docs/simd-plan.md).

## Adreno Vulkan

- Capture native ARM64 GPU/SKU/OS/driver/loader/ICD/dependency identities and exact
  Vulkan queues/features/extensions/limits, subgroup/FP16/integer-dot/cooperative
  matrix support and available memory-budget telemetry. Reject software/emulated
  execution and preserve explicit unknowns/conservative admission.
- Bound a public native Windows ARM64 OpenCL feasibility comparison alongside
  Vulkan, checking actual source/binary availability, packed Swift operations,
  fallback and API features. End with a reproducible candidate or no-go; keep
  Qualcomm's internal assembly kernels separate. A public OpenCL comparator is
  not a change to the selected Vulkan/CPU profiles or a new UI dependency.
- Build a separately locked pinned-Prism `GGML_VULKAN=ON` candidate with reviewed
  ARM64 SDK/shader/compiler/library identities, unrelated accelerators disabled
  and a bounded experimental profile preserving the CPU lane and service policy.
- Produce the complete PQ2/PTQ1 operation/placement/fallback/transfer ledger,
  including embedding/output indexing, quantizers/transforms, attention/RoPE,
  GDN/convolution, hybrid state and adapters. Verify actual descriptor placement
  without forced-limit overrides.
- Prove packed/operator and full-model logits/KV/GDN/lifecycle parity against
  same-artifact CPU controls, including Vulkan Q8_1 versus CPU Q8_K/Q8_0 arithmetic,
  awkward shapes and separate no-LoRA/approved-LoRA basis/forward lanes.
- Screen CPU-only, bounded partial and maximum feasible offload in device CI.
  Measure prefill/decode, batches/threads/waits, transfer/submission/kernel costs
  and phase migrations. Attribute CPU preparation/packing, command submission,
  GPU service, completion waits and sampling separately; distinguish a slow kernel
  from an idle GPU awaiting host work. Qualify attention/cache/flash changes
  independently and compare eligible Hexagon candidates.
- Optimize only measured kernel, layout, residency, descriptor/pipeline/allocation,
  scheduling and cache bottlenecks. Compare packed-field/coalesced layouts,
  vector unpacking, shape-specific tiles and supported submission replay;
  image/texture-backed weights require the installed native API's actual support.
  Charge cold preparation and retained copies, count physical memory once,
  bound/invalidate caches and preserve asynchronous buffer lifetimes. Rerun
  numerical/state/adapter/lifecycle gates after changes.
- Qualify per-device GPU profiles for quality, comfort, memory and useful benefit;
  integrate current-budget admission and effective-plan reporting. Exercise device
  absence/loss, allocation pressure, stale evidence, cancellation and upgrade
  invalidation with committed-state preservation and a reproducible CPU rollback.
- After Rust CPU parity, separately qualify Rust GPU coverage and audited device
  safety against the same full-model/state and per-device acceptance contracts.

Reference: [Adreno architecture and qualification contracts](docs/adreno-gpu-plan.md).

## Hexagon NPU

- Discover bounded native signed access independently on X1/v73 and X2/v81,
  recording driver/firmware/SDK/compiler/source/DLL/DSP/ABI/license identities,
  signatures and actual mapping/execution. Audit DMA64, low-bit Hadamard and HTP
  leads; prove a small supported-type dispatch fixture and retain scoped no-go
  reasons. Trust/Secure Boot changes require a separate owner decision.
- Map Swift operations/types/shapes/strides/transforms/basis, state, support/fallback
  and resident/scratch bytes for prefill/decode. Choose one bounded prototype
  region; generic Q2/Q4/Q8 support does not establish PQ2/PTQ1 compatibility.
- Implement exact lossless PQ2 tiling/fused packed matvec and separately qualify
  PTQ1 canonicality/Q8_0, including symbol/scale/rounding/tail/malformed fixtures.
  Audit any PQ2-to-Q2 bridge's metadata and storage premium before timing.
- Evaluate eligible HVX packed work and genuine batched HMX prefill. Bound VTCM,
  DMA, padding/repack/expansion scratch; prove dependency/fusion arithmetic,
  transform/grouped recurrent basis and independent adapter execution. Exercise
  descriptor-ring saturation, partial-tile initialization, double-buffer ownership
  and cancellation with transfers outstanding. Check requested versus effective
  VTCM and allocation refusal; use the documented DMA-ring failure as a regression
  pattern without assuming the selected Swift path has that defect.
- Prove ownership and restoration of resident weights, scratch, KV/convolution/GDN,
  checkpoints and phase layouts across fresh/reused/edited prefixes, cancellation,
  reset and failed continuation. Qualify chunkwise scheduling separately.
- Establish total peak memory/address-space accounting across host/device mappings,
  copies, RPC, state, caches and load overlap. Verify the selected X1 mapping window
  and signed X2 DMA64 path; start with one session and justify partitioned sessions.
  Preserve Swift admission/reserve and account for resources outside Job commit.
- Screen CPU, qualified Adreno, contiguous Hexagon and mixed phase placements on
  X1/X2 with fixed model/adapter/sampler/context/precision. Include startup,
  migrations, mapping/traffic/RPC/synchronization, contention and sustained power;
  prioritize X2 packed prefill/decode investigation when a target is available.
  Separate RPC/queue work, dynamic quantization, DMA, HVX/HMX execution, wake-up and
  completion waits. Compare contiguous regions with fragmented offload; phone
  phase rankings and unavailable root/DVFS controls do not choose a Windows winner.
- Integrate a separately locked experimental profile with checked effective
  placement/readiness, child-only environment and owned helper teardown. Preserve
  authentication/counting/one-slot/deadline/cancellation policies and reject arbitrary
  overrides; qualify isolation/resources beyond the single-process Job.
- Cover unsupported shapes, bounded waits, allocation/mapping failure, device loss,
  cancellation after submission and stale compiled/repacked caches. Never free
  in-use buffers, replay secretly or continue partial work on CPU; restore complete
  committed state or close admission through verified teardown.
- Qualify/publish scoped device profiles or no-go results through paired repeated
  CI over admitted short/near-limit prompts, full output reservation, fixed/natural
  completions, edited/reused state, AC/battery, pressure and 15–30-minute sustained
  use. Require correctness/security/quality/resources, LoRA parity and useful
  benefit, retaining raw evidence, uncertainty, CPU rollback and upgrade rules.
- Separately qualify Rust NPU coverage and audited device safety after Rust CPU
  parity; signed deployment and full-state correctness remain mandatory.

Reference: [Hexagon deployment and arithmetic contracts](docs/hexagon-npu-plan.md).

## Survey ideas outside the current laptop backlog

Keep T-SAR/ExaGEMM instruction/datapath modifications, RVV/SVE/SME ports requiring
unavailable hardware, proprietary internal kernels and Android-root/QPT/DVFS
controls as literature context. They are not prerequisites or implementation
tasks for the current X Elite/X2 Elite program.

Methods requiring new trained weights/architectures/readers (GDN-2, Sherry,
ReaDy), large drafter training,
full-Hessian quantization pipelines and wholesale lattice/rotation refits are
not scheduled. CoS-VQ/YAQA/FastKron/ELUTQ/QTEA/HARP/Tetra can inform a future
bounded native preflight, but their published GPU pipelines do not establish a
reasonable laptop job or a better already-ternary Swift artifact. Likewise,
ESTAR/BudgetThinker require compatible training; existing thinking/effort controls
remain the practical task-token baseline.

Defer DASC/Tail-Replay checkpoint omission until exact reuse demonstrably cannot
meet an actual cache need; simple suffix recomputation comes first. Defer wide
TreeWY trees, JoLT/iS-KV SVD caches and certified sparse-attention index ports
until a batch-one/8K bottleneck and bounded native benefit justify them. VeriCache
and multi-tier/multi-request serving do not supply a total-RAM saving for the
current one-request shared-memory runner. Retain withdrawn PolarQuant and the
unresolved Litespark claims as negative evidence, not implementation work.

## Deferred Prism migration

Broader runtime implementation remains owned by
[Prism Inference](https://github.com/symbiogenesis/prism-inference).

- Complete full reference qualification and hosted provenance: every backend
  partition, all 22,817 ordered evaluation/support geometries, the separate
  1,530 performance workloads and exactly 70 Windows-applicable source results
  with the Apple exclusion explicit. Ratchet baselines only from complete
  reviewed evidence, retaining required green IDs and full diagnostic handoffs.
- Complete numerical operations, custom/map callbacks and backward graphs within
  pinned shape/stride/alias/arithmetic contracts. Preserve transactional gradients,
  packed ADD1 restrictions and explicit unavailable GEGLU/FLASH_ATTN_BACK
  derivatives; do not invent skips or port an aborting native constructor blindly.
- Finish remaining packed importance encoders, including IQ2_XXS/XS and IQ1, and
  full-source codec qualification. Preserve native-unavailable encoder identities
  and numeric-zero canonicalization rather than undefined native payload bytes.
- Complete owned `qb-*` tensor/transport APIs, persistent model sessions, file
  transfers and real supported clocks for quantization/function/performance/stats
  sources: all 144 trait slots, 22 eligible types, original layouts/offsets/
  schedules/thresholds and actual pinned F32/F16 statistics.
- Complete general chat direct apply/statistics, differential template analysis,
  generated parsers, format detection, reasoning/tool mapping and all six named
  capability-execution gaps. Preserve original assertions, supplied time,
  undefined-variable behavior, byte-prefix diffs, reconstruction and distinct
  byte-regex/UTF-8 domains; finish server reasoning budgets, template kwargs,
  Responses/developer conversion and model-specific thinking/tool ordering.
- Complete production CLI registration/negation/environment handling across all
  27 contexts, parameter mutation, numeric/CSV validation and speculative
  output-limit conversion. Finish bounded HTTP/HTTPS/curl retrieval with original
  headers/timeouts and 200/404/oversize/cancellation/body-framing behavior.
- Qualify resolver URL/multi-shard/redirect/Content-Length domains, recursive
  cached snapshots and metadata-inferred draft resolution. Support or explicitly
  reject native ECMAScript-only quant-tag regex syntax.
- Provide generic encoder/decoder contexts, vocabulary/logit ownership, graph
  reservation/export, sequence/KV state, independent checkpoints, file/buffer
  save/restore and concurrent persistent sessions with callback errors. Retain
  generated-tensor/progress callbacks, streaming GGUF saving and nonfinite rejection.
- Implement architecture-family graphs, tensor/metadata inventory and hybrid
  memory/session contracts across the source's 149 names, 151 variants and
  128 supported variants, preserving unsupported distinctions and original
  encode/decode/NMSE/export/reload/restored-logit assertions.
- Complete original model/progress/state/concurrency sources against unchanged
  stories15M assets and full schedules, recurrent rollback, backend sampling,
  graph export and KV mean-centering behavior.
- Complete the four DSpark sources' metadata/log-SNR, forward/loop/speculative
  state and real-eval APIs with original synthetic references and the full
  24-prompt AR/verification schedule. Complete actual LoRA conversion,
  merge/application and inference, retaining approved production corrections.
- Provide actual LLGuidance/model-session grammar integration and public C
  header/ABI through safe Rust boundaries. Keep `test-c.c` compile-only with
  zero behavioral assertions.
- Finish every tokenizer-source qualification, including original decoder/loader
  failures, file wrappers/generators, randomized HF references, immutable corpora,
  complete SPM/BPE token/Unicode roundtrips and ignore-merges behavior. Preserve
  the invalid-UTF-8 skip, Falcon exclusion and verified Llama 3 correction.

Reference: [exact conformance requirements](docs/prism-upstream-tests.md).

## Deferred dependency experiments

- Follow the [dependency audit](docs/dependency-audit.md) and
  [MXC #1579](https://github.com/microsoft/mxc/issues/1579) for backend/capability
  dependency features in the supported SDK before changing supervisor boundaries.
  Keep all three supported Windows wgpu
  backends explicitly selected; any future removal needs a separate support
  decision and device/fallback qualification. Consider separating artifact archive
  APIs only if independently built speech companions show a worthwhile build/size
  benefit. Keep containment,
  accessibility and native UI behavior intact; all timing benchmarks remain CI-only.
- Qualify the pinned SIMD-JSON tape-reuse candidate through main-only CI. Require
  unchanged Unicode/duplicate/unknown-field/shape/bound/canonical-byte checks to
  pass and at least 5% lower equal-weight geometric-mean time
  with no case more than 5% slower in both cold and reused-request screens,
  including supplemental scale-setting cases. Preserve the existing dependency decision
  until those gates pass; see [dependency contracts](docs/dependency-choice.md).

## Optional agents

- Qualify creative chat before enabling agent automation. Evaluate OpenCode first
  and Aider as fallback, preserving a bounded authenticated inference bridge.
- Qualify whole-agent, plugin, hook and descendant containment through MXC
  ProcessContainer with `allowDaclMutation:false`, no audit mode for untrusted
  execution and no unsandboxed fallback. Exercise filesystem/reparse, handle,
  subprocess and network bypasses; prompts are not containment. A VM tier needs
  separate acceptance when a separate kernel is required.

## Promotion criteria

- Complete each profile's correctness, security, full-state recovery, creative
  quality and resource gates, including independent LoRA TPS/quality requirements.
- Demonstrate bounded memory without sustained paging and owner-accepted
  responsiveness/heat/fan behavior over sustained use.
- Establish a useful paired target-device CI benefit: initially 10% lower
  end-to-end latency, or a documented capacity/energy benefit within accepted
  latency/quality limits. Predeclare tolerances and retain losing/no-go outcomes.
- Reproduce accepted results on settled `main`, with immutable artifact/build/device
  identities, run/capture references, declared limitations and a rollback profile.
  All benchmarks and full suites stay in their owning main-only CI workflows;
  local builds/analyzers and focused correctness checks remain allowed under
  [validation policy](docs/validation.md).
