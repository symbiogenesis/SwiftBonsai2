# Prism migration TODO: Swift Bonsai 2 MVP

The MVP is a safe Rust CPU engine that runs the pinned Swift Bonsai 2 model on
a 32 GB Snapdragon X Elite laptop with Windows ARM64. Prioritize only the
engine behavior needed for that path. **Everything else is deferred.**

The migration is not complete. Verified loading, packed CPU kernels, the Qwen35
hybrid graph, tokenization and transactional token generation already exist;
full-model numerical parity and complete device lifecycle qualification remain
open. The experimental native session and owned broker path are implemented.
Pinned Prism remains the
comparison and fallback engine until the replacement is qualified.

Runtime implementation follow-ups below refer to
[Prism Inference source](https://github.com/symbiogenesis/prism-inference).
Runner still provisions its immutable `0.1.1` release lock; source repairs require
a separately reviewed binary release and lock update before that provisioned
runtime includes them.

## MVP boundary

| Area | Required for this MVP |
| --- | --- |
| Model | The exact `Swift-Bonsai-2-PQ2_0.gguf` in the [assessment lock](../config/swift-assessment.lock.json), with verified length/SHA-256 and metadata |
| Architecture and weights | Dense `qwen35`; the [851-tensor inventory](../config/swift.metadata.json): 353 F32, 96 BF16 and 402 PQ2_0 tensors |
| Execution | CPU only; existing scalar reference and eligible baseline NEON paths, with explicit unsupported-operation errors |
| State and bounds | One conversation, 8,192-token context, FP16 attention K/V, F32 recurrent/convolution state, bounded model/state/scratch memory |
| Input and output | Swift's embedded chat template, exact prompt token accounting, declared special/stop tokens and incremental text output |
| Generation | Ordinary decoding with the [current inference defaults](inference-defaults.md), explicit thinking on/`xhigh`, and preserved supplied reasoning history |
| Request lifecycle | Verified readiness, one active generation, streaming, cancellation, deadlines, reset and recovery |

Use the pinned PQ2 artifact without LoRA for this first engine MVP. Current-weight
PTQ1, other models and adapter forward execution are deferred. This does not
qualify the intended LoRA-enabled standard profile or change its separate
[throughput and quality requirements](swift-lora.md#required-standard-profile-qualification).
Keep prompt-cache reuse, speculation and accelerator offload disabled.

## Ordered MVP work

- [x] **M1 — Verify the actual Swift model loads within its CPU budget.** Reuse
  the verified GGUF loader and owned tensor storage. Check the pinned artifact's
  complete tensor inventory, dimensions, metadata, tokenizer/template identity
  and Hadamard/sign declarations. Retain immutable artifact handling, malformed
  input rejection and bounded allocations. Close this with a real Swift load
  and recorded memory accounting; generic model generation/saving and support
  for other architectures or packed weight formats are deferred.

  Evidence: the actual pinned artifact and all 851 layouts loaded on the target
  Snapdragon device; the 8K graph initialized all 16 attention and 48 recurrent
  blocks. Peak process commit was 7,410,827,264 bytes, within the 12 GiB allowance.
  See the [retained load and comparison receipt](../config/swift-native-mvp.evidence.json).

- [ ] **M2 — Qualify the Swift Qwen35 forward path against pinned Prism.** Reuse
  the existing graph and PQ2 packed CPU operations, including their Q8_K
  activation contract, transforms, MRoPE, attention and GDN/convolution state.
  Cover all 16 full-attention and 48 recurrent blocks, prompt ingestion and
  successive decode steps on the actual model. Compare logits, state and greedy
  token sequences under declared numerical tolerances, then repair only the
  operators/layouts this graph requires. Check scalar and enabled NEON behavior;
  new SIMD optimizations, unrelated backend operations and backward graphs are
  deferred. Exit with retained full-model correctness evidence, not just
  synthetic kernel checks or a successful load.

  **Still unqualified:** the three supplied-token trajectories have byte-exact
  Rust scalar/NEON logits and state, but native comparisons fail the declared
  `1e-3 + 1e-3 * abs(reference)` tolerance. Maximum logit error is about 0.149;
  2,331,246 of 117,866,496 state elements fail, starting in block 1 convolution.
  Matching next-token observations do not qualify a generated greedy sequence.
  [Focused operator evidence](../config/swift-native-operators.evidence.json)
  identifies RMS multiplication grouping and other small arithmetic differences;
  their full-model causal effect remains unproven.

  Implementation follow-up: the Qwen35 graph now has a separate weighted RMS
  path matching the pinned Windows ARM64 binary's `(input * weight) * scale`
  rounding. Fresh, hash-verified native operator captures at widths
  16/64/128/5120 match that order exactly; an independent native-output bit
  regression rejects the previous order. The generic scalar RMS API retains
  its original semantics. This repairs one measured operator discrepancy;
  its effect on full-model logits/state and the remaining arithmetic differences
  still needs the unchanged M2 qualification.

  The [actual-model follow-up](../config/swift-native-mvp-followup.evidence.json)
  repeated all three supplied-token steps against a fresh native capture. The
  repaired graph still exceeds the declared logits/state tolerances, beginning
  in block 1 convolution. Observed greedy choices match; a generated greedy
  continuation remains unqualified.

- [x] **M3 — Connect Swift's chat prompt to the exact tokenizer.** Render the
  verified embedded template with the existing Jinja/chat APIs, current
  thinking/effort defaults and supplied assistant reasoning history. Feed those
  exact rendered bytes through the production Qwen35 tokenizer; compare prompt
  bytes, token IDs, BOS/EOS policy and counts with pinned Prism. Reserve the
  complete output allowance within the 8K context and reject overflow before
  execution. Reuse tokenization sessions/worker/client and incremental UTF-8
  output. The token driver does not yet render chat templates; generic template
  discovery, tool parsing and other tokenizer families are deferred.

  Evidence: `SwiftChatSession` and the separate Swift worker/client render and
  count the exact verified template. Three actual Swift cases match pinned
  native template bytes and tokenizer IDs (61/104/63 tokens), including supplied
  reasoning, Unicode and control spellings. BOS/EOS policy, the full 4K output
  reservation, overflow rejection and reuse after rejection are covered by
  [retained captures](https://github.com/symbiogenesis/prism-inference/blob/main/tests/fixtures/swift-prompt-parity.json).

- [x] **M4 — Wire the Swift sampler and generation termination.** Connect the
  production sampler to the Swift runtime with temperature 1, top-p 0.95,
  top-k 20, min-p 0, repetition penalty 1 and presence/frequency penalties 0.
  Support Swift's full 248,320-token vocabulary within bounded allocations;
  the [production candidate storage](https://github.com/symbiogenesis/prism-inference/blob/main/src/sampling/upstream.rs)
  now admits the complete vocabulary before top-k filtering, with a hard
  1,048,576-candidate bound.
  Qualify Prism RNG/draw ordering and seeded sampling against the same logits;
  the driver's independent SplitMix64 reference sampler is not that parity
  evidence. Honor the verified EOS/EOT/EOM policy and complete output cap,
  without an added reasoning cutoff. Test stop handling and sampler rollback
  after cancellation or failed delivery. Other sampler modes are deferred.

  Evidence: the production MT19937 chain matches 192 seeded native observations
  across four full-vocabulary distributions and four seeds, including native
  top-k ties. The verified native five-token EOG policy is applied; focused
  regressions cover cancellation/delivery rollback, stop draws and reset.
  [Sampler captures](https://github.com/symbiogenesis/prism-inference/blob/main/tests/fixtures/swift-sampler.json)
  are independent of the driver's SplitMix64 reference sampler.

- [ ] **M5 — Complete the minimal Swift session and broker wiring.** Reuse
  the existing runtime, token worker/client and broker boundaries for pinned
  startup identity/readiness, render/count/completion consistency and bounded
  streaming. Preserve current authentication, admission, process ownership and
  resource policies. Exercise cancellation, deadlines, abandoned output, reset
  and a subsequent successful request; failure must restore consistent KV,
  recurrent/convolution and sampler state or close admission until reset.
  Require only this one-model, one-conversation API. Generic model-session
  transport, multi-sequence scheduling, portable state files, cache reuse and
  new UI/client features are deferred.

  Implementation is connected through `SwiftSession`, bounded Swift worker/client,
  and `--native-engine` with the independently released server in an ordinary broker build. The existing 12 GiB Job,
  16 GiB startup admission, 4 GiB reserve, private credentials and listener-PID
  checks remain in force. Focused lifecycle and protocol regressions pass, and
  the actual owned path counted/prefilled 53 prompt tokens and streamed one
  sampled token. Complete device recovery evidence remains with M6.

  Recovery follow-up: dropping an in-flight private request future now signals
  its model cancellation token even when the server retains the worker. A
  focused regression covers provisional output, drop, rollback, closed
  admission, explicit reset and a successful subsequent request. Runner also
  bounds the complete canonical backend request to 1 MiB before counting;
  fixed policy fields can expand an otherwise admitted client body past that
  limit. Loopback coverage checks context rejection followed by a successful
  multi-turn request, preserving supplied reasoning and identical count and
  generation bytes. These synthetic checks do not replace M6 device evidence.

- [ ] **M6 — Demonstrate bounded end-to-end Swift operation.** On the target
  Windows ARM64 device, run the pinned model through the connected engine path:
  load, render/count, prefill, sampled text streaming, model stop/output limit,
  cancellation and restart/reset followed by another request. Include a
  multi-turn prompt with supplied history and a context-bound rejection. Retain
  the tested revision, model/runtime identities, effective settings, hardware,
  correctness comparisons and memory/admission outcome. MVP completion requires
  this Swift evidence and the relevant regression checks; completing the entire
  upstream catalog is deferred. Keep focused local checks/builds allowed, full
  suites and every benchmark CI-only. Performance optimization and benchmark
  campaigns are deferred; do not claim qualified throughput, creative quality,
  daily security or standard-profile readiness from this engine demonstration.

  **Partial:** [device session evidence](../config/swift-native-session.evidence.json)
  records verified readiness, exact counting, sampled SSE output, length stop
  and acknowledged reset. The diagnostic was deliberately stopped during its
  post-reset request when the user authorized handoff. Actual multi-turn
  execution, context rejection, cancellation and restart still need a complete
  run; M2 also remains unqualified. CI monitoring and repairs were handed to
  the user. No full local suites or benchmarks ran.

  A later owner-authorized memory diagnostic completed the actual model load
  and three forward steps starting with 13.73 GiB free. Peak process commit was
  7.02 GiB and minimum available RAM was 7.31 GiB, preserving the sampled 12 GiB
  process threshold and 4 GiB physical reserve. Approximately 13.7 GiB free is
  accepted for that bounded workload; this observation leaves the full-service
  16 GiB startup policy and complete M6 recovery requirements unchanged. See the
  [follow-up receipt](../config/swift-native-mvp-followup.evidence.json).

## Deferred until after the MVP

Do not start these to close the Swift engine MVP. Reopen an item only when a
concrete Swift failure proves it is a dependency, and narrow the work to that
dependency.

| Deferred area | Scope retained for later |
| --- | --- |
| Full Prism conformance | All 70 Windows-applicable source contracts, the complete 22,817 eval/support geometry matrices, compact handoff repairs, full reference qualification and baseline ratcheting |
| Broad performance work | All 1,530 backend performance workloads, profiling/optimization campaigns and new SIMD/ISA tuning |
| Other models and formats | Other architecture builders/tokenizer families, PTQ1/alternate model profiles, remaining IQ/other encoders, quantization tools/statistics and codec-wide transport parity |
| Generic APIs and lifecycle | Full model/context/session compatibility, generated models, GGUF saving, generic concurrency/state-file contracts and the public C ABI |
| Training and speculation | Optimizers/datasets, backward/gradient operations, DSpark, speculative decoding and advanced recurrent rollback schedules |
| Chat and grammar ecosystem | Non-Swift templates, general chat analysis/auto-parser parity, tool calls, multimodal content, grammar/schema/LLGuidance integration and full server API compatibility |
| CLI and remote acquisition | All upstream example option contexts, broad CLI parity, general HF resolution, HTTPS/download/multi-shard compatibility; the MVP uses the existing pinned artifact setup |
| Adapter and accelerator paths | LoRA forward execution and standard-profile qualification, Adreno/Vulkan, Hexagon/NPU and other offload paths |
| Application expansion | New UI, agent automation and writing/retrieval workflows beyond the minimal existing broker interface |

The [deferred migration backlog and evidence](prism-migration-backlog.md)
preserves the previous detailed contracts, focused implementation results,
historical CI failures/repairs and pending qualification. The
[conformance guide](prism-upstream-tests.md),
[71-source catalog](https://github.com/symbiogenesis/prism-inference/blob/main/tools/prism/upstream/catalog.json) and
[migration inventory](https://github.com/symbiogenesis/prism-inference/blob/main/tools/prism/upstream/migration.json) remain the record
of broader migration scope. Deferred does not mean passed, removed or complete.

This is a work-priority change. Keep existing required green source identities,
immutable fixtures, upstream assertions/tolerances, explicit unsupported results
and required production bridges. It does not change CI workflows, ratchet
`baseline.json` or report a new test verdict. Track implementation and measured
qualification separately, and promote only the scope actually demonstrated.
