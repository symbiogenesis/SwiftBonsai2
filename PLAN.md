# Swift Bonsai 2 implementation plan

Build a local creative assistant for a 32 GB Snapdragon X Elite laptop running
Windows ARM64. Prioritize writing, revision, brainstorming and art direction,
with short waits, bounded memory and comfortable sustained operation.

## 1. Current direction

Swift Bonsai 2 `PQ2_0` is the current focus. Its pinned Prism CPU runtime passes
a bounded 16-token decode check. An exact current-weight PTQ1 export also
decodes, with and without the approved pretrained adapter. Creative quality,
sustained performance and daily security remain unqualified.

The earlier literature choice, Qwen3.8-27B `UD-Q3_K_XL`, remains the default
reference lock; select Swift explicitly. Gemma 4 12B IT QAT is a
literature-nominated fallback. See [model selection](docs/model-selection.md)
and the [Swift assessment](docs/swift-memory.md). Model selection uses published
evidence; subsequent device measurements qualify the chosen engine and profiles.

Use the pinned native engine as the control while developing the
[Rust replacement](docs/native-rust-engine.md). Establish correct CPU behavior
before promoting optimized paths. Adreno is the first GPU candidate; Hexagon
access and compatibility discovery can proceed alongside it. Neither
accelerator is enabled in current launch profiles.

