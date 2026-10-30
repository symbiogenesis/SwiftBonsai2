# Validation

Runner CI owns this application's native Rust UI, broker, process ownership,
artifact provisioning and released-runtime integration. Runtime algorithms and upstream conformance
belong to [Prism Inference](https://github.com/symbiogenesis/prism-inference/blob/main/docs/build-and-validation.md).
Local builds, Release analyzers and focused named tests are allowed. Complete
suites and every benchmark run only in CI; do not approximate a full suite by
batching local filters.

All qualification/capture workflows targeting the physical Snapdragon runner,
including speech and sandbox trials, share the non-cancelling
`swift-lora-snapdragon` concurrency group. Hosted targets keep independent groups.
This prevents overlapping jobs from consuming the laptop's CPU and memory while
the X2 thread screen runs; it does not establish an otherwise idle machine.

Speech CI keys Cargo/native static outputs by the same checked native build-host
identity used by source startup, before restoring any compiled cache. Every
restore prefix retains that identity. Version-two static-library receipts bind
the full host hash, source identity and each library's SHA-256 before reuse.
Each speech lane retains its model-free host report, including cache hits.

The manual [TTS evaluation](../.github/workflows/tts-evaluation.yml) compares the
pinned Q8_0 and Q4_K_M VoiceDesign profiles with several CPU thread budgets.
It alternates fresh Q8/Q4 processes, reversing pair order between repetitions,
and separates first-request voice bootstrap from warm Base synthesis. Receipts
retain the tested revision, CPU, exact artifacts/executable, readiness, first
audio, real-time factor and synthetic WAV clips for listening. A Rust supervisor
owns a one-process kill-on-close Job, caller lifetime, deadlines and bounded logs,
preserving 9 GiB admission, a 16 GiB commit ceiling and a 5 GiB physical reserve.
Memory reports distinguish sampled 25 ms phase private/resident maxima, the OS
process-lifetime working-set peak, Job commit peak and system available-memory
minimum. Samples can miss brief allocations; mapped residency is outside Job
commit, and Design/Base talkers never coexist. Failed sessions retain receipts
and cannot enter completed paired comparisons.
Receipts retain unsigned numeric/hex child exit status, its observation before
or after supervisor shutdown, and the last observed phase/sample count. A child
exit observed before supervisor shutdown rejects the trial even after complete
audio; an earlier capture failure retains precedence.
Select the owner-managed Snapdragon runner for device-specific conclusions;
hosted ARM64 results describe only that hosted CPU. A successful run does not
establish equivalent voice quality: the report leaves human listening pending.
The [completed hosted resource comparison](speech-memory.md) retains the paired
counter differences and their qualification scope.
All evaluation execution, including a single profile or short fixture, remains
CI-only. Invoke the workflow on `main`; its script rejects local execution.
`Invoke-TtsEvaluation.ps1 -PlanOnly` inspects the bounded schedule locally
without provisioning or loading a speech model. Focused `speech_evaluation::tests::`
and `speech_evaluation::windows::tests::` checks cover scheduling, context policy,
phase ordering, incomplete stops, counter domains and prelaunch rejection.

The [UI architecture](ui-architecture.md) requires first-party UI application
code to remain pure Rust and native. Native UI builds and focused client checks
must preserve the broker's documented HTTP/JSON/SSE subset. Window launch and
streaming checks do not establish broad SDK compatibility, model quality or
runtime performance. Browser assets and browser-runtime UI dependencies are not
an accepted implementation path.

## Runner gates

The manually dispatched [native-runtime application acceptance](../.github/workflows/native-runtime-acceptance.yml)
runs one explicitly ignored actual-model broker case on the owner-managed
Snapdragon X/X2 Elite runner and settled `main`. It requires already provisioned
original PQ2 and the pinned Rust release, native sandbox admission, unchanged
memory limits and retained exact artifact handles. It covers supplied-history
streaming, framed context rejection and reuse, owned shutdown, abandonment after
generated content and fresh restart. It uploads only explicit credential-free
receipts; protected credential directories are excluded. Failure or missing
resources reject the run without an unisolated retry. Its short outputs do not
qualify 8K context, 4K generation, numerical state, LoRA, sustained resources,
performance or broker-only isolation. The [promotion evidence evaluator](runtime-promotion.md)
keeps these requirements separate. The ignored case never runs in ordinary
Foundation suites or locally.

Its optional `extended_history` input adds a supplied-history request with the
full 4,096-token output reservation and a larger-history framed context
rejection, followed by a short request against the same model instance. The
fixtures contain 3,900 and 4,500 repetitions of a synthetic forest-note word in
assistant history; these counts are fixture sizes, not claimed token counts.
Actual public tokenizer admission must reject the larger fixture and admit the
smaller one with the unchanged 8K context, 64-token margin and 4K output reserve.
Streaming must finish with valid HTTP framing, a supported terminal reason and
`[DONE]`, then existing abandonment, teardown and fresh-restart checks still run.
Every exchange keeps the ten-minute production deadline; no output, memory or
containment limit is relaxed. The receipt records visible output bytes, terminal
reason and completion-token usage when the runtime supplies it. Missing usage
remains unknown; SSE event counts are never treated as token counts. Natural
early completion remains visible and does not qualify full 4K generation or
exact 8K occupancy. This optional workload has not supplied device acceptance
evidence until a successful native CI receipt is retained.

The focused model-free
`native_extended_acceptance_distinguishes_output_usage_from_delta_count` case
checks the extended fixture and that reported usage stays separate from visible
bytes and event counts. It does not load a model.

The optional `stalled_consumer` input reads through the first nonempty visible
content event of a full-output-reservation request, keeps that connection open
without further reads for one second, and invokes explicit owner Stop. The
owned runner must finish within 30 seconds, confirm the backend Job is empty
and release its listener without relying on client closure. Operation counters
must show one additional failed operation and no additional completed operation;
natural response completion cannot qualify interruption. Subsequent acceptance
requires generated content from a freshly owned backend and another successful
fresh-owner request after the existing abandonment trial, without replaying the
interrupted request. Interrupted instances never reopen admission.

Its credential-free receipt distinguishes unfinished broker forwarding from
unknown backend generation activity at Stop. This bounded workload does not
establish socket-buffer saturation, maximum-size backlog, cancellation latency
measurements or a performance gain. Actual X2 qualification remains pending a
successful main-only device CI receipt. Model-free named
`native_stalled_consumer_retains_fragmented_prefix_connection_until_dropped`
and `native_stalled_consumer_never_qualifies_natural_completion_as_interruption`
regressions cover fragmented UTF-8/SSE intake, retained client ownership and the
natural-completion guard without loading a model. The named
`native_stalled_consumer_rejects_error_or_unframed_stream_headers` case requires
HTTP 200 with chunked event-stream framing before accepting that prefix.

[Foundation](../.github/workflows/foundation.yml) runs only on `main` pushes or
manual dispatch, skipping documentation-only pushes (root Markdown and `docs/`).
Its lightweight planner selects changed Cargo packages and every transitive
workspace consumer, then starts only their Rust lanes and relevant app groups.
The final `foundation` gate requires every selected group to pass; only skips
explicitly authorized by the plan are accepted.

| Group | Coverage |
| --- | --- |
| Broker core, native UI, speech and maintenance | Affected registered app packages and their workspace consumers, with independent build caches |
| Standards | Pinned PowerShell analysis, script contracts and strict embedded-C# analyzers |
| Foundation/native scripts | Download/provisioning, invocation plans, native-process wrappers and packaged supervisor contracts |
| Released-runtime integration | Pinned archive/install verification, profile/adapter/admission policy, released GGUF wrappers and server-help switching |

The foundation script lane also runs the complete portable chat package lifecycle,
tamper/path rejection and copied-launcher dependency checks. Its released GGUF
setup runs `Get-Inference.ps1 -IncludeCapabilities`, validating the actual pinned
runtime's model-free report through the checked maintenance tool. These checks
do not establish publisher signing, actual-model behavior or device qualification.

Pinned C++ runtime `--version` and `--help` checks use a fixed-command inspection
launch that retains the 512 MiB and one-process Job limits, caller lifetime,
output bounds and deadline. It permits Windows shell DLL initialization required
by the release. Inference workers retain Win32k denial; a successful inspection
does not qualify this legacy release for that worker policy. Rust runtime
capability discovery continues to use the ordinary headless policy.

Startup policy, checked build reuse, native compiler preparation and speech file
provisioning are Rust maintenance regressions. Early-window cases prove that
only the presentation executable is required before the native shell opens,
and deferred setup retains backend policy without relinking the running UI.
Focused UI cases cover draft preservation, bounded preparation output, failure
and owned descendant cleanup on cancellation. These are correctness checks;
they do not measure startup latency or inference performance.
Their focused cases cover warm
launches that skip Cargo/preparation, changed and damaged inputs, owned-service
selection, model-only provisioning and bounded resumable downloads. PowerShell
retains copied-package bridge and single-instance process checks; the former
cache/native-startup script suites have been retired.
Additional focused regressions cover build-variant reuse, non-build edits,
embedded-input changes, incomplete receipts, interrupted source publication and
bounded partial-download recovery. Synthetic compiler trees verify that
cancellation, timeout and early root exit leave no descendants before the build
lock can be reacquired. Native boundary checks retain Unicode/profile environment
entries and reject protected-variable overlays.

For example, `src/broker/**` runs only broker checks. A change to a shared Windows
boundary crate also checks its broker/CLI consumers. A CLI change runs its own
crate checks and app integration, without testing unrelated crate sources.
PowerShell/C# standards run only for their own inputs. Embedded configuration
and fixtures are mapped to their owning crates in
[the routing configuration](../.github/ci/impact.json).

Focused OpenCL discovery checks are
`opencl_inventory_rejects_unbounded_or_malformed_native_properties` in
`bonsai-windows-process` and
`opencl_inventory_rejects_overrides_and_unqualified_claim_changes` in
`bonsai-tools`. They exercise property/count bounds and unqualified report
admission without calling a driver. `host-opencl` is a separately bounded
model-free information query; it creates no executable GPU workload and does
not establish an accelerator performance or inference verdict. Its public
source-coverage no-go is retained in [OpenCL feasibility](opencl-feasibility.md).

The maintenance job owns Windows Job lifetime/resource-limit tests when those
packages are affected. It produces one CLI artifact for selected app groups;
script-only changes reuse/build the CLI without running unrelated Rust suites.
Script and released-runtime groups start as soon as maintenance completes;
they do not wait for the independent broker, UI or native speech checks.
The artifact also carries the feature-gated native Rust completion test probe,
so supervisor lifecycle fixtures obey the same headless process mitigations as
production workers. Consumers verify the revision and both executables' digests
and ARM64 identities. The standards job alone
installs the .NET SDK; script lanes use PowerShell's bundled `Add-Type` compiler.

The broker/service and UI packages use separate lanes. ASR and TTS each use their
own speech lane and consume the checked CLI for pinned native-source preparation,
avoiding an additional uncached CLI build. TTS prepares its four native static
libraries through the existing Cargo build script and caches only those libraries
and their digest receipt, keyed by the exact build inputs and runner image. Its
Clippy, rustdoc, tests and executable build reuse these libraries through
`BONSAI_TTS_NATIVE_BUILD_DIR`; all Rust checks still execute. A missing cache
rebuilds the libraries, and mismatched identities or digests fail the job.

Each workflow compares against its most recent successful main-push ancestor,
including changes left unvalidated by failed or cancelled runs. A missing base,
unclassified input, or shared workspace/lockfile/toolchain/CI change selects full
coverage. Manual Foundation dispatch also runs all packages and app groups.
The newest automatic push supersedes older Foundation push runs on the same
branch, releasing runners for the current revision. Its successful-ancestor
baseline includes every earlier change left unvalidated by cancellation, so
disjoint crate edits remain covered. Manual dispatches have independent run
groups and are never superseded. The small Rust `bonsai-ci` planner and gate
regression checks run before selection on every run. `tools/ci/` owns workspace discovery, successful-ancestor
GitHub API lookup, conservative diff selection and aggregate gate enforcement.
It reads `.github/ci/impact.json` without running Cargo metadata or resolving
registry dependencies.
The planner caches its exact Linux compiler and hands its tested executable to
the final gate as a current-run artifact, so aggregation needs no compiler install
or second Cargo invocation. The final gate requires the independent maintenance
and speech jobs as well as broker, standards and script groups when selected.

The workspace and [package registry](../config/rust-packages.psd1) contain only
these app packages. Upstream source/backend catalogs, baseline policy, codec
corpora, overlays and compiler research are absent from this gate. They run in
Prism's [runtime conformance](https://github.com/symbiogenesis/prism-inference/blob/main/.github/workflows/runtime-conformance.yml),
[Foundation](https://github.com/symbiogenesis/prism-inference/blob/main/.github/workflows/foundation.yml),
[standards](https://github.com/symbiogenesis/prism-inference/blob/main/.github/workflows/standards.yml)
and [compiler](https://github.com/symbiogenesis/prism-inference/blob/main/.github/workflows/rust-nightly.yml)
workflows. Focused upstream diagnostics also run there.

The optional [LoRA device workflow](../.github/workflows/swift-lora-qualification.yml)
remains here because it qualifies the application's selected model/profile. It
requires the owner-managed runner, reviewed artifacts and explicit dispatch;
hosted app checks do not establish throughput or creative quality.
Its standalone tool build remains separate from Foundation's artifact handoff.

## Standards

Rust uses the exact [toolchain pin](../rust-toolchain.toml), edition 2024, checked
Release arithmetic, warnings-as-errors and strict Clippy/documentation lints.
Safe package roots forbid unsafe code; individually documented Win32 operations
remain confined to the Windows boundary crates. The maintenance CLI reuses those
boundaries. Scoped allowances retain their reasons and safety documentation.

The shared Rust `doc` phase emits JSON for selected packages using
`cargo doc --no-deps -Z unstable-options --output-format json` on the pinned
nightly. This applies to local `Test-Rust.ps1` checks and every Foundation Rust
lane through the same command planner. It preserves explicit package/features,
Release settings and warnings-as-errors, including missing documentation,
intra-doc links, markup and Rust code-block lints. JSON generation writes no
rustdoc HTML or JavaScript assets. It replaces the HTML documentation check
that triggered Defender's quarantine of a generated `trait.Send.js`; this does
not establish whether that detection was a false positive. Existing cached
HTML is not removed, and machine-wide Cargo settings are not changed. The
[JSON output option](https://doc.rust-lang.org/nightly/cargo/commands/cargo-doc.html#option-cargo-doc---output-format)
is unstable and must be rechecked when changing the pinned toolchain. Ordinary
validation does not build browsable documentation or run doctests; complete
test execution retains its separate CI guard.

PowerShell 7.6+ scripts use latest strict mode, terminating native-command errors,
advanced functions and typed output contracts. PSScriptAnalyzer 1.25.0 rejects
unsuppressed diagnostics. The .NET SDK is pinned in [global.json](../global.json);
Release C# analysis checks every embedded Add-Type helper with nullable checking,
checked arithmetic and unsafe compilation disabled.

## Focused local checks

Keep scripts, logs, captures and native working directories in a unique system
temp directory outside Git. Use expanded absolute paths, preserve inherited
Windows/profile environment variables, and select the compiler explicitly.

```powershell
$tooling = Join-Path ([IO.Path]::GetTempPath()) ('Runner-Tooling-' + [guid]::NewGuid().ToString('N'))
Save-Module PSScriptAnalyzer -RequiredVersion 1.25.0 -Repository PSGallery -Path $tooling -Force
./tests/Test-Standards.ps1 -AnalyzerModulePath (Join-Path $tooling 'PSScriptAnalyzer/1.25.0/PSScriptAnalyzer.psd1')
```

For a focused package analyzer check:

```powershell
$scratch = Join-Path ([IO.Path]::GetTempPath()) ('Runner-Checks-' + [guid]::NewGuid().ToString('N'))
./tests/Test-Rust.ps1 -Package bonsai-broker -TargetDirectory (Join-Path $scratch 'target') -EvidenceDirectory (Join-Path $scratch 'evidence')
```

Select a regression relevant to the change, such as
`./tests/Test-Foundation.ps1 -Case Download` or
`./tests/Test-NativeProcess.ps1 -Case Capture`. Direct Cargo tests should name one
regression and use an external target/working directory. `-FullTests` is reserved
for main-branch GitHub Actions; benchmark execution is always CI-only.

The chat UI's focused `window::interaction_tests::` selection exercises actual
egui input and accessibility with inert model channels. It covers Enter versus
Shift+Enter and selection across streaming appends without starting a broker,
speech runtime or microphone. Ordinary runs require no GPU. For visual review,
set `BONSAI_UI_CAPTURE_DIRECTORY` to an expanded absolute directory outside Git
and select `window::interaction_tests::capture_chat_layouts_when_requested`;
that case renders wide, narrow and empty layouts to PNG using the test renderer.
The capture set also includes maximum-length drafts in wide and minimum-size
windows.
These captures do not establish native screen-reader, model or speech behavior.

Memory-storage regressions use untimed ownership and encoded-byte checks:
`request_buffer::tests::` and `backend::memory_tests::` in the broker;
`backend::payload_tests::` additionally checks final-destination source reads,
bytewise and larger fragments, coalesced prefixes, chunk boundaries, empty reads
and sticky truncation/I/O teardown. Large eligible payload reads bypass the
decoder's 8 KiB staging buffer; framing tails retain bounded buffered copies.
These pointer/byte checks do not measure X2 throughput or process peak memory.
`response::memory_tests::` additionally checks direct final-storage JSON reads,
uninitialized spare tails, bounded growth, exact-limit probing, framing errors,
deadlines and cancellation ownership;
`bounded_json::tests::`,
`client::tests::committed_and_pending_history_share_payloads_without_sharing_turn_mutations`
and
`window::persistence::tests::recovery_bounds_escaped_json_before_retaining_an_oversized_workspace`
in the UI. The TTS
`build_overlay::tests::generated_decode_workspace_eviction_releases_only_completed_graphs`
case compiles the actual generated eviction block with the provisioned native
compiler and checks graph lifetime, cache hits, window/batch changes, allocation
failure and teardown against model-free allocations. Overlay checks also reject
changed source anchors and preserve the immutable source cache. These are focused
correctness checks; they do not measure model peak memory or rebuild latency.

Recovery admission checks include
`recovery::tests::recovery_reserves_snapshot_before_encoding_through_worker_completion`
and
`window::persistence::tests::recovery_backpressure_skips_encoding_until_the_snapshot_slot_is_free`.
They hold a synthetic worker save open and verify that another snapshot is not
encoded. Startup capture checks in `startup_process::tests::` cover UTF-8
allocation ownership, unchanged lossy decoding and exact stdout/stderr log order.
Speech intake checks include
`http::intake_tests::memory_body_intake_reuses_headers_and_preserves_coalesced_audio`
and the `request::tests::memory_multipart_` cases for fixed field storage and
excess-field rejection before decoding.
The `fragmented_headers_scan_each_window_once_and_preserve_coalesced_payload`
native HTTP case and companion `fragmented_headers_scan_each_window_once_and_preserve_coalesced_audio`
case count delimiter comparisons under bytewise and larger fragments. The
`fragmented_response_headers_keep_exact_limit_framing_and_coalesced_body` and
`fragmented_request_headers_keep_exact_limit_framing_and_coalesced_audio` cases
parse exact-limit headers across every delimiter split and preserve body storage.
These establish untimed framing/work behavior; device latency remains unqualified.
The UI's
`canary::tests::multipart_upload_borrows_pcm_and_preserves_bytes_under_partial_writes`
case checks borrowed PCM identity, canonical/extensible WAV byte equality,
collision-free multipart boundaries, partial writes, maximum clip admission and
cancelled writes. The authenticated transcription case also checks exact upload
bytes. These checks do not load Canary or measure transcription latency.
The `canary::tests::response_body_` cases preserve the header allocation and
coalesced payload, check direct final-storage reads under partial delivery at
the JSON/WAV limits, and reject excess, truncated or cancelled input. The
authenticated transcription and strict HTTP-framing cases exercise the shared
adapter contract. These checks do not measure response latency or peak memory.
These are untimed storage/lifetime checks. They do not qualify the separately
owner-selected 15.5 GiB admission/11.5 GiB Job development budget; its total
model-memory acceptance still requires complete-workload target-device evidence.

The focused `window::transcript::cache_tests::` and
`window::transcript::search::performance_tests::` cases count layout/search work
directly to guard completed-message reuse during streaming and find navigation.
They are correctness regressions, with no elapsed-time or throughput claim. The
`highlighted_layout_cache_keeps_constant_size_keys_during_streaming_and_navigation`
and `highlight_identity_tracks_same_length_queries_replacement_and_result_eviction`
cases check fixed-size highlight identities at 10,000 results, unchanged-layout
range visits, navigation, same-length query/body changes, clearing/reopening Find
and results exposed by eviction. Layout caches retain no duplicate range vectors.
The
`streamed_find_scans_only_appended_bytes_and_keeps_one_continuation` case checks
linear appended-byte work and tail-only scratch. The Unicode append-boundary
oracle, replacement and saturated-lookahead cases preserve matching behavior
across streamed updates without rescanning an unchanged prefix. The
`streamed_find_appends_only_new_results_without_rebuilding_completed_caches`,
`streamed_find_saturated_prefix_skips_tail_cache_and_result_work` and
`streamed_find_exact_limit_preserves_results_when_lookahead_becomes_limited`
cases additionally count copied results and rebuilt cache entries, preserve
selection/navigation during appends and check saturation followed by eviction.
The
`bounded_find_matches_folded_reference_across_expansion_and_prefix_boundaries`
case compares the bounded matcher with the prior folded-buffer behavior;
`bounded_find_scratch_depends_on_query_bytes_and_retains_the_result_lookahead`
checks query-sized scratch and result limits over a long repeated prefix.
The `client::memory_tests::` cases assert that owned request bytes survive partial
writes and flush, are released before the first response read, and are released
on send failures. They also preserve exact wire bytes and successful terminal SSE
and HTTP framing. The `canary::pcm::tests::pcm_framing_reuses_one_buffer_without_byte_sized_source_reads`
and `pcm_ready_prefix_arrives_before_remaining_chunk_or_framing` cases check source
read counts and gate payload delivery without timing. Companion cases cover odd
read fragmentation, buffered cancellation, truncation and terminal validation.
These checks do not establish actual-model latency or process peak memory. The
`window::transcript::search::shortcuts::tests::` selection covers Find keyboard
navigation, IME handling and Escape precedence without model or speech services.
Transcript accounting cases also check exact byte totals across streaming,
replacement, eviction and recovery, with no completed-message visits during
ordinary append accounting. The `transcript_accounting_caps_empty_messages_before_bytes_require_eviction`,
`transcript_accounting_count_eviction_preserves_find_and_recovery` and
`transcript_accounting_count_and_byte_eviction_remove_one_combined_prefix`
cases enforce the shared 512-message live/restore bound, retain Find selection
and byte accounting, and remove one combined count/byte prefix. They do not
exercise an actual long-conversation application session.
The `client::tests::http_payload_` cases gate the
underlying reader to prove that ready SSE payload is delivered before the rest
of its declared HTTP body or chunk ending arrives, while incomplete framing
still rejects. The `speech::command_queue::tests::` and
`speech::stream::tests::` selections check stalled-worker queue limits,
coalescing, overload, cancellation, fresh-stream recovery and bounded tail
compaction without loading a speech model or timing synthesis.
The focused
`speech::tests::cold_speech_worker_waits_for_commands_and_wakes_for_stop_and_shutdown`,
`speech::tests::speech_command_wait_preserves_monitoring_for_warm_active_and_retained_input`
and
`speech::command_queue::tests::blocking_speech_receiver_wakes_when_the_sender_disconnects`
cases observe the actual queue wait policy and synchronize command/disconnect
wakeups. They retain periodic monitoring while resources are owned and establish
no elapsed-time, idle CPU or energy result.
The `window::persistence::tests::recovery_rejects_excess_` cases accept the exact
conversation/history/transcript count limits, reject an extra object before
decoding its fields and preserve the live workspace on failure.
The `speech::pipeline::tests::pcm_` cases check packet-allocation reuse,
segment-tail framing, bounded spare capacity and nonblocking recycling. The existing
`tiny_pcm_callbacks_form_fixed_frames_and_flush_the_final_tail_exactly` and
synthetic output cases retain byte identity and bounded playback behavior.
The `speech::pipeline::tests::playback_` cases check stable descriptor allocation
reuse, cleared packed WinMM fields, the eight-descriptor bound and retained
header/PCM identity when unprepare refuses release. Fake release callbacks
exercise the production ownership helper, including teardown quarantine; they
do not invoke WinMM or qualify an actual audio driver.
These checks do not measure target-device peak memory or speech/chat overlap
latency, and they do not exercise a physical audio driver.

For prebuilt runtime promotion tooling, use
`./tests/Test-Inference.ps1 -Case ReleaseLock`. It checks candidate generation
and rejection with synthetic bundles without loading a model. The normal
Foundation `-Case All` invocation includes that case. Follow the
[runtime lock update procedure](development-guide.md#prepare-a-runtime-lock-update)
for independent source/run review and real checked artifacts; generating a
candidate alone does not promote the compiled runtime pin.

## Acceptance limits

The TTS `philox::tests::` known-answer block and f32 rounding cases, together
with `native::philox_tests::rust_philox_scalar_abi_matches_pinned_cpp_bits`,
qualify the safe Rust scalar generator against the unmodified pinned Qwen
source across 4,411 deterministic edge/corpus cases. The comparison invokes the
Rust C ABI export without loading a model. The build-overlay regression
`rust_philox_overlay_removes_native_algorithm_and_rejects_source_drift` checks
both sampler call replacements and complete native algorithm removal. Run the
oracle with the provisioned `BONSAI_TTS_COMPILER` and native compiler environment,
as with the existing decode-cache fixture. Actual-model speech behavior and
performance remain separate CI qualification; see the
[Rust sampler port](../src/speech-tts/README.md#rust-sampler-port).

The native ARM64 ISA regression
`native::isa::tests::native_isa_queries_neon_dotprod_and_neon_i8mm_independently`
checks exact Windows NEON/dot-product/NEON-I8MM feature IDs with an injected
query. The maintenance case
`cpu_evaluation::conditions::tests::condition_comparison_rejects_known_drift_and_unknowns_without_treating_charge_as_energy`
checks screen comparison eligibility across topology/power changes and missing
observations. The X2 screen additionally retains CPU/power snapshots before and
after each owned sample, outside process wall time. Its pair comparison rejects
changed boundary observations for either sample or between the pair, even
when the full batch returns to its initial power mode. Missing or malformed
sample observations suppress ratios. These are boundary checks, with no
inside-sample monitoring or energy result. The model-free legacy completion
headless-start check retained in the X2 record failed DLL initialization;
inspection-only version/help success does not qualify normal worker startup.
These model-free checks neither execute optional kernels nor
qualify performance; native ISA observations and device screen status are
retained in [the X2 tuning record](x2-cpu-optimization.md).

The main-only [native sandbox qualification](../.github/workflows/sandbox-qualification.yml)
workflow runs the explicitly ignored PSEC fixtures on hosted ARM64 or the selected
owner-managed host. It records the revision, SDK and OS identities and retains
synthetic technical logs. It checks file/temp authority, external network denial,
the broader host-loopback scope, real process limits, artifact/key validation and
abrupt-owner cleanup. Ordinary Foundation suites do not execute these ignored
native cases. The workflow loads no model and runs no benchmark.

Synthetic fixtures establish app policy, parser, artifact and process contracts.
Native fixtures establish their exact tested Windows capability behavior; they
do not establish complete actual-model/daily acceptance, broker-only/same-user
bypass prevention, quality, throughput or sustained comfort. Bounded source-runtime
checks do not qualify the provisioned release; runtime promotion requires the
separate reviewed release and lock-update procedure.
