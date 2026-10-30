# Dependency audit and choices

Retain **Serde 1.0.229, serde_json 1.0.154, Tokio 1.54.1 and the native Rust UI**.
The reviewed alternatives did not qualify. This page combines dependency usage,
security/footprint evidence and parser/runtime comparisons; [PLAN.md](../PLAN.md#deferred-dependency-experiments)
owns pending experiments. Pins live in [Cargo.toml](../Cargo.toml) and [Cargo.lock](../Cargo.lock).

## Scope and security result

The 2026-10-26 review covered all 12 workspace manifests, the lockfile, direct
source usage, normal/build/dev features, cached manifests and selected build
scripts, including Windows ARM64, all-target and optional `json-candidate` graphs.
It was not an exhaustive transitive/native-source audit.

| Audit snapshot | Packages / database | Result |
| --- | --- | --- |
| Baseline `f9c7ade2de57eb537f397429eed42a297740fd06` | 449 locked; RustSec `3279c99911354c400e1d41e47c7d60e39258b147`, 1,378 advisories | No known vulnerabilities or yanked packages |
| Concurrent MXC integration; then ZIP 9.1.2 | 492 locked; same database | Same result |
| [2026-10-29 refresh](#dependency-refresh-2026-10-29) | 490 dependencies; RustSec `ee1a49a2769a24a596afc1c20cb33748d2769586`, 1,386 advisories | Same result with `cargo-audit 0.22.2 --deny yanked` |

All scans retained three unmaintained-package notices: `zvariant 5.15.1`
([0370](https://rustsec.org/advisories/RUSTSEC-2026-0370.html)), `zbus_names 4.3.5`
([0371](https://rustsec.org/advisories/RUSTSEC-2026-0371.html)), and `zvariant_derive 5.15.1`
([0372](https://rustsec.org/advisories/RUSTSEC-2026-0372.html)). They enter through
eframe → egui-winit → AccessKit's Unix AT-SPI/zbus implementation and do not compile
into supported Windows binaries. Upstream moved them into zbus 6; AccessKit's
compatible requirements still select zbus 5. Keep notices visible; removing
accessibility or forcing an incompatible major is inappropriate. Exact pins
provide reproducibility, not continuing advisory notification.

## Changes made

- Removed the broker's unused direct `sha2` edge; hashing remains in `bonsai-artifacts`.
  This claims no binary-size or speed gain.
- Updated ZIP 9.1.0 → 9.1.2 on 2026-10-26, retaining disabled defaults and only
  `deflate-flate2-zlib-rs`. Upstream hardened [symlink extraction](https://github.com/zip-rs/zip2/pull/1116)
  and [Unicode-path renaming](https://github.com/zip-rs/zip2/pull/1177). Runner already
  uses bounded custom extraction, so this is preventive maintenance, not evidence
  of an exploitable Runner flaw. The later refresh selects 9.1.3.
- Added [Dependency audit CI](../.github/workflows/dependency-audit.yml) for relevant
  main pushes, weekly and manual runs. Pinned cargo-audit fetches RustSec anew;
  vulnerabilities/yanks fail, maintenance notices remain visible without blanket ignores.

## Dependency footprint and duplication

Counts are distinct name/version pairs in Windows ARM64 **normal + build** trees,
including internal/root crates, at the 2026-10-26 review. They are not linked-code
counts, binary sizes or runtime costs. Cargo unifies requested features, so each
executable's graph can differ from the workspace graph.

| Selection | Baseline | With MXC integration |
| --- | ---: | ---: |
| Workspace | 211 (199 external) | 262 (250 external) |
| Chat | 162 | 228 |
| Broker | 43 | 126 |
| Maintenance | 35 | 123 |
| ASR | 48 | 130 |
| TTS | 39 | 126 |
| CI planner | 36 | 36 |

Baseline Windows counts rise to 213 with dev and 219 with all features/dev;
all-target counts are 425/427/434 respectively. Foreign-platform lockfile entries
do not imply those runtimes ship in Windows binaries.

| Genuine baseline version duplicate | Reason |
| --- | --- |
| `hashbrown` 0.16.1 / 0.17.1 | GPU allocator versus other UI consumers |
| `miniz_oxide` 0.8.9 / 0.9.1 | PNG versus flate2 |
| `rustc-hash` 1.1.0 / 2.1.3 | Naga/wgpu versus type-map |
| `syn` 2.0.119 / 3.0.9 | Windows/zerocopy versus newer derive macros |
| `windows-sys` 0.52.0 / 0.61.2 | winit versus current Windows consumers |

Requirements are incompatible; do not force unification with patches. MXC adds
`base64 0.22.1` beside CI's 0.23.1 (separate executables), `getrandom 0.2.17` beside
0.4.3, and `windows-sys 0.59.0` via winreg. Same-version log/memchr/Serde rows can
be host/target or feature build units, not multiple library versions per executable.

### MXC has the largest new footprint

`mxc-sdk =1.0.1` plus `flatbuffers =25.12.19` broadens every shared-process-boundary
consumer. VM/WSL/Hyperlight features are disabled; the Nanvix download is gated
by the disabled `microvm` feature. The SDK nevertheless unconditionally requests
Tokio `full`, URL/IDNA/ICU, chrono, temp files, UUIDs, older random/Base64 APIs,
broad Windows bindings and SHA-2 defaults. Cargo features are additive; our
`default-features = false` cannot undo another crate's requests.

Runner's PSEC calls use synchronous Windows APIs and generated FlatBuffers.
The broader dependencies serve SDK Sandbox/WSLC lifecycle, proxy parsing and
learning-mode tracing. Neither 1.0.1 nor main `ad54c69452147bd4ca15d027388ad6074903dbff`
has a PSEC-only switch. A minimal module would need loader/handle/capability/policy
APIs plus separated errors, UTF-16 helpers, telemetry and build dependencies;
simply making dependencies optional breaks compilation. See the [manifest](https://github.com/microsoft/mxc/blob/v1.0.1/src/mxc-sdk/Cargo.toml).

Hidden PSEC exports remain unsupported compatibility interfaces. A compile-only
consumer of documented `v1::spawn`/`Containment::Process` also reproduced the graph
and passed ARM64 checking. [MXC #1579](https://github.com/microsoft/mxc/issues/1579)
requests supported backend/capability-selective builds without weakening validation.
Dependency counts alone prove no slowdown: build tools and linker elimination
matter. Consider a separate supervisor only if measured build/size savings justify
its lifecycle/IPC cost; containment must remain mandatory and fail closed.

### Renderer features are now explicit

The initial eframe `wgpu` feature re-enabled `egui-wgpu/default` and broad wgpu
defaults. `wgpu_no_default_features` plus a pinned direct wgpu edge now selects
`std`, `parking_lot`, `dx12`, `vulkan`, `gles`, `wgsl`. All three supported Windows
backends remain real fallbacks; upstream `NativeOptions` selects `PRIMARY | GL`
unless overridden by the environment.

Production browser/Metal defaults are excluded. At the original baseline the
all-target chat graph falls 368 → 366; the Windows graph is unchanged, and GPU
test dependencies still request Metal/all Windows backends. No size/speed gain
is claimed. Removing a Windows backend requires a separate support/device decision.
Font parsers, color-font rendering, `vello_cpu` and `glifo` serve actual native UI
functionality, not an inference framework or browser frontend.

Format, Release Clippy, docs/build and ten DX12 capture layouts passed; default
and forced-DX12 previews were responsive and closed. This does not qualify every
backend/device or inference performance. Evidence:
`C:\Users\symbi\AppData\Local\Temp\Bonsai-WgpuRefine-2143bcc4c15b4cdfbfa4b483ea4d7937`.

### Native inference source review

The external Rust runtime pins optional `fearless_simd` 1.1.0 to source revision
`b8a50a2c9ce03ee5384038ae8e6f45749662e2f6`, crate SHA-256
`3296eb97d869113eaf148a6bdf01884e90b3828ad7ae2340b0f93362fc158611`, matching its
Cargo.lock. Default `std` has no external dependencies; `libm`/the macro crate
are unused, and there is no build script. The review covers slice/array bounds,
widening, bitcasts, baseline NEON and the dependency's internal unsafe boundary;
runtime source forbids unsafe code. See the
[immutable source](https://github.com/linebender/fearless_simd/tree/b8a50a2c9ce03ee5384038ae8e6f45749662e2f6/fearless_simd),
[SIMD contract](simd-plan.md) and [license terms](licensing.md#reviewed-dependencies).

## Direct dependency decisions

| Dependency | Decision |
| --- | --- |
| `serde`, `serde_json`, `tokio` | Retain: [JSON](#json-selection)/[runtime](#runtime-screening) screens did not qualify replacements. Tokio explicitly enables io-util/net/rt/signal/sync/time; test-util is dev-only, while MXC broadens features. |
| `http`, `httparse` | Complementary typed policy and bounded wire parsing; no general web framework needed. |
| `subtle` | Constant-time bearer comparison; preserve disabled release debug assertions. |
| `sha2`, `zip` | Artifact verification and minimal deflate extraction at ZIP 9.1.3; custom extraction policy remains necessary. |
| `socket2`, `windows-sys` | Pre-bind options/backlog/exclusive sockets and selected process/credential/audio/accessibility APIs. |
| `windows`, `windows-future` | Compatible WinRT microphone permission/capture; distinct from windows-sys. Retain future 0.3.2 with windows 0.62.2; 0.100 is incompatible. |
| `eframe`, `wgpu` | Native UI, accessibility, clipboard, fallback/system fonts and all three explicit Windows backends. |
| `egui_kittest`, `egui_system_font_provider` | Test harness/font checks; capture-only splitting offers little while production needs graphics. |
| `toml`, `ureq` | CI-only manifest parsing (parse/serde/std) and synchronous HTTPS (Windows Schannel, otherwise Rustls); ureq defaults/compression disabled. |
| `transcribe-cpp` | Static CPU ASR, defaults disabled; reviewed CMake script downloads no source and explicitly enables MSVC optimization. |
| `simd-json` | Exact optional qualification-only crates.io pin; ordinary requests still use serde_json. |
| `mxc-sdk`, `flatbuffers` | Required security integration; pursue upstream modularity. |
| Internal crates | Retain shared process/credential/audio/policy/artifact boundaries; no unused direct path edge found. |

ASR/TTS intentionally use separate executables and GGML versions. ASR disables
Metal/optional accelerators and pins CPU-only CMake. TTS disables accelerators,
BLAS, OpenMP, KleidiAI, dynamic backends, tests and examples. Combining their
native libraries risks version conflicts; RustSec does not fully cover bundled C/C++.
Speech only needs `VerifiedFile` from `bonsai-artifacts`, whose ZIP APIs are
unconditional. Splitting them may reduce standalone compile scope, but workspace
feature unification/linker elimination limit likely benefit. Pending work stays in PLAN.

## JSON selection

The gate requires **at least 5% lower equal-weight geometric-mean time and no case
more than 5% slower**. Six workloads cover plain/escaped small inputs,
8/32/128-message histories and near-1 MiB bodies. Timing includes immutable-input
preparation, schema/history validation, canonical serialization, allocations/copies
and destruction. Discard one warmup round; rotate order over nine measured rounds.
Unicode, duplicate/unknown fields, shape, bounds and canonical-byte correctness
must pass before timing is interpreted.

| Candidate | Time / serde_json | Worst case | Decision |
| --- | ---: | ---: | --- |
| serde_json | 1.0000× | 1.0000× | Retain |
| Sonic 0.5.10 | 1.0026× | 1.1160× near limit | No qualifying gain |
| Fused Serde visitor | 1.0408× | 1.2442× small plain | No qualifying gain |
| SIMD-JSON 0.18.1 | Not timed | Strict Unicode mismatch | Reject |
| Fixed source `ff8f5c8` | 1.3807× | 1.9802× near limit | No qualifying gain |
| `ff8f5c8`, padded tape | 1.3919× | 1.7595× near limit | No qualifying gain |

SIMD-JSON 0.18.1 accepted `{"messages":[{"role":"user","content":"\ud800"}]}`
and canonicalized the lone surrogate to `\u0000`; production rejects it as
value-free `ChatSchema`. [Fix `ff8f5c8`](https://github.com/simd-lite/simd-json/commit/ff8f5c8bb75dfc56803321c2f5a92656c7de842f)
rejects lone high surrogates, malformed escapes and second escapes outside the
low-surrogate range. It differs from the evaluated v0.18.1 despite the same version.
Sonic's 3.5–4.7% history gains and the visitor's 6.9% 128-message gain did not offset
regressions; these are workload-specific prototype decisions, not library rankings.

| Fixed-source CI evidence | Runner / artifact | Supplemental scale cases |
| --- | --- | --- |
| [Copying path](https://github.com/symbiogenesis/SwiftBonsai2Runner/actions/runs/40405967479) | `9448284a4057fb70fb039520debef44250851a76`; `json-candidate-40405967479-1` | 1.7289–1.7979× production |
| [Padded path](https://github.com/symbiogenesis/SwiftBonsai2Runner/actions/runs/40408246424) | `6a582c1a61ef0ebb2c28df256b08bc927cedd6f9`; `json-candidate-40408246424-1` | Up to 1.9431× production |

Both ran on Windows ARM64 Cobalt 100 with the pinned nightly. Correctness passed,
but all six original cases were slower and both original/overall screens failed.
Artifacts retain rounds, decisions and compiler/CPU/OS/source identities. These
separate runs do not isolate copy-removal speedup. [Follow-up #1](https://github.com/symbiogenesis/SwiftBonsai2Runner/issues/1)
uses `fill_tape_padded` then `Tape::deserialize` (`padded-tape-serde`), avoiding the
Serde entry point's internal copy while timing immutable-body copy, padding,
allocation, validation, serialization and destruction.

### Current candidate and pending gate

The optional `json-candidate` now pins published **0.19.0**, upstream
[`00a71f6`](https://github.com/simd-lite/simd-json/commit/00a71f6a931f7f365c23d7fc8d1f84ad693304d3),
with Cargo.lock registry checksum; no submodule is required. It includes Unicode
fixes and [PR #495](https://github.com/simd-lite/simd-json/pull/495) for
[issue #494](https://github.com/simd-lite/simd-json/issues/494), merged 2026-10-27 as
[`77ec801`](https://github.com/simd-lite/simd-json/commit/77ec80113f9990ef7fd89b0ee859e7415bfc4827).
Only package-version metadata differs from that commit. `Deserializer::from_tape`
recovers/resets tape allocation while preserving borrowing/validation.

`padded-tape-serde-reuse` reuses padded input, parser buffers and tape across requests:

| Screen | Included work |
| --- | --- |
| Cold | Scratch setup/destruction plus all per-request work below. |
| Reused | Immutable-body copy, validation, canonical serialization, output destruction and decimal/error fallback; excludes scratch setup/destruction. |

Explicit adapter scales and SIMD parse failures fall back to production parsing:
correctly rounded SIMD conversion can differ from serde_json's default without
either being wrong. Supplemental scale cases include fallback cost. Main-only
`json-candidate.yml` runs correctness before the original six-case and supplemental
5%/5% gates; any case over 5% slower rejects qualification.
**Both cold/reused screens must pass; results remain pending.** Updating/building
the crate neither promotes it nor erases earlier losing results.
The reviewed open [AVX-512 #475](https://github.com/simd-lite/simd-json/pull/475)
targets x86; [#479](https://github.com/simd-lite/simd-json/pull/479)/[#477](https://github.com/simd-lite/simd-json/pull/477)
concern cleanup/`no_std`, not evidence for changing this ARM64 decision.

## Runtime screening

Reused loopback echo connections used 256/4096-byte frames and one/nine concurrent
connections, excluding setup/warmup. Ratios are geometric means of median exchange
time, not full broker/model qualification.

| Candidate | Time / Tokio | Decision |
| --- | ---: | --- |
| Tokio current-thread | 1.0000× | Retain |
| smol 2.0.2 | 1.0264× | No advantage |
| Compio 0.19.2 | 1.0214× | No advantage |
| Blocking threads | 0.7357× | Does not qualify replacement |

Blocking gains came from nine client threads versus one for async candidates;
one-connection ratios were roughly flat (0.9702×/1.0234×). The broker admits one
active model request. The screen omitted thread creation/stacks, HTTP policy,
queues, disconnect cancellation and absolute deadlines.

## Evidence limits

The [original comparison artifact](https://github.com/symbiogenesis/SwiftBonsai2Runner/actions/runs/40039211957/artifacts/12538059153)
retains hashes, calculations, exclusions and compiler/CPU/OS identities from a
four-core Cobalt 100 Windows ARM64 host. This is broker-preparation evidence,
not Snapdragon inference or stable-versus-nightly evidence. The pinned nightly
supports branch-sensitive broker lending; this screen proves no compiler speedup.
All benchmarks run in CI only.

## Validation and evidence

The original audit passed format, Release Clippy/docs/build for `bonsai-artifacts`,
`bonsai-broker`, `bonsai-tools`; named regressions covered archive paths, expansion
budgets and hostile source names. actionlint 1.7.12 and YAML/event/permission checks
passed. That review dispatched no hosted validation and ran no local full suite
or benchmark; it claimed no performance gain. Raw audit/source/metadata/logs:
`C:\Users\symbi\AppData\Local\Temp\Bonsai-DependencyAudit-b2037a17954346d992abd517055f1ecb`.

## Dependency refresh 2026-10-29

| Dependency | Previous | Reviewed selection |
| --- | --- | --- |
| `zip` | 9.1.2 | 9.1.3 |
| Optional `simd-json` | 0.18.1 at `77ec801` | crates.io 0.19.0 at `00a71f6` |
| `openssl-probe` | 0.2.1 | 0.2.2 |
| `rustix` | 1.1.5 | 1.1.6 |
| `schannel` | 0.1.29 | 0.1.30 |
| `unicode-width` | 0.2.2 | 0.2.3 |
| .NET SDK | 10.0.402 | 10.0.403 |

[ZIP 9.1.3](https://github.com/zip-rs/zip2/releases/tag/v9.1.3) adds archive hardening
without changing minimal deflate features. SIMD-JSON's registry update adds no
qualification evidence. The [.NET release page](https://dotnet.microsoft.com/en-us/download/dotnet/10.0)
listed installed SDK 10.0.403. At review, `cargo update --dry-run --verbose` found
zero further compatible updates; windows-future 0.3.2 remained deliberate.
Actions, PSScriptAnalyzer 1.25.0, cargo-audit 0.22.2 and LLVM 23.1.4 were current.
Nightly, inference/speech sources/binaries and model locks remain separately
qualified through the [runtime promotion procedure](development-guide.md#prepare-a-runtime-lock-update).

All-feature Release Clippy (all targets), format, rustdoc and native builds passed
for ten workspace packages outside separately provisioned ASR/TTS binaries.
Optional-comparison strict-lint fixes did not change parsing. Exact Release cases:

- `tests::zipped_installation_verifies_deflated_payload_and_rejects_drift`
- `speech_build::tests::source_staging_preserves_bytes_and_rejects_changed_archive_or_cache`
- `json_candidate::fixed_candidate_preserves_unicode_and_rejections`
- `json_candidate_parser::tests::reuse_preserves_parity_through_failures_and_size_changes`

PSScriptAnalyzer passed 62 scripts; strict Release C# passed the one extracted
helper under SDK 10.0.403. Full suites/benchmarks remained CI-only and were not run locally.
Evidence directories under `C:\Users\symbi\AppData\Local\Temp\`:

| Evidence | Path suffix |
| --- | --- |
| Fresh audit JSON | `Runner-DependencyAudit-ee63cbf6ce19403d9afcfef303829a94\audit.json` |
| Rust update checks | `Runner-DependencyUpdate-1793293946274` |
| Standards | `SwiftBonsai-Standards-4f0235a8a0164835a225fed72989ab83` |