The approved abliteration LoRA is intended for the standard profile. Acceptance
requires **decode tokens/second and output quality each to match or exceed the
same engine without LoRA**, using predeclared tolerances. Lower token use or
fewer refusals cannot excuse either regression. A bounded RAM premium is
acceptable. Capture/scoring tools exist; device results, adapter-basis evidence
and human reviews remain pending. See [required qualification](docs/swift-lora.md#required-standard-profile-qualification).

## 2. Hardware envelope

| Component | Target / recorded inventory | Implication |
| --- | --- | --- |
| OS | Native Windows ARM64 | Host binaries, libraries and tooling must match |
| CPU | Snapdragon X Elite X1E-78-100 | Verify actual ISA support and kernel dispatch |
| GPU | Adreno X1-85 | Query shared-memory budgets; dedicated-memory reports do not describe model capacity |
| NPU | Hexagon | Signed Windows access and exact graph support are pending |
| RAM | 32 GB installed; 33,897,037,824 usable bytes recorded | Admit against currently available RAM, commit and device budgets |

Run `Doctor.ps1` for current inventory. Recorded driver/OS details are evidence
for that capture, not a continuing compatibility guarantee. Use GiB/MiB for
allocation budgets and bytes for artifact identities.

## 3. Model and runtime contracts

Immutable model/runtime pins, hashes and metadata live in `config/`. Keep Swift,
the Qwen reference, the source-overlay build and derived PTQ1 as distinct
identities. Never mix fork binaries with unrelated ggml libraries.

Preserve tokenizer, embedded template, special tokens, transforms, activation
quantization and sampler when comparing implementations. Lossless weight
conversion does not establish equal logits when activation arithmetic differs.
The publisher's older PTQ1 model also has different weights from current PQ2;
it is distinct from this project's current-weight export.

Verify length/SHA-256, tensor inventory, architecture and template identity
before execution. Record runtime commit, build options and effective settings
with every qualification result. Publisher hashes identify approved bytes;
they do not establish parser isolation or source-to-binary provenance.

Swift's reference sampler is temperature 1, top-p 0.95, top-k 20, min-p 0,
repetition penalty 1 and presence/frequency penalties 0. CPU checks and the
development server request thinking on and `xhigh` explicitly, preserving the
verified templates' defaults. Keep FP16 K/V, native RoPE and exact token
reservation. No extra system persona or mandatory revision loop is injected.
A thinking-off or shorter-output creative profile needs separate quality
qualification. See [inference defaults and evidence](docs/inference-defaults.md)
and [development commands](docs/development-guide.md).

## 4. Backend delivery

| Path | Current state | Next gate |
| --- | --- | --- |
| Pinned Prism CPU | Bounded Swift decode works | Real service behavior, creative/comfort and daily security qualification |
| Native Rust CPU | Verified loader, hybrid graph, transactional token generation, scalar and baseline NEON packed kernels | Full-model parity, tokenizer/template/sampler, adapter forward execution and broker integration |
| Adreno Vulkan | CPU-only builds and profiles today | Native ARM64 build, exact-model placement/parity and paired device evidence |
| Hexagon | Discovery and implementation plan only | Signed access, exact packed kernels, hybrid state and bounded deployment |

The [SIMD S0–S6 plan](docs/simd-plan.md), [Adreno G0–G5 plan](docs/adreno-gpu-plan.md)
and [Hexagon N0–N7 plan](docs/hexagon-npu-plan.md) own their detailed exits.
Source support or device detection does not qualify this laptop. OpenCL and
conversion-based ONNX/QNN routes require separate exact-format/graph audits.
Keep a reproducible CPU fallback and report unsupported placements explicitly.

## 5. Service, UI and isolation

The Rust broker implements bounded HTTP admission, exact-count orchestration,
single-slot scheduling, streaming supervision and an explicit Windows
development launcher for the Qwen reference or Swift PQ2 assessment. Each
selection uses its own compiled pins, batch sizes and memory admission. The
launcher verifies artifacts, protects credentials and owns the backend through
a Windows Job. Real model
execution through that service and same-user backend bypass prevention remain
unqualified. See the [broker contract](docs/broker-policy.md).

Daily chat and its UI are not enabled. The first user flow is authenticated,
offline creative chat, using the pinned server's UI where it can satisfy the
complete policy. Qualify:

- Numeric-loopback binding, private credentials, exact Host/Origin and required
  routes; reject tools/MCP, dynamic loading and inherited capability overrides.
- Bounded input/output bytes, exact prompt/output tokens, queue, concurrency and
  total duration; abandoned generation must stop reliably.
- Safe rendering, no automatic remote resources, visible finish reasons and
  explicit context/output limits.
- Restricted inference identity, read-only approved artifacts, private bounded
  cache/temp storage and denied unrelated files, credentials and egress.
- Offline readiness/restart, cancellation, owned-process cleanup and recovery
  without restart loops or automatic cloud fallback.

A Job provides lifecycle and commit limits, not a filesystem, identity or
network boundary. Same-user ACLs and loopback alone do not prevent bypass.
Keep prompts, artwork, credentials, histories and state outside Git; routine
logs contain technical diagnostics rather than prompt bodies.

Agent automation is optional after creative chat works. OpenCode is the first
client candidate; Aider is the fallback. MXC ProcessContainer is the proposed
native containment route, pending exact-policy enforcement tests. Contain the
whole agent, plugins, hooks and descendants, with a bounded inference bridge.
Require `allowDaclMutation:false`, no audit mode for untrusted execution and no
unsandboxed fallback. Test filesystem/reparse, handle, subprocess and network
bypasses; permission prompts are not containment. A separate-kernel workload
needs a separately qualified VM tier. The reviewed LiteBox Windows userland
path and NVX do not establish a native ARM64 substitute.

## 6. Hybrid state and writing context

Swift's main graph has 16 full-attention and 48 recurrent blocks. Budget live
KV, recurrent/convolution state, checkpoints and prompt-cache storage
separately. At 8K tokens, one sequence and FP16 K/V, live KV is 0.5 GiB;
recurrent/convolution state is approximately 0.146 GiB before extra snapshots
and allocation overhead. Derive each profile's estimate from its actual GGUF.

Current CPU checks disable reuse and speculation. Future reuse must restore
matching KV **and** recurrent state, retain exact token/model/template identity,
bound snapshots and eviction, and survive edited turns, cancellation and
conversation switches. Saved state is private conversation data. Q8/Q4 KV,
Flash Attention and altered recurrent precision need separate numerical and
quality gates; smaller storage alone is insufficient.

For writing, start with scene packets: relevant outline, author-approved facts,
voice/viewpoint, current scene and selected excerpts. Keep stable instructions
first. Count the rendered prompt and reserve the complete output allowance;
an 8K context with 4K output leaves less than 4K for history and sources.
Reject overflow or offer an explicit validated context/summary choice; never
silently discard canon or shift recurrent state.

Original manuscripts and the author-edited project bible remain authoritative.
Generated summaries need source references, versions and author acceptance.
Start with manual excerpts or lexical search; add embeddings only for a
demonstrated retrieval need. Image critique needs a matching qualified vision
path; image generation needs a separate model and shared-memory budget.

## 7. Memory and storage budgets

| Development CPU profile | Weight file | Service allowance | Available physical RAM required |
| --- | ---: | ---: | ---: |
| Swift `PQ2_0` assessment | 6.71 GiB | 12 GiB | 16 GiB |
| Qwen reference | 12.24 GiB | 18 GiB | 22 GiB |

Both use one 8K sequence, FP16 K/V and F32 recurrent/convolution state. Startup
requires the full service allowance plus a 4 GiB physical reserve. These are
configured budgets, not
measured peaks or installed-RAM recommendations. Current-weight PTQ1 saves
1.173 GiB of serialized payload without reducing the Swift admission policy.

Track process private bytes/working set, Job commit, system commit, available
RAM and device allocations. Mapped weights are not fully bounded by the Job's
commit cap; shared-memory accounting can both overlap and hide copies. Admit
scratch, repacking, checkpoints, vision and optional tool workers together.
Reduce context/cache/batch or unload inference when headroom is insufficient.
Do not force memory locking or alter the pagefile to manufacture a result.

Store artifacts in an owner-selected absolute directory outside Git, with
immutable packs and versioned builds. Keep diagnostics in unique system-temp
lanes with an explicit absolute working directory and valid inherited Windows
environment. Verify ownership and resolved paths before cleanup.

## 8. Profiles to qualify

The Qwen reference and explicit Swift assessment are development CPU paths,
not accepted daily profiles. Keep their fixed settings reproducible.

| Proposed profile | Starting point | Qualification focus |
| --- | --- | --- |
| Creative | One 8K FP16 sequence; extend to 16K only as needed | Useful prose, explicit output budget, short waits and sustained comfort |
| Daily | Best accepted CPU/GPU placement; bounded reuse | Security, cancellation, responsiveness and memory |
| Long context | Explicit larger context and separately qualified KV/cache | Continuity/retrieval quality and peak memory |
| Battery | Phase-specific threads and low-polling candidates | Energy per useful task and acceptable latency |
| NPU experimental | Explicit profile after kernel/state correctness | Signed deployment, device memory, placement and recovery |

For initial reuse experiments, two checkpoints and 256 MiB prompt-cache storage
are proposals, not current defaults. Larger caches need observed total-memory
evidence. Tune prefill and decode separately; more threads, offload or processors
can increase contention and heat. Report effective settings and fallback reasons.

## 9. Speculative decoding and advanced optimization

Follow [R0–R7](docs/native-rust-engine.md) and the
[algorithm research map](docs/inference-algorithms.md). Reusable logits/scratch,
compatible sibling activation preparation, state-only intermediate prompt
ingestion, physical FP16 KV and baseline packed NEON are implemented in the
experimental Rust reference. Bounded sequential prefill scheduling, immutable KV
checkpoint pages and cumulative backend diagnostics are also implemented.
The token driver honors declared model stops, emits lossless token bytes with
incremental UTF-8 text, and offers explicit seeded reference sampling with
transactional sampler recovery. Bounded plain-text input composes pinned Qwen35
Unicode boundaries, explicit special-token recognition and ranked byte-pair
merges. Explicit opt-in BOS/EOS insertion honors declared flags within the prompt
allowance; raw text and token IDs keep their existing defaults. A verified
tokenization-only command returns IDs/count without weight retention or graph
allocation. Chat templates, full-model tokenizer parity, persistent broker count
integration and Prism sampler/RNG parity remain open.
Verified tokenizer sessions now reuse exact prompt/context/output accounting;
the public incremental UTF-8 decoder and bounded prompt-file/stdin input are
available for service integration. A persistent bounded JSONL worker exposes
verified count/encode requests with fixed policy and explicit readiness. Library
callers share the driver's checked model/caller stops and bounded UTF-8 prompt
reader. Their focused correctness checks do not establish chat-template parity,
broker integration or daily-service acceptance.
Tiled prefill kernels, full-model allocation,
parity and performance qualification remain open.

Profile before expanding optimization work. Priorities include exact packed
lookup kernels, compatible activation sharing, bounded block prefill and hybrid
checkpoint sharing. Preserve quantizers, transforms, rounding and rollback;
declare tolerances before changing reductions. NoGraphicsAPI is not a planned
replacement for ggml Vulkan. Speculation is a separate experiment: start with
model-free prompt lookup, prove complete hybrid-state rollback, then require
end-to-end benefit. Approximation, retraining and new draft models need separate
scope and quality evidence.

## 10. Validation and performance evidence

Local builds, analyzer checks and focused correctness tests are allowed. Full
suites and **all** benchmarks, including smoke/dry/debug runs, run only in CI.
Do not approximate a full suite with local filtered batches. See
[validation](docs/validation.md) for commands and workflow coverage.

Hosted Foundation checks build/tooling and synthetic correctness; it does not
establish Snapdragon latency, GPU/NPU behavior, memory acceptance or comfort.
Target-device benchmarks require a reviewed, manually triggered self-hosted
workflow. Such a runner is not configured here. Use reviewed `main`, pinned
dependencies, minimal credentials, synthetic/approved fixtures and separate
private evidence lanes; never run untrusted pull-request code on the laptop.

Use a small paired workload set rather than a full Cartesian sweep: drafting,
constrained revision, continuity and art direction; fresh/warm/edited prefixes;
short and sustained operation; fixed-length outputs and natural completions.
Record artifact/build/profile identities, CI revision/run, prefill, first answer,
decode TPS, useful task time, retries/truncation, peak memory and available
power/thermal telemetry. Interleave repeated trials and report uncertainty.
Keep private manuscripts and artwork out of shared evidence.

### Promotion criteria

Promote each profile only when:

1. Required correctness/security suites pass, including state restoration,
   streaming, cancellation and the daily inference boundary. Enabled agents
   also pass whole-agent containment checks.
2. Memory stays within budget without sustained paging/eviction, and Windows
   remains responsive through sustained work with owner-accepted heat/fan behavior.
3. Creative quality meets a declared voice/continuity/revision rubric with no
   unresolved critical failures. Standard LoRA separately passes both same-engine
   TPS and quality parity against no LoRA.
4. Paired device CI establishes a useful optimization benefit: initially target
   10% lower end-to-end latency, or a documented capacity/energy benefit within
   accepted latency and quality limits. Record any creative quality/speed tradeoff.
5. The settled `main` revision reproduces the result and retains a rollback
   manifest/profile with evidence and clear limitations.

## 11. Repository responsibilities

| Location | Responsibility |
| --- | --- |
| `config/` | Immutable artifacts, metadata, explicit profiles and qualification inputs |
| `src/broker/`, `src/windows-*/` | Request policy, service, credentials and process ownership |
| `src/prism-runtime/` | Experimental safe Rust CPU engine |
| `tools/` | GGUF/codec, Prism overlay, maintenance and compiler-trial components |
| `scripts/` | Provisioning, builds, planning and development checks |
| `tests/`, `.github/workflows/` | Focused regressions and hosted/full-suite handoff |
| `docs/` | Current contracts, evidence limits and remaining work |

Keep third-party source pinned and external; retain licenses and fixtures.
Use dedicated worktrees for delegated work, integrate into local `main`, and
remove eligible session worktrees/branches after verification. Remote publication,
when authorized, uses only `main`. Git history holds implementation chronology.

## 12. Delivery milestones and stop/go decisions

| Milestone | Current state | Remaining exit |
| --- | --- | --- |
| M0a — literature selection | Qwen decision recorded; Swift is current memory priority | Reopen only on documented support, quality or resource failure |
| M0 — reproducibility | Inventory, locks, provisioning and GGUF inspection implemented | Keep identities and capability evidence current |
| M1 — CPU creative chat | Swift bounded decode and development broker/launcher implemented separately | Real service behavior, offline UI, bypass prevention and restricted daily process |
| M1a — creative/comfort | Pending | Useful creative output, standard LoRA qualification, latency and sustained device acceptance |
| M1b — optional agents | Pending | Qualified client and whole-agent containment before tools are enabled |
| M2 — Adreno | G0–G5 pending | Native exact-model correctness and useful device benefit, or documented no-go |
| M3 — cache/context | Bounded experimental Rust state exists | Full-model reuse, context, allocation and quality qualification |
| M4 — daily profiles | Pending | Reproducible installation and accepted creative/daily/battery profiles |
| M5 — Hexagon | N0–N7 pending | Signed exact-model execution, bounded state/memory and device benefit, or no-go |
| M6 — advanced kernels/speculation | Research candidates | Correctness and measured benefit justify maintenance |
| M7 — Rust replacement | Loader, hybrid graph, token runtime and selected optimizations implemented | R0–R7 parity, complete service integration and per-profile replacement acceptance |

The immediate delivery path is M1 → M1a → M2/M3 → M4. Develop M7 against the
pinned control; begin M5 access/compatibility discovery in parallel without
blocking usable CPU/GPU chat. Keep M1b and M6 optional. Stop unsupported lanes
with a documented no-go instead of weakening correctness or security requirements.
