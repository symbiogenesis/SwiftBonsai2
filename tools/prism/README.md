# Rust LoRA storage sharing for pinned Prism

`bonsai-prism` replaces the repository's C++ sharing, file-loading and tensor-view
logic and native regression with Rust. The policy in `src/pool.rs` forbids unsafe
code. `src/loader.rs` owns file reads, bounds checks, native metadata extraction,
view creation and backend writes. The crate has no runtime dependencies.
Pinned build dependency cbindgen 0.29.4 generates the C ABI from Rust into Cargo's
external output directory. No native header is checked into the repository.
The static library is linked into pinned Prism commit
`b87ca7225456b06b0975b8239c66b5aecf07e37c`.

Only ordinary CPU-buffer, contiguous F32 rank-one B vectors are eligible.
Context, buffer type, dimensions, strides and all payload bytes must match.
Signed zero and NaN payload bits remain distinct. Each candidate is at most
64 KiB; the cache holds at most 64 entries and 1 MiB. Existing matches remain
usable after either cap. Failed cache allocations retain independent storage.
Each native view keeps its own tensor name; only canonical owners are loaded.

The approved adapter's 129 identical 20,480-byte B vectors can avoid
**2,621,440 bytes (2.5 MiB)** of native payload allocation. The Rust native
regression checks that exact allocator difference and byte-identical ggml
MUL_MAT output. These checks do not qualify model throughput or output quality.

## Native boundary

The external Prism/llama.cpp engine remains C/C++. Replacing its inference
kernels, parser and platform backends is separate work. The source applicator
inserts only ownership, calls and exception translation into that loader.
The generated declarations and these call sites are the remaining interop;
payload I/O, copying, bounds checks and sharing behavior are authored in Rust.

The [native Rust engine TODO](../../docs/native-rust-engine.md) tracks full
inference replacement as M7/R0–R7: baseline profiling, reference parity, packed
CPU kernels, Rust runtime, Adreno execution and qualification. It preserves the
model's numerical/decoding contract and this component's storage behavior. The
rough 2× performance goal and conditional 3–5× upside are unmeasured; this crate
does not implement or qualify that replacement.

`src/loader.rs` is the audited engine boundary: it reads only the pinned tensor's
initial C layout fields and calls the public ggml/gguf C APIs. It owns one file handle and
a reusable fallible read buffer. Windows denies file writes/deletion during
loading; other platforms require the caller to preserve file immutability.
Native views skip loading because their canonical owner is loaded separately.
Null inputs, missing descriptors, changed file lengths, invalid offsets and
size mismatches fail closed. Opaque handles require exclusive access and native
objects must remain live and disjoint from Rust state. Foreign callers remain
responsible for pointer validity and the exact pinned engine layout.
`tests/native.rs` separately allows documented unsafe calls into pinned ggml.
The rest of the crate denies unsafe code and the policy forbids it.

## Build and checks

Use PowerShell 7.6, the pinned Rust 1.99.1 toolchain and a Clang-compatible
CMake/Ninja environment for the external engine. Download and verify the ZIP
identified by `source.lock.json` in external storage. Fetch the locked build
dependencies once from a unique external scratch directory, then:

```powershell
cargo +1.99.1 fetch --locked --manifest-path <repo>/tools/prism/Cargo.toml
$result = ./scripts/Build-PrismEngine.ps1 -SourceArchive $absoluteZip -BuildMode All
ctest --test-dir (Join-Path $result.EvidenceDirectory 'build-probe') --output-on-failure
```

The builder verifies the complete ZIP, extracts a fresh external temp lane,
checks both original source hashes, applies exact substitutions, and checks
both resulting hashes before writing. It builds the Rust static library,
records its hash and toolchain, builds pinned ggml for the Rust native test,
and links completion/server to Rust. Builds use locked offline dependencies.
It does not run tests, install binaries, load a model or promote a profile.
`-PrepareOnly` compiles Rust and generates its header, but requires no C++ compiler.
`-BuildMode Probe` builds only Rust and the focused native regression.
Windows Visual Studio generators select ClangCL for the engine.

Run Cargo from a unique system-temp working directory, with absolute manifest
and target paths. A focused safe-policy check is:

```powershell
cargo +1.99.1 test --locked --offline --release --manifest-path <repo>/tools/prism/Cargo.toml --target-dir <temp>/build --test policy pool_policy_preserves_existing_cpp_cases -- --exact
```

Native tests are opt-in with `--features native-tests --test native` and need
`BONSAI_PRISM_NATIVE_LIB_DIR` containing absolute ggml library directories
separated by the OS path separator. Static linking is the default. Reviewed
Windows import libraries additionally require
`BONSAI_PRISM_NATIVE_LINK_KIND=dylib` and matching DLLs on the process PATH.
The exact math case is `native_storage_and_math`; a separate rejection case
checks actual F16/strided tensors, missing descriptors and invalid file ranges.
Full suites and benchmarks remain CI-only. Foundation checks Rust formatting, Clippy, documentation,
policy/FFI correctness, source drift/repeat rejection, native allocator/math
correctness, and the actual engine's Rust linkage on hosted Linux ARM64.

The upstream MIT notice is retained in `PRISM-LICENSE.txt` for loader snippets.
The complete engine sources remain in the verified external archive.
