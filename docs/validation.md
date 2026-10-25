# Validation

Use the [development guide](development-guide.md) for setup and the [plan](../PLAN.md)
for pending qualification. Local builds, Release analyzers and focused correctness
checks are allowed. Complete suites and every benchmark run only in CI; do not
approximate a complete suite by batching local filters.

## Language standards

| Language | Enforced policy |
| --- | --- |
| Rust nightly-2026-10-22 / edition 2024 | The next trait solver and Polonius Alpha are enabled by this compiler's defaults. Warnings, missing API documentation, unchecked casts, production unwrap/expect/panic and Clippy all/pedantic/nursery/cargo findings are rejected. Release arithmetic overflow checks are enabled; Release debug assertions remain disabled for the constant-time dependency's contract. |
| PowerShell 7.6+ | Latest strict mode, terminating errors including native-command failures, typed parameters, advanced functions and explicit output contracts. PSScriptAnalyzer 1.25.0 defaults plus selected safety rules reject every unsuppressed diagnostic. |
| C# 14 / .NET 10 | Nullable checking, checked arithmetic, unsafe compilation disabled and warnings as errors for every Add-Type invocation. A separate Release build checks .NET analyzers, internal helpers, XML API documentation and selected style diagnostics. |

The nightly pin is temporary. Move to beta at the earliest release that promotes
both the next trait solver and Polonius, then to stable as soon as both reach
stable. Verify the borrowing/solver fixtures, alias-rejection control and actual
broker/runtime checks before switching; update the compiler pin, scripts and CI
together. Continued use of nightly requires an actual remaining compiler need.
Track the [next-solver stabilization](https://github.com/rust-lang/rust/pull/164209)
and [Polonius promotion](https://github.com/rust-lang/goals/issues/118) as upgrade signals.

Rust is the default for repository-owned systems components. The broker, artifact verification, CPU policy, codecs,
GGUF tools, maintenance tool and native CPU reference forbid unsafe code. Unsafe
operations are confined to documented Win32 boundaries in `bonsai-windows-process`
and `bonsai-windows-credentials`, plus Prism's C ABI/loader and native test bindings.
The sharing policy remains safe Rust; the external Prism/llama.cpp engine remains
C/C++. The separate [released codec probe](../tools/codec-probe/README.md) confines
dynamic DLL loading and codec calls to a reviewed native boundary. The C# build
now checks only the embedded HTTP transport test helper. See the
[Prism boundary](../tools/prism/README.md#native-boundary).

Scoped unsafe allowances do not waive checks for unsafe operations in unsafe
functions, invalid reference casts or mutable-static references. Unwinding FFI
calls are rejected. Each unsafe operation needs its own documented block, and
every lint allowance needs a reason. These checks support review; they do not
prove the external engine memory-safe.

The root Cargo workspace shares exact dependency pins, package metadata, lint
policy and arithmetic profiles. [The package registry](../config/rust-packages.psd1)
drives Foundation and compiler compatibility checks with explicit feature/native
linking selections. `rustfmt.toml` selects the 2024 language and formatting editions.
The .NET SDK baseline in `global.json` is 10.0.402 with patch roll-forward within
10.0.4xx. Analyzer exceptions are documented at their scope; the legacy PowerShell
UTF-8 BOM rule and obsolete API profiles do not apply to this baseline.

The external engine builder requires C11/C++17, MSVC conformance mode and SDL
checks. It retains upstream warning policy and does not meet the first-party
warning standard: an upstream MSVC C4244 remains in `llama-graph.cpp`. Changed
source-build configuration requires a fresh lane or reviewed removal of the old
owned build directory.

Validation entry points named `Test-*` live under `tests/` alongside the
regressions; reusable operational modules remain under `scripts/`.

## Cargo test dependency review

The October 24, 2026 review checked all 12 workspace package manifests, test and
example imports, enabled features and locked registry packages against crates.io.
All direct third-party dependencies use exact pins. Production dependencies also
support unit/integration tests; the direct test-specific choices are:

| Dependency | Decision and purpose |
| --- | --- |
| `tokio` 1.54.1 | Keep the current release and `test-util` for deterministic paused-clock tests. Its feature already enables `rt`; the duplicate dev feature was removed. |
| `serde_json` 1.0.154 | Keep the current release for protocol assertions, fixtures and retained upstream reports. |
| `sha2` 0.11.1 | Keep the current release, with default features disabled, for immutable fixture identities. |
| `zip` 9.1.0 | Upgrade from 9.0.5 and share one workspace declaration between the artifact verifier and maintenance test fixtures. Keep only `deflate-flate2-zlib-rs`, with defaults disabled. |
| GGUF scratch directories | Use a small standard-library owner with exclusive directory creation and cleanup on early returns. This removes the direct `tempfile` test dependency while preserving explicit cleanup errors after successful tests. Optional native binding generation can still require `tempfile` transitively. |
| `cbindgen` 0.29.4 | Keep the current optional build dependency for native Prism test declarations, with its unused CLI defaults disabled. |

The remaining direct dependencies (`serde` 1.0.229, `http` 1.5.0, `httparse`
1.10.1, `subtle` 2.6.1, `windows-sys` 0.61.2 and `fearless_simd` 1.1.0) were
already at their latest stable releases. Keep standard assertions and explicit
process supervision: replacing them with a test framework adds no demonstrated
coverage or reliability benefit. Preserve intentionally retained native/bridge
evidence and Windows handle-release fixtures.

The ZIP update also advances `typed-path` from 0.12.4 to 0.13.0. Its newly locked
WebAssembly support packages are target-specific dependencies, not enabled ZIP
compression features or Windows build requirements. Older major lines of `syn`,
`toml`, `toml_datetime`, `winnow`, `r-efi`, `wasi` and `windows-link` remain
constrained by their parents (`cbindgen`, `toml`, `getrandom`, `mio` and
`windows-sys`). Do not force incompatible versions with patches. Re-resolve the
lockfile when the parents support those versions. The all-features graph contains
70 registry packages; the [OSV database](https://osv.dev/) returned no advisories
for those exact versions on the review date. Every package declares a license.

Release and API references: [ZIP 9.1.0 changelog](https://github.com/zip-rs/zip2/blob/v9.1.0/CHANGELOG.md),
[Tokio features](https://docs.rs/crate/tokio/1.54.1/features) and
[TempDir cleanup](https://docs.rs/tempfile/3.27.0/tempfile/struct.TempDir.html).
Local validation should exercise deflated archive verification, GGUF export and
paused-clock scheduling, plus a named [upstream C++ adapter](prism-upstream-tests.md).
Complete suites remain the main-branch Foundation workflow's responsibility.

## Focused local checks

Run from native Windows ARM64 PowerShell 7.6+. Keep scratch scripts, logs, captures
and native working directories in a unique directory under system temp, outside
all checkouts. Use absolute paths, preserve inherited Windows/profile environment
variables, and select the Rust toolchain explicitly when running outside Git.

For PowerShell/C# standards, use an existing PSScriptAnalyzer 1.25.0 installation
or save an isolated copy:

```powershell
$tooling = Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-Tooling-' + [guid]::NewGuid().ToString('N'))
Save-Module PSScriptAnalyzer -RequiredVersion 1.25.0 -Repository PSGallery -Path $tooling -Force
./tests/Test-Standards.ps1 -AnalyzerModulePath (Join-Path $tooling 'PSScriptAnalyzer/1.25.0/PSScriptAnalyzer.psd1')
```

The checker extracts embedded C# helpers and keeps outputs/logs in system temp.
For registered Rust formatting, Release Clippy and documentation checks:

```powershell
$scratch = Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-Checks-' + [guid]::NewGuid().ToString('N'))
./tests/Test-Rust.ps1 -Package bonsai-broker -TargetDirectory (Join-Path $scratch 'target') -EvidenceDirectory (Join-Path $scratch 'evidence')
```

Select a regression relevant to the change, for example:

```powershell
./tests/Test-Foundation.ps1 -Case Download
./tests/Test-NativeProcess.ps1 -Case Capture
```

For a direct Cargo regression, resolve `$repo` from the checkout first:

```powershell
$repo = (Get-Location).Path
$scratch = Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-Rust-' + [guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($scratch)
$previous = [Environment]::CurrentDirectory
Push-Location $scratch
try {
    [Environment]::CurrentDirectory = $scratch
    cargo +nightly-2026-10-22-aarch64-pc-windows-msvc test --locked --offline --release --manifest-path (Join-Path $repo 'src/broker/Cargo.toml') --target-dir (Join-Path $scratch 'target') --test http_reception wire_duplicates_and_framing_cannot_be_normalized_away -- --exact
    if ($LASTEXITCODE) { throw 'Focused broker regression failed.' }
} finally {
    [Environment]::CurrentDirectory = $previous
    Pop-Location
}
```

The [mutation checker](../tests/Test-Mutations.ps1) validates six codec guards
in an external temporary copy. Each original assertion must pass; its mutant
must compile and fail that same assertion. Compiler errors, unrelated failures,
timeouts and surviving mutants fail the check. A focused local case is:

```powershell
$evidence = Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-Mutations-' + [guid]::NewGuid().ToString('N'))
./tests/Test-Mutations.ps1 -Case ZeroScale -EvidenceDirectory $evidence
```

Receipts retain source hashes and bounded compile/test logs. This measures
sensitivity to the six listed faults, not whole-program mutation coverage.

## CI coverage

The main-only [Foundation workflow](../.github/workflows/foundation.yml) runs on
push and manual dispatch. Its aggregate `foundation` check requires every job to
succeed, including artifact retention; skipped jobs are rejected.

| Area | Coverage |
| --- | --- |
| Standards and scripts | PowerShell/C# analyzers, configuration/path policy, provisioning recovery and exclusive hard-link publication, native invocation, engine build policy and installed runtime/help verification. |
| Broker and Windows boundaries | Strict admission/framing, exact-count context bounds, response limits, deterministic scheduling/fragmentation/cancellation, synthetic loopback service, protected credentials and Job lifecycle/abrupt-parent-death regressions. |
| GGUF and ternary | Bounded metadata/census/export, independent base-3 expected bytes, binary16 conversions, immutable C++ fixture parity, released native DLL FP32 parity and all six guard mutations. |
| Native CPU reference | Formatting, Release Clippy/rustdoc, complete synthetic correctness suite, token-ID driver, packed SIMD/scalar parity, no-CLI/feature-disabled builds and non-ARM scalar fallback. |
| Prism overlay | Source drift/repeat rejection, Rust policy/FFI checks, native allocator/math regression and actual engine linkage on Linux ARM64. |
| Upstream Prism baseline | Verified 71-source inventory, safe Rust inputs/references/assertions targeting production Prism and Rust, retained unported C++ controls, per-source outcomes and a required nonregression floor. Native Windows ARM64 acceptance targets Snapdragon X Elite; full C++ retirement awaits the remaining applicable ports. See the [baseline guide](prism-upstream-tests.md). |

Foundation downloads runtime/source packages but no model weights and runs no
inference benchmark. Caches isolate compiler, runner image, architecture and
package/native-linking lanes; every check still runs after cache restoration.
Rust checks batch selected packages by their effective feature policy, rather
than starting Cargo separately for every package and phase. Formatting covers
exactly the selected packages in one invocation. Restricted Prism feature modes
remain separate from all-feature packages because Cargo unifies selected
package features. A full all-package selection uses seven Cargo invocations
instead of 48; a Broker selection uses four instead of 24. Batched tests keep
`--no-fail-fast` so a failing test binary does not prevent the remaining selected
test binaries from running. Schema-2 receipts record each batch's packages,
phase, elapsed time and result, including failures.
Broker and maintenance packages run in parallel lanes, each with its own build
cache and evidence. Both selections come from the package registry, and the
aggregate gate requires both lanes. The x86-64 scalar fallback also reuses its
own dependency/build cache while repeating every feature and correctness check.
Foundation's script regressions run in two parallel lanes with the same required
aggregate gate. In hosted Foundation and nightly checks, the pinned Cargo uses
[content fingerprints](https://doc.rust-lang.org/nightly/cargo/reference/unstable.html#checksum-freshness)
instead of checkout timestamps to validate restored Rust
artifacts. Changed source still rebuilds; every analyzer and test command still
runs. Build-script inputs retain Cargo's normal timestamp checks. The first run
populates the new fingerprint caches; subsequent runs can reuse unchanged builds.
Hosted jobs also cache the exact pinned Rust toolchain directory, isolated by
runner image, architecture and required components. Every restore verifies the
compiler host, sysroot and installed components; rustup settings and credentials
are excluded. Owner-managed runners keep their ordinary rustup installation path.
Prism caches the ccache installation packages for the exact Linux runner image,
avoiding repeated package-index downloads. Its overlay checks and engine build
share the cached Rust engine-loader artifacts while preparing fresh verified
source trees; native-test artifacts remain separate. All overlay rejection,
native build, analyzer and correctness checks still run. Nightly job summaries
record the runner image and exact compiler/build cache hits so cold image
transitions can be distinguished from warm runs.
Script-only jobs can reuse an exact-source packaged maintenance executable
without restoring a compiler or Cargo target tree. The cache includes a source
identity receipt; selection verifies the executable digest and ARM64 PE machine.
Jobs that compile native Rust probes still restore the compiler and build cache.
The three focused Windows lifecycle selections share that job's external Cargo
target directory instead of compiling three independent copies.

The Windows upstream Prism lane also caches native bridge build trees with an
exact source, compiler, generator and runner-image key. Each run verifies the
archive and extracts fresh evidence, then compares every cached upstream source
file's length and SHA-256 and the complete file inventory with that extraction.
Unchanged native sources retain stable paths and timestamps; changed, extra or
missing files reject reuse. CMake configuration and build still run, with cache
and configuration diagnostics copied into the fresh evidence directory. See
[CMake build trees](https://cmake.org/cmake/help/latest/manual/cmake.1.html) and
[Actions cache matching](https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching).

The complete binary32 arithmetic proof accounts for all 4,294,967,296 patterns.
It compares both rounding operations for every finite input. Integer interval
accounting classifies nonfinite patterns and the original undefined cast domain
outside the hot loop. Bounded standard-library workers distribute ranges and
reduce counts and failures deterministically. Focused tests compare interval
counts with the original floating-point oracle at boundaries and sampled ranges.
Codec mutation checks run Cargo's emitted
test executable directly after compilation; compilation failures cannot count as
killed mutants. The PTQ1 export oracle compares the entire expected byte payload,
including padding and gaps, in one native comparison. These changes add no test
framework or worker-pool dependency. All exhaustive runs remain CI-only.

Prism ordinary conformance uses four Windows ARM64 runners: randomized
tokenization, thread safety, quantization statistics with LLGuidance, and the
remaining sources. Each runner builds its selected pinned native features and
runs up to two independent adapters concurrently, starting the largest timeout
budgets first. Each adapter retains sequential C++ and Rust controls, separate
logs and bridge evidence. The final gate requires all four revision-bound
receipts and exactly one result for every assigned source on each target before
combining the backend partitions and applying the unchanged complete baseline.
Results return to catalog order. Focused regressions reject missing, duplicate,
misplaced and foreign-revision handoffs. Exhaustive workloads remain CI-only.

Randomized tokenizer references use parallel IDs-only encoding and decoding
batches, retaining the same inputs, RNG order, per-item error order, batch bounds
and source early-stop policy. Quantization statistics retain every original
16,384-element codec call and each worker's scalar accumulation order, grouping
up to 16 consecutive chunks into one bounded file transport handshake. The
default fixture uses 5,434 transport groups for 78,342 source chunks. This reduces
transport overhead without removing codec calls or numerical assertions. Action
JSON is serialized in memory before a single file write.

Backend Plan, JoinPlan and Run stages overlap the independent C++ and Rust
target chains with a limit of two workers. Each target retains its sequential
mode order, source random offsets, isolated process state and evidence paths.
Stage receipts record each concurrent chain's elapsed milliseconds. Optional benchmark
execution starts only after both correctness chains finish and stays sequential.
The interpreter writes typed scalars directly into existing storage, and input
staging serializes bounded blocks while computing their digest. Tensor snapshot
alias indexes replace repeated full-handle scans while retaining canonical IDs
and alias order. Exact-bit, bounds, digest and scheduler regressions cover these
changes; full matrices and performance measurements remain CI-only.

The production GGUF tokenizer narrows a sorted vocabulary index by input bytes
instead of scanning every token with the same first byte. Longest-match,
duplicate-token and score ties retain each tokenizer family's original rules.
Immutable byte alphabets, fallback IDs, WordPiece regexes and Unigram score
minima are prepared once per vocabulary. BPE pieces share bounded merge scratch;
BPE and SPM omit unused raw-vocabulary indexes. PLaMo scores retain lazy range rejection.
Focused validation uses all 694 original corpus predicates across 15 pinned
vocabularies; this establishes correctness, not a measured CI speedup.

Native bridge CMake inputs are mirrored into a content-verified stable directory,
so checkout timestamps do not force unchanged adapter sources to compile again.
Native cache fallback keys retain the exact source and compiler identity while
allowing orchestration changes. Fresh extraction, source verification, CMake
configuration and every correctness check still run. Evidence uses faster
compression without dropping request/response files. The pinned grammar dependency
also compiles from a stable source mirror verified against each fresh archive
extraction, keeping its Cargo inputs and CMake header include paths reusable.

Seven library or binary roots with no unit tests disable their empty unit-test
harnesses. Their ordinary builds, Clippy coverage, integration tests and doctests
remain enabled. This avoids seven unnecessary test compilations and links per
all-package check without removing assertions or adding a test runner dependency.

Upstream Prism ports own the original corpus expectations. Bundled and repository
GGUF tokenizer checks share those ports rather than repeat them in ignored
integration tests. Focused Rust schema checks live beside the upstream schema
port and share its conversion/type predicates and failure diagnostics. Stronger
batch and reasoning observations extend existing upstream cases without removing
their original assertions. Keep separate Rust API regressions for ownership,
malformed inputs, resource limits and recovery when those contracts differ from
the upstream source. Isolated-byte PTQ decode/dot checks supplement the original
upstream CUDA-host schedule with separate assertion counts. Exhaustive PTQ byte
classification owns overlapping byte examples; dedicated mutation guards remain
independently addressable.

Scratch/evidence remains external. Select the published maintenance executable
from the exact evaluated main revision via `BONSAI_TOOLS_PATH` or `-ToolPath`;
version/protocol checking is not source-to-binary attestation.

The manual main-only [nightly checks](../.github/workflows/rust-nightly.yml) use
the production nightly pinned in [configuration](../config/rust-nightly-trial.json).
Each Runtime/Broker group splits its core package and remaining registered
packages into separate lanes. All four lanes run formatting, Release
analyzers/documentation and full correctness suites; separate probes isolate
trait-solver/Polonius modes and
retain stable 1.99.1 as a control for standalone fixtures only.
Prism loader APIs compile there, while native linking remains in Foundation.
The final `compatibility` check requires all lanes and probes, including retained
receipts. The workspace requires the pinned nightly. See the [trial guide](../tools/rust-trial/README.md)
for focused local comparisons.

## Acceptance limits

Frozen pre-port PowerShell policies and full argument vectors check the shared
CPU policy and paired-evaluation invocation builder. Script regressions exercise
the packaged JSON boundary without loading models.

Synthetic archive/PE fixtures check the shared artifact verifier. Windows
regressions exercise existing writers, cancellation, installation drift, and
write/delete/rename denial until guards drop. A supervised synthetic child checks
runtime handles remain live through execution and release after exit.

Synthetic fixtures establish policy, numerical and ownership contracts. They do
not establish full-model parity, allocation counts, throughput, latency, power,
thermals, process-memory acceptance or sustained creative/comfort quality.
State-only ingestion, physical FP16 KV storage, reusable workspace and SIMD
implementation have correctness coverage; device qualification remains separate.

The manual [LoRA workflow](../.github/workflows/swift-lora-qualification.yml)
requires the owner-managed Snapdragon runner and verified model/adapter artifacts.
Its throughput and human-reviewed quality gates are independent; missing reviews,
native logits or basis evidence remain pending. Hosted Foundation cannot provide
these device verdicts. See [paired qualification](lora-qualification.md).

GPU, NPU and native Rust promotion follow their [GPU](adreno-gpu-plan.md),
[NPU](hexagon-npu-plan.md) and [native engine](native-rust-engine.md) plans.
All timing/profile captures and benchmarks remain CI-only. Backend bypass
prevention, restricted inference identity/filesystem/egress, offline UI and daily
security acceptance remain open even where development ownership checks pass.
