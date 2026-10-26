# Native Canary-Qwen recognition

This Rust library runs the selected **NVIDIA Canary-Qwen 2.5B** checkpoint
through the CPU-only `transcribe-cpp` 0.4.1 binding. Its packaged native sources
come from `handy-computer/transcribe.cpp` commit
`4d7c1ccefd39e5a9ea36cca5f8fefc6dcc14b99b`. It needs no Python, NeMo, FFmpeg,
browser runtime or GPU provider.

The service must verify and retain the exact Q8_0 file in `model.lock.json`
before calling `Recognizer::load`. The library maps that immutable file,
verifies Canary architecture/variant/audio capabilities, and reuses one
four-thread session with an 8,192-token decoder context. It accepts normalized
mono 16 kHz samples for at most thirty seconds and returns complete text only.
Request cancellation discards partial text. Encoder work may finish before a
cooperative decoder cancellation is observed; the service still owns the process
deadline and memory limits.

Windows ARM64 native builds require clang-cl and Ninja, the ARM64 MSVC libraries,
and Windows SDK. ggml's ARM backend does not support MSVC cl.exe. Build with CPU
features only, `GGML_NATIVE=OFF`, and OpenMP disabled; the launcher/build tooling
owns that explicit child environment. Upstream build scripts were reviewed:
they compile/install vendored C++ sources and create a verified short build
junction outside Git, without downloading or executing speech models or Python.

The original model is Copyright NVIDIA Corporation, **CC BY 4.0**. The converted
GGUF preserves that model license; its immutable source revision is recorded in
the lock. transcribe.cpp and its Rust bindings are **MIT** licensed. Recognition
quality, accuracy and performance still require explicit native device
qualification; compilation and protocol tests alone do not establish them.

Focused Windows ARM64 verification successfully loaded the pinned Q8_0 model and
recognized the public JFK fixture from the same immutable upstream commit. This
establishes one real recognition path; it does not measure general accuracy or
performance. The Release analyzer and focused model-header regression also pass.
