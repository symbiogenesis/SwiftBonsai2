# Prism migration TODO: Swift Bonsai 2 MVP

The MVP is a safe Rust CPU engine for the pinned Swift Bonsai 2 model on a
32 GB Snapdragon X Elite laptop with Windows ARM64. Prioritize the engine
behavior required for that path. Broader runtime work is deferred.

Runtime implementation belongs to
[Prism Inference](https://github.com/symbiogenesis/prism-inference). The bounded
source CPU MVP is implemented and qualified for its limited workloads. Runner
provisions the reviewed `0.1.3` prebuilt release containing those repairs through
its [immutable lock](../config/prism-inference.lock.json). Future promotions use
the [lock-update procedure](development-guide.md#prepare-a-runtime-lock-update).
Source qualification and release identity checks do not qualify full-context
generation, performance, creative quality or daily-service isolation.

The [native Rust chat UI](ui-architecture.md) separately consumes the broker's
HTTP/JSON/SSE contract. Its availability does not expand runtime qualification.
The later [Snapdragon X1/X2 hardware goal](hardware-profiles.md) requires qualified
per-device CPU/GPU/NPU plans; current launch policies remain CPU-only.

## MVP boundary

| Area | Required behavior |
| --- | --- |
| Model | Exact `Swift-Bonsai-2-PQ2_0.gguf` from the [assessment lock](../config/swift-assessment.lock.json), with verified length, SHA-256 and metadata |
| Weights | Dense `qwen35`; the [851-tensor inventory](../config/swift.metadata.json): 353 F32, 96 BF16 and 402 PQ2_0 tensors |
| Execution | CPU only; scalar reference and eligible baseline NEON paths, with explicit unsupported-operation errors |
| State | One 8,192-token conversation; FP16 attention K/V, F32 recurrent/convolution state and bounded allocations |
| Input/output | Verified embedded template, exact prompt accounting, declared stop tokens and incremental text output |
| Generation | [Current inference defaults](inference-defaults.md), optional thinking with `xhigh` effort, preserved supplied reasoning history |
| Lifecycle | Verified readiness, one active generation, authentication, streaming, cancellation, deadlines, reset and recovery |

Use the pinned PQ2 artifact without LoRA for this engine MVP. Current-weight
PTQ1, other models and adapter execution are separate work. This does not qualify
the LoRA-enabled standard profile or replace its
[throughput and quality requirements](swift-lora.md#required-standard-profile-qualification).
Keep prompt-cache reuse, speculation and accelerator offload disabled.

## Implemented source scope

| Milestone | Bounded scope |
| --- | --- |
| M1 — Model loading | Verified Swift artifact, all tensor layouts and the 8K graph's 16 attention and 48 recurrent blocks, within the CPU memory budget |
| M2 — Forward parity | Scalar and baseline NEON logits, complete logical recurrent/convolution state and live FP16 KV rows against pinned native execution; unchanged `1e-3 + 1e-3 * abs(reference)` tolerance |
| M3 — Prompt/tokenizer | Exact template bytes, token IDs, BOS/EOS policy, Unicode and supplied reasoning; complete output reservation and overflow rejection |
| M4 — Sampling/termination | Production MT19937 draw ordering, seeded sampling, native tie handling, declared stop policy and cancellation/delivery rollback |
| M5 — Session/broker | Owned Swift worker/session, identical canonical requests for counting and generation, 1 MiB request bound, deadlines, reset and recovery |
| M6 — End-to-end operation | Actual-model supplied-history streaming, output-limit termination, seeded replay, context rejection/recovery, abandonment, owned shutdown and fresh restart |

The source project owns the active
[prompt parity fixtures](https://github.com/symbiogenesis/prism-inference/blob/main/tests/fixtures/swift-prompt-parity.json)
and [sampler fixtures](https://github.com/symbiogenesis/prism-inference/blob/main/tests/fixtures/swift-sampler.json).
These bounded checks do not establish full 8K-context/4K-output behavior or
performance. Generic graph arithmetic and unrelated source contracts retain
their existing requirements.

## Remaining Runner delivery

The `0.1.3` receipt identifies source revision
`f3b0a1bf28d81d7e420161d11100e635d9d4cf5e` and successful
[Foundation run 40278541900](https://github.com/symbiogenesis/prism-inference/actions/runs/40278541900).
The offline release review reproduced the checked-in lock from the released
bundle, receipt and exact source archive. Runner's verified model-free probe
also accepts that release's capability report. This is release identity and
integration evidence. The separate
[full runtime-conformance run 40278541862](https://github.com/symbiogenesis/prism-inference/actions/runs/40278541862)
failed a deferred DSpark session/lifetime timeout; complete conformance remains
unqualified.

- Keep reviewing the exact source revision, required validation and immutable
  ARM64 bundle for every future release-lock candidate. Release `0.1.3` is pinned;
  retain Runner's released-runtime integration checks on each settled revision.
- Preserve the production 16 GiB available-memory admission, 12 GiB Job commit
  limit and 4 GiB physical reserve. Temporary diagnostic settings are not
  supported launch profiles.
- Qualify full-context/long-output behavior, throughput, creative quality,
  LoRA and daily-service isolation separately before claiming those outcomes.

## Deferred runtime work

Reopen a deferred area for this MVP only when a concrete Swift failure proves
it is a dependency, and narrow the change to that dependency.

| Area | Deferred scope |
| --- | --- |
| Full Prism conformance | Complete source contracts, eval/support geometry matrices, full reference qualification and baseline ratcheting |
| Performance | Backend timing workloads, profiling, optimization campaigns and new SIMD/ISA tuning |
| Models/formats | Other architectures and tokenizer families, alternate packed formats, quantizers, codec statistics and transport parity |
| Generic APIs | Generated models, GGUF saving, concurrency, portable state files and the public C ABI |
| Training/speculation | Optimizers, datasets, backward graphs, DSpark and speculative decoding |
| Chat/grammar | Non-Swift templates, general parsers, tool calls, multimodal content, LLGuidance and broader server compatibility |
| Acquisition/CLI | Broad option parity, general HF resolution, HTTPS/download and multi-shard behavior |
| Adapters/accelerators | LoRA qualification, Adreno/Vulkan, Hexagon/NPU and automatic X1/X2 hardware profiles |
| Application expansion | Agent automation, writing/retrieval workflows and UI features beyond the native chat demo |

See the [deferred contracts](prism-migration-backlog.md) and
[upstream validation ownership](prism-upstream-tests.md). Preserve required green
source identities, immutable fixtures, original assertions/tolerances, explicit
unsupported results and required production bridges. Runtime CI belongs to Prism
Inference; Runner owns application and released-runtime integration checks.
Full suites and all benchmarks remain CI-only.
