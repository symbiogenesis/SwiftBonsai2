# Validation

Use the [development guide](development-guide.md) for setup and the [plan](../PLAN.md)
for pending qualification. Local builds, Release analyzers and focused correctness
checks are allowed. Complete suites and every benchmark run only in CI; do not
approximate a complete suite by batching local filters.

## Language standards

| Language | Enforced policy |
| --- | --- |
| Rust 1.99.1 / edition 2024 | Warnings, missing API documentation, unchecked casts, production unwrap/expect/panic and Clippy all/pedantic/nursery/cargo findings are rejected. Release arithmetic overflow checks are enabled; Release debug assertions remain disabled for the constant-time dependency's contract. |
| PowerShell 7.6+ | Latest strict mode, terminating errors including native-command failures, typed parameters, advanced functions and explicit output contracts. PSScriptAnalyzer 1.25.0 defaults plus selected safety rules reject every unsuppressed diagnostic. |
| C# 14 / .NET 10 | Nullable checking, checked arithmetic, unsafe compilation disabled and warnings as errors for every Add-Type invocation. A separate Release build checks .NET analyzers, internal helpers, XML API documentation and selected style diagnostics. |

Rust is the default for repository-owned systems components. The broker, codecs,
GGUF tools, maintenance tool and native CPU reference forbid unsafe code. Unsafe
operations are confined to documented Win32 boundaries in `bonsai-windows-process`
and `bonsai-windows-credentials`, plus Prism's C ABI/loader and native test bindings.
The sharing policy remains safe Rust; the external Prism/llama.cpp engine remains
C/C++. See the [Prism boundary](../tools/prism/README.md#native-boundary).

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
./scripts/Test-Standards.ps1 -AnalyzerModulePath (Join-Path $tooling 'PSScriptAnalyzer/1.25.0/PSScriptAnalyzer.psd1')
```

The checker extracts embedded C# helpers and keeps outputs/logs in system temp.
For registered Rust formatting, Release Clippy and documentation checks:

```powershell
$scratch = Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-Checks-' + [guid]::NewGuid().ToString('N'))
./scripts/Test-Rust.ps1 -Package bonsai-broker -TargetDirectory (Join-Path $scratch 'target') -EvidenceDirectory (Join-Path $scratch 'evidence')
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
    cargo +1.99.1-aarch64-pc-windows-msvc test --locked --offline --release --manifest-path (Join-Path $repo 'src/broker/Cargo.toml') --target-dir (Join-Path $scratch 'target') --test http_reception wire_duplicates_and_framing_cannot_be_normalized_away -- --exact
    if ($LASTEXITCODE) { throw 'Focused broker regression failed.' }
} finally {
    [Environment]::CurrentDirectory = $previous
    Pop-Location
}
```

The [mutation checker](../scripts/Test-Mutations.ps1) validates six codec guards
in an external temporary copy. Each original assertion must pass; its mutant
must compile and fail that same assertion. Compiler errors, unrelated failures,
timeouts and surviving mutants fail the check. A focused local case is:

```powershell
$evidence = Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-Mutations-' + [guid]::NewGuid().ToString('N'))
./scripts/Test-Mutations.ps1 -Case ZeroScale -EvidenceDirectory $evidence
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
| GGUF and ternary | Bounded metadata/census/export, exact codec bytes and binary16 conversions, immutable C++ fixture parity and all six guard mutations. |
| Native CPU reference | Formatting, Release Clippy/rustdoc, complete synthetic correctness suite, token-ID driver, packed SIMD/scalar parity, no-CLI/feature-disabled builds and non-ARM scalar fallback. |
| Prism overlay | Source drift/repeat rejection, Rust policy/FFI checks, native allocator/math regression and actual engine linkage on Linux ARM64. |

Foundation downloads runtime/source packages but no model weights and runs no
inference benchmark. Caches isolate compiler, runner image, architecture and
package/native-linking lanes; every check still runs after cache restoration.
Scratch/evidence remains external. Select the published maintenance executable
from the exact evaluated main revision via `BONSAI_TOOLS_PATH` or `-ToolPath`;
version/protocol checking is not source-to-binary attestation.

The manual main-only [nightly trial](../.github/workflows/rust-nightly.yml) compares
stable with the nightly pinned in [configuration](../config/rust-nightly-trial.json).
Four compiler/package-group lanes run formatting, Release analyzers/documentation
and full correctness suites; separate probes isolate trait-solver/Polonius modes.
Prism loader APIs compile there, while native linking remains in Foundation.
The final `compatibility` check requires all lanes and probes, including retained
receipts. Stable remains the production baseline. See the [trial guide](../tools/rust-trial/README.md)
for focused local comparisons.

## Acceptance limits

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
