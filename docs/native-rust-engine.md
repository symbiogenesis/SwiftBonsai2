# Native Rust Prism engine

Implementation status, October 23, 2026. The goal is a
native Rust implementation of the targeted Swift/Bonsai inference path, using
useful implementation techniques from Camelid, Orangu and other engines. The
pinned Prism engine remains the comparison and fallback until replacement
qualification passes. The existing Rust LoRA overlay is an implemented storage
component. The experimental [native CPU reference](../src/prism-runtime/README.md)
now implements bounded verified loading, packed scalar arithmetic, the Qwen35
hybrid graph and transactional token generation. It accepts token IDs and greedy
decoding; it has no C/C++ inference dependency. R2/R5 have implementation progress,
but their complete parity and integration exits remain outstanding. Tokenizer,
chat-template and stochastic-sampler parity, adapter forward execution, broker
backend integration, broader optimized kernels and device qualification remain
to do. The first opt-in [baseline NEON PQ2 kernel](simd-plan.md) now passes exact
synthetic integer/matvec checks through safe core SIMD APIs. Scalar remains the
default; this does not complete R3 or establish target-device speed/parity.

The separate [Hexagon NPU plan](hexagon-npu-plan.md) expands M5 into N0–N7.
Its signed-access and compatibility discovery can run alongside this roadmap;
packed kernels, transform fixtures and hybrid-state contracts share the same
reference work. External Prism/GenieX controls do not satisfy the native Rust
replacement gate. A later Rust NPU path needs a safe ownership interface and
audited driver boundary while preserving the no-C/C++-inference-library scope.

## Scope and performance goals

Keep the exact model weights, decoded scales, activation/cache precision,
Hadamard transforms, attention topology, sampler and ordinary decoding method.
Allow lossless layouts, SIMD, tiling, buffer reuse, operation fusion and better
scheduling. Floating-point reassociation still needs declared numerical
tolerances; unchanged algorithms do not guarantee bit-identical full-model
output. Precision reduction, approximate attention, pruning, retraining and
speculative decoding belong to separate experiments.

These are engineering estimates for end-to-end throughput, not measured
Snapdragon results or mandatory speedup thresholds:

| Scenario | Planning estimate |
| --- | ---: |
| Equivalent C++ to Rust translation | Approximately parity; regression is possible |
| Tune a reasonably optimized implementation | 1.2–2× |
| Strong hardware-specific implementation | 2–3× optimistic target |
| Eliminate a severely inefficient existing path | 3–5× or more, conditional upside |

Aim for roughly **2×**; treat **3–5×** as a stretch case requiring evidence of
major baseline inefficiencies. Rust itself earns no assumed speedup, and gains
from different projects cannot be multiplied together. Measure prompt processing,
generation and total task time separately. Once necessary memory traffic
saturates sustainable bandwidth, faster arithmetic alone offers little gain.

## Ordered work

- [ ] **R0 — Audit reusable techniques and pin the comparison.** Review Camelid,
  Orangu and relevant packed-kernel implementations at immutable revisions.
  Record source paths, licenses, notices, hardware assumptions, known bugs and
  correctness evidence before reusing code; independently implement techniques
  when direct reuse is incompatible. Inventory the exact Swift artifact's
  tensor types, dimensions, transforms and adapter contract. Keep PQ2 and the
  verified current-weight PTQ1 export as distinct comparison lanes because their
  activation arithmetic can differ. Preserve the earlier Qwen reference lane;
  this work does not change model selection or silently substitute artifacts.
  Exit with an implementation map and reproducible baseline identities.

- [ ] **R1 — Profile the native ARM64 baseline in CI.** Pin model/adapter hashes,
  Prism commit, compiler/options, driver, context, batch sizes, sampler, threads
  and power mode. Capture packed matrix operations, unpacking, Hadamard work,
  recurrent/full-attention work, allocation/copy costs, synchronization and
  actual memory traffic. Separate prefill from decode and distinguish cold load
  from warm operation. Use Amdahl and bandwidth bounds to rank work on the
  critical path rather than summing overlapping CPU/GPU time. Exit with the
  first kernel target and a justified headroom estimate, not an assumed 2× win.

- [ ] **R2 — Build the Rust correctness reference and loader.** Reuse or factor
  the existing bounded Rust GGUF and codec components instead of adding another
  parser. Implement the selected artifact's complete tensor-type inventory,
  quantized scales/symbols, Hadamard/sign/group transforms, embedding/output
  handling, Qwen35 full attention and Gated Delta Net recurrent graph. Preserve
  tokenizer, chat template, special/stop tokens, sampler and RNG behavior.
  Reuse the LoRA sharing policy through Rust-owned storage; the current ggml
  pointer bindings cannot serve as the replacement's tensor implementation.
  Validate malformed/truncated inputs, allocation limits and unsupported
  contracts before execution. Exit with exact decoded-weight/metadata fixtures,
  scalar-operation parity and bounded full-model correctness against Prism.

