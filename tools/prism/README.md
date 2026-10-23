# Rust LoRA storage sharing for pinned Prism

`bonsai-prism` replaces the repository's C++ B-vector sharing class and native
regression with Rust. The sharing policy in `src/pool.rs` forbids unsafe code.
The crate has no third-party dependencies. Its static library is linked into
Prism commit `73bfe6cc485554822f80c473a3c0c4f25e80f41b` through the C declarations
in `include/bonsai_prism.h`.

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
still inserts the necessary C++ calls into that pinned external loader; it
contains no sharing policy. The only repository header contains C ABI
declarations, with no executable C++ implementation.

`src/ffi.rs` is the narrow unsafe allowance for C ABI exports, allocation and
converting live caller-owned pointers to Rust borrows. The opaque pool has one
owner and is not accessed concurrently. Layouts are copied, payload slices
must be live and disjoint from pool state, and native owner tokens must remain
live and immutable until the pool is freed. Rust never dereferences a ggml
owner/context/buffer token. Null inputs and ineligible or mismatched lengths
fail closed. Foreign callers remain responsible for pointer validity.
`tests/native.rs` separately allows documented unsafe calls into pinned ggml.
The rest of the crate denies unsafe code and the policy forbids it.

## Build and checks

Use PowerShell 7.6, the pinned Rust 1.99.1 toolchain and a Clang-compatible
CMake/Ninja environment for the external engine. Download and verify the ZIP
identified by `source.lock.json` in external storage, then:

```powershell
$result = ./scripts/Build-PrismEngine.ps1 -SourceArchive $absoluteZip -BuildMode All
ctest --test-dir (Join-Path $result.EvidenceDirectory 'build-probe') --output-on-failure
```

The builder verifies the complete ZIP, extracts a fresh external temp lane,
checks both original source hashes, applies exact substitutions, and checks
both resulting hashes before writing. It builds the Rust static library,
records its hash and toolchain, builds pinned ggml for the Rust native test,
and links completion/server to Rust. It does not run tests, install binaries,
load a model or promote a profile. `-PrepareOnly` requires no compiler.
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
The exact native case is `native_storage_and_math`. Full suites and benchmarks
remain CI-only. Foundation checks Rust formatting, Clippy, documentation,
policy/FFI correctness, source drift/repeat rejection, native allocator/math
correctness, and the actual engine's Rust linkage on hosted Linux ARM64.

The upstream MIT notice is retained in `PRISM-LICENSE.txt` for loader snippets.
The complete engine sources remain in the verified external archive.
