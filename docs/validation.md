# Validation

Runner CI owns this application's broker, process ownership, artifact provisioning
and released-runtime integration. Runtime algorithms and upstream conformance
belong to [Prism Inference](https://github.com/symbiogenesis/prism-inference/blob/main/docs/build-and-validation.md).
Local builds, Release analyzers and focused named tests are allowed. Complete
suites and every benchmark run only in CI; do not approximate a full suite by
batching local filters.

## Runner gates

[Foundation](../.github/workflows/foundation.yml) runs only on `main` pushes or
manual dispatch, skipping documentation-only pushes (root Markdown and `docs/`).
Its final `foundation` gate requires every app group to pass;
failed, cancelled or skipped groups are not accepted.

| Group | Coverage |
| --- | --- |
| Broker core and maintenance | The six registered app packages: broker, artifact verification, CPU policy, Windows process/credentials and maintenance CLI |
| Standards | Pinned PowerShell analysis, script contracts and strict embedded-C# analyzers |
| Foundation/native scripts | Download/provisioning, invocation plans, native-process wrappers and packaged supervisor contracts |
| Released-runtime integration | Pinned archive/install verification, profile/adapter/admission policy, released GGUF wrappers and server-help switching |

The maintenance lane owns Windows Job lifetime/resource-limit tests and builds
the maintenance CLI once. Script and released-runtime jobs download that run's
checked executable and verify its revision, digest and ARM64 identity. They do
not compile another copy. The standards job alone installs the .NET SDK; the
script lanes use PowerShell's bundled compiler for `Add-Type` helpers.

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
prevention, quality, throughput or sustained comfort. The separate bounded Swift
[numerical](../config/swift-native-mvp-followup.evidence.json) and
[owned HTTP](../config/swift-owned-http-mvp.evidence.json) receipts retain their
exact source/diagnostic revisions and do not update ordinary release pins.
