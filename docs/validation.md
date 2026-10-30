# Validation

Runner owns application validation: native UI, broker, process/artifact policy,
provisioning and released-runtime integration. Algorithms/upstream conformance
belong to [Prism](https://github.com/symbiogenesis/prism-inference/blob/main/docs/build-and-validation.md).
**Local builds, Release analyzers and focused named tests are allowed. Complete
suites and every benchmark run only in CI; do not recreate a full suite with local filters.**

Physical Snapdragon capture/qualification workflows, including speech/sandbox,
share the non-cancelling `swift-lora-snapdragon` concurrency group. Hosted targets
have independent groups. Serialization prevents overlapping laptop CI jobs,
without proving the machine otherwise idle. [PLAN.md](../PLAN.md) owns pending acceptance.

## Runner gates

### Actual-model application acceptance

Main-only, manually dispatched [native-runtime acceptance](../.github/workflows/native-runtime-acceptance.yml)
runs an explicitly ignored case on the owner-managed Snapdragon X/X2 runner.
It requires provisioned original PQ2, the pinned Rust release, native sandbox
admission, unchanged limits and retained artifact handles. It tests supplied-history
streaming, framed context rejection/reuse, shutdown, abandonment after content,
and fresh restart. Only credential-free receipts are uploaded; protected key
directories are excluded. Failure rejects without an unrestricted retry.
Ordinary Foundation/local runs never execute the ignored case.

| Workload | Required behavior | Evidence limit |
| --- | --- | --- |
| Default | Valid streaming, context rejection/reuse, owned shutdown, abandonment and fresh restart | Short outputs do not qualify 8K/4K, numerical state, LoRA, sustained resources, speed or broker-only isolation. |
| Optional `extended_history` | Admit 3,900 synthetic forest-note repetitions in assistant history with the full 4,096-token output reserve; reject 4,500, then reuse the same instance for a short request | Fixture repetitions are not tokens. Record visible bytes, terminal reason and usage when supplied; missing usage is unknown, events are not tokens, early completion does not prove 4K generation/exact 8K occupancy. |
| Optional `stalled_consumer` | Read first nonempty content, retain the connection without reads for one second, then explicit owner Stop. Finish within 30 seconds with empty Job/released listener; require one additional failed operation and no completed operation, then fresh-owner content/restart after abandonment | Natural completion cannot qualify interruption. The receipt proves unfinished broker forwarding, not backend activity at Stop, socket saturation, maximum backlog or measured cancellation latency. |

All exchanges retain the ten-minute deadline, 8K context, 64-token margin, output,
memory and containment limits. Streams require complete HTTP framing, a supported
terminal reason and `[DONE]`. Interrupted instances never reopen admission or replay
the request. Optional workloads remain unqualified until successful device CI
receipts are retained. The [promotion evaluator](runtime-promotion.md) keeps exits independent.

Model-free acceptance regressions:

| Exact case | Property |
| --- | --- |
| `native_extended_acceptance_distinguishes_output_usage_from_delta_count` | Fixture policy; usage separate from bytes/events |
| `native_stalled_consumer_retains_fragmented_prefix_connection_until_dropped` | Fragmented UTF-8/SSE intake and retained connection |
| `native_stalled_consumer_never_qualifies_natural_completion_as_interruption` | Operation-counter interruption guard |
| `native_stalled_consumer_rejects_error_or_unframed_stream_headers` | HTTP 200 and chunked event-stream framing before accepting content |

### Foundation and routing

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
protected overlays. PowerShell retains copied-package/single-instance checks;
old cache/native-startup script suites are retired. These establish correctness,
not startup speed or inference performance.

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
Use these untimed regression locations; pointer/byte/work-count checks establish
ownership and bounded work, not device latency or process peak memory.

| Area | Focused selections / properties |
| --- | --- |
| Broker request/backend storage | `request_buffer::tests::`, `backend::memory_tests::`, `backend::payload_tests::`: final-destination reads, fragments/prefixes/chunks, empty reads and sticky truncation/I/O teardown; eligible payloads bypass 8 KiB staging, tails stay bounded. |
| Broker final JSON | `response::memory_tests::`: spare-tail initialization, bounded growth, exact-cap overflow probe, framing, deadlines and cancellation. |
| UI history/recovery bytes | `bounded_json::tests::`; `client::tests::committed_and_pending_history_share_payloads_without_sharing_turn_mutations`; `window::persistence::tests::recovery_bounds_escaped_json_before_retaining_an_oversized_workspace`. |
| TTS generated graph eviction | `build_overlay::tests::generated_decode_workspace_eviction_releases_only_completed_graphs`: compile actual generated block with provisioned compiler; verify hits, window/batch changes, allocation failure, lifetime/teardown, immutable source and drift rejection. |
| Recovery backpressure | `recovery::tests::recovery_reserves_snapshot_before_encoding_through_worker_completion`; `window::persistence::tests::recovery_backpressure_skips_encoding_until_the_snapshot_slot_is_free`: held synthetic save prevents another encoding. |
| Startup capture | `startup_process::tests::`: UTF-8 allocation ownership, unchanged lossy decode, exact stdout/stderr order. |
| Speech intake | `http::intake_tests::memory_body_intake_reuses_headers_and_preserves_coalesced_audio`; `request::tests::memory_multipart_`: fixed fields and excess rejection before decode. |
| Linear headers | `fragmented_headers_scan_each_window_once_and_preserve_coalesced_payload` / `fragmented_headers_scan_each_window_once_and_preserve_coalesced_audio`: count delimiter comparisons. `fragmented_response_headers_keep_exact_limit_framing_and_coalesced_body` / `fragmented_request_headers_keep_exact_limit_framing_and_coalesced_audio`: every delimiter split at exact limits. |
| Borrowed microphone upload | `canary::tests::multipart_upload_borrows_pcm_and_preserves_bytes_under_partial_writes`: canonical/extensible WAV identity, boundary collision avoidance, partial/cancelled writes, max clip; authenticated transcription checks wire bytes. |
| Speech final-storage response | `canary::tests::response_body_`: retained headers/coalesced payload, direct JSON/WAV reads under partial delivery, exact limits, overflow/truncation/cancellation; authenticated transcription/strict framing share the contract. |
| Request send ownership | `client::memory_tests::`: owned bytes survive partial writes/flush, release before first response read or on failure, preserve wire bytes and terminal SSE/HTTP. |
| Ready speech PCM | `canary::pcm::tests::pcm_framing_reuses_one_buffer_without_byte_sized_source_reads`; `pcm_ready_prefix_arrives_before_remaining_chunk_or_framing`: gated delivery/read counts, odd fragments, buffered cancellation, truncation and terminal validation. |
| Ready chat SSE | `client::tests::http_payload_`: ready payload precedes remaining body/chunk framing, while truncation still rejects. |
| Speech backpressure | `speech::command_queue::tests::`, `speech::stream::tests::`: stalled-worker bounds, coalescing, overload, cancellation, fresh recovery and tail compaction. |
| Speech waits | `speech::tests::cold_speech_worker_waits_for_commands_and_wakes_for_stop_and_shutdown`; `speech::tests::speech_command_wait_preserves_monitoring_for_warm_active_and_retained_input`; `speech::command_queue::tests::blocking_speech_receiver_wakes_when_the_sender_disconnects`: command/disconnect wakeups with periodic monitoring while resources are owned. |
| Recovery counts | `window::persistence::tests::recovery_rejects_excess_`: exact count limits, reject extra objects before fields, preserve live workspace. |
| PCM/WinMM recycling | `speech::pipeline::tests::pcm_`, `tiny_pcm_callbacks_form_fixed_frames_and_flush_the_final_tail_exactly`, synthetic output and `speech::pipeline::tests::playback_`: bounded packet/spare reuse, segment tails, byte identity, nonblocking recycling, stable descriptors/cleared packed fields, eight-descriptor limit, retained PCM/header on unprepare refusal and quarantine teardown. Fake callbacks do not invoke a driver. |

Find/transcript work checks use `window::transcript::cache_tests::` and
`window::transcript::search::performance_tests::`. They count work directly and
make no elapsed-time claim:

| Exact cases | Guard |
| --- | --- |
| `highlighted_layout_cache_keeps_constant_size_keys_during_streaming_and_navigation`; `highlight_identity_tracks_same_length_queries_replacement_and_result_eviction` | Fixed identities at 10,000 results, unchanged-range visits, navigation, same-length edits, Find reopen/eviction; no duplicated range vectors. |
| `streamed_find_scans_only_appended_bytes_and_keeps_one_continuation` | Linear appended-byte work, tail scratch; Unicode append oracle, replacement/saturated lookahead avoid prefix rescans. |
| `streamed_find_appends_only_new_results_without_rebuilding_completed_caches`; `streamed_find_saturated_prefix_skips_tail_cache_and_result_work`; `streamed_find_exact_limit_preserves_results_when_lookahead_becomes_limited` | Result-copy/cache counts, preserved navigation/selection, saturation then eviction. |
| `bounded_find_matches_folded_reference_across_expansion_and_prefix_boundaries`; `bounded_find_scratch_depends_on_query_bytes_and_retains_the_result_lookahead` | Prior folded-match oracle; query-sized scratch and lookahead limits. |
| `window::transcript::search::shortcuts::tests::` | Find keyboard/IME/Escape precedence without services. |
| `transcript_accounting_caps_empty_messages_before_bytes_require_eviction`; `transcript_accounting_count_eviction_preserves_find_and_recovery`; `transcript_accounting_count_and_byte_eviction_remove_one_combined_prefix` | Shared 512-message live/restore bound, exact byte totals across append/replacement/eviction/recovery, one combined count/byte eviction prefix, preserved Find selection, no completed-message visits for ordinary append accounting. |

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
| OpenCL inventory | `opencl_inventory_rejects_unbounded_or_malformed_native_properties` (process crate), `opencl_inventory_rejects_overrides_and_unqualified_claim_changes` (tools): property/count bounds without a driver. `host-opencl` only queries bounded information; no executable GPU workload or inference verdict. See [feasibility](opencl-feasibility.md). |
| Native containment | Main-only [sandbox qualification](../.github/workflows/sandbox-qualification.yml) runs explicitly ignored PSEC fixtures on hosted ARM64/selected owner hardware, retaining revision/SDK/OS and synthetic logs. Checks file/temp authority, external-network denial, broader host-loopback scope, process limits, artifact/key validation and abrupt-owner cleanup. Foundation excludes these cases; no model/benchmark runs. |

The X2 screen records CPU/power before/after each sample outside process wall
time and rejects changed/unknown observations within or between paired boundaries,
even if the batch returns to its initial mode. There is no inside-sample monitoring
or energy verdict. Its retained legacy headless startup failed DLL initialization;
inspection success does not qualify worker startup. See [X2 evidence](x2-cpu-optimization.md).

Synthetic checks establish the tested policy/parser/storage/process properties;
native checks establish only the tested Windows behavior. Neither establishes
actual-model/daily acceptance, broker-only/same-user bypass prevention, quality,
throughput, sustained comfort, physical audio-driver behavior or speech/chat overlap.
In particular, untimed storage checks do not qualify the 15.5 GiB admission/11.5 GiB
Job budget. Source checks do not qualify a provisioned binary; release acceptance
requires independently reviewed artifacts and promotion evidence.
