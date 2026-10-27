# Swift Bonsai 2 Runner TODO

This is the single list of project work. Completed implementation steps are
separate from outstanding qualification. Technical contracts, commands and
supported-subset boundaries live in `docs/`.

Prioritize actual-model application acceptance, daily-service isolation and
creative/comfort qualification. Preserve the Swift CPU runtime boundary while
closing those exits. Hardware discovery and bounded Vulkan/Hexagon access
investigations can proceed independently; automatic profile promotion requires
qualified device evidence. Reopen broader Prism work only for a concrete Swift
dependency or an explicitly chosen later milestone.

## Completed memory reductions

These implementation steps address the allocation and bounded-resource
work below. Their storage contracts are documented in
[bounded streaming resources](docs/streaming-resources.md); complete-model peak
memory, throughput and device qualification remain outstanding.

- [x] Bound retained native TTS decode scratch to the current attention-window
  graph, evicting completed obsolete graphs before replacement allocation while
  preserving weights, KV, hidden-state storage and graph arithmetic.
- [x] Share immutable committed chat content/reasoning across worker, UI and
  request/restore snapshots; copy only the user task augmented by Extra effort.
  Bound recovery/request serialization before oversized output accumulates.
  Preserve independent histories, exact JSON text and interrupted-turn recovery.
- [x] Bound canonical broker serialization as bytes are written and release the
  generation request allocation after write/flush, before response intake.
  Preserve the exact same canonical bytes for counting and generation.
- [x] Admit one recovery snapshot before serializing it, retaining that slot
  through encryption and disk completion. Skip occupied-worker serialization
  and preserve dirty-state retry, explicit erase ordering and the final save.
- [x] Parse speech multipart fields and header lines lazily, rejecting excess
  fields without retaining a body-sized delimiter index. Reuse the authorized
  HTTP intake allocation for the body and bound fallible request-body growth.
- [x] Transfer valid UTF-8 startup captures into their final strings and write
  stdout/stderr logs sequentially, eliminating duplicate full-output buffers
  while preserving lossy decoding, output limits and exact log ordering.

