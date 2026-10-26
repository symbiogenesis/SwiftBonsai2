# Swift Bonsai 2 - Runner implementation plan

Build a local creative assistant for Snapdragon X1 and X2 systems running
Windows ARM64, automatically selecting the best qualified configuration for
each device, workload and interactive/battery objective. Start from the recorded
32 GB Snapdragon X Elite machine. Prioritize writing, revision, brainstorming
and art direction, with short waits, bounded memory and comfortable sustained
operation. The [hardware profile plan](docs/hardware-profiles.md) owns this
ultimate goal; automatic selection is not implemented yet.

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
before promoting optimized paths. Adreno Vulkan and packed Hexagon are candidate
routes on both generations, with X2 NPU batched prefill and packed decode a
priority investigation. Qualify each actual device/capability class independently;
neither backend must precede the other's discovery, and neither accelerator is
enabled in current launch profiles.

The active implementation scope is the pinned Swift PQ2 CPU MVP in the
[Prism migration TODO](docs/prism-upstream-todo.md). Exact Swift chat rendering,
token accounting and native sampler parity are implemented, as is the
experimental owned service path. Bounded full-model forward parity and owned
HTTP recovery are qualified in the source runtime. A reviewed prebuilt runtime
release and lock update remain before ordinary Runner consumes those changes.
Broader acceleration, adapters and runtime optimization remain deferred. The
application MVP adds a separate pure-Rust native chat UI; it does not expand the
engine MVP's runtime acceptance scope. The permanent
[UI architecture](docs/ui-architecture.md) keeps first-party presentation in
Rust and local LLM interoperability at the HTTP/JSON/SSE boundary.

