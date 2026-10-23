# Swift Bonsai 2 implementation

The first implementation selects **Qwen3.8-27B UD-Q3_K_XL** from published creative-writing evidence. [The decision](docs/model-selection.md) records the artifact, ARM64 compatibility, memory estimate, limitations, fallback, and stop conditions. The repository name is retained from the original Swift-first research plan.

Implemented: read-only host inventory, immutable artifact locks, resumable size/hash-verified model provisioning, verified native ARM64 CPU runtime installation, a pinned source-build script, an explicit 8K reference configuration, and foundation CI. **Chat launch is not enabled.** CPU loading, complete GGUF inspection, authenticated lifecycle, daily isolation, and creative/comfort qualification remain ahead. The reference configuration is an unqualified development candidate.

## Inventory and provisioning

Use native Windows ARM64 PowerShell 7.4 or later. Choose an absolute data directory outside all Git checkouts. Junctions and device paths are unsupported by provisioning. Run inventory first:

```powershell
./scripts/Doctor.ps1
# Optional: existing external data directory and a new external report file
./scripts/Doctor.ps1 -DataPath D:\BonsaiData -OutputPath D:\Reports\doctor.json
```

Doctor reports OS-usable bytes, process/OS architecture, CPU, graphics driver, storage, and tools. If a sandbox denies CIM queries, the report marks facts unavailable. JSON includes local tool paths; review it before sharing.

After choosing storage, provision only the locked artifacts:

```powershell
$data = 'D:\BonsaiData' # Replace with your chosen absolute directory
./scripts/Get-Engine.ps1 -DataDirectory $data
./scripts/Get-Model.ps1 -DataDirectory $data

# Offline integrity checks; Get-Engine also runs a bounded --version probe
./scripts/Get-Engine.ps1 -DataDirectory $data -VerifyOnly
./scripts/Get-Model.ps1 -DataDirectory $data -VerifyOnly
```

The model is approximately 13.15 GB; the CPU runtime archive is approximately 12.44 MB. Provisioning never fetches the fallback, a projector, or full-precision weights. Model bytes live under `models/<revision>/`, the archive under `downloads/`, and its runtime under `runtime/<commit>/`. Runtime installation checks every file against the verified archive, native PE architecture, and reported build identity. Publisher metadata supplies expected digests; these do not independently attest the source-to-binary build.

Interrupted downloads retain one `.partial` file and resume against the immutable URL. If the server ignores Range, the download restarts. Incorrect size/hash fails before the final filename is published; inspect and remove an invalid partial before retrying. A failed extraction retains a unique staging directory for inspection. Scripts never silently substitute another model/runtime. Verified artifacts do not establish parser isolation or inference correctness.

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

One focused local correctness case is allowed, for example:

```powershell
./tests/Test-Foundation.ps1 -Case Download
```

The full suite runs only in the main-only [Foundation workflow](.github/workflows/foundation.yml), on hosted ARM64 Windows. It checks configuration, unsafe paths, synthetic HTTP failures/resumption, PE rejection, inventory, and the pinned runtime package. It downloads no model weights and runs no inference benchmark. Test scratch/evidence stays in unique system-temp directories.

All benchmarks and full engine suites remain CI-only. Device latency, power, thermals, memory acceptance, creative outputs, and daily security require later gates. No laptop self-hosted runner is configured here; hosted foundation CI cannot supply those verdicts. See [PLAN.md](PLAN.md) for delivery order and [implementation research](docs/implementation-research.md) for pre-step literature and sanity checks.
