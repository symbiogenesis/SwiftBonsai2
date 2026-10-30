# Native Qwen VoiceDesign inference

This library owns the native Qwen3-TTS 1.7B VoiceDesign GGML engine through a
small Rust binding to its public C ABI. Safe Rust implements the shared Philox
random draws for the semantic and predictor samplers. A reviewed build helper
routes both callers to Rust and adds bounded thread control and a SIMD compile
guard to a private copy of the external C++ core. The default talker
and codec are independently pinned Q8_0 GGUF conversions
of the named upstream checkpoints; the calling service must verify and retain all artifact files
before loading. Model identity, immutable URLs, byte lengths and SHA-256 values
are recorded in `model.lock.json`.

Only plain text plus a plain-language voice description are exposed. The native
design context must report `voice_design`, and the reusable-speaker context must
report `base`; speaker presets, user reference recordings and arbitrary native
options are absent from this interface. Generated audio stays
in memory and must be finite
mono 24 kHz PCM within the existing 120-second bound. Suspected token-ceiling
output is rejected. Upstream request-bearing diagnostics are discarded.

`Synthesizer::load(model, codec, voice_model)` initializes the owned design model, using two fewer
threads than the available CPU count to leave headroom for the UI and text
runtime (minimum one inference thread).
`load_with_threads(model, codec, voice_model, Some(count))` explicitly selects 1–256 threads
for controlled evaluation. The companion accepts the same override as
`--threads count`; it never changes the parent process's environment.
`synthesize(text, voice)` returns `Audio { samples, sample_rate }`.
`synthesize_owned(text, voice)` retains the independently allocated native
waveform and exposes validated samples through `AsRef<[f32]>`; the companion
uses this owner through bounded PCM conversion and HTTP writes.
`synthesize_stream(text, voice, sink)` emits validated mono 24 kHz chunks on the
native worker while generation continues. The sink is synchronously scoped and
serialized by a mutex; a rejected chunk or caught sink panic prevents successful
completion. Heard samples can precede a later generation failure. Run this in
its separately supervised process so cancellation, startup/synthesis deadlines,
memory admission and abrupt application exit remain enforced by the owner.
The library does not open audio devices or network listeners.

