# Validation

Runner CI owns this application's native Rust UI, broker, process ownership,
artifact provisioning and released-runtime integration. Runtime algorithms and upstream conformance
belong to [Prism Inference](https://github.com/symbiogenesis/prism-inference/blob/main/docs/build-and-validation.md).
Local builds, Release analyzers and focused named tests are allowed. Complete
suites and every benchmark run only in CI; do not approximate a full suite by
batching local filters.

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
Snapdragon X Elite runner and settled `main`. It requires already provisioned
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
These are untimed storage/lifetime checks. They do not qualify the separately
owner-selected 15.5 GiB admission/11.5 GiB Job development budget; its total
model-memory acceptance still requires complete-workload target-device evidence.

The focused `window::transcript::cache_tests::` and
`window::transcript::search::performance_tests::` cases count layout/search work
directly to guard completed-message reuse during streaming and find navigation.
They are correctness regressions, with no elapsed-time or throughput claim. The
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
ordinary append accounting. The `client::tests::http_payload_` cases gate the
underlying reader to prove that ready SSE payload is delivered before the rest
of its declared HTTP body or chunk ending arrives, while incomplete framing
still rejects. The `speech::command_queue::tests::` and
`speech::stream::tests::` selections check stalled-worker queue limits,
coalescing, overload, cancellation, fresh-stream recovery and bounded tail
compaction without loading a speech model or timing synthesis.

For prebuilt runtime promotion tooling, use
`./tests/Test-Inference.ps1 -Case ReleaseLock`. It checks candidate generation
and rejection with synthetic bundles without loading a model. The normal
Foundation `-Case All` invocation includes that case. Follow the
[runtime lock update procedure](development-guide.md#prepare-a-runtime-lock-update)
for independent source/run review and real checked artifacts; generating a
candidate alone does not promote the compiled runtime pin.

## Acceptance limits

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
