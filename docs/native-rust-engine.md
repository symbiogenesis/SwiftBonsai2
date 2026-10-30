# Native Rust Prism engine contract

[Prism Inference](https://github.com/symbiogenesis/prism-inference) owns the safe Rust
CPU runtime, tokenizer, packed kernels and hybrid graph. Runner owns the app, broker,
provisioning and [release integration](../config/prism-inference.lock.json). The pinned
publisher engine remains the comparison/fallback; Rust promotion requires the complete
scale-1.3 LoRA profile under the [promotion contract](#default-runtime-promotion).
[PLAN.md](../PLAN.md#runtime-and-correctness) owns outstanding work. The [native Rust
UI](ui-architecture.md) is a separate broker client: UI availability or browser/server
feature parity does not qualify inference.

## Swift CPU supported subset

The target is native Windows ARM64, initially the 32 GB Snapdragon X Elite. Runner
pins [**0.1.6**](https://github.com/symbiogenesis/prism-inference/releases/tag/v0.1.6),
main revision `02c8a24b31ac1c35f2ffd3530f6a753ce440bdc0`, with passing
[Foundation](https://github.com/symbiogenesis/prism-inference/actions/runs/40449357453)
and
[conformance](https://github.com/symbiogenesis/prism-inference/actions/runs/40449357329).
Release/focused integration does not establish full-context, sustained-performance,
creative, adapter or daily-isolation acceptance.

| Implementation | Evidence boundary |
| --- | --- |
| PTQ1 Q8_0 scale/inverse | Automatic ARM64 three-token logits/live state match native bytes; scalar/explicit NEON and broader qualification remain pending in the [PTQ1 contract](swift-memory.md#current-weight-ptq1-option). Generic/PQ2 preparation is preserved. |
| Reused sampler candidates; 32-channel attention-value tiles with at most 2 KiB accumulator payload | Transaction clones exclude scratch; frozen sampler/graph and bitwise attention fixtures preserve arithmetic/recovery. No measured TPS or model-peak-RAM gain. |
| Atomic checkpoint restore | Stages fallible KV copies, then restores recurrent/convolution bits into live storage, preserving independent checkpoints and failure atomicity. Avoids 156,893,184 bytes (149.625 MiB) transient Swift payload. |
| Aligned F32/F16 projections | Native-backed operator evidence only; generic, BF16 and unqualified tails are unchanged. No broader PTQ1 or reduced-admission claim. |

| Area | Contract |
| --- | --- |
| Model | Exact `Swift-Bonsai-2-PQ2_0.gguf` from the [assessment lock](../config/swift-assessment.lock.json), verified by length, SHA-256 and metadata |
| Weights | Dense `qwen35`; [851 tensors](../config/swift.metadata.json): 353 F32, 96 BF16 and 402 PQ2_0 |
| Execution | CPU only; scalar reference and eligible baseline NEON, with explicit unsupported-operation errors |
| State | One 8,192-token conversation; 16 full-attention and 48 recurrent blocks; FP16 attention K/V, F32 recurrent/convolution state and bounded allocation |
| Input/output | Verified embedded template, exact prompt tokens, supplied reasoning history, declared stops and incremental text |
| Generation | [Inference defaults](inference-defaults.md), optional thinking with `xhigh` effort, production MT19937 draw ordering and native sampler tie behavior |
| Lifecycle | Verified readiness, identical canonical count/generation requests, one active generation, authentication, streaming, cancellation, deadlines, reset and recovery |
| Admission | 15.5 GiB available physical memory, 11.5 GiB Job commit limit and 4 GiB running physical reserve; owner-selected development budget, with full-context resource qualification pending; diagnostic allowances are not launch profiles |

The service supports **PQ2 without LoRA, cache reuse, speculation or offload**.
PTQ1, other families and adapters need separate qualification, including [LoRA
TPS/quality](swift-lora.md#standard-profile-qualification). [Hardware discovery](hardware-capabilities.md)
reports inventory; runtime kernel eligibility/`Auto` does not implement Runner's planned
qualified-device selector.

Bounded Swift numerical comparisons use the unchanged `1e-3 + 1e-3 * abs(reference)`
tolerance for logits, complete logical recurrent/convolution state and live FP16 KV rows
against pinned native execution. Exact chat/tokenizer and sampler references belong to
the source project's [prompt
fixtures](https://github.com/symbiogenesis/prism-inference/blob/main/tests/fixtures/swift-prompt-parity.json)
and [sampler
fixtures](https://github.com/symbiogenesis/prism-inference/blob/main/tests/fixtures/swift-sampler.json).
Supplied-history streaming, output-limit termination, seeded replay, context
rejection/recovery, abandonment, shutdown and restart checks are bounded evidence; full
8K-context/4K-output and daily acceptance remain pending.

## Numerical and replacement boundaries

Preserve model/adapter identities, decoded scales, activation/cache precision,
transforms, attention topology, sampler and ordinary decoding. PQ2, verified
current-weight PTQ1 and Qwen are separate controls; lossless weights do not imply
identical activation arithmetic. Generic PQ2's fourth symbol remains valid despite
Swift's ternary census.

Lossless layout changes, SIMD, tiling, buffer reuse, fusion and scheduling use the same
arithmetic contract. Reassociation and altered reductions/FMA need declared numerical
tolerances. Precision reduction, approximate attention, pruning, retraining and
speculation are separate numerical experiments. The [algorithm
reference](inference-algorithms.md) defines packed-lookup, attention-rounding, chunkwise
recurrence and state-recovery constraints.

A CPU replacement owns Rust model/LoRA storage, has no C/C++ inference-library dependency
and confines device/OS calls to audited safe boundaries; ggml pointer bindings do not
satisfy it. Source reuse needs immutable revisions, licenses/notices, hardware assumptions
and correctness evidence.

The performance objective is roughly 2× end-to-end throughput; 3–5× depends on major
baseline inefficiencies and is a stretch hypothesis. Prefill, decode and completed-task
time are separate measurements, with cold load/repacking, allocation/copying,
synchronization, memory traffic and the measured critical path included. [Orangu's
published results](https://mnemosyne-systems.github.io/orangu/doc/server.html#scope) on
CIX P1/Mali use [different activation/repacking
choices](https://mnemosyne-systems.github.io/orangu/doc/server-internals.html);
[Camelid's results](https://github.com/timtoole02/Camelid/blob/main/docs/BONSAI2.md)
include speculation. Neither establishes unchanged-arithmetic Snapdragon gains.

## Token and worker API semantics

These generic token-driver APIs are separate from the pinned Swift chat session and
native sampler:

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

Generation, graph work and caller-owned cloneable sampling state participate in one
transaction. Admission, sampling, decoder, delivery, cancellation and deadline failures
restore state, including failed graph recovery. Successful stop draws advance sampling
state without entering conversation history. Absolute deadlines bracket synchronous
decoder and delivery work; an in-flight operation must return before expiry is detected.

Reusable logits and graph scratch never alias independently owned checkpoints.
State-only prompt ingestion omits unused output/logit work while validating every
computed state value; final uncached tokens and decode steps produce logits, and exact
prefix hits restore saved output. Explicit 1–256-token blocks retain sequential
arithmetic and whole-request rollback, with cancellation and deadline checks between
tokens. Sequential blocks do not imply tiled matrix-matrix execution or measured prefill
benefit.

Matvec activation preparation is call-scoped and shared only for matching input,
basis/permutation, width and quantizer; mixed Q8_K/Q8_0 or transformed/plain inputs stay
separate. Full-attention state stores exact binary16 bits and widens them before scalar
products; convolution/GDN stays FP32.
Snapshot budgets conservatively charge every referenced sealed 64-token KV page, page
table, mutable-tail capacity and recurrent state per snapshot, without depending on
sharing for admission.

## Acceptance ownership

The [conformance contract](prism-upstream-tests.md) preserves original source inputs,
assertions, tolerances, unsupported results and production bridges. Full source
conformance, generic APIs and accelerator coverage exceed the bounded Swift subset.
The [lock-update procedure](development-guide.md#prepare-a-runtime-lock-update)
separately reviews source, immutable released bytes and Runner integration.

[Validation policy](validation.md) permits local builds, analyzers, assembly inspection
and focused correctness checks; full suites and benchmarks are CI-only. The [promotion
contract](#default-runtime-promotion) requires reviewed main-only target-device evidence,
native fallback, independent standard-LoRA TPS/quality and no material correctness,
quality, security or resource regression. Hosted ARM64 results cannot establish
Snapdragon performance. Backend changes also require complete-state phase handoff and
compatible state/numerical identity.

## 2026 literature: runtime overhead and work placement

Sources reviewed 2026-10-29 and their platform limits live in the
[systems catalog](research-register.md#2026-systems-measurement-and-output-budget-work);
none establishes a defect or new profile in the pinned runtime.

| Register source | Project implication |
| --- | --- |
| S1 ProfInfer | Correlate timelines/counters before optimizing; qualify Windows instrumentation overhead separately from eBPF. |
| S5 rvllm | Audit cache replacement, broadcasts and contiguous operations for repeated copies. Shared immutable storage and independent mutable snapshots need distinct ownership. |
| S7 SMG | Attribute tokenization, parsing and backpressure separately from accelerator service; its broader gateway API is unnecessary here. |
| S4 Elastic Gang | Dynamic participants need acknowledged barrier/epoch transitions; compare existing phase/thread controls before adding a scheduler. |
| S6 Event-triggered SGLang | Measure idle polling, wake latency and active decode separately; affinity only relocates polling. |
| S3 Beyond the Buzz | Charge state handoff, rate matching and coordination before selecting CPU/GPU/NPU phase placement. |

Measure work/traffic and allocation lifetimes before changing representation, ownership,
scheduling or fusion: preallocation can hide full-size copies, and in-place writes can
alias snapshots.
[Streaming contracts](streaming-resources.md) cover Runner queues/text/PCM;
runtime experiments belong in Prism. Follow the
[measurement guide](performance-expectations.md#2026-literature-measurement-and-comparison)
for CI-only timing/tracing, actual-model comparisons and evidence limits.

## Default-runtime promotion

`Get-RuntimePromotion.ps1` reports the reviewed requirements for replacing the current
Swift PQ2 standard profile with Rust Prism while retaining its pinned LoRA at scale 1.3.
It reads the compiled release/model/adapter/template locks and requires an explicit full
Runner revision. PTQ1 is evaluated separately.

### Inspect the policy

```powershell
$revision = git rev-parse main
$plan = ./scripts/Get-RuntimePromotion.ps1 -RunnerRevision $revision
$plan.policy.requiredGates
./scripts/Get-RuntimePromotion.ps1 -RunnerRevision $revision -EvidencePath D:\Evidence\promotion.json
```

The evaluator checks offline completeness and byte integrity, without inference,
benchmarks, selection changes, reviewer authentication or network provenance checks.
Independently review CI/source/report/reviewer identities and require `promotionReady:
true`; command success alone cannot authorize promotion.

### Required evidence

Required gates cover release provenance, runtime Foundation/conformance, Runner
Foundation, 8K logits/full state, prompts/sampling/stops, 4K-output lifecycle/recovery,
complete-model resources, adapter basis/logits, standard-scale TPS/human quality,
actual-model sandbox/transport, speech/offline recovery, sustained device/creative
acceptance, installation and rollback. Released private-service adapter readiness and
per-request scale/state lifecycle qualify independently of source-graph support or
adapter-disabled smoke checks.

Device gates require self-hosted ARM64 Snapdragon X Elite, X2 Elite or X2 Elite
Extreme evidence, with the same family across all completed device gates.
`processorName` must match that family; only legacy X Elite receipts may omit it.
Paired promotion requires predeclared **10% end-to-end latency benefit**, or reviewed
capacity/energy benefit within accepted limits; same-engine LoRA TPS is independent.
Full suites/benchmarks stay CI-only; focused local correctness checks are allowed.

### Manifest format

The 1 MiB UTF-8 JSON evidence manifest must live outside Git alongside its retained
reports. Copy `policySha256` and the entire `policy.identities` object from the plan.
`gates` may omit unfinished work; every omitted gate stays pending. A complete pending
skeleton is also returned as `evidenceTemplate` by the plan. A supplied record has this
shape:

```json
{
  "schemaVersion": 1,
  "policySha256": "<exact plan policySha256>",
  "identities": { "<copy every policy.identities member>": "<unchanged>" },
  "gates": [
    {
      "id": "runner-foundation",
      "status": "passed",
      "note": "Reviewed required jobs, exact revision and retained final verdict.",
      "report": { "file": "runner/foundation.json", "sha256": "<report SHA-256>" },
      "execution": {
        "repository": "symbiogenesis/SwiftBonsai2Runner",
        "revision": "<exact selected Runner main revision>",
        "gitRef": "refs/heads/main",
        "runId": "123456789",
        "runAttempt": 1,
        "architecture": "ARM64",
        "deviceModel": "Snapdragon X Elite",
        "selfHosted": true
      },
      "reviewerId": "<actual independent reviewer>",
      "humanReviewed": false
    }
  ]
}
```

Each completed (`passed` or `rejected`) gate requires a nonempty report, bounded to 16
MiB, with an exact lowercase SHA-256 and a relative path without traversal. Reports must
remain outside Git, namespace paths and reparse points. Runtime gates bind the selected
Prism source revision; application gates bind the supplied settled Runner revision. Each
named CI execution must use `refs/heads/main`. Passed gates require an identified
reviewer; human quality and sustained comfort also require `humanReviewed: true`.
Pending records need only `id`, `status` and a bounded nonempty `note`.

### Evaluation rules

Unknown or duplicate gates, malformed identities, non-main provenance, wrong devices,
missing reports and digest mismatches fail the command. Any known rejected gate makes
the aggregate rejected even when other work remains pending. Only every required gate
passing produces `promotionReady: true`.

Evidence collection/review follows [validation](validation.md), the supported runtime contract above and [PLAN.md](../PLAN.md).
