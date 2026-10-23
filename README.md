# Swift Bonsai 2 implementation

The owner now prioritizes **Swift Bonsai 2 PQ2_0** for the low-memory CPU path and BITCOS assessment. [The assessment](docs/swift-memory.md) records its separate immutable model/runtime pins and qualification limits. The earlier [Qwen3.8 literature decision](docs/model-selection.md) and 8K reference profile remain reproducible; their default lock is not silently substituted with a different model.

Implemented: read-only host inventory, immutable artifact locks, resumable size/hash-verified provisioning, native ARM64 CPU runtime installation/source builds, full GGUF metadata inspection, a guarded synthetic CPU check, an explicit 8K reference configuration, a fixed server-policy preflight, Rust bounded HTTP I/O and exact-count admission, single-slot scheduling, request supervision, a bounded development broker executable, private-key provisioning, and foundation CI. The artifacts are provisioned under the owner's selected `%LOCALAPPDATA%\SwiftBonsai2` directory. **Daily model chat launch is not enabled.** Integrated backend ownership/identity, daily isolation, offline UI and creative/comfort qualification remain ahead. The reference configuration is an unqualified development candidate.

Swift assessment has pinned/verified artifacts, [actual PQ2 symbol counts](config/swift.bitcos-census.json), explicit Prism inspection, a verified streaming PQ2→PTQ1 export, and experimental [ARM64 BITCOS dot kernels](tools/bitcos/README.md). PTQ1 saves 1.173 GiB while preserving the current weight symbols/scales and passes explicit bounded native decode. BITCOS accounting retains about 0.956 GiB after conditional row-index overhead, but its first ARM synthetic comparison fails the speed gate. Both remain development experiments. See [qualification and commands](docs/swift-memory.md) and [additional allocation options](docs/swift-memory-options.md). No new training is required.

## Inventory and provisioning

Use native Windows ARM64 PowerShell 7.6 or later. Choose an absolute data directory outside all Git checkouts. Junctions and device paths are unsupported by provisioning. Run inventory first:

```powershell
./scripts/Doctor.ps1
# Optional: existing external data directory and a new external report file
./scripts/Doctor.ps1 -DataPath D:\BonsaiData -OutputPath D:\Reports\doctor.json
```

Doctor reports OS-usable bytes, process/OS architecture, CPU, graphics driver, storage, and tools. If a sandbox denies CIM queries, the report marks facts unavailable. JSON includes local tool paths; review it before sharing.

After choosing storage, provision only the locked artifacts:

```powershell
$data = Join-Path $env:LOCALAPPDATA 'SwiftBonsai2' # Owner-selected location
./scripts/Get-Engine.ps1 -DataDirectory $data
./scripts/Get-Model.ps1 -DataDirectory $data

# Offline integrity checks; Get-Engine also runs a bounded --version probe
./scripts/Get-Engine.ps1 -DataDirectory $data -VerifyOnly
./scripts/Get-Model.ps1 -DataDirectory $data -VerifyOnly
./scripts/Read-Gguf.ps1 -ModelPath (Join-Path $data 'models/4ca720788d1e01f1bff70c033e0d0028fd02e502/Qwen3.8-27B-UD-Q3_K_XL.gguf')
```

The model is approximately 13.15 GB; the CPU runtime archive is approximately 12.44 MB. Provisioning never fetches the fallback, a projector, or full-precision weights. Model bytes live under `models/<revision>/`, the archive under `downloads/`, and its runtime under `runtime/<commit>/`. Runtime installation checks every file against the verified archive, native PE architecture, and reported build identity. Publisher metadata supplies expected digests; these do not independently attest the source-to-binary build.

Interrupted downloads retain one `.partial` file and resume against the immutable URL. If the server ignores Range, the download restarts. Incorrect size/hash fails before the final filename is published; inspect and remove an invalid partial before retrying. A failed extraction retains a unique staging directory for inspection. Scripts never silently substitute another model/runtime. Verified artifacts do not establish parser isolation or inference correctness.

`Read-Gguf.ps1` verifies the complete file size/SHA-256 before reading bounded metadata and tensor descriptors. It reports tensor types, actual architecture dimensions, MTP tensors, special token IDs, a raw tokenizer-record digest, and embedded-template UTF-8 digests. It skips tensor payloads. Its optional `-OutputPath` must name a new JSON file outside Git. The inspector deliberately supports little-endian GGUF v3 within documented limits; it is not a general validator for every GGUF extension.

