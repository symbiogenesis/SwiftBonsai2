# Immutable LoRA B storage sharing

The Rust source overlay implements a lossless CPU allocation reduction for the
approved adapter. All 129 rank-one B tensors have identical F32 payloads.
The loader now gives each target its own tensor metadata and name over a
same-context view of one immutable allocation. It skips writes through aliases.
The model weights, adapter artifact, scales and graph arithmetic are unchanged.

Only ordinary CPU-buffer, contiguous F32 rank-one B tensors are eligible.
Shape, strides, buffer type, context and full byte equality must match. Signed
zero and NaN payload bits remain distinct. Candidate tensors are capped at
64 KiB; the temporary comparison pool is bounded to 64 entries and 1 MiB.
Ineligible, unequal or uncached payloads retain independent allocations.
Different adapter lifetimes and devices never share storage.

Before migration, the focused C++ native regression compiled with strict ARM64 MSVC options against
the verified pinned publisher DLLs. It found exactly **2,621,440 fewer backend
allocation bytes (2.5 MiB)** for 129 equal B vectors, preserved distinct names
and layouts, and obtained byte-identical native matrix-operation outputs for
independent and shared storage. It also checked payload/layout/device/context
and pool-limit fallback cases. This is an allocator/arithmetic correctness
result, not a full-model performance or quality verdict.

The sharing policy and regression are now implemented in the
[Rust Prism crate](../tools/prism/README.md), with no runtime dependencies.
The policy forbids unsafe code; documented C ABI pointer handling and native
test calls are confined to their boundary modules. Rust also owns metadata
extraction, one-handle file reads, bounds checks, tensor view creation and loading.
The external engine retains only call sites and exception translation; cbindgen
generates the C declarations from Rust outside the repository. Rust's focused
native regression again verifies the exact allocation saving, tensor names,
layouts and byte-identical matrix-operation output against pinned ARM64 ggml.

The [source lock](../tools/prism/source.lock.json) pins Prism commit
`73bfe6cc485554822f80c473a3c0c4f25e80f41b`, the complete 39,423,279-byte codeload
ZIP SHA-256 `5ee8738706229e84e85db7051d7a40797c91e22a7ffa8f4bfb8fea4bf2cb0293`,
and the before/after adapter-loader and CMake source hashes. The
[overlay applicator](../tools/prism/Apply-Overlay.ps1) rejects drift, ambiguous
insertion points and repeat application before writing. The
[build script](../scripts/Build-PrismEngine.ps1) requires that verified archive,
extracts a fresh external lane, applies the overlay and records source,
compiler, configuration, Rust static library and binary identities. It never installs a runtime,
launches a model or promotes a profile. `-PrepareOnly` performs source preparation
without a C++ compiler; it still builds Rust and generates its declarations.
The build route requires Clang for the full Prism ARM CPU
sources; this laptop currently lacks the actual clang-cl compiler.

Foundation CI builds the actual modified completion/server linked to Rust,
and builds the Rust regression against real ggml compiled with Clang on hosted
Ubuntu ARM64, then runs the correctness test and strict Rust checks.
That lane checks the modified loader translation unit as part of the engine;
it does not certify a Windows binary or Snapdragon throughput. The publisher
runtime remains the assessment executable until a separately verified Windows
build and the mandatory model correctness/TPS/quality gates qualify a replacement.

This implementation saves duplicate B payload, a small part of the previously
observed roughly 560 MiB Job-counter difference. It does not explain or recover
that entire diagnostic increment. Initial graph reservation and rank-one graph
fusion remain separate unimplemented candidates. There is no retraining,
adapter quantization, rank pruning or dense merge into the ternary base.
