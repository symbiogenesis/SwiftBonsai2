# Validation

Runner CI owns this application's native Rust UI, broker, process ownership,
artifact provisioning and released-runtime integration. Runtime algorithms and upstream conformance
belong to [Prism Inference](https://github.com/symbiogenesis/prism-inference/blob/main/docs/build-and-validation.md).
Local builds, Release analyzers and focused named tests are allowed. Complete
suites and every benchmark run only in CI; do not approximate a full suite by
batching local filters.

The manual [TTS evaluation](../.github/workflows/tts-evaluation.yml) compares the
pinned Q8_0 and Q4_K_M VoiceDesign profiles with several CPU thread budgets.
It records the tested revision, CPU identity, cold ready time, first audio,
warm synthesis and real-time factor, retaining synthetic WAV clips for listening.
Select the owner-managed Snapdragon runner for device-specific conclusions;
hosted ARM64 results describe only that hosted CPU. A successful run does not
establish equivalent voice quality: the report leaves human listening pending.
All evaluation execution, including a single profile or short fixture, remains
CI-only. Invoke the workflow on `main`; its script rejects local execution.

The [UI architecture](ui-architecture.md) requires first-party UI application
code to remain pure Rust and native. Native UI builds and focused client checks
must preserve the broker's documented HTTP/JSON/SSE subset. Window launch and
streaming checks do not establish broad SDK compatibility, model quality or
runtime performance. Browser assets and browser-runtime UI dependencies are not
an accepted implementation path.

## Runner gates

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
Consumers verify its revision, digest and ARM64 identity. The standards job alone
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

The focused `window::transcript::cache_tests::` and
`window::transcript::search::performance_tests::` cases count layout/search work
directly to guard completed-message reuse during streaming and find navigation.
They are correctness regressions, with no elapsed-time or throughput claim. The
`window::transcript::search::shortcuts::tests::` selection covers Find keyboard
navigation, IME handling and Escape precedence without model or speech services.

For prebuilt runtime promotion tooling, use
`./tests/Test-Inference.ps1 -Case ReleaseLock`. It checks candidate generation
and rejection with synthetic bundles without loading a model. The normal
Foundation `-Case All` invocation includes that case. Follow the
[runtime lock update procedure](development-guide.md#prepare-a-runtime-lock-update)
for independent source/run review and real checked artifacts; generating a
candidate alone does not promote the compiled runtime pin.

## Acceptance limits

Synthetic fixtures establish app policy, parser, artifact and process contracts.
They do not establish daily filesystem/egress isolation, same-user backend bypass
prevention, quality, throughput or sustained comfort. Bounded source-runtime
checks do not qualify the provisioned release; runtime promotion requires the
separate reviewed release and lock-update procedure.
