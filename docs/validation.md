# Validation

For setup and development commands, see the [development guide](development-guide.md).

Repository-owned Rust, PowerShell and C# enforce strict language standards in local tooling and Foundation CI:

Rust is the default for repository-owned systems components. The broker, ternary codecs,
GGUF tooling and LoRA sharing policy forbid unsafe code. The LoRA overlay and
its native regression are now Rust, with documented unsafe allowances confined
to the C ABI/native loader and test bindings. Rust owns metadata extraction,
file I/O, bounds checks, tensor view creation and loading. The pinned external
Prism/llama.cpp engine retains the native call sites; its C declarations are
generated from Rust. See the [native boundary and build](../tools/prism/README.md).
The separate `bonsai-windows-process` and `bonsai-windows-credentials` crates are
audited Win32 boundaries with documented unsafe calls, RAII ownership and strict
analyzers. The broker itself retains `forbid(unsafe_code)`. CPU/LoRA qualification
uses the packaged safe Rust maintenance tool and the same audited process
boundary. Small independent C# codec/probe references remain in tests.

Windows regressions also check explicit `SO_EXCLUSIVEADDRUSE` before listener
binding, Job crash policy, shutdown completion and evidence-file sharing.
Provisioning's focused `Download` case checks retained-handle verification,
no-overwrite hard-link publication and failure recovery. Publication requires a
hard-link-capable filesystem such as NTFS. These checks establish OS contracts,
not measured inference performance; the [Windows review](implementation-research.md#windows-integration-review-october-23)
records the rationale and tuning choices that still need device evidence.

The experimental [native Rust CPU reference](../src/prism-runtime/README.md)
forbids unsafe code and links no C/C++ inference library. Its Windows ARM64
Foundation lane checks formatting, Release Clippy/documentation, the complete
synthetic correctness suite and the token-ID driver build. It does not download
model weights or establish full-model parity, throughput, broker integration or
daily security acceptance. Local validation remains limited to focused cases;
all full suites and benchmarks run only in CI.

Focused native regressions also cover token-driver validation before model I/O,
including prompt/output capacity, positive resource bounds and bounded stop IDs.
Failed backend rollback preserves the public committed token prefix while
admission stays closed until reset. Loader fixtures retain exact signed-zero
packed scale bytes and native PQ2's fourth symbol, while rejecting nonfinite
scales and noncanonical PTQ1. The export codec keeps its stricter finite nonzero,
ternary-only conversion contract.

The CPU SIMD implementation follows the [S0–S6 plan](simd-plan.md).
Exact PQ2/PTQ1 integer regressions check all packed byte values/positions, each
lane/code including activation -128, deterministic unaligned blocks and the
immutable C++ packed-byte fixture. Matvec regressions check bitwise outputs,
scale/quantization extremes, ordered block sums, odd-group PQ2, PTQ1's independent
Q8_0 scales, scalar fallback, malformed inputs and storage recovery.
Native Release Clippy/rustdoc, CLI/no-CLI builds and assembly inspection pass.
The runtime retains `forbid(unsafe_code)`; the optional core Fearless SIMD
dependency owns its internal intrinsic boundary. Default builds automatically
select baseline NEON for supported Windows ARM64 packed operations; explicit
scalar requests, unsupported operations and feature-disabled builds use scalar.
Foundation's all-feature native lane runs the complete suite, plus explicit
feature-disabled fallback and no-CLI NEON builds; Windows x86-64 checks that the
feature safely falls back on another architecture. This does not change the
ARM64 target or use x86 emulation for the laptop's kernel.

Local builds, disassembly and focused correctness checks are allowed. All timing
benchmarks remain CI-only; promotion requires paired target-device kernel and
end-to-end evidence. S0 profiling, fuller dispatch diagnostics, broader per-path
memory evidence, optional ISA work, later floating-reduction tolerances and S6
promotion remain pending.

Future Adreno work follows the [GPU G0–G5 roadmap](adreno-gpu-plan.md): native
ARM64 capability/build evidence, exact-artifact kernel and whole-graph parity,
explicit fallback/transfer placement, shared-memory admission, and paired
target-device CI qualification. Include the approved LoRA, recurrent/KV state,
cancellation/device-loss recovery and sustained creative/comfort behavior.
Local builds and focused correctness checks are allowed; all benchmarks,
timing-profile captures and full suites remain CI-only. These gates are pending;
current builds and launch policies select CPU-only execution.

Future Hexagon work follows [M5/N0–N7](hexagon-npu-plan.md): signed native
access, exact packed arithmetic/transforms, per-operation placement, complete
KV/recurrent-state recovery, host/shared/device memory accounting and guarded
broker integration. Local builds and focused untimed correctness checks are
allowed. All NPU profiling, microkernel timing, full suites and end-to-end
benchmarks remain CI-only; promotion requires paired target-laptop evidence,
the mandatory same-backend LoRA TPS/quality gates and the project promotion
criteria. A supported-type device control does not qualify Swift. These are
pending gates; this documentation change requires no CI dispatch or push.

The native decoder's stable `evaluate_into` path reuses caller-owned logits and
retained graph/kernel workspace. Focused correctness cases check numerical
fixtures, output storage reuse, checkpoint independence, malformed outputs and
rollback after evaluation failure or cancellation. These are synthetic ownership
and correctness checks, not allocation profiling or model performance evidence.
The existing Foundation native-runtime lane includes them in its complete suite;
same-model prefill/decode allocation counts/bytes and peak-memory qualification
remain separate R4.2 work and run only in CI when requested.

State-only prompt ingestion skips the Qwen35 output head only before the final
uncached prompt token. Focused cases check hybrid continuation/restoration,
invalid tokens and state arithmetic, explicit skipped-output validation, exact
prefix hits, cancellation and rollback. Full-attention KV caches now store exact
binary16 bits physically, retaining the existing scalar products and reductions.
Packed-bit/storage, mixed-state accounting and pinned graph-logit regressions
cover snapshots and admission. These checks close the R4.3/R4.5 implementation
items; full-model parity, process-memory and performance qualification remain open.

The safe optimization pass also grows native KV storage geometrically within
the validated context bound, reuses scalar RoPE frequencies across query/key
and full-attention layers for one evaluation, and validates/selects greedy
logits in one pass. Checkpoint accounting still charges actual retained vector
capacities. PTQ unpacking uses a small immutable digit/canonicality table rather
than re-encoding the whole decoded block; exhaustive byte-position checks and
the pinned C++ fixtures guard its exact representation. GGUF inspection tracks
its logical cursor instead of querying the OS position for each scalar, while
record hashing restores the underlying file position. Broker parsing borrows
unescaped message text until canonical serialization and bounds array growth
at the existing message limit; archive verification compares sorted filenames
without cloning a second tree. Focused correctness and ownership checks cover
these changes. No measured throughput, allocation-count or device-memory
improvement is claimed; full suites and all benchmarks remain CI-only.

Ordinary build tools, GGUF commands and version/help probes share
`scripts/Bonsai.Native.psm1`. It passes literal argument arrays, reads stdout and
stderr concurrently, inherits the environment with explicit child overrides,
and terminates an active process tree during exceptional cleanup. Callers retain
their exit-code policy, output formatting and execution deadlines; GGUF execution
remains unbounded by the wrapper. Output drainage after exit is limited to ten
seconds per stream, including for build tools, so inherited pipes cannot hang a
finished command indefinitely. Timeout logs retain captured output. This helper
does not provide Windows Job ownership after a parent exits; model supervision
continues to use the separate resource-limited launcher. Consolidating this
orchestration introduces no Rust bootstrap, additional native process, inference
change or compiler-concurrency change; it makes no measured performance claim.
The focused `tests/Test-NativeProcess.ps1 -Case Capture` check exercises literal
arguments and simultaneous output; Foundation CI runs all invocation cases.

Pure completion policy and argument construction live in
`scripts/Bonsai.Completion.psm1`; CPU checks and paired LoRA evaluation share the
CPU-only argument prefix and the same pinned source/PTQ1 artifact selection.
Their distinct token limits, sampler, timing flags and adapter syntax remain at
their callers. `scripts/Bonsai.Build.psm1` owns the engine's unchanged compiler
budget and build arguments. Focused policy and scheduling tests import these
modules directly rather than extracting functions from executable script source.
Module imports run no native tools, model loads or inventory queries. These
consolidations add no native process or Rust bootstrap and retain the existing
inference arguments, memory guards and build concurrency; no timing improvement
is claimed. `tests/Test-CpuCheckPolicy.ps1 -Case Artifacts` also checks rejection
of mismatched or incomplete PTQ1 identities before model-file verification.

CPU and LoRA supervision now reuse `bonsai-windows-process` through the
[packaged maintenance tool](../tools/bonsai/README.md). The Rust helper owns
artifact verification and retained handles, the complete Job lifetime, parent
identity/liveness, memory watchdogs, deadlines and output bounds. One LoRA batch
shares verification across every sample. PowerShell retains parameter binding,
fixed completion arguments and receipt assembly. Credential creation reuses
`bonsai-windows-credentials`; the production C# launcher is retired. Existing
independent C# codec/native probes remain as correctness references.

The qualification scorer uses safe Rust with strict object-only inputs and
bounded original-file reads. Frozen legacy fixtures preserve control canonical
bytes/digests, Unicode response identities and accepted/pending/rejected reports.
Known regressions still override pending evidence; missing reviews, logits and
basis verification cannot become acceptance. Scoring does not launch inference.

A root Cargo workspace shares package metadata, exact dependency pins, arithmetic
profiles and common lint policy. Every safe target keeps `forbid(unsafe_code)`;
audited native boundaries keep scoped allowances. The shared package/check
registry drives Foundation and nightly compatibility without combining native
Prism features or compiler/target lanes. Mutation checks copy a minimal isolated
workspace. Full suites and benchmarks remain CI-only.

Select an executable from the exact evaluated main revision through
`BONSAI_TOOLS_PATH` or `-ToolPath`. Foundation builds one tool per consuming job
and publishes an ARM64 executable artifact. Source fallback uses pinned Rust in
an external incremental lane. Planning/diagnostic commands may resolve the
maintenance executable but never launch a model. Ordinary setup, inventory,
provisioning and native build orchestration remain PowerShell; the general tool
runner retains its separate inherited-environment/output contract. No inference
speedup or device performance acceptance is claimed by this migration.

| Language | Enforced policy |
| --- | --- |
| Rust 1.99.1 / edition 2024 | Unsafe code forbidden in policy/tooling; documented native-boundary allowances in the Windows crates and Prism FFI/native test. Warnings, missing API documentation, unchecked casts, production unwrap/expect/panic and Clippy all/pedantic/nursery/cargo findings rejected; Release arithmetic overflow checks enabled. Release debug assertions remain disabled to preserve the constant-time dependency's release contract. |
| PowerShell 7.6+ | Latest strict mode, terminating errors including native-command failures, typed parameters, advanced functions and explicit output contracts; pinned PSScriptAnalyzer 1.25.0 defaults plus safety rules, with every unsuppressed diagnostic rejected. |
| C# 14 / .NET 10 | Nullable checking, checked arithmetic, unsafe compilation disabled and compiler warnings as errors for every Add-Type invocation; all .NET analyzers, internal helpers, XML API documentation and selected code-style diagnostics checked in a separate Release build. |

Run the combined PowerShell/C# standards check with an existing PSScriptAnalyzer 1.25.0 installation, or save an isolated copy:

```powershell
$tooling = Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-Tooling-' + [guid]::NewGuid().ToString('N'))
Save-Module PSScriptAnalyzer -RequiredVersion 1.25.0 -Repository PSGallery -Path $tooling -Force
./scripts/Test-Standards.ps1 -AnalyzerModulePath (Join-Path $tooling 'PSScriptAnalyzer/1.25.0/PSScriptAnalyzer.psd1')
```

The checker extracts every embedded C# helper and keeps build outputs/logs in a unique system-temp directory. The SDK baseline is stable 10.0.402 with patch roll-forward within 10.0.4xx. Rust commands run from external scratch must explicitly select `cargo +1.99.1-aarch64-pc-windows-msvc`, because a toolchain file is discovered from the working directory. Manifest lint settings apply to ordinary Cargo builds too. The native source builder explicitly requires C11/C++17 without language-extension fallback, MSVC conformance mode and SDL checks. The pinned external engine retains upstream warning handling: an actual strict warning audit found upstream MSVC C4244 in `llama-graph.cpp`; it is not claimed to meet the first-party warning policy. A different source-build configuration requires a fresh build lane or reviewed removal of the old owned build directory.

Rust safety checks remain active inside the audited native boundaries: implicit
unsafe operations in unsafe functions, invalid reference casts and references to
mutable statics are forbidden independently of the scoped `unsafe_code`
allowance. Calls through an unwinding FFI ABI are rejected. Clippy requires a
separate documented block for each unsafe operation and an explicit `reason` on
every lint allowance. These checks support
review of the FFI contracts; they do not prove external code is memory-safe or
cannot unwind. See the [compiler lint reference](https://doc.rust-lang.org/rustc/lints/listing/allowed-by-default.html)
and [Clippy unsafe-block rule](https://rust-lang.github.io/rust-clippy/stable/index.html#multiple_unsafe_ops_per_block).

Unreachable public items, redundant imports/lifetimes, unnecessary import braces
and malformed macro metavariables are rejected by ordinary Cargo builds. Internal
native helpers use crate-scoped visibility. The root `rustfmt.toml` selects both
the Rust 2024 language and formatting editions for Cargo and standalone rustfmt.
Foundation also checks Prism's optional loader documentation with all features
after the native library paths are configured.

The opt-in [Rust nightly trial](../tools/rust-trial/README.md) compares the stable
baseline with `nightly-2026-10-22` on native Windows ARM64. The candidate is pinned
in [its configuration](../config/rust-nightly-trial.json); the default toolchain,
production decoders and Foundation gates retain stable Rust. Its separate manual,
main-only [workflow](../.github/workflows/rust-nightly.yml) runs formatting, Release
Clippy, documentation and complete correctness suites for both toolchains. Prism
loader APIs are compiled, while native linking and engine regression remain in
Foundation. Trial targets and logs use separate external storage.

Stable and nightly checks run concurrently across two package groups. Runtime
keeps ternary, GGUF, Prism, the CPU reference and the borrowed-logits trial
together; Broker keeps the Windows boundary crates with the broker. Each group
reuses one compiler-specific target directory, and a separate job runs the full
compiler acceptance matrix. The final `compatibility` check requires all four
compiler/group jobs and the probes to succeed, including artifact retention.
Every formatting, Release Clippy/documentation and complete correctness check
still runs. Build-cache fallback can reuse unchanged dependencies after lockfile
changes while retaining compiler, runner-image and group isolation. Superseded
manual trials are cancelled. Local compatibility-only builds may select
`-PackageGroup Runtime` or `-PackageGroup Broker`; `All` remains the default.

The compiler probes compare all four trait-solver/Polonius combinations using
the same nightly binary, with stable as an additional baseline. Positive and
invalid-alias controls distinguish intended acceptance from broken compilation;
expected failures require the specific borrow/type error code. Receipts include
compiler identities, exact arguments, source hashes and verdicts. A synthetic
borrowed-logits cache demonstrates stable-compatible output storage reuse. The
trial makes no claim of improved model memory, inference speed or checkpoint
ownership. Full trial selections and complete suites remain CI-only; the script
supports one focused compiler case locally and rejects local `-FullTests`.

Analyzer exceptions are documented at their scope. PowerShell's UTF-8 BOM rule targets legacy Windows PowerShell and is excluded for this 7.6 baseline; its obsolete API compatibility profiles are not applied to .NET 10. Dynamic PowerShell output annotations support analysis and documentation rather than enforcing return types at runtime. No blanket Rust restriction group is enabled, since it contains mutually incompatible rules; selected safety restrictions are enforced explicitly.

Use stable PowerShell and .NET features supported by the 7.6 baseline where they improve clarity or correctness. Artifact paths use the [array form of `Join-Path -ChildPath`](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.management/join-path?view=powershell-7.6), and configuration reads use [`ConvertFrom-Json -DateKind String`](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.utility/convertfrom-json?view=powershell-7.6) to preserve JSON strings exactly.

Stable toolchains were checked on October 23, 2026: [PowerShell 7.6.8](https://github.com/PowerShell/PowerShell/releases/tag/v7.6.8), [.NET SDK 10.0.402 / C# 14](https://dotnet.microsoft.com/en-us/download/dotnet/10.0), and [Rust 1.99.1 / edition 2024](https://blog.rust-lang.org/2026/10/15/Rust-1.99.1/).

GitHub Actions pins were checked on the same date against the latest stable upstream releases: [checkout v7.0.1](https://github.com/actions/checkout/releases/tag/v7.0.1), [setup-dotnet v6.0.0](https://github.com/actions/setup-dotnet/releases/tag/v6.0.0), [cache v6.1.0](https://github.com/actions/cache/releases/tag/v6.1.0) and [upload-artifact v7.0.2](https://github.com/actions/upload-artifact/releases/tag/v7.0.2). Each action is pinned to its release commit; these Node.js 24 actions require Actions Runner 2.327.1 or later, including the owner-managed LoRA runner. PSScriptAnalyzer remains at its latest stable release, [1.25.0](https://github.com/PowerShell/PSScriptAnalyzer/releases/tag/1.25.0).

JSON schema readers also use `ConvertFrom-Json -NoEnumerate` so array roots cannot become objects through pipeline enumeration. GGUF text decoding reuses a bounded buffer with `ReadExactly` and span-based UTF-8 decoding; boolean validation uses `ContainsAnyExceptInRange`, and FP16 scale reads use explicit little-endian `BinaryPrimitives`. These are correctness and allocation changes without a measured performance claim.

One focused local correctness case is allowed, for example:

```powershell
./tests/Test-Foundation.ps1 -Case Download
```

### Fast synthetic regressions and mutation checks

Broker unit tests enumerate all 1,093 role histories through six messages against
a fixed accepted corpus. Small policy fixtures check exact header count/byte
limits, every credential position, escaped duplicate keys, equivalent JSON
encodings and completion-dependent context boundaries, including valid controls.
They use no model files or network access.

The scheduler's deterministic simulations compare the real gate with an
independent queue/ownership model over 144 seeded histories and 18,432 generated
operations. Scripted histories also exercise reservation order versus first
polling order, cancellation, expiry at the exact deadline, delayed polling after
handoff and permanent shutdown. Tokio time is paused; failures include replayable
seeds and operation traces. The in-memory supervisor regressions vary every
individual split of client, token-count and generation frames, reject truncated
replies and verify cancellation and incomplete SSE output across the components.
These tests create no sockets, model processes or wall-clock sleeps.

The [mutation checker](../scripts/Test-Mutations.ps1) changes six specific codec
guards in an external temporary copy: zero/nonfinite scales, reserved symbols,
five-trit aliases, four-trit tail canonicality and packed rounding. Each exact
guard test must pass on the original source. Each mutant must compile and fail
its named assertion test; a surviving mutant, compiler error, timeout or unrelated
test failure fails the check. This finite catalog establishes sensitivity to
those faults, rather than a whole-program mutation score. The source is restored
between mutants, the checkout is never modified, and JSON results plus logs are
retained with source hashes and bounded compile/test deadlines. No additional
test dependency or tool installation is required.

For a focused local mutation check, run from the checkout:

```powershell
$evidence = Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-Mutations-' + [guid]::NewGuid().ToString('N'))
./scripts/Test-Mutations.ps1 -Case ZeroScale -EvidenceDirectory $evidence
```

For focused Rust checks, use the external working/target directory convention
above and select `--lib tests::every_short_role_history_matches_the_accepted_language -- --exact`,
`--test policy_boundaries`, `--test scheduler_simulation`, or `--test fragmentation`.
Foundation's existing broker suite discovers these tests automatically. Its
ternary lane runs the codec guards and all six mutations, retaining mutation
receipts and logs even on failure. Compilation is separate from test execution;
these correctness checks make no inference-performance or benchmark claim.

The full suite runs only in the main-only [Foundation workflow](../.github/workflows/foundation.yml), on hosted ARM64 Windows. It checks configuration, unsafe paths, synthetic HTTP failures/resumption, PE rejection, inventory, strict server policy, and the pinned runtime package plus planned switches. Separate native Rust jobs run formatting, Clippy, documentation and full broker/ternary-codec/GGUF correctness, including immutable C++ reference parity. It downloads no model weights and runs no inference benchmark. Test scratch/evidence stays in unique system-temp directories.

Foundation reuses Cargo downloads and compiled artifacts across runs, with cache identities bound to the runner image, architecture, Rust version and build inputs. All checks still run after restoration. The broker reuses its head buffer for small request bodies, frames streamed chunks in one reusable buffer, and grows JSON storage from 8 KiB as needed within the existing 4 MiB cap. GGUF inspection validates discarded boolean arrays in chunks and skips unused nested descriptors. These changes remove redundant work; no measured speedup or inference-performance result is claimed.

Foundation runs standards, core correctness, GGUF/codec, runtime policy and Windows process-lifecycle checks in parallel. GGUF suites share their own cached CLI build, and the assessment/publisher checks retain their shared runtime provisioning within one job. The existing `foundation` check requires every job to succeed, including Prism, broker, ternary codecs and standards, and rejects skipped jobs. Every check still executes after cache restoration. Rust documentation uses the same Release profile as Clippy and tests to avoid compiling a second set of dependency metadata. Superseded push runs are cancelled while manual runs remain separate; the pinned PowerShell analyzer is reused.

Prism keeps its source-download/compiler-object cache separate from Cargo downloads and Rust build artifacts. Changing Rust cache paths therefore cannot invalidate the native cache's path identity. Its optional external Rust target directory isolates engine-loader and native-test artifacts so native linking cannot overwrite the policy archive. The source archive is reverified and fresh source/build lanes plus the native regression are retained on every run, with at most four compiler workers in hosted CI. GGUF also uses a stable external Cargo target bound to the runner image, architecture, compiler and both GGUF/ternary codec sources. The manual LoRA workflow retains lighter artifact compression and its fixed samples and qualification gates.

All benchmarks and full engine suites remain CI-only. Device latency, power, thermals, memory acceptance, creative outputs, and daily security require later gates. No laptop self-hosted runner is configured here; hosted foundation CI cannot supply those verdicts. See [PLAN.md](../PLAN.md) for delivery order and [implementation research](implementation-research.md) for pre-step literature and sanity checks.
