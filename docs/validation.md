# Validation

Runner owns application validation: native UI, broker, process/artifact policy,
provisioning and released-runtime integration. Algorithms/upstream conformance
belong to [Prism](https://github.com/symbiogenesis/prism-inference/blob/main/docs/build-and-validation.md).
**Local builds, Release analyzers and focused named tests are allowed. Complete
suites and every benchmark run only in CI; do not recreate a full suite with local filters.**

Owner-laptop diagnostics, model qualification and benchmarks have a five-minute
total maximum, preferably less, including preparation, loading, comparison and
cleanup. The same limit applies to self-hosted CI; its job timeout is five minutes.
Shrink the workload or retain an unqualified gate when it cannot fit. Do not split
a long matrix into repeated runs to work around this limit. Hosted cloud CI keeps
its existing limits.

Physical Snapdragon capture/qualification workflows, including speech/sandbox,
share the non-cancelling `swift-lora-snapdragon` concurrency group. Hosted targets
have independent groups. Serialization prevents overlapping laptop CI jobs,
without proving the machine otherwise idle. [PLAN.md](../PLAN.md) owns pending acceptance.

## Runner gates

### Actual-model application acceptance

Main-only, manually dispatched [native-runtime acceptance](../.github/workflows/native-runtime-acceptance.yml)
runs an explicitly ignored case on the owner-managed Snapdragon X/X2 runner.
The default case requires provisioned original PQ2, the pinned Rust release, native
sandbox admission, unchanged limits and retained artifact handles. It tests supplied-history
streaming, framed context rejection/reuse, shutdown, abandonment after content,
and fresh restart. Uploads contain credential-free receipts and bounded sanitized
worker logs; protected key directories are excluded. Failure rejects without an unrestricted retry.
Ordinary Foundation/local runs never execute the ignored case.

All owner-laptop diagnostics share a **five-minute total ceiling**, including
setup, model loading, execution and cleanup. The workflow's owner job stops at
five minutes; each actual-model case has one **240-second aggregate soft deadline**,
including startup and cleanup. Per-request deadlines cannot extend it. Do not
split a long matrix across repeated owner jobs. Hosted ARM64 prepares exact
source-bound test and maintenance binaries without loading models; the owner job
verifies their run/revision, SHA-256 and native architecture before direct execution,
with no Cargo build on the laptop.

The workflow defaults to `profile: pq2-no-adapter`, preserving the original case
within that aggregate bound. Both long optional workloads are disabled for all
profiles pending a budget-compliant design. Explicit `profile: ptq1-adapter`
selects a minimal required-sandbox smoke case for the exact current-weight export
and approved adapter: one scale-1.3 streamed request with a four-token cap and
owned cleanup. A scale-zero two-token request and invalid-scale HTTP 400/reuse
may run only if the same budget retains a conservative teardown reserve. Skipped
checks remain pending. Signed-zero/scale-two trials, supplied history, context
rejection, restart, long output and sustained use are not qualified by this smoke.
Credential-free progress receipts bind the complete runtime/model/adapter locks,
terminal reason, reported usage when supplied and cleanup. Partial failure retains
completed observations with `passed: false`. This does not establish complete-state
parity, adapter authoring basis, performance, output quality or default promotion.

Before runner temp cleanup, the workflow reads only `backend.stdout` and
`backend.stderr` from at most eight immediate protected session directories, with
a 64 KiB limit per log and strict UTF-8 decoding. It validates both actual keys
for every session, redacts those values across all retained logs, checks again
before publication, and suppresses logs when key validation fails. Only two fixed
sanitized top-level log names and `startup-diagnostics.json` enter the upload
selection, in a separate artifact gated on successful collection and final
regular-file, reparse and credential checks of all three paths. Unsafe output
directories or reparse points remain untouched and suppress the entire diagnostic
artifact; the three base acceptance JSON receipts retain their existing upload.
`host.json` retains fixed broker startup stage/error/exit-status markers;
the acceptance test process exit code is recorded separately. No raw exception,
argument or key content is copied into those classifications.

| Workload | Required behavior | Evidence limit |
| --- | --- | --- |
| Default | Valid streaming, context rejection/reuse, owned shutdown, abandonment and fresh restart | Short outputs do not qualify 8K/4K, numerical state, LoRA, sustained resources, speed or broker-only isolation. |
| Disabled `extended_history` (pending bounded redesign) | Admit 3,900 synthetic forest-note repetitions in assistant history with the full 4,096-token output reserve; reject 4,500, then reuse the same instance for a short request | Fixture repetitions are not tokens. Record visible bytes, terminal reason and usage when supplied; missing usage is unknown, events are not tokens, early completion does not prove 4K generation/exact 8K occupancy. |
| Disabled `stalled_consumer` (pending bounded redesign) | Request the full 4,096-token output reservation, read first nonempty content, retain the connection without reads for one second, then explicit owner Stop. Finish within 30 seconds with empty Job/released listener; require one additional failed operation and no completed operation, require generated content from a fresh owner after Stop and a further successful fresh-owner request after the existing abandonment trial | Natural completion cannot qualify interruption. The receipt proves unfinished broker forwarding, not backend activity at Stop, socket saturation, maximum backlog or measured cancellation latency. |

The original per-request safety limits, 8K context, 64-token margin, output,
memory and containment limits remain; the shorter aggregate device budget takes
precedence over every longer timeout. Streams require complete HTTP framing, a supported
terminal reason and `[DONE]`. Interrupted instances never reopen admission or replay
the request. Optional workloads remain unqualified until successful device CI
receipts are retained. The [promotion evaluator](native-rust-engine.md#default-runtime-promotion) keeps exits independent.

Model-free acceptance regressions:

| Exact case | Property |
| --- | --- |
| `native_extended_acceptance_distinguishes_output_usage_from_delta_count` | Fixture policy; usage separate from bytes/events |
| `native_stalled_consumer_retains_fragmented_prefix_connection_until_dropped` | Fragmented UTF-8/SSE intake and retained connection |
| `native_stalled_consumer_never_qualifies_natural_completion_as_interruption` | Operation-counter interruption guard |
| `native_stalled_consumer_rejects_error_or_unframed_stream_headers` | HTTP 200 and chunked event-stream framing before accepting content |

### Foundation and routing

[Premerge portable checks](../.github/workflows/premerge.yml) provide a limited
hosted Linux ARM64 screen for pull requests to `main` and manual dispatch: workspace
Rust formatting, portable CI/CPU-policy/broker compilation and six exact named
model-free routing, gate and request-policy regressions. A missing named case fails.
The read-only workflow runs no complete suite, model, benchmark or self-hosted job;
it qualifies neither native Windows behavior nor actual-model/device acceptance.

[Foundation](../.github/workflows/foundation.yml) runs on `main` pushes/manual
dispatch, skipping root-Markdown/`docs/`-only pushes. Its [Rust planner](../tools/ci/README.md)
reads [impact routing](../.github/ci/impact.json), selects changed packages plus
transitive workspace consumers, and requires every selected lane/app group to
pass. Only planned skips are accepted. The [package registry](../config/rust-packages.psd1)
contains application packages, not upstream runtime catalogs.

| Group | Coverage |
| --- | --- |
| Broker, UI, speech, maintenance | Affected packages/consumers with independent caches |
| Standards | PowerShell contracts/analyzers and strict embedded C# |
| Foundation/native scripts | Provisioning, plans, process wrappers, packaged supervision, portable package lifecycle/tamper/path checks and copied-launcher dependencies |
| Released runtime | Archive/install/profile/adapter/admission policy, GGUF wrappers, help switching and actual pinned `Get-Inference.ps1 -IncludeCapabilities` reports |

For example, a broker edit selects broker checks; shared Windows-boundary edits
also select broker/CLI consumers. CLI edits select CLI/app integration; script
standards run only for their inputs. Missing/unclassified diffs or changes to
workspace/lockfile/toolchain/CI inputs select full coverage. Manual dispatch
selects everything. Each workflow compares with its newest successful main-push
ancestor, covering failed/cancelled intervening changes. New automatic pushes
supersede older Foundation push runs; manual dispatch groups remain independent.
Planner/gate regressions run first; the tested cached Linux planner executable
is handed to aggregation, avoiding a second compiler install/build or Cargo metadata.

Maintenance owns affected Windows Job tests and produces a CLI plus feature-gated
Rust completion probe. Consumers verify revision, both digests and ARM64 identities;
lifecycle fixtures retain production headless mitigations. Script/released-runtime
lanes start after maintenance without waiting for independent broker/UI/speech.
Only standards installs .NET; scripts use PowerShell's bundled `Add-Type` compiler.

ASR/TTS use separate lanes and the checked CLI for pinned source preparation.
TTS caches four native static libraries plus digest receipt through
`BONSAI_TTS_NATIVE_BUILD_DIR`, keyed by build inputs/runner image; all Rust checks
still run. Missing caches rebuild; identity/digest mismatches fail. Speech
Cargo/native caches also bind the checked native build-host hash in every restore
prefix. Version-two library receipts bind full host/source hashes and each library
SHA-256; each lane retains its model-free host report, including cache hits.

Pinned C++ `--version`/`--help` use fixed-command inspection with 512 MiB,
one-process Job limits, caller lifetime, output bounds and deadlines. Inspection
permits required shell-DLL initialization; inference retains Win32k denial.
Inspection success cannot qualify legacy worker startup. Rust discovery retains
the ordinary headless policy. Package/runtime checks prove neither signing nor
actual-model/device acceptance.

Maintenance/UI regressions cover early-window preparation, preserved drafts,
bounded output, failures, descendant teardown, warm/build-variant reuse, damaged
inputs/receipts, non-build versus embedded-input changes, interrupted publication,
model-only provisioning and resumable partial downloads. Synthetic compiler trees
must leave no descendants after cancellation/timeout/early root exit before lock
reacquisition. Environment checks preserve Unicode/profile entries and reject
protected overlays. PowerShell checks copied packages and single-instance launch. These establish
correctness, not startup speed or inference performance.

Upstream-only coverage runs in Prism's [conformance](https://github.com/symbiogenesis/prism-inference/blob/main/.github/workflows/runtime-conformance.yml),
[Foundation](https://github.com/symbiogenesis/prism-inference/blob/main/.github/workflows/foundation.yml),
[standards](https://github.com/symbiogenesis/prism-inference/blob/main/.github/workflows/standards.yml)
and [compiler](https://github.com/symbiogenesis/prism-inference/blob/main/.github/workflows/rust-nightly.yml)
workflows, including source/backend catalogs, baseline/codec corpora, overlays and
focused upstream diagnostics. Runner's optional [LoRA device workflow](../.github/workflows/swift-lora-qualification.yml)
keeps its standalone tool build because it qualifies the selected application
profile; it requires reviewed artifacts, owner-managed hardware and explicit dispatch.

### Paired speech comparison

Main-only manual [TTS evaluation](../.github/workflows/tts-evaluation.yml) alternates
fresh Q8_0/Q4_K_M VoiceDesign processes over bounded thread budgets, reversing pair
order between repetitions. It separates first-request voice bootstrap from warm
Base synthesis. The Rust supervisor owns one kill-on-close process, caller lifetime,
deadlines and bounded logs, with **9 GiB admission, 16 GiB commit and 5 GiB reserve**.

Receipts retain revision, CPU, exact artifacts/executable, readiness, first audio,
real-time factor and synthetic WAV clips. Report sampled 25 ms phase private/resident
maxima, OS lifetime working-set peak, Job commit peak and minimum system available
memory separately: sampling misses brief peaks, mapped residency is outside commit,
and Design/Base talkers never coexist. Retain unsigned numeric/hex exit status,
observation before/after shutdown and last phase/sample count. Pre-shutdown exit
rejects even after complete audio; an earlier failure retains precedence.
Failed sessions cannot enter completed paired comparisons.

All execution, even one short profile, remains CI-only; the script rejects local
runs. `Invoke-TtsEvaluation.ps1 -PlanOnly` inspects locally without provisioning/loading.
Focused `speech_evaluation::tests::`/`speech_evaluation::windows::tests::` cover schedules,
context, phases, incomplete Stop, counter domains and prelaunch rejection.
[Hosted resource evidence](performance-expectations.md#q4-speech-resource-comparison)
describes that CPU alone; Snapdragon conclusions need its runner and quality
still needs independent listening.

## Standards

Rust uses the [exact nightly](../rust-toolchain.toml), edition 2024, checked Release
arithmetic, warnings-as-errors and strict Clippy/rustdoc. Safe roots forbid unsafe
code; documented Win32 operations stay in boundary crates reused by maintenance.
Scoped allowances retain reasons and safety documentation. UI checks preserve
[native Rust presentation and the explicit API subset](ui-architecture.md).

`bonsai-cpu-policy` shares profile limits, memory accounting, completion flags and
source/PTQ1 selection. Qualification derives argv from validated controls and
reports their canonical identity. Prism's ARM source route requires Clang rather
than silently reusing the MSVC builder; archives, configuration and overlays stay
pinned. Compiler-object and Cargo caches are separate and cannot bypass checks.
Native codec checks run the production Rust codec against the pinned DLL in a
supervised probe, using independent base-3 bytes, frozen FP32 scale bits and
captured C++ fixtures.

The [pinned nightly](../rust-toolchain.toml) enables Polonius branch-sensitive
broker lending; caller-owned reusable logits also work on stable.
[Compiler probes](https://github.com/symbiogenesis/prism-inference/blob/main/tools/rust-trial/README.md)
retain stable as an isolated control. Release overflow checks stay enabled and
debug assertions disabled to preserve
[subtle's release contract](https://github.com/dalek-cryptography/subtle/blob/2.6.1/src/lib.rs#L50-L53).

Shared `doc` planning uses `cargo doc --no-deps -Z unstable-options --output-format json`
for selected packages/features in Release, locally through `Test-Rust.ps1` and in
Foundation. Missing docs, intra-doc links, markup and Rust-block lints still fail.
JSON emits no rustdoc HTML/JS, avoiding the generated `trait.Send.js` quarantine
trigger; this does not determine whether Defender's detection was false.
Existing HTML caches/machine settings are untouched. Recheck the [unstable output option](https://doc.rust-lang.org/nightly/cargo/commands/cargo-doc.html#option-cargo-doc---output-format)
on toolchain changes. Ordinary validation builds neither browsable docs nor doctests;
complete tests retain their CI guard.

PowerShell 7.6+ uses latest strict mode, terminating native errors, advanced
functions and typed output. PSScriptAnalyzer 1.25.0 rejects unsuppressed diagnostics.
The [pinned .NET SDK](../global.json) checks every embedded Add-Type helper in
Release with nullable/checked arithmetic and unsafe compilation disabled.

## Focused local checks

Keep scripts/logs/captures/native working directories in a unique system-temp
lane outside Git. Expand absolute paths, preserve inherited Windows/profile
environment variables and choose the compiler explicitly.

```powershell
$tooling = Join-Path ([IO.Path]::GetTempPath()) ('Runner-Tooling-' + [guid]::NewGuid().ToString('N'))
Save-Module PSScriptAnalyzer -RequiredVersion 1.25.0 -Repository PSGallery -Path $tooling -Force
./tests/Test-Standards.ps1 -AnalyzerModulePath (Join-Path $tooling 'PSScriptAnalyzer/1.25.0/PSScriptAnalyzer.psd1')

$scratch = Join-Path ([IO.Path]::GetTempPath()) ('Runner-Checks-' + [guid]::NewGuid().ToString('N'))
./tests/Test-Rust.ps1 -Package bonsai-broker -TargetDirectory (Join-Path $scratch 'target') -EvidenceDirectory (Join-Path $scratch 'evidence')
```

Choose a relevant named regression, e.g. `./tests/Test-Foundation.ps1 -Case Download`
or `./tests/Test-NativeProcess.ps1 -Case Capture`. Direct Cargo tests select one
regression with external target/working directories. `-FullTests` is reserved
for main-branch GitHub Actions; benchmarks are always CI-only.

`window::interaction_tests::` drives egui input/accessibility with inert channels:
Enter/Shift+Enter, streaming selection and startup drafting, with no GPU/model/audio
required. Set external absolute `BONSAI_UI_CAPTURE_DIRECTORY` and select
`window::interaction_tests::capture_chat_layouts_when_requested` for PNGs of wide,
narrow, empty and maximum-draft/minimum-window layouts. Captures do not qualify
native screen readers, actual-model streaming or audio.

The [streaming/storage contract](streaming-resources.md) owns resource invariants.
Choose one named regression from the source locations below. These untimed checks
establish byte/pointer identity, ownership and bounded work; they measure neither
device latency nor process peak memory. Module prefixes locate cases, not permission
to run a complete module or reconstruct a suite locally.

| Area / source | Regression coverage |
| --- | --- |
| Broker [`request_buffer`](../src/broker/src/request_buffer.rs), [`backend` storage/payload tests](../src/broker/src/backend/) and [`response` memory tests](../src/broker/src/response/memory_tests.rs) | Final-destination/spare-tail reads, bounded growth, fragments/coalesced prefixes/chunk tails, empty reads, sticky truncation/I/O failure, exact-cap overflow probes, framing, deadlines and cancellation. Eligible payloads bypass 8 KiB staging. |
| UI [`model_lifecycle`](../src/chat-ui/src/window/model_lifecycle.rs) and [`window` lifecycle regressions](../src/chat-ui/src/window/tests.rs) | Explicit model states; late readiness, completion and teardown notices cannot reopen Send in the wrong phase. Retry waits for teardown and explicit action; terminal failure stays unavailable. Stop preserves completed history, retained effort drafts and newer composer text. Recoverable rejections preserve newer drafts. These model-free checks do not qualify actual-model lifecycle behavior. |
| UI [`bounded_json`](../src/chat-ui/src/bounded_json.rs), [`client` memory tests](../src/chat-ui/src/client/memory_tests.rs), [`recovery`](../src/chat-ui/src/recovery.rs) and [`persistence`](../src/chat-ui/src/window/persistence.rs) | Escaped-byte limits before full encoding; immutable body sharing with independent turn vectors; request bytes retained through partial write/flush and released before response reads/on failure; snapshot reservation through encryption/save; busy saves skip encoding. Exact restore counts reject extra objects before fields and preserve the live workspace. |
| [`startup_process`](../tools/bonsai/src/startup_process.rs) | UTF-8 allocation transfer, lossy-decode identity and stdout-before-stderr ordering. |
| Completion [`memory`](../tools/bonsai/src/completion/memory.rs), [`completion`](../tools/bonsai/tests/completion.rs) | Optional output attribution keeps first stdout observations separate from token latency, partitions current counters by observed output/exit, leaves unsampled peaks unknown and never assigns lifetime peaks to a stage. Native supervision requires a stdout cap and preserves total sample coverage. Model-phase and actual-model/device qualification remain pending. |
| Speech service [`http`](../src/speech-service/src/http.rs), [`request`](../src/speech-service/src/request.rs) and UI [`canary`](../src/chat-ui/src/canary.rs) | Borrowed multipart fields/PCM, pre-decode excess-field rejection, canonical/extensible WAV identity, boundary collisions, maximum clip, authenticated wire bytes and partial/cancelled writes. Final-storage JSON/WAV reads cover retained headers, coalesced/partial delivery, exact limits, overflow and truncation. Header tests count delimiter comparisons and cover every delimiter split at exact limits. |
| UI [`client`](../src/chat-ui/src/client.rs) and [`canary::pcm`](../src/chat-ui/src/canary/pcm.rs) | Ready prefixes arrive before remaining body/chunk framing. Gated reads check fixed-buffer reuse, odd PCM fragments, buffered cancellation, truncation and terminal validation without byte-sized socket reads. |
| [`speech` workers](../src/chat-ui/src/speech/) and [`command_queue`](../src/chat-ui/src/speech/command_queue.rs) | Stalled-worker queue/payload bounds, coalescing, overload, tail compaction, cancellation and fresh recovery. Cold workers wake on command/disconnect/Stop/shutdown; active, warm and retained-input workers continue periodic monitoring. |
| [`speech::pipeline`](../src/chat-ui/src/speech/pipeline.rs) | Bounded packet/spare reuse across segment tails, byte identity, nonblocking recycling, stable descriptors with cleared fields, eight-descriptor limit, retained PCM/header on unprepare refusal and quarantine teardown. Fake callbacks do not invoke an audio driver. |
| TTS [`build_overlay`](../src/speech-tts/build_overlay.rs) | Compiles the actual generated decode block with the provisioned compiler; checks cache hits, window/batch replacement, allocation failure, completed-graph lifetime/teardown, immutable source and drift rejection. |
| Transcript [`cache_tests`](../src/chat-ui/src/window/transcript/cache_tests.rs), [`search` work tests](../src/chat-ui/src/window/transcript/search/performance_tests.rs) and [`shortcuts`](../src/chat-ui/src/window/transcript/search/shortcuts.rs) | Fixed highlight identities at 10,000 results; navigation, same-length replacements/queries, Find reopen/eviction and no duplicated range vectors. Appends scan/copy only new bytes/results with one continuation, query-sized scratch and folded-Unicode oracle checks; saturated prefixes skip tail work while preserving lookahead, selection and navigation. Accounting checks exact totals and a combined count/byte eviction prefix at the shared 512-message live/restore limit, without visiting completed messages on ordinary append. Keyboard/IME/Escape precedence needs no service. |

For promotion tooling, `./tests/Test-Inference.ps1 -Case ReleaseLock` (also in
Foundation `-Case All`) uses synthetic bundles without a model. Candidate generation
cannot promote a pin; follow the [lock-update review](development-guide.md#prepare-a-runtime-lock-update).

## Acceptance limits

| Boundary | What focused/native evidence establishes |
| --- | --- |
| TTS bootstrap | `voice_bootstrap` checks native reference pointer identity/limits and handoff through Design teardown/Base initialization/extraction; failure releases once and retains at most one model. |
| Rust speech sampler | `philox::tests::`, `native::philox_tests::rust_philox_scalar_abi_matches_pinned_cpp_bits`: block/f32 vectors and 4,411 deterministic edge/corpus cases against pinned C++, without a model. `rust_philox_overlay_removes_native_algorithm_and_rejects_source_drift` checks both call replacements/complete native removal. Use provisioned `BONSAI_TTS_COMPILER`/native environment; see [sampler port](../src/speech-tts/README.md#rust-sampler-port). |
| ISA discovery | `native::isa::tests::native_isa_queries_neon_dotprod_and_neon_i8mm_independently`: exact Windows feature IDs through an injected query, without executing optional kernels. |
| X2 conditions | `cpu_evaluation::conditions::tests::condition_comparison_rejects_known_drift_and_unknowns_without_treating_charge_as_energy`; `condition_comparison_binds_exact_native_host_even_when_core_counts_match`; `host_cpu::tests::host_identity_retains_native_cpu_kernel_and_feature_facts_separately`: reject topology/power/registry/kernel/feature drift, malformed/missing identities, legacy receipts and cross-sample changes; separate identity from live availability. |
| OpenCL inventory | `opencl_inventory_rejects_unbounded_or_malformed_native_properties` (process crate), `opencl_inventory_rejects_overrides_and_unqualified_claim_changes` (tools): property/count bounds without a driver. `host-opencl` only queries bounded information; no executable GPU workload or inference verdict. See [feasibility](adreno.md#opencl-access-and-source-audit). |
| Native containment | Main-only [sandbox qualification](../.github/workflows/sandbox-qualification.yml) runs explicitly ignored PSEC fixtures on hosted ARM64/selected owner hardware, retaining revision/SDK/OS and synthetic logs. Checks file/temp authority, external-network denial, broader host-loopback scope, process limits, artifact/key validation and abrupt-owner cleanup. Foundation excludes these cases; no model/benchmark runs. |

The X2 screen records CPU/power before/after each sample outside process wall
time and rejects changed/unknown observations within or between paired boundaries,
even if the batch returns to its initial mode. There is no inside-sample monitoring
or energy verdict. Its retained legacy headless startup failed DLL initialization;
inspection success does not qualify worker startup. See [X2 evidence](cpu-kernels.md#x2-cpu-thread-screen).

Synthetic checks establish the tested policy/parser/storage/process properties;
native checks establish only the tested Windows behavior. Neither establishes
actual-model/daily acceptance, broker-only/same-user bypass prevention, quality,
throughput, sustained comfort, physical audio-driver behavior or speech/chat overlap.
In particular, untimed storage checks do not qualify the 15.5 GiB admission/11.5 GiB
Job budget. Source checks do not qualify a provisioned binary; release acceptance
requires independently reviewed artifacts and promotion evidence.

## Evidence retention

Device acceptance, sandbox, LoRA, PTQ1, X2 CPU and TTS workflows request 90-day
artifact retention. This extends the review window; Actions artifacts remain
temporary, and repository or organization policy can shorten their availability.
Before using a capture for promotion or a published claim, preserve its existing
credential-free upload selection in durable owner-managed storage. Keep the exact
files and their SHA-256 digests, repository revision, run URL/attempt, artifact
identity, device/build identities and independent reviews, including losing/no-go
results. Verify the archived bytes against the recorded digests before review.
Do not expand upload selections to include credentials, private conversations,
model weights or adapters. Run URLs and expired artifacts alone cannot reproduce
evidence; unavailable evidence leaves the associated gate pending.

## Promotion criteria

Every promoted runtime/profile must:

- Pass its correctness, security, full-state recovery, creative-quality and
  resource gates, including independent LoRA decode-TPS and quality requirements.
- Keep memory bounded without sustained paging, with owner-accepted
  responsiveness, heat and fan behavior during sustained use.
- Show paired target-device CI benefit: initially **10% lower end-to-end latency**,
  or documented capacity/energy benefit within accepted latency/quality limits.
  Predeclare tolerances and retain losing/no-go results.
- Reproduce on settled `main` with immutable artifact, build and device identities,
  run/capture references, limitations and a rollback profile.

Full suites and benchmarks run only in their owning main-only CI workflows.
Local builds, analyzers and focused correctness checks remain allowed. Passing
an experiment or synthetic check does not waive any independent promotion gate.
