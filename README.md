# Swift Bonsai 2 implementation

The first implementation selects **Qwen3.8-27B UD-Q3_K_XL** from published creative-writing evidence. [The decision](docs/model-selection.md) records the artifact, ARM64 compatibility, memory estimate, limitations, fallback, and stop conditions. The repository name is retained from the original Swift-first research plan.

Implemented: read-only host inventory, immutable artifact locks, resumable size/hash-verified provisioning, native ARM64 CPU runtime installation/source builds, full GGUF metadata inspection, a guarded synthetic CPU check, an explicit 8K reference configuration, a fixed server-policy preflight, Rust request admission and bounded HTTP reception, and foundation CI. The chosen model/runtime are provisioned under the owner's selected `%LOCALAPPDATA%\SwiftBonsai2` directory; the complete artifact metadata is recorded in [model.metadata.json](config/model.metadata.json). **Chat launch is not enabled.** CPU loading, authenticated lifecycle, daily isolation, and creative/comfort qualification remain ahead. The reference configuration is an unqualified development candidate.

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

```powershell
./scripts/Invoke-CpuCheck.ps1 -DataDirectory $data -CheckOnly
./scripts/Invoke-CpuCheck.ps1 -DataDirectory $data
```

This fixed synthetic check uses the in-process `llama-completion` executable, a 16-token limit, 8K FP16 state, CPU-only placement, no reusable prompt cache, and a ten-minute safety timeout. It preserves the embedded template's default thinking behavior. The pinned completion path does not reliably forward thinking-control kwargs, and modern `llama-cli` starts a local server; neither is used to claim a qualified thinking-disabled chat profile.

Before launch, the check requires 22 GiB currently available physical RAM: the complete 18 GiB service allowance plus a conservative 4 GiB reserve. It then watches current process working set/private bytes against 18 GiB and available physical memory against the 4 GiB floor. It fails closed rather than closing other applications. These volatile admission/watchdog readings are not peak measurements, OS-enforced limits, or performance acceptance. On October 23 the host had about 8 GiB available, so the check refused to start; CPU load/decode remains pending. Failure logs stay in unique system-temp directories. A trusted-input development check does not qualify daily process isolation.

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

## Broker request admission and reception

The [Rust policy core](docs/broker-policy.md) checks bearer credentials, exact Host/Origin/route, header/body limits, strict text-chat JSON, and a 1–4096 completion-token cap. It rebuilds requests with the reference sampler; a trusted callback receives the canonical body for exact prompt-token counting before the identical immutable body is admitted for generation. Unknown/duplicate fields, tools, media and caller overrides are rejected.

The new receiver consumes one supplied stream with a 16 KiB complete raw-head limit, 64 header values, a 1 MiB body cap, and absolute five-second header/thirty-second intake deadlines shortened by the supervisor deadline. It preserves duplicate wire headers and validates the raw route before normalization. Errors and cancellation drop the owned stream; success returns it for one future response followed by close. It supplies no listener, response/backend transport, model launcher or production token counter. Generation cancellation, lifecycle and daily isolation remain pending.

Use the existing native ARM64 Rust toolchain to build it. Run Cargo from a unique system-temp working directory, with absolute `--manifest-path` and `--target-dir` paths. For a focused regression, `cargo test --locked --offline --manifest-path <repo>/src/broker/Cargo.toml --target-dir <session-temp>/build --test http_reception wire_duplicates_and_framing_cannot_be_normalized_away -- --exact`. Full broker tests remain CI-only.

## Build the CPU server from source

Install Visual Studio 2026 with native ARM64 C++ tools and CMake first, or select the supported Visual Studio 2022 generator explicitly. The builder installs no toolchain. Doctor discovers the existing executable paths:

```powershell
$doctor = ./scripts/Doctor.ps1 -PassThru
$gitPath = ($doctor.Tools | Where-Object Name -eq Git).Path
$cmakePath = ($doctor.Tools | Where-Object Name -eq CMake).Path
./scripts/Build-Engine.ps1 -DataDirectory $data -GitPath $gitPath -CMakePath $cmakePath -FetchSource
```

The script fetches only the pinned commit into `sources/<commit>/`, requires an exact clean checkout on reuse, and builds only the Release `llama-server` target under `builds/<commit>/cpu/`. It records compiler/configuration, hashes and version in an external build receipt. It enables Git long paths per command without changing global settings. It runs no test suite, inference, or benchmark. This initial build disables subprocess tools, accelerator backends, and automatic dependency/UI downloads. It has **no full embedded chat UI**; reviewed UI assets and M1 security/lifecycle qualification remain separate work.

## Validation

Use stable PowerShell and .NET features supported by the 7.6 baseline where they improve clarity or correctness. Artifact paths use the [array form of `Join-Path -ChildPath`](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.management/join-path?view=powershell-7.6), and configuration reads use [`ConvertFrom-Json -DateKind String`](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.utility/convertfrom-json?view=powershell-7.6) to preserve JSON strings exactly.

One focused local correctness case is allowed, for example:

```powershell
./tests/Test-Foundation.ps1 -Case Download
```

The full suite runs only in the main-only [Foundation workflow](.github/workflows/foundation.yml), on hosted ARM64 Windows. It checks configuration, unsafe paths, synthetic HTTP failures/resumption, PE rejection, inventory, strict server policy, and the pinned runtime package plus planned switches. A separate native Rust job runs broker formatting, Clippy, and the full admission/reception suite with locked dependencies. It downloads no model weights and runs no inference benchmark. Test scratch/evidence stays in unique system-temp directories.

All benchmarks and full engine suites remain CI-only. Device latency, power, thermals, memory acceptance, creative outputs, and daily security require later gates. No laptop self-hosted runner is configured here; hosted foundation CI cannot supply those verdicts. See [PLAN.md](PLAN.md) for delivery order and [implementation research](docs/implementation-research.md) for pre-step literature and sanity checks.