The [companion HTTP contract](../../tools/qwen-tts/README.md#http-contract)
owns request schemas, health capabilities, complete WAV and streaming PCM framing,
completion rules and wire limits. App provisioning, controls, CPU budgets and
service lifetime belong to the [speech operation guide](../../docs/development-guide.md#speech-setup-and-operation).

## Voice conditioning

The first request for a voice profile designs a fixed reference phrase with seed
42. Its text never comes from the message being read. The design model is released
before loading the separately pinned Base talker, whose speaker embedding and RVQ
reference codes are retained in memory and supplied with the reference transcript
to both complete and streaming synthesis. Every paragraph and message with that
profile reuses the same conditioning. An explicit profile change creates a new
reference; process restarts regenerate it from the same phrase, seed and profile.
Line breaks and request/conversation IDs never choose the reference.
This follows Qwen's [Voice Design then Clone workflow](https://github.com/QwenLM/Qwen3-TTS#voice-design-then-clone).
Only one talker model is resident at a time, and reference acquisition failures
fail speech without falling back to independently designed segment voices.

Voice bootstrap retains its independently allocated native waveform across Design
teardown, Base loading and synchronous reference extraction. The matching native
allocator releases it after extraction or any failed handoff. Pointer-identity
and model-free lifecycle checks cover success, Base load failure, extraction
failure and the reference limits in the [resource contract](../../docs/streaming-resources.md#native-speech-generation-and-streaming).
That contract owns sample limits and avoided-copy bounds; its
[evidence limits](../../docs/streaming-resources.md#evidence-limits) govern device qualification.

## Native ABI and failure handling

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

Set `BONSAI_TTS_SOURCE_DIR` to that complete source directory, `BONSAI_CMAKE`
to an installed CMake executable, and `BONSAI_TTS_COMPILER` to the provisioned
pinned ARM64 `clang-cl.exe`. Native Windows ARM64 with the Visual Studio
18 ARM64 C++ tools and SDK is required. The build uses Ninja Multi-Config and
LLVM's ARM NEON/FP16 kernels. A private-source compile guard rejects scalar
fallback; MSVC's missing ARM feature macros must not silently disable SIMD.
`BONSAI_TTS_NATIVE_BUILD_DIR` optionally
reuses an externally provisioned compatible native build.

The Cargo build script performs no downloads. Before a fresh native build, it
copies the verified source tree into its unique external build lane. A minimal,
reviewed overlay (`build_overlay.rs`) requires active ARM64 NEON kernels, adds a bounded atomic CPU-thread setter
and replaces the upstream logical-CPUs-divided-by-two heuristic when that setter
is used. Exact replacement anchors reject mismatched inputs. The immutable
provisioned source cache is never patched. Cached native libraries must include
this overlay; the hosted cache identity covers all build helper sources and
the embedded pinned Philox source anchor.
The build script builds only the external CPU
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

## Rust sampler port

`src/philox.rs` ports the pinned engine's Philox4x32-10 generator with no unsafe
code, allocations or additional dependencies. Both native stochastic samplers
call its private scalar C ABI export. The build overlay replaces the complete
native `philox.h` algorithm with that declaration and rewrites each single-draw
call, preserving the seed, subsequence and counter arguments. Changed source
anchors reject the build. The sampler policy and GGML graph stay upstream.

Wrapping integer arithmetic, signed seed/subsequence bit patterns and separate
f32 rounding match the pinned source. Its upper endpoint can round to `1.0`;
the port deliberately retains this behavior. Known-answer block and rounding
tests accompany a 4,411-case model-free comparison of the Rust scalar export
against the unmodified C++ source in `tests/reference/philox.h`. That source is
compiled only into the independent test oracle. The derived Rust module and
reference retain the attribution in `LICENSE.qwen-core`.

The source audit also considered the BPE tokenizer, sampling policy, mel
constants and resampler. BPE needs vocabulary/lifecycle integration and exact
Unicode/token qualification; sampling policy and mel changes need separate
numerical evidence. Resampling is outside the current already-24-kHz reference
path. The exact integer generator is the bounded production port justified
here. Full-model speech output, quality and resource acceptance remain CI work
under `PLAN.md`; bit parity alone makes no performance claim.

## Native waveform ownership

Complete WAV requests retain the native `qt_audio` allocation through bounded
PCM conversion and HTTP writes. `Synthesizer::synthesize_owned` validates the
whole waveform before returning an immutable owner; the service borrows its
samples and the matching `qt_audio_free` releases them after success, validation
failure or a failed write. The original vector-returning `synthesize` API remains
available. In pinned `pipeline-tts.cpp`, `fill_qt_audio` makes an independent
`malloc` allocation; `qwen.cpp` releases that allocation with `free`. No model
context owns or mutates it after synchronous synthesis returns, and Rust never
adopts it as a vector.

The [resource contract](../../docs/streaming-resources.md#native-speech-generation-and-streaming)
owns sample ceilings, conversion buffers and avoided-copy bounds. The service
disarms/joins its watchdog after inference returns and before response writes.
Native ownership preserves streaming PCM, voice conditioning, sampling, admission
and reserves.

Model-free regressions retain the original sample pointer, exercise the actual
pinned native release on success and invalid output, check exactly one owner
drop after validation/write failures, and compare complete HTTP/WAV bytes from
both storage variants. Device acceptance follows the
[resource evidence limits](../../docs/streaming-resources.md#evidence-limits)
and [PLAN.md](../../PLAN.md).

The [companion artifact contract](../../tools/qwen-tts/README.md#artifacts)
defines its explicit quantization selections and immutable verification boundary.
