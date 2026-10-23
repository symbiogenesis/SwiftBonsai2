# Validation

For setup and development commands, see the [development guide](development-guide.md).

Repository-owned Rust, PowerShell and C# enforce strict language standards in local tooling and Foundation CI:

Rust is the default for repository-owned systems components. The broker, BITCOS,
GGUF tooling and LoRA sharing policy forbid unsafe code. The LoRA overlay and
its native regression are now Rust, with documented unsafe allowances confined
to the C ABI adapter and native test bindings. The pinned external Prism/llama.cpp
engine retains C/C++ tensor allocation calls that materialize Rust sharing
decisions. See the [native boundary and build](../tools/prism/README.md).
The separate `bonsai-windows-process` and `bonsai-windows-credentials` crates are
audited Win32 boundaries with documented unsafe calls, RAII ownership and strict
analyzers. The broker itself retains `forbid(unsafe_code)`. Existing C# process
helpers remain in use by the independent CPU/LoRA qualification probes.

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

Analyzer exceptions are documented at their scope. PowerShell's UTF-8 BOM rule targets legacy Windows PowerShell and is excluded for this 7.6 baseline; its obsolete API compatibility profiles are not applied to .NET 10. Dynamic PowerShell output annotations support analysis and documentation rather than enforcing return types at runtime. No blanket Rust restriction group is enabled, since it contains mutually incompatible rules; selected safety restrictions are enforced explicitly.

Use stable PowerShell and .NET features supported by the 7.6 baseline where they improve clarity or correctness. Artifact paths use the [array form of `Join-Path -ChildPath`](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.management/join-path?view=powershell-7.6), and configuration reads use [`ConvertFrom-Json -DateKind String`](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.utility/convertfrom-json?view=powershell-7.6) to preserve JSON strings exactly.

Stable toolchains were checked on October 23, 2026: [PowerShell 7.6.7](https://github.com/PowerShell/PowerShell/releases/tag/v7.6.7), [.NET SDK 10.0.402 / C# 14](https://dotnet.microsoft.com/en-us/download/dotnet/10.0), and [Rust 1.99.1 / edition 2024](https://blog.rust-lang.org/2026/10/15/Rust-1.99.1/). JSON schema readers also use `ConvertFrom-Json -NoEnumerate` so array roots cannot become objects through pipeline enumeration. GGUF text decoding reuses a bounded buffer with `ReadExactly` and span-based UTF-8 decoding; boolean validation uses `ContainsAnyExceptInRange`, and FP16 scale reads use explicit little-endian `BinaryPrimitives`. These are correctness and allocation changes without a measured performance claim.

One focused local correctness case is allowed, for example:

```powershell
./tests/Test-Foundation.ps1 -Case Download
```

The full suite runs only in the main-only [Foundation workflow](../.github/workflows/foundation.yml), on hosted ARM64 Windows. It checks configuration, unsafe paths, synthetic HTTP failures/resumption, PE rejection, inventory, strict server policy, and the pinned runtime package plus planned switches. Separate native Rust jobs run formatting, Clippy, documentation and full broker/BITCOS/GGUF correctness, including immutable C++ reference parity. It downloads no model weights and runs no inference benchmark. Test scratch/evidence stays in unique system-temp directories.

Foundation reuses Cargo downloads and compiled artifacts across runs, with cache identities bound to the runner image, architecture, Rust version and build inputs. All checks still run after restoration. The broker reuses its head buffer for small request bodies, frames streamed chunks in one reusable buffer, and grows JSON storage from 8 KiB as needed within the existing 4 MiB cap. GGUF inspection validates discarded boolean arrays in chunks and skips unused nested descriptors. These changes remove redundant work; no measured speedup or inference-performance result is claimed.

Foundation runs standards, policy/codec correctness and Windows process-lifecycle checks in parallel, reuses the pinned PowerShell analyzer, and cancels superseded push runs while keeping manual runs separate. The existing `foundation` check requires both policy/codec and lifecycle groups to succeed, including rejecting skipped groups. Standards logs identify each file and the C# build stage to help diagnose stalls. Prism rechecks its cached source archive, uses a compiler cache across fresh source/build lanes, and permits at most four compiler workers in hosted CI. BITCOS caches Rust artifacts and always reruns its checks; the former C++ synthetic benchmark workflow has been retired with its sources. Historical timing records remain available. The manual LoRA workflow uses lighter artifact compression; its fixed samples and qualification gates remain unchanged.

All benchmarks and full engine suites remain CI-only. Device latency, power, thermals, memory acceptance, creative outputs, and daily security require later gates. No laptop self-hosted runner is configured here; hosted foundation CI cannot supply those verdicts. See [PLAN.md](../PLAN.md) for delivery order and [implementation research](implementation-research.md) for pre-step literature and sanity checks.