## First CPU loading check

For the owner-prioritized Swift assessment, use `-Profile swift-pq2-assessment`.
It selects the separate Swift/Prism lock, 8K FP16 state, batch 128 / microbatch 64,
and a fixed 12 GiB service allowance plus the unchanged 4 GiB physical reserve.
The default profile below remains the earlier Qwen reference. Swift passed
the fixed load/decode check after a Swift-only system-preamble repair;
see [assessment evidence](docs/swift-memory.md). Finished-answer quality,
performance and daily-service qualification remain pending.

The approved pretrained OrcaRouter adapter is planned for the standard profile,
with a [mandatory overall performance/quality comparison against no LoRA](docs/swift-lora.md#required-standard-profile-qualification)
before acceptance. It is separately pinned. Provision it
with `./scripts/Get-Adapter.ps1 -DataDirectory $data`, then explicitly request
`./scripts/Invoke-CpuCheck.ps1 -DataDirectory $data -Profile swift-pq2-assessment -UseAbliterateLora`.
The same fixed memory guards apply. Native loading/decode has passed; refusal
behavior, creative quality, numerical basis and runtime overhead remain under
qualification. See [adapter evidence](docs/swift-lora.md). It requires no training.

The [required paired evaluation](docs/lora-qualification.md) now has a fixed
capture workflow and an independent TPS/quality gate scorer. A target Snapdragon
CI runner, native correctness evidence and external human reviews are still
required; no performance or quality parity is claimed from successful loading.

A [pinned CPU source overlay](docs/prism-storage-overlay.md) now shares identical
immutable LoRA B storage. Its native regression verifies a 2.5 MiB allocation
saving and unchanged matrix-operation outputs. Whole-model TPS and quality remain
gated; the installed publisher runtime is retained during qualification.

```powershell
./scripts/Invoke-CpuCheck.ps1 -DataDirectory $data -CheckOnly
./scripts/Invoke-CpuCheck.ps1 -DataDirectory $data
```

This fixed synthetic check uses the in-process `llama-completion` executable, a 16-token limit, 8K FP16 state, CPU-only placement, no reusable prompt cache, and a ten-minute safety timeout. It preserves the embedded template's default thinking behavior. The pinned completion path does not reliably forward thinking-control kwargs, and modern `llama-cli` starts a local server; neither is used to claim a qualified thinking-disabled chat profile.

Before launch, the check requires 22 GiB currently available physical RAM: the complete 18 GiB service allowance plus a conservative 4 GiB reserve. It then watches current process working set/private bytes against 18 GiB and available physical memory against the 4 GiB floor. It fails closed rather than closing other applications. These volatile admission/watchdog readings are not peak measurements or performance acceptance. After the owner stopped WSL on October 23, available RAM rose to about 20.1 GiB; admission still refused model launch. CPU load/decode remains pending.

The CPU check now creates its process inside a Windows Job with kill-on-close, an 18 GiB **job-wide committed-memory** cap, and one active process with no breakaway. The Job is assigned through the process-creation attribute list while the child is suspended, before any child code executes; only its standard handles are inherited. Stop terminates the whole owned Job and waits for it to become empty. Output goes directly to technical files under a unique external temp working directory, and inherited Windows system/profile variables are preserved. The reported Windows Job peak is a diagnostic counter that may include denied allocation requests. The committed-memory cap does not bound all resident file-backed model mappings; the physical-memory watchdog remains necessary. No restricted token, filesystem or egress boundary is established. Synthetic regressions qualify ownership/limits, including abrupt parent death; this remains a trusted-input development check.

## Prepare the server policy

```powershell
# Pure planning: no processes, keys, storage creation, or listener
./scripts/Get-ServerPlan.ps1 -DataDirectory $data
# Offline runtime verification plus bounded version/help probes; no model needed
./scripts/Get-ServerPlan.ps1 -DataDirectory $data -VerifyRuntime
# Also hash-check the existing model; never downloads or loads it
./scripts/Get-ServerPlan.ps1 -DataDirectory $data -VerifyArtifacts
```

The preflight rejects unreviewed profile fields/types, mapped network storage, unsafe paths, and inherited runtime overrides, including the separate `AIP_*` and `MTMD_*` namespaces. It preserves Windows system/profile environment variables. The returned argument array uses numeric loopback, a planned key-file path, no host tools/MCP/UI, one 8K CPU slot, FP16 state, no reuse/speculation, and the publisher's thinking-mode sampler. Verification checks every planned switch against the installed pinned binary's help; that does not validate option values or enforcement. Reports contain local paths; review before sharing.

The preflight **cannot launch chat** and creates no key. It lists the required enforcement work: native CORS does not reject Host/Origin, control routes remain registered, and request settings can exceed the `n-predict` default. The socket timeout and offline flag do not enforce a total request duration or OS egress denial. Authentication, request bounds, restricted process lifecycle, backend bypass prevention, and the offline UI remain pending. The October 23 artifact preflight passed integrity/switch checks, while its admission-only memory check still refused model launch (about 9.3 GiB available versus 22 GiB required).

## Broker request admission and HTTP I/O

The [Rust policy core](docs/broker-policy.md) checks bearer credentials, exact Host/Origin/route, header/body limits, strict text-chat JSON, and a 1–4096 completion-token cap. It rebuilds requests with the reference sampler; a trusted callback receives the canonical body for exact prompt-token counting before the identical immutable body is admitted for generation. Unknown/duplicate fields, tools, media and caller overrides are rejected.

The receiver consumes one supplied stream with a 16 KiB complete raw-head limit, 64 header values, a 1 MiB body cap, and absolute five-second header/thirty-second intake deadlines shortened by the supervisor deadline. It preserves duplicate wire headers and validates the raw route before normalization. Errors and cancellation drop the owned stream; success returns it for one response followed by close.

The response adapter accepts an owned, already HTTP-decoded backend response and checks status/media type against the admitted request. It rebuilds fixed headers, buffers bounded JSON or streams SSE with chunked framing, and caps decoded output at 4 MiB. Backend reads share the supervisor deadline; each entire client write/flush operation has a five-second limit. Backend failure or excess bytes abort streaming before HTTP completion. Cancellation drops both supplied endpoints. See the [response contract](docs/broker-policy.md#one-response-writer) for transport preconditions and limits.

The [private backend adapter](docs/broker-policy.md#private-backend-http-exchange-and-exact-counts) sends canonical requests on supplied owned streams with fixed routes and private headers. It bounds response heads, decodes explicit Content-Length or chunked bodies, and treats incomplete framing as an error. Payload, chunk metadata and absolute deadlines are bounded. Its count helper validates the pinned endpoint's strict 4 KiB response and admits the identical body for generation.

The [development service](docs/broker-policy.md#development-orchestration-and-loopback-transport) now joins these components. It reserves capacity before accepting a socket, allows one active request and at most eight waiting reservations, and enforces a thirty-second queue limit and ten-minute total deadline. A fixed numeric-loopback connector uses the same private endpoint/key for counting and generation. Actual frontend reads detect disconnect or extra input while backend work is pending; both streams close on cancellation. Clients must keep their sending side open until the response finishes. Shutdown wakes accepting, queued and active work. Interrupted generation permanently closes admission because socket closure does not acknowledge model cancellation.

These components are exercised with synthetic loopback backends. An explicit development executable and private-key helper are described below. Backend process identity/bypass prevention, actual model tokenization and cancellation acknowledgement, restricted filesystem/egress and daily isolation remain unqualified. The fixed PowerShell model preflight and reference profile still disable launch.

Use the existing native ARM64 Rust toolchain to build it. Run Cargo from a unique system-temp working directory, with absolute `--manifest-path` and `--target-dir` paths. For a focused regression, `cargo test --locked --offline --manifest-path <repo>/src/broker/Cargo.toml --target-dir <session-temp>/build --test http_reception wire_duplicates_and_framing_cannot_be_normalized_away -- --exact`. Full broker tests remain CI-only.

## Explicit development broker

The executable serves the bounded text-chat API against an **already running development backend**. It does not launch a model, establish the backend's identity, or enable the daily profile. It requires both `--development` and `--allow-unverified-key-files`: loading the files does not verify their Windows ACLs or ancestor ownership. The separate helper below creates protected owner/LocalSystem ACLs; same-user processes and a parent-directory owner can still access or replace credentials. Choose an existing local parent directory you control, outside Git.

```powershell
$keys = ./scripts/New-DevelopmentKeys.ps1 -ParentDirectory $data
# Returns paths only; neither credential is printed. -WhatIf creates nothing.
```

The helper creates a fresh GUID directory on each invocation, with distinct 32-byte cryptographic random keys encoded as exactly 64 lowercase ASCII hex characters. It uses protected ACLs at directory/file creation, verifies the directory owner/DACL, and opens each file with CreateNew. Existing credentials and ACLs are never overwritten. Failed setup may retain a partial private directory for inspection; remove only a verified directory belonging to that invocation when it is no longer useful.

Build the executable from a new external scratch directory, using an absolute repository path:

```powershell
$repo = (Get-Location).Path
$scratch = Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-Broker-' + [guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($scratch) | Out-Null
Push-Location $scratch
try {
    cargo +1.98.0-aarch64-pc-windows-msvc build --locked --release --manifest-path (Join-Path $repo 'src/broker/Cargo.toml') --target-dir (Join-Path $scratch 'build')
    if ($LASTEXITCODE) { throw 'Broker build failed.' }
    # Configure the separate development backend to use $keys.BackendKeyFile.
    # This command starts only the broker. It does not configure that backend.
    & (Join-Path $scratch 'build/release/bonsai-broker.exe') --development --allow-unverified-key-files `
        --frontend-port 5173 --backend-port 8081 `
        --frontend-key-file $keys.FrontendKeyFile --backend-key-file $keys.BackendKeyFile --max-waiting 2
} finally { Pop-Location }
```

Only `127.0.0.1` is bound. Frontend port zero requests an OS-assigned port; the executable prints the actual endpoint without credentials. All other ports are unprivileged. Keys are accepted only from bounded regular files, with no reparse components; Windows read-only handles remain open to deny write/delete opens during serving. The runner holds at most one active request plus the configured zero through eight waiting connections, creates no detached request tasks, and observes Ctrl-C before accepting more work. Shutdown closes admission and drains owned futures/sockets. Interrupted generation closes the whole service; there is no automatic reset or restart. No UI route is supplied.

## Build the CPU server from source

Install Visual Studio 2026 with native ARM64 C++ tools and CMake first, or select the supported Visual Studio 2022 generator explicitly. The builder installs no toolchain. Doctor discovers the existing executable paths:

```powershell
$doctor = ./scripts/Doctor.ps1 -PassThru
$gitPath = ($doctor.Tools | Where-Object Name -eq Git).Path
$cmakePath = ($doctor.Tools | Where-Object Name -eq CMake).Path
./scripts/Build-Engine.ps1 -DataDirectory $data -GitPath $gitPath -CMakePath $cmakePath -FetchSource
```

The script fetches only the pinned commit into `sources/<commit>/`, requires an exact clean checkout on reuse, and builds only the Release `llama-server` target under `builds/<commit>/cpu/`. It records compiler/configuration, hashes and version in an external build receipt. It enables Git long paths per command without changing global settings. It runs no test suite, inference, or benchmark. This initial build disables subprocess tools, accelerator backends, and automatic dependency/UI downloads. It has **no full embedded chat UI**; reviewed UI assets and M1 security/lifecycle qualification remain separate work.

Build parallelism now adapts to CPU count and currently free memory, with a maximum of eight jobs by default. The same limit applies to project scheduling and shared compiler workers, avoiding multiplied compiler concurrency. This is a starting heuristic: it reserves two CPU threads and 2 GiB, then budgets 1 GiB per compiler; it falls back to at most two jobs if memory cannot be queried. Use `-Parallel 1` through `-Parallel 12` to override it. Receipts record the selected budget. Reusing the same external data directory preserves incremental build outputs; configuration and source integrity are checked on every invocation.

## Validation

Repository-owned Rust, PowerShell and C# enforce strict language standards in local tooling and Foundation CI:

| Language | Enforced policy |
| --- | --- |
| Rust 1.98.0 / edition 2024 | Unsafe code forbidden for every target; warnings, missing API documentation, unchecked casts, production unwrap/expect/panic and Clippy all/pedantic/nursery/cargo findings rejected; Release arithmetic overflow checks enabled. Release debug assertions remain disabled to preserve the constant-time dependency's release contract. |
| PowerShell 7.6+ | Latest strict mode, terminating errors including native-command failures, typed parameters, advanced functions and explicit output contracts; pinned PSScriptAnalyzer 1.25.0 defaults plus safety rules, with every unsuppressed diagnostic rejected. |
| C# 14 / .NET 10 | Nullable checking, checked arithmetic, unsafe compilation disabled and compiler warnings as errors for every Add-Type invocation; all .NET analyzers, internal helpers, XML API documentation and selected code-style diagnostics checked in a separate Release build. |

Run the combined PowerShell/C# standards check with an existing PSScriptAnalyzer 1.25.0 installation, or save an isolated copy:

```powershell
$tooling = Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-Tooling-' + [guid]::NewGuid().ToString('N'))
Save-Module PSScriptAnalyzer -RequiredVersion 1.25.0 -Repository PSGallery -Path $tooling -Force
./scripts/Test-Standards.ps1 -AnalyzerModulePath (Join-Path $tooling 'PSScriptAnalyzer/1.25.0/PSScriptAnalyzer.psd1')
```

The checker extracts every embedded C# helper and keeps build outputs/logs in a unique system-temp directory. The SDK is pinned to stable 10.0.4xx patches. Rust commands run from external scratch must explicitly select `cargo +1.98.0-aarch64-pc-windows-msvc`, because a toolchain file is discovered from the working directory. Manifest lint settings apply to ordinary Cargo builds too. The native source builder explicitly requires C11/C++17 without language-extension fallback, MSVC conformance mode and SDL checks. The pinned external engine retains upstream warning handling: an actual strict warning audit found upstream MSVC C4244 in `llama-graph.cpp`; it is not claimed to meet the first-party warning policy. A different source-build configuration requires a fresh build lane or reviewed removal of the old owned build directory.

Analyzer exceptions are documented at their scope. PowerShell's UTF-8 BOM rule targets legacy Windows PowerShell and is excluded for this 7.6 baseline; its obsolete API compatibility profiles are not applied to .NET 10. Dynamic PowerShell output annotations support analysis and documentation rather than enforcing return types at runtime. No blanket Rust restriction group is enabled, since it contains mutually incompatible rules; selected safety restrictions are enforced explicitly.

Use stable PowerShell and .NET features supported by the 7.6 baseline where they improve clarity or correctness. Artifact paths use the [array form of `Join-Path -ChildPath`](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.management/join-path?view=powershell-7.6), and configuration reads use [`ConvertFrom-Json -DateKind String`](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.utility/convertfrom-json?view=powershell-7.6) to preserve JSON strings exactly.

One focused local correctness case is allowed, for example:

```powershell
./tests/Test-Foundation.ps1 -Case Download
```

The full suite runs only in the main-only [Foundation workflow](.github/workflows/foundation.yml), on hosted ARM64 Windows. It checks configuration, unsafe paths, synthetic HTTP failures/resumption, PE rejection, inventory, strict server policy, and the pinned runtime package plus planned switches. A separate native Rust job runs broker formatting, Clippy, and the full admission/reception/response suite with locked dependencies. It downloads no model weights and runs no inference benchmark. Test scratch/evidence stays in unique system-temp directories.

Foundation reuses Cargo downloads and compiled artifacts across runs, with cache identities bound to the runner image, architecture, Rust version and build inputs. All checks still run after restoration. The broker reuses its head buffer for small request bodies, frames streamed chunks in one reusable buffer, and grows JSON storage from 8 KiB as needed within the existing 4 MiB cap. GGUF inspection validates discarded boolean arrays in chunks and skips unused nested descriptors. These changes remove redundant work; no measured speedup or inference-performance result is claimed.

All benchmarks and full engine suites remain CI-only. Device latency, power, thermals, memory acceptance, creative outputs, and daily security require later gates. No laptop self-hosted runner is configured here; hosted foundation CI cannot supply those verdicts. See [PLAN.md](PLAN.md) for delivery order and [implementation research](docs/implementation-research.md) for pre-step literature and sanity checks.
