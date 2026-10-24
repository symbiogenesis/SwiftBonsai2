# Native Rust Prism engine

The experimental [Rust CPU reference](../src/prism-runtime/README.md) implements
verified loading, the Qwen35 hybrid graph and transactional greedy generation
from token IDs, without a C/C++ inference library. Eligible Windows ARM64
PQ2/PTQ1 operations use baseline NEON automatically; scalar remains the reference
and fallback. **Full-model parity, broker integration and device qualification
are pending.** Pinned Prism remains the comparison and fallback engine.

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
  artifact's tensor inventory, tokenizer, chat template, special/stop tokens,
  sampler/RNG and adapter forward path. Reuse bounded GGUF/codec components and
  Rust-owned LoRA storage; ggml pointer bindings are not replacement tensors.
  Retain malformed-input, allocation and unsupported-contract checks. Exit with
  exact weight/metadata fixtures, scalar parity and bounded full-model comparisons
  against Prism.

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

  - [x] **R4.1 — Reuse logits and graph scratch.** Stable `Decoder::evaluate_into`
    reuses caller-owned output; Qwen35 retains graph/kernel workspace and stages
    state deltas before commit. Checkpoints own independent state/logits.
    Synthetic fixtures cover output validation, reuse, cancellation and rollback.
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

  - [ ] **R5.1 — Reassess borrowing only for a concrete interface limitation.**
    The production reusable-output API works on stable. Use the [nightly trial](../tools/rust-trial/README.md)
    only when a real lending/cache/stream interface demonstrates a limitation;
    record the stable alternative, production benefit and compatibility evidence
    before considering migration. Checkpoint ownership still applies.
  - [x] **R5.2 — Share immutable KV checkpoint pages.** Sealed 64-token FP16
    pages are shared immutably; the mutable tail and exact convolution/recurrent
    state are copied. Page-boundary and edited-prefix regressions prove snapshot
    isolation and recovery. Snapshot admission conservatively charges every
    referenced page per snapshot, including page tables, private tail capacity
    and recurrent storage, without relying on sharing to meet the bound.
    Full-model process-memory benefit remains R4.2.

- [ ] **R6 — Add Adreno after CPU parity.** Follow [G0–G5](adreno-gpu-plan.md),
  implementing the same packed types, transforms and complete hybrid graph.
  Reuse Prism capability reports/fixtures, but qualify Rust GPU coverage and
  safety separately. Evaluate resident state/scratch, fusion, submission reuse
  and phase-specific placement, including migration and shared-memory costs.
  Preserve [attention rounding constraints](inference-algorithms.md#attention-locality-and-the-io-bound).
  Exit with correct whole-model ARM64/Adreno execution and target-device CI benefit.
  [Hexagon N0/N1 discovery](hexagon-npu-plan.md) can proceed independently; neither
  accelerator bypasses R5 ownership or R7 qualification.

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