- [ ] **R3 — Prove the hottest packed CPU kernel first.** Implement fused
  unpack-and-dot using the detailed [SIMD plan](simd-plan.md), S0–S4 and S6.
  Evaluate stable safe SIMD APIs before introducing a separate audited intrinsic
  boundary; keep the runtime's unsafe-code prohibition. Begin with baseline NEON
  integer unpack/dot and unchanged scalar quantization/scale ordering; qualify
  PTQ1 and optional dotprod/i8mm independently. Preserve the
  baseline's activation quantization and accumulation contract; avoid persistent
  expanded weight copies unless a separately measured layout tradeoff wins.
  Validate actual Oryon feature detection and instruction results, with a scalar
  fallback. Do not assume another ARM board's i8mm support, big/little topology
  or Linux affinity policy applies to Windows. Cover block edges, awkward
  dimensions, zero/extreme values, scale bits and tail handling. Exit with
  focused correctness and CI microkernel plus end-to-end evidence before
  committing to the remaining optimized kernel inventory.
  The first experimental PQ2 kernel, safe dependency route, explicit selection
  and focused regressions are implemented. Profiling, complete diagnostics,
  other formats/ISAs and performance promotion remain pending; see S1/S2.

- [ ] **R4 — Optimize CPU memory and prompt processing.** Add phase-specific
  tiling, lossless packing/interleaving, reusable scratch, persistent thread
  pools and fused operations where R1/R3 show benefit. Preserve accumulation
  precision, account for rounding changes, and measure repacking time, peak
  copies and resident bytes. Carry forward immutable LoRA B payload sharing,
  distinct tensor metadata and the existing allocation/math regression. Qualify
  adapter loading/basis and forward execution with the unchanged artifact;
  rank-one fusion must preserve its arithmetic contract. Exit with incremental
  same-workload CI results rather than combined hypothetical multipliers.
  [SIMD S5](simd-plan.md#delivery-sequence-and-exits) details later transforms,
  quantizers, floating reductions and prefill tiles; changing the reduction or
  FMA contract needs separate numerical qualification before performance capture.

  - [x] **R4.1 — Integrate reusable decoder logits and graph scratch.**
    `Decoder::evaluate_into` replaces a caller-owned logits buffer on stable
    Rust; the owned `evaluate` interface remains compatible. `Runtime` retains
    its output allocation across token steps, prefix restoration and reset,
    validates every output, and restores independently owned logits/state after
    failed evaluation or cancellation. `Qwen35` retains graph workspace and
    kernel activation storage outside checkpoints, computing into reusable
    per-layer deltas before committing state. Focused synthetic regressions
    cover numerical fixtures, storage reuse, invalid output, cancellation and
    recovery. Immutable tensor payloads and owned checkpoint isolation remain
    unchanged. This implements storage reuse, without qualifying a full-model
    allocation, memory or throughput benefit.
  - [ ] **R4.2 — Qualify the production allocation benefit.** Compare the owned
    output baseline with reusable logits/scratch using the same model and
    workload. Record allocation counts/bytes and peak memory for prefill and
    decode; check logits, generated tokens, prefix restoration and failure
    recovery. Run performance measurements only in CI when requested. Synthetic
    address/capacity reuse is not evidence of a full-model memory or speed gain.

- [ ] **R5 — Implement and qualify the complete Rust runtime.** Add bounded
  conversation state, cache/checkpoint ownership, exact prefix restoration,
  streaming, cancellation, edited turns, slot reset and failure recovery. Keep
  unsafe SIMD and OS/device calls in small documented boundaries with safe
  interfaces, runtime self-checks and reviewed fallbacks. Apply the existing
  strict Rust standards and dependency/provenance review. Integrate the broker's
  count/completion/readiness contract, authentication, Host/Origin/route policy,
  process ownership and memory limits. Exit with native Windows ARM64 builds,
  focused regressions and full CI correctness/security coverage. The accepted
  Rust path must have no C/C++ inference-library dependency; necessary driver
  interfaces and device shaders remain explicit boundaries.

  - [ ] **R5.1 — Reassess solver requirements against a concrete lending API.**
    Apply the pinned [nightly trial](../tools/rust-trial/README.md) when a real
    tensor-cache, streaming-reader or backend-output interface encounters a
    conditional-borrow or higher-ranked associated-type limitation. Record the
    stable alternative and the safe-code simplification or capability gained.
    Keep stable as the default until a production benefit and compatibility
    evidence justify migration; the existing reusable-buffer prototype already
    works on stable and does not remove checkpoint ownership requirements.
    The production caller-owned output API also works on stable and encounters
    neither limitation, so it requires no nightly migration or solver change.

- [ ] **R6 — Optimize the Adreno path after CPU parity.** Implement the same
  packed types, transforms and full hybrid graph on a supported GPU backend.
  Follow the [Adreno G0–G5 roadmap](adreno-gpu-plan.md), reusing the initial Prism
  capability report, correctness fixtures and placement ledger. Initial Prism
  M2 bring-up can proceed before the Rust replacement; Rust GPU acceptance
  still requires its own implementation and evidence.
  Investigate persistent device state/scratch, fused kernels, fewer submissions,
  graph/command reuse and synchronization reduction. Qualify CPU/GPU placement
  separately for prefill and decode, including migration costs, shared-memory
  contention and partial fallback. Check optimized kernels against reference
  calculations before enabling them. Exit with correct whole-model Windows
  ARM64/Adreno execution and CI benefit; another board's Vulkan result or an
  untested DX12 backend is insufficient. Coordinate phase-specific placement
  and shared-memory accounting with [M5/N3–N6](hexagon-npu-plan.md); Hexagon
  kernels and state must pass their own exits before heterogeneous comparisons.
  NPU discovery does not depend on completing R6, and neither accelerator path
  bypasses R5 ownership or R7 replacement qualification.

- [ ] **R7 — Qualify replacement and retire the old path deliberately.** Run
  paired CI comparisons on the target laptop, with fixed workloads, repeated
  interleaved trials, predeclared tolerances and retained provenance/results.
  Record cold startup/repacking, prefill, first-token latency, steady decode,
  useful completed-task time, peak/resident memory, energy, thermals, failures
  and sustained behavior. Compare selected logits/recurrent states with declared
  tolerances, deterministic token streams where applicable, and creative output
  quality. Preserve the mandatory [standard LoRA TPS and quality gates](swift-lora.md#required-standard-profile-qualification)
  against the same Rust engine without LoRA, as well as Prism/Rust comparisons
  at the same adapter setting. Require no material correctness, quality,
  security or resource regression; declare per-metric performance acceptance
  before capture instead of treating the 2× goal as evidence. Publish the
  verdict and limitations, promote only the qualified profile, and retain a
  reproducible Prism fallback. Remove obsolete build/overlay/FFI paths only
  after their replacement is verified, retaining useful fixtures and notices.

R0/R1 lead to a scoped kernel experiment; the first R3 kernel can be compared
through the existing Prism reference harness before the complete R2 scalar graph
is ready. R2/R3 precede broader optimization.
R4/R5 establish the CPU replacement, R6 adds the accelerator, and R7 qualifies
each proposed profile. A failed speed experiment should narrow the next task,
not trigger numerical shortcuts or disable the working baseline. Record Rust
consistency/maintenance benefits separately if throughput remains near parity.

## Evidence and validation boundaries

Orangu's maintainers report Bonsai 2 PQ2 generation at 3.9 versus Prism's
0.79 tokens/s on a CIX P1/Mali board, about 4.9×. Its fast path uses int8
activation quantization; PTQ1 additionally repacks into PQ2 at roughly 21% more
packed bytes. Those results are implementation leads, not unchanged-arithmetic
controls or Snapdragon forecasts. [Measurements](https://mnemosyne-systems.github.io/orangu/doc/server.html#scope)
and [kernel details](https://mnemosyne-systems.github.io/orangu/doc/server-internals.html).

Camelid's published CPU gains of 1.18–2.70× concern speculative decoding and are
excluded from this work's performance budget. Audit its packed execution,
transform validation and correctness fixtures separately. [Bonsai 2 implementation and measurements](https://github.com/timtoole02/Camelid/blob/main/docs/BONSAI2.md).

Sources reviewed October 23, 2026; moving documentation must be pinned and
rechecked during R0. Neither Rust nor another project's tests establish this
engine's dependability or security. Use the [existing validation policy](validation.md):
local builds/analyzers and focused correctness checks are allowed; full suites
and every benchmark, including single-kernel or smoke runs, remain CI-only.
Target-device performance needs reviewed, manual main-only self-hosted CI;
generic hosted ARM64 runners do not provide Snapdragon/Adreno acceptance.