Swift's owner-selected development budget now admits at 15.5 GiB available
memory with an 11.5 GiB Job commit ceiling, reducing each by 512 MiB from the
original 16/12 GiB budget while preserving the 4 GiB physical reserve and
memory-pressure shutdown. This is a
budget adjustment, not measured model-memory savings or completed qualification.
Full-context/4K-output resource acceptance remains outstanding. Further
current-weight PTQ1 reductions also need numerical qualification; see
[its numerical and admission boundaries](docs/ptq1-readiness.md#numerical-and-admission-boundaries).

## Application and daily service

- Qualify actual-model startup, supplied-history streaming, full 8K-context/4K-output
  requests, long conversations, speech input/output, offline restart and recovery.
  Exercise Stop, abandonment, deadlines, memory pressure, process cleanup and
  explicit retry without replaying interrupted turns.
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
  are insufficient. Qualify the implemented native MXC containment, including
  actual-model loading and private temporary storage. Close its broader host
  loopback authority with proven broker-only communication, preserving atomic
  Job limits and exact worker identity without weaker fallback tiers. Follow the
  [worker sandbox architecture](docs/sandboxing.md),
  [Windows integration review](docs/windows-integration.md) and qualify the
  [daily-service boundary](docs/broker-policy.md).
- Qualify the complete transport/security boundary: numeric loopback, exact
  Host/Origin/routes, private credentials, tools/MCP and dynamic-loading rejection,
  capability-override rejection, bounded queues/bytes/tokens/duration and reliable
  cancellation when clients disconnect or stop consuming output. Extend the
  [bounded streaming regressions](docs/streaming-resources.md) to actual-model
  ready-prefix delivery, long-transcript updates, stalled-consumer speech backlog,
  speech/chat overlap and recovery trials; capture complete-task latency and peak
  memory in target-device CI before claiming performance gains.
- Qualify recovery under load, allocation failure and backend loss without restart
  loops, silent profile changes, state corruption or automatic cloud fallback.

## Runtime and correctness

- Audit comparison and reusable engine/kernel sources at immutable revisions,
  including model/adapter/tensor/transform identities, hardware assumptions,
  numerical evidence, licenses and notices. Keep PQ2, current-weight PTQ1 and
  the Qwen reference as separate controls.
- Qualify the pinned native Rust runtime over full-context/long-output Swift
  workloads, including scalar/NEON logits, complete live KV/recurrent/convolution
  state, exact prompts/token IDs, sampler/termination and lifecycle recovery.
  Preserve the declared `1e-3 + 1e-3 * abs(reference)` numerical tolerance.
- Complete Rust adapter execution and its full-model basis/logits/generation
  qualification before admitting adapter-enabled replacement profiles. Keep
  current-weight PTQ1 and other model/format support separately qualified.
- Measure complete model-level allocation, scratch lifetimes and peak memory for
  reusable logits, state-only prompt ingestion, activation sharing, physical FP16
  KV, checkpoint pages and retained sampler candidates. Establish retained
  memory/throughput benefits rather than inferring them from fewer buffers;
  qualify tiled attention-value traversal on complete long-context workloads.
- Develop exact packed lookup/Vec-LUT kernels and genuine bounded tiled prefill
  where profiling justifies them. Prove causal scheduling, hybrid-state parity,
  scratch bounds, cancellation and whole-request rollback; qualify chunkwise GDN
  separately. See [algorithm constraints](docs/inference-algorithms.md).
- Qualify full-model prompt reuse with exact model/template/token identity and
  matching KV plus recurrent/convolution state. Bound snapshots/eviction/private
  storage and cover edits, conversation switches, cancellation and restore.
  Evaluate the proposed two-checkpoint/256 MiB cache budget against total memory.
- Investigate model-free prompt lookup speculation only after a complete block
  verifier, sampler/RNG acceptance correction and hybrid-state rollback are proven;
  require end-to-end benefit. New draft models, approximation,
  retraining and reduced state precision need separate quality/numerical scope.
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
  startup, first-audio, synthesis latency and resource evidence.
- Investigate lossless adapter graph-reservation and rank-one accumulation changes
  only with allocator and strict rounding/order checks. Resolve unexplained memory
  counter differences before claiming saved RAM; preserve dynamic scale and
  load-without-apply behavior.
- Qualify current-weight PTQ1 full-model logits/live state on scalar and NEON,
  including no-LoRA/LoRA basis and forward execution. After correctness, run the
  bounded paired device screen and review complete responses, context/recovery,
  quality and comfort. Keep PQ2 as rollback; see the [PTQ1 contract](docs/ptq1-readiness.md).
- Promote PTQ1 to the default only after suitability and lower admission thresholds
  are demonstrated. Use the exact verified 1,259,520,000-byte packing difference
  for a candidate reduction; account for mapped residency separately from private
  commit and preserve the physical reserve.
- Capture prefill, first useful answer, decode TPS, complete-task time,
  retries/truncation, peak working/private/Job/system/device memory, hard faults,
  energy where observable, UI responsiveness and sustained heat/fan behavior.
  Interleave paired trials, report uncertainty and qualify X1/X2 independently.
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

## Hardware selection

- Extend shared discovery beyond scoped CPU observations: native CPU/OS ISA and
  topology, actual SoC/SKU, GPU/NPU API/features/architecture, driver/runtime/firmware,
  signed backend readiness and static accelerator identities. Reuse the checked
  representation in Doctor and launch policy; keep unknowns explicit and live
  resource budgets separate from immutable compatibility evidence.
- Qualify safe scalar/NEON CPU controls independently on X1 and X2, including
  optional ISA eligibility and phase-specific thread/batch/wait/power choices.
  Do not assume four threads, all cores or a marketing family is sufficient.
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
  malformed-input rejection and actual-backend diagnostics; compare exact
  TL1_1/TL2_1 lookup candidates without changing fourth-symbol semantics.
- Audit Windows optional-ISA detection against SDK/OS APIs before separately
  dispatched dotprod and i8mm variants. Inspect Release code generation and
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
  and phase migrations; qualify attention/cache/flash changes independently and
  compare eligible Hexagon candidates.
- Optimize only measured kernel, layout, residency, descriptor/pipeline/allocation,
  scheduling and cache bottlenecks. Count physical memory once, bound/invalidate
  caches and rerun numerical/state/adapter/lifecycle gates after changes.
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
  transform/grouped recurrent basis and independent adapter execution.
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
  prioritize X2 packed prefill/decode investigation.
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

- Follow the [2026-10-26 dependency audit](docs/dependency-audit.md) and
  [MXC #1579](https://github.com/microsoft/mxc/issues/1579) for backend/capability
  dependency features in the supported SDK before changing supervisor boundaries.
  Keep all three supported Windows wgpu
  backends explicitly selected; any future removal needs a separate support
  decision and device/fallback qualification. Consider separating artifact archive
  APIs only if independently built speech companions show a worthwhile build/size
  benefit. Keep containment,
  accessibility and native UI behavior intact; all timing benchmarks remain CI-only.
- Reopen the pinned SIMD-JSON submodule experiment only after a concrete change
  addresses the fixed-source timing no-go. Require that
  unchanged Unicode/duplicate/unknown-field/shape/bound/canonical-byte checks
  pass and main-only CI shows at least 5% lower equal-weight geometric-mean time
  with no case more than 5% slower. Preserve the existing dependency decision
  until those gates pass; see [dependency contracts](docs/dependency-choice.md).
  [Follow-up #1](https://github.com/symbiogenesis/SwiftBonsai2Runner/issues/1)
  also rejected the padded tape path. Upstream
  [tape reuse #494](https://github.com/simd-lite/simd-json/issues/494) remains
  unqualified for this workload.

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
