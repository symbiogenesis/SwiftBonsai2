# Dependency audit

The original review below is dated 2026-10-26. The
[2026-10-29 refresh](#dependency-refresh-2026-10-29) records the current updates
and fresh security scan.

The direct dependencies are purposeful and mostly well trimmed. The audit found
one unused direct declaration and an available ZIP hardening update; both were
fixed. No evidence supports replacing Serde, Tokio or the native UI toolkit.
The main remaining footprint concerns are graphics fallback backends and the
concurrently developed MXC sandbox integration.

## Scope and security result

Reviewed all 12 workspace manifests, the shared lockfile, direct source usage,
normal/build/dev feature trees, cached third-party manifests and selected build
scripts. Compared Windows ARM64 with the all-target graph and the optional
`json-candidate` feature. This is a dependency/usage review, not an exhaustive
source audit of every transitive package or bundled native engine.

The committed starting revision was `f9c7ade2de57eb537f397429eed42a297740fd06`
(449 locked packages). Concurrent, initially uncommitted sandbox work added
`mxc-sdk =1.0.1` and `flatbuffers =25.12.19` to `bonsai-windows-process`
(492 locked packages). Both snapshots were scanned; their changes were preserved.

`cargo-audit 0.22.2`, using freshly fetched RustSec database revision
`3279c99911354c400e1d41e47c7d60e39258b147` (1,378 advisories), reported:

- **Zero known vulnerabilities** and no yanked-package warnings in either snapshot.
- The final working lockfile with ZIP 9.1.2 still has 492 packages and the same result.
- Three unmaintained-package notices: `zvariant 5.15.1`
  ([RUSTSEC-2026-0370](https://rustsec.org/advisories/RUSTSEC-2026-0370.html)),
  `zbus_names 4.3.5`
  ([RUSTSEC-2026-0371](https://rustsec.org/advisories/RUSTSEC-2026-0371.html)),
  and `zvariant_derive 5.15.1`
  ([RUSTSEC-2026-0372](https://rustsec.org/advisories/RUSTSEC-2026-0372.html)).

Those notices enter through eframe → egui-winit → AccessKit's Unix implementation
→ AT-SPI/zbus. They do **not** compile into the supported Windows binaries.
Upstream moved these standalone crates into zbus 6; AccessKit's current compatible
requirements still select zbus 5. Removing accessibility or forcing an incompatible
zbus major is inappropriate. Keep the warnings visible and track upstream migration.

No direct pins were yanked. The crates.io release check found the other direct
pins current, apart from intentionally retaining `windows-future 0.3.2`: the newer
0.100 line is incompatible with the `windows 0.62.2` dependency family. Exact pins
provide reproducibility, not continuing advisory notification.

## Changes made

1. Removed the broker's unused direct `sha2` declaration. Broker hashing already
   delegates to `bonsai-artifacts`; SHA-256 remains available there. This removes
   a redundant dependency edge, with no claimed binary-size or speed improvement.
2. Updated ZIP 9.1.0 → **9.1.2**, retaining disabled defaults and only
   `deflate-flate2-zlib-rs`. Today's patch hardens convenience extraction against
   symlink following ([upstream fix](https://github.com/zip-rs/zip2/pull/1116)) and
   rejects Unicode-path extra fields that rename writer entries
   ([upstream fix](https://github.com/zip-rs/zip2/pull/1177)). Runner already uses
   custom path/type/size validation and bounded extraction rather than those
   convenience extraction methods; this is preventive maintenance, not a claim
   of a demonstrated exploitable Runner vulnerability.
3. Added [Dependency audit](../.github/workflows/dependency-audit.yml) on relevant
   main pushes, weekly and manual dispatch. It installs pinned cargo-audit with
   its locked dependencies and checks the complete lockfile against a freshly
   fetched database. Vulnerabilities and yanked packages fail; maintenance notices
   remain visible without blanket ignores. No application dependency was added.

## Dependency footprint and duplication

Counts below are distinct package name/version pairs in Windows ARM64
**normal + build** trees, including the root and internal workspace crates.
They are not linked-code counts, binary sizes or measured runtime costs.
Per-executable graphs differ because Cargo unifies the features requested by the
selected packages; a workspace-wide graph cannot describe every standalone build.

| Selection | Committed baseline | With pending MXC integration |
| --- | ---: | ---: |
| Whole workspace | 211 (199 external) | 262 (250 external) |
| Chat | 162 | 228 |
| Broker | 43 | 126 |
| Maintenance | 35 | 123 |
| ASR | 48 | 130 |
| TTS | 39 | 126 |
| CI planner | 36 | 36 |

At baseline, workspace dev dependencies raise the Windows count from 211 to 213;
all features plus dev raise it to 219. The all-target counts are 425 normal/build,
427 with dev and 434 with all features/dev. macOS/Linux/Android/Wasm packages in
the lockfile do not imply those runtimes ship in the Windows application.

Five genuine baseline Windows version-duplicate groups remain:

| Crate | Versions | Why retained |
| --- | --- | --- |
| `hashbrown` | 0.16.1 / 0.17.1 | GPU allocator versus other UI consumers |
| `miniz_oxide` | 0.8.9 / 0.9.1 | PNG versus flate2 |
| `rustc-hash` | 1.1.0 / 2.1.3 | Naga/wgpu versus type-map |
| `syn` | 2.0.119 / 3.0.9 | Windows/zerocopy macros versus newer derive macros |
| `windows-sys` | 0.52.0 / 0.61.2 | winit versus current Windows consumers |

These have incompatible upstream requirements; do not force them together with
patches. MXC additionally introduces `base64 0.22.1` alongside CI's 0.23.1,
`getrandom 0.2.17` alongside 0.4.3 and `windows-sys 0.59.0` through winreg.
The base64 versions serve separate executables. Same-version duplicate rows for
log/memchr/Serde internals can represent distinct host/target or feature build
units, rather than multiple library versions in one executable.

### MXC has the largest new footprint

`mxc-sdk` disables optional VM/WSL/hyperlight features here, but its own manifest
unconditionally requests Tokio `full`, URL/IDNA/ICU, chrono, temporary files,
UUIDs, older random/base64 APIs and broad Windows bindings. It also re-enables
SHA-2 defaults. Cargo features are additive: our `default-features = false`
cannot remove features requested by another dependency. The SDK build script's
Nanvix download path is gated by `microvm`, which is not enabled.

Putting this SDK in the shared process boundary therefore materially broadens
every application consumer's compile graph. This does not establish a runtime
slowdown: retained dependency counts include build tools, and linker elimination
can remove unused code. Prefer upstream backend-specific dependency/features;
consider a separate supervisor executable only if the remaining size/build cost
warrants its extra lifecycle and IPC complexity. Sandbox enforcement must remain
mandatory and fail closed; dependency trimming must not weaken containment.

Further source review confirmed that Runner's low-level PSEC calls use synchronous
Windows APIs and generated FlatBuffers. Tokio is used by the SDK's Windows
Sandbox/WSLC daemon and lifecycle paths, URL/ICU by proxy/network parsing, and
chrono by learning-mode tracing. Those packages are not intrinsic to Runner's
PSEC calls, but version 1.0.1 unconditionally compiles the broader facade. Existing
optional features already exclude VM/WSL/Hyperlight paths; there is no published
PSEC-only switch. Upstream main at `ad54c69452147bd4ca15d027388ad6074903dbff`
has the same limitation.

A minimal upstream PSEC module could expose the API loader, handle owner,
capability queries and generated policy types without the full runner/proxy/
learning-mode graph. Its errors and UTF-16 helpers must be separated from current
learning-mode/Base64 coupling, and telemetry/build dependencies need feature
gating too. Simply marking dependencies optional would break compilation.
See the [published manifest](https://github.com/microsoft/mxc/blob/v1.0.1/src/mxc-sdk/Cargo.toml).

Runner's hidden PSEC exports are unsupported compatibility interfaces, so their
small dependency needs do not establish a supported SDK contract. A separate
compile-only consumer of the documented `v1::spawn` API with `Containment::Process`
also reproduced the broader dependency graph and passed Windows ARM64 checking.
[MXC #1579](https://github.com/microsoft/mxc/issues/1579) therefore requests
backend/capability-selective builds of the supported SDK, covering all plausibly
optional runtime and build dependencies while retaining security validation.

### Renderer features are now explicit

The initial audit found that eframe's `wgpu` feature re-enabled
`egui-wgpu/default` and broad wgpu defaults despite disabling eframe defaults.
The follow-up uses `wgpu_no_default_features` and a pinned direct wgpu dependency
solely to select `std`, `parking_lot`, `dx12`, `vulkan`, `gles` and `wgsl`.
All three existing Windows backends remain enabled, as requested by the user.
The application uses default `NativeOptions` renderer configuration, whose
upstream backend selection is `PRIMARY | GL` unless overridden by the environment.
These are real fallback paths.

This excludes production browser/Metal backend defaults but leaves the Windows
package graph unchanged. At the original audit baseline, the all-target chat
normal/build graph drops from 368 to 366 packages; upstream GPU test dependencies
still request Metal and all Windows backends. No Windows size or speed improvement
is claimed. Narrowing the supported Windows backend set remains a separate policy
and device-qualification decision. System fonts also pull in font parsers,
color-font rendering, `vello_cpu` and `glifo` for actual UI functionality. None is
an accidental inference framework or browser frontend.

The renderer follow-up passed local chat format, Release Clippy, documentation
and build checks. The exact capture regression rendered ten layouts with DX12
forced; native preview windows with default and DX12 selection were responsive
and closed successfully. These checks do not qualify every backend/device or
establish inference performance. Evidence remains outside Git in
`C:\Users\symbi\AppData\Local\Temp\Bonsai-WgpuRefine-2143bcc4c15b4cdfbfa4b483ea4d7937`.

## Direct dependency decisions

| Dependency | Decision and rationale |
| --- | --- |
| `serde`, `serde_json` | Keep shared pins; required protocol/configuration APIs. Existing correctness and CI timing screens reject replacement candidates. |
| `tokio` | Keep bounded current-thread broker design and explicit io-util/net/rt/signal/sync/time features; test-util stays dev-only. Pending MXC broadens these features as described above. |
| `http`, `httparse` | Keep complementary typed HTTP policy and bounded wire parsing. A general web framework would add scope. |
| `subtle` | Keep constant-time bearer comparisons; release debug assertions remain disabled as required by its contract. |
| `sha2` | Keep artifact/tool verification; remove only the unused broker edge. |
| `zip` | Keep minimal deflate configuration at 9.1.3; custom extraction policies remain necessary. |
| `socket2` | Keep pre-bind options, bounded backlog and Windows exclusive-address behavior. |
| `windows-sys` | Keep selected Win32 process/credentials/audio/accessibility APIs. |
| `windows`, `windows-future` | Keep compatible WinRT microphone permission/capture APIs. They serve a different role from windows-sys. |
| `eframe`, `wgpu` | Keep native Rust UI, accessibility, clipboard and fallback/system fonts; select all three supported Windows graphics backends explicitly. |
| `egui_kittest`, `egui_system_font_provider` | Keep test-only harness/font checks; capture-only feature splitting gives little benefit while production already needs graphics. |
| `toml` | Keep CI manifest parsing with only parse/serde/std enabled. |
| `ureq` | Keep CI-only synchronous HTTPS; Windows uses Schannel and non-Windows Rustls. Defaults/compression remain disabled. |
| `transcribe-cpp` | Keep required static CPU ASR engine with defaults disabled. Bundled CMake source has no source download in the reviewed build script; MSVC optimization is explicitly enabled. |
| `simd-json` | Keep qualification-only optional feature and exact crates.io release pin; never used by ordinary requests. Existing measured results are slower and do not support promotion. |
| `mxc-sdk`, `flatbuffers` | Concurrent sandbox work: security purpose is clear, footprint is substantial; preserve integration and pursue upstream backend modularity. |
| Internal crates | Keep process/credential boundaries, shared audio codec, policies and artifact checks; no unused direct path dependency found. |

Native ASR and TTS have separate GGML cores and executables by design. ASR disables
default Metal and optional accelerators; startup pins CPU-only CMake settings.
TTS disables accelerators, BLAS, OpenMP, KleidiAI, dynamic backends, tests and
examples. Linking the engines together to deduplicate native libraries risks
mixing incompatible GGML versions. RustSec scanning does not comprehensively
cover vulnerabilities in bundled C/C++ source.

The speech service only needs `VerifiedFile`, while `bonsai-artifacts` also
unconditionally brings in ZIP archive APIs. A future feature/API split could
reduce independently built speech companions' compile scope, but workspace
feature unification and linker elimination limit the likely benefit. Accepted
follow-up experiments are tracked in [PLAN.md](../PLAN.md#deferred-dependency-experiments).

## Validation and evidence

Local format, Release Clippy, documentation and build checks passed for
`bonsai-artifacts`, `bonsai-broker` and `bonsai-tools`. Named regressions cover
unsafe archive policy, entry/expansion budgets and hostile native-source names.
The audit workflow passed actionlint 1.7.12 and YAML/event/permission checks.
Full suites and benchmarks were not run locally. No performance improvement is
claimed. Hosted validation was not dispatched because this session remains local
at the user's request; the new workflow first runs after publication or dispatch.

Raw audit JSON, source/release checks, metadata and local-check logs are retained
outside Git in the session directory
`C:\Users\symbi\AppData\Local\Temp\Bonsai-DependencyAudit-b2037a17954346d992abd517055f1ecb`.

## Dependency refresh 2026-10-29

Checked every direct Rust dependency against current upstream releases and
refreshed the complete Cargo lockfile. Updated:

| Dependency | Previous | Current |
| --- | --- | --- |
| `zip` | 9.1.2 | 9.1.3 |
| Optional `simd-json` | 0.18.1 at `77ec801` | crates.io 0.19.0, upstream `00a71f6` |
| `openssl-probe` | 0.2.1 | 0.2.2 |
| `rustix` | 1.1.5 | 1.1.6 |
| `schannel` | 0.1.29 | 0.1.30 |
| `unicode-width` | 0.2.2 | 0.2.3 |
| .NET SDK | 10.0.402 | 10.0.403 |

[ZIP 9.1.3](https://github.com/zip-rs/zip2/releases/tag/v9.1.3) adds archive
hardening while retaining the existing minimal deflate features. The
[SIMD-JSON 0.19.0 release](https://github.com/simd-lite/simd-json/commit/00a71f6a931f7f365c23d7fc8d1f84ad693304d3)
contains both the Unicode fixes and tape-reuse API; only package-version metadata
differs from the previous source pin. The optional dependency now uses the exact
published crates.io release and Cargo.lock checksum instead of a Git submodule.
CI records the resolved registry package identity; source checkout instructions
and attribution follow the published crate. Ordinary parsing still uses
serde_json; this update supplies no new timing or qualification evidence.
The [.NET 10 release page](https://dotnet.microsoft.com/en-us/download/dotnet/10.0)
lists SDK 10.0.403, which is also installed on the validation host.

`cargo update --dry-run --verbose` resolves zero further compatible updates.
The newer `windows-future 0.100.0` remains incompatible with the selected
`windows 0.62.2` family, so its 0.3.2 pin remains deliberate. GitHub Actions,
PSScriptAnalyzer 1.25.0, cargo-audit 0.22.2 and LLVM 23.1.4 were already current.
The pinned nightly and independently qualified inference/speech source, binary
and model locks remain separate from routine library updates; runtime changes
still require the [reviewed promotion procedure](development-guide.md#prepare-a-runtime-lock-update).

Fresh `cargo-audit 0.22.2 --deny yanked` passed against RustSec revision
`ee1a49a2769a24a596afc1c20cb33748d2769586` (1,386 advisories, 490 dependencies):
zero known vulnerabilities and no yanked-package warnings. The same three Unix
accessibility maintenance notices documented above remain visible. Raw JSON is
retained outside Git at
`C:\Users\symbi\AppData\Local\Temp\Runner-DependencyAudit-ee63cbf6ce19403d9afcfef303829a94\audit.json`.

Format, all-feature Release Clippy (all targets), rustdoc and native builds passed
for the ten workspace packages outside the separately provisioned ASR/TTS engine
binaries. This includes the chat UI, broker, maintenance CLI and shared speech
service. Fixed existing strict-lint failures in the optional comparison path
without changing parsing behavior. Four exact Release regressions passed:

- `tests::zipped_installation_verifies_deflated_payload_and_rejects_drift`
- `speech_build::tests::source_staging_preserves_bytes_and_rejects_changed_archive_or_cache`
- `json_candidate::fixed_candidate_preserves_unicode_and_rejections`
- `json_candidate_parser::tests::reuse_preserves_parity_through_failures_and_size_changes`

PSScriptAnalyzer 1.25.0 passed all 62 PowerShell files; the strict Release C# build
passed for the one extracted helper under SDK 10.0.403. Complete suites and
benchmarks remain CI-only and were not run locally. Rust validation logs are in
`C:\Users\symbi\AppData\Local\Temp\Runner-DependencyUpdate-1793293946274`; standards evidence is in
`C:\Users\symbi\AppData\Local\Temp\SwiftBonsai-Standards-4f0235a8a0164835a225fed72989ab83`.