The approved abliteration LoRA is intended for the standard profile. Acceptance
requires **decode tokens/second and output quality each to match or exceed the
same engine without LoRA**, using predeclared tolerances. Lower token use or
fewer refusals cannot excuse either regression. A bounded RAM premium is
acceptable. Capture/scoring tools exist; device results, adapter-basis evidence
and human reviews remain pending. See [required qualification](docs/swift-lora.md#required-standard-profile-qualification).

## 2. Hardware envelope

| Component | Recorded baseline / planned coverage | Implication |
| --- | --- | --- |
| OS | Native Windows ARM64 | Host binaries, libraries and tooling must match |
| CPU | Recorded X1E-78-100; planned Snapdragon X1/X2 SKU coverage | Detect actual CPU/OS capabilities; qualify ISA, thread and batch choices per device |
| GPU | Recorded Adreno X1-85; X2 Adreno variants are separate candidates | Query features and shared-memory budgets; qualify actual drivers and kernels |
| NPU | Recorded X1 Hexagon v73; X2/v81 extended-DMA candidate | Verify architecture, signed matching runtime/kernels, mapping path and exact graph support |
| RAM | Recorded 32 GB installed; 33,897,037,824 usable bytes; other capacities detected | Admit against currently available RAM, commit and device budgets; do not infer headroom from generation |

Run `Doctor.ps1` for current inventory. Recorded driver/OS details are evidence
for that capture, not a continuing compatibility guarantee. Use GiB/MiB for
allocation budgets and bytes for artifact identities.

Use generation/SKU as inventory, then select by verified capabilities and
qualified evidence. The [X1/X2 plan](docs/hardware-profiles.md) defines unknown
hardware handling, compatible CPU fallback and independent device acceptance.

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
| Native Rust CPU | Verified Swift loader, hybrid graph, transactional generation, exact Swift chat/tokenizer, native sampler and experimental broker path | Reviewed prebuilt consumption; broader models, full-context workloads and adapters remain deferred |
| Adreno Vulkan | CPU-only builds and profiles today | Native ARM64 build, exact-model placement/parity and paired device evidence |
| Hexagon | Discovery and implementation plan only; recent upstream X2 DMA and low-bit/Hadamard leads | Signed matching kernels, exact packed arithmetic, hybrid state and bounded per-device deployment |
| Automatic X1/X2 profiles | Initial H0 inventory/contracts implemented; no selector or accepted accelerator defaults | Shared capabilities, measured profile catalog, deterministic selection and qualified fallback on each generation |

The [SIMD S0–S6 plan](docs/simd-plan.md), [Adreno G0–G5 plan](docs/adreno-gpu-plan.md)
and [Hexagon N0–N7 plan](docs/hexagon-npu-plan.md) own their detailed exits.
Source support or device detection does not qualify this laptop. OpenCL and
conversion-based ONNX/QNN routes require separate exact-format/graph audits.
Keep a reproducible CPU fallback and report unsupported placements explicitly.
The [first H0 implementation](docs/hardware-capabilities.md) provides versioned
host/runtime inventory and checked profile/evidence contracts without selecting
a profile. The [hardware H0-H5 plan](docs/hardware-profiles.md#delivery-sequence) combines
these exits into automatic selection. Rank qualified profiles by complete-task
latency or energy within constraints; tune prefill/decode separately and account
for state migration, shared bandwidth, load/compile/repack and sustained behavior.

## 5. Service, UI and isolation

The Rust broker implements bounded HTTP admission, exact-count orchestration,
single-slot scheduling, streaming supervision and an explicit Windows
development launcher for the Qwen reference or Swift PQ2 assessment. Each
selection uses its own compiled pins, batch sizes and memory admission. The
launcher verifies artifacts, protects credentials and owns the backend through
a Windows Job. The source native Swift path has bounded owned HTTP recovery evidence from
explicit diagnostic binaries. Released packaging, same-user backend bypass
prevention and restricted daily execution remain unqualified. See the [broker contract](docs/broker-policy.md).

The native `bonsai-chat` MVP provides the first-party chat interface through
the existing authenticated broker. UI application code must remain pure Rust,
using native controls or a native Rust toolkit; JavaScript/TypeScript,
HTML/CSS frontends, browser shells, WebViews, Electron and Tauri HTML frontends
are excluded. Do not substitute the pinned server's browser UI. Preserve local
LLM conventions through the documented OpenAI-style chat JSON/SSE API and
explicit adapters for the broker's strict subset. See
[UI architecture and compatibility](docs/ui-architecture.md).

Everyday service acceptance remains open. Qualify:

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

The native Rust UI retains Standard as its default and offers an opt-in
**Extra effort (experimental)** mode: up to three sequential plan, draft and
review/revision calls, with compact working state and the complete draft kept.
This small client workflow runs above the broker, retaining existing
inference settings and per-call limits, plus a shared deadline and cancellation.
Focused behavior checks suffice for initial experimental exposure; an extensive
benchmark campaign is not a prerequisite. Quality gains remain unproven. This is
an opt-in workflow whose creative-quality gains remain unqualified. See the
[mode contract](docs/inference-defaults.md#experimental-extra-effort-mode).

Completed native chat experience follow-ups:

- [x] Recover after Stop or an uncertain request failure through an explicit
  fresh owned-model retry, preserving completed conversation turns. Restore
  locally rejected prompts for editing instead of ending the app session.
- [x] Expose the bounded experimental plan/draft/review workflow with visible
  progress and retained complete drafts; keep Standard as the default.
- [x] Show the 8,192-token context bound and reasoning-inclusive completion
  reservation, with native 512/1,024/2,048/4,096-token output-cap selection.
- [x] Keep a bounded conversation workspace with separate drafts/settings, native
  rename and confirmed deletion, and exact committed-history restoration.
- [x] Add opt-in Windows-encrypted recovery of conversations and unsent drafts,
  bounded asynchronous writes, atomic replacement and a single-writer lock.
  Preserve corrupt recovery and never replay interrupted requests.
- [x] Find Unicode message text with highlighted next/previous matches and
  jump to latest, keeping search controls visible during long-history scrolling.
- [x] Provide `Start-Chat.ps1` with checked reusable external build outputs and
  actionable setup guidance; remove the obsolete demo launcher.
- [x] Allow drafting during model startup and keep settings usable at the minimum
  window size, with focused native interaction and rendered-layout checks.

Next application exits: packaged installation/update/rollback, actual-model
startup/long-chat/speech acceptance, Windows assistive-technology review, and
the restricted daily-service boundary from section 5. UI polish does not qualify
those runtime, security, creative-quality or sustained-performance requirements.

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
| Daily | Automatic best qualified CPU/GPU/NPU plan for the actual X1/X2 device; bounded reuse | Security, cancellation, responsiveness, memory and visible effective selection |
| Long context | Explicit larger context and separately qualified KV/cache | Continuity/retrieval quality and peak memory |
| Battery | Qualified per-device placement, phase-specific threads and low-polling candidates | Energy per useful task and acceptable latency; safe reevaluation when power policy changes |
| NPU experimental | Explicit profile after kernel/state correctness | Signed deployment, device memory, placement and recovery |

For initial reuse experiments, two checkpoints and 256 MiB prompt-cache storage
are proposals, not current defaults. Larger caches need observed total-memory
evidence. Tune prefill and decode separately; more threads, offload or processors
can increase contention and heat. Report effective settings and fallback reasons.
Keep qualification keyed by actual capabilities, runtime/driver and numerical
identity. Automatic selection consumes retained results; ordinary startup does
not launch a benchmark search. Unknown or invalidated accelerator compatibility
uses an eligible reviewed CPU control. Stable request/state handoff and complete
selection criteria live in the [hardware profile contract](docs/hardware-profiles.md#selection-contract).

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
allocation. The generic token driver retains those raw-text/token-ID contracts;
the pinned Swift chat and sampler integration is described below.
Verified tokenizer sessions now reuse exact prompt/context/output accounting;
the public incremental UTF-8 decoder and bounded prompt-file/stdin input are
available for service integration. A persistent bounded JSONL worker exposes
verified count/encode requests with fixed policy and explicit readiness. Library
callers share the driver's checked model/caller stops and bounded UTF-8 prompt
reader. Raw-ID worker requests preserve exact sequences under the same verified
vocabulary and output reservation. A reusable verified output session releases
the model file and supplies lossless pieces to independent display streams.
The bounded synchronous worker client pins readiness and checks text count/encode
responses, correlation and recovery without owning process or I/O deadlines.
Its raw-ID operations also validate local vocabulary/context bounds and exact
returned sequences. The driver and library accept bounded JSON token arrays
from files or stdin, keeping long raw prompts outside process arguments.
For the pinned Swift artifact, the separate Swift session/worker connects the
verified embedded chat template, exact tokenizer, supplied reasoning history and
native MT19937 sampler. Retained native captures cover prompt bytes/token IDs
and seeded sampler draws; the independently released server connects that
session to the owned experimental broker. Separate source-runtime evidence
qualifies bounded Swift full-model forward parity. Other templates/models,
reviewed prebuilt consumption and daily service remain separate exits in the
[active MVP checklist](docs/prism-upstream-todo.md). Tiled prefill kernels, broader
model/full-context allocation, broader model parity and performance
qualification remain open.

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
Qualify X1 and X2 separately, recording SKU/capabilities, effective phase
placement and selection decisions. Hardware lanes must exercise automatic
selection/fallback and safe state handoff; one device's result does not qualify
another generation or an untested compatibility class.

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
| `src/chat-ui/` | Pure-Rust native chat UI and local HTTP/JSON/SSE client |
| [Prism Inference](https://github.com/symbiogenesis/prism-inference) | Independent CPU engine, codecs, GGUF tools and storage overlay; consumed as pinned releases |
| `tools/` | Application maintenance CLI and embedded-C# validation support |
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
| M0 — reproducibility | Inventory, locks, provisioning and GGUF inspection implemented | Extend shared capability discovery for X1/X2; keep identities and evidence current |
| M1 — CPU creative chat | Swift bounded decode, experimental owned broker streaming and a pure-Rust native chat UI | Complete service/UI acceptance, bypass prevention and restricted daily process |
| M1a — creative/comfort | Pending | Useful creative output, standard LoRA qualification, latency and sustained device acceptance |
| M1b — optional agents | Pending | Qualified client and whole-agent containment before tools are enabled |
| M2 — Adreno | G0–G5 pending | Per-device X1/X2 exact-model correctness and useful benefit, or documented no-go |
| M3 — cache/context | Bounded experimental Rust state exists | Full-model reuse, context, allocation and quality qualification |
| M4 — daily profiles and automatic hardware selection | Initial H0 discovery/contracts implemented; H1-H5 pending | Reproducible installation; automatic qualified interactive/battery configurations and CPU fallback independently accepted on X1 and X2 |
| M5 — Hexagon | N0–N7 pending; prioritize X2 packed-kernel investigation alongside controls | Signed exact-model execution, bounded state/memory and per-device benefit, or no-go |
| M6 — advanced kernels/speculation | Research candidates | Correctness and measured benefit justify maintenance |
| M7 — Rust replacement | Swift loader/graph, chat/tokenizer, native sampler and experimental owned service implemented | Reviewed runtime consumption, followed by broader R0–R7 and per-profile acceptance |

The immediate engine work follows M2 → M5 → M6 in the
[Swift-only MVP checklist](docs/prism-upstream-todo.md), against the pinned native
control. Those checklist labels are separate from this table's project
milestones. After the engine MVP, resume M1/M1a and the hardware H1-H5 path,
developing M2/M5 candidates independently before M4 automatic promotion. H0
design/inventory can proceed now; keep accelerator implementation, adapters,
agents and optimization campaigns deferred while closing the CPU MVP. Stop
unsupported lanes with a documented no-go instead of weakening correctness or
security requirements.
