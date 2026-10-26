# Native Qwen VoiceDesign inference

This library owns the native Qwen3-TTS 1.7B VoiceDesign GGML engine through a
small Rust binding to its public C ABI. No C++ source is maintained in this
repository. It has no Python or LibTorch dependency. The talker and
codec are independently pinned Q8_0 GGUF conversions of the named upstream
checkpoint; the calling service must verify and retain both artifact files
before loading. Model identity, immutable URLs, byte lengths and SHA-256 values
are recorded in `model.lock.json`.

Only plain text plus a plain-language voice description are exposed. The native
context must report `voice_design`; speaker presets, reference recordings and
arbitrary native options are absent from this interface. Generated audio stays
in memory, is copied before its native allocation is freed, and must be finite
mono 24 kHz PCM within the existing 120-second bound. Suspected token-ceiling
output is rejected. Upstream request-bearing diagnostics are discarded.

`Synthesizer::load(model, codec)` initializes one owned CPU model.
`synthesize(text, voice)` returns `Audio { samples, sample_rate }`. Run this in
its separately supervised process so cancellation, startup/synthesis deadlines,
memory admission and abrupt application exit remain enforced by the owner.
The library does not open audio devices or network listeners.

The Rust binding retains the exact ABI-v6 structures and upstream defaults,
with compile-time layout checks. Ordinary native status failures remain redacted
Rust errors. Unlike the former C++ catch-all bridge, an unexpected C++ exception
escaping initialization, synthesis or release terminates the supervised speech
companion. The binding uses `C-unwind` and an aborting `catch_unwind` boundary;
it never lets a foreign exception cross a non-unwind Rust ABI, examines its
payload, or reuses potentially damaged model state. The owning UI reports speech
failure while retaining text chat. See
[Rust's foreign-unwind contract](https://doc.rust-lang.org/std/panic/fn.catch_unwind.html).

## Build boundary

Provision the reviewed immutable source archives outside Git before Cargo:

- Qwen engine `ServeurpersoCom/qwentts.cpp` revision
  `5916bf7c79dbb4e66fc059cdc507a79a23a28bb4`.
- Its GGML fork `ServeurpersoCom/ggml` revision
  `cc5df49a6b878db2b68b9a7b45976e279594627d`, placed at the engine's `ggml/`.

Set `BONSAI_TTS_SOURCE_DIR` to that complete source directory and `BONSAI_CMAKE`
to an installed CMake executable. Native Windows ARM64 with the Visual Studio
18 ARM64 C++ tools is required. `BONSAI_CMAKE_GENERATOR` can explicitly select a
compatible Visual Studio generator. `BONSAI_TTS_NATIVE_BUILD_DIR` optionally
reuses an externally provisioned compatible native build.

The Cargo build script performs no downloads. It builds only the external CPU
static `qwen-core` target; Cargo compiles all Runner-owned glue as Rust. The
third-party Qwen/GGML inference core still requires a C++ compiler. Shared libraries, dynamic backend
loading, OpenMP downloads, KleidiAI downloads, CUDA, Vulkan, Metal, BLAS, upstream
HTTP servers, CLIs and tests are excluded from its requested targets. MSVC ARM64
uses the engine's native feature probes; build and run on the intended ARM64
host. All compiler working directories and outputs must remain outside Git.

The third-party engine and GGML are MIT licensed. Their exact notices accompany
this crate as `LICENSE.qwen-core` and `LICENSE.ggml`. The original Qwen model is
Apache-2.0; preserve its attribution and notices when distributing model files.

Local builds and focused tests are allowed. Full suites and every benchmark
remain CI-only. Window launch or successful compilation alone does not qualify
speech content, voice quality, latency or combined memory use with Swift.
