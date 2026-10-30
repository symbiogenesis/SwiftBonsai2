# Headless UCRT argv source candidate

The [patch](headless-ucrt-argv.patch) removes the reproduced legacy completion
startup failure in a CPU-enabled source build on this Snapdragon X2 Elite Extreme.
The same-source control exits `0xC0000142`; the candidate's `--version` exits zero
under the unchanged headless worker policy. The [manifest](headless-ucrt-argv.json)
and [model-free evidence](headless-ucrt-argv.evidence.json) retain exact source,
compiler, binary, dependency and supervision identities. Runner's executable lock
and profile defaults remain unchanged. This is an upstream review artifact.

The source is the MIT-licensed Prism revision
[`73bfe6cc485554822f80c473a3c0c4f25e80f41b`](https://github.com/PrismML-Eng/llama.cpp/tree/73bfe6cc485554822f80c473a3c0c4f25e80f41b).
Its `common/arg.cpp` uses `CommandLineToArgvW` for Unicode argument repair.
Read-only PE inspection confirms that this imports `SHELL32.dll` through
`llama-common.dll`. The candidate replaces that MSVC/clang-cl path with UCRT wide
argv initialization. The rebuilt DLL imports `_configure_wide_argv`, `__p___wargv`
and `__p___argc` from `api-ms-win-crt-runtime-l1-1-0.dll`, and no longer imports
`SHELL32.dll`. The paired startup result identifies this source change as a
workable fix for the reproduced source-build failure; it does not identify every
possible failure in the independently built released binary.

UCRT configuration is guarded by a thread-safe function-local static, reusing a
preexisting wide startup table. Reconfiguring UCRT would overwrite its pointer
without freeing its old allocation; repeated parsing instead borrows one table.
Wildcard expansion is disabled when this helper initializes it. Existing UTF-8
conversion and the argument-count repair guard remain in place. Configuration
failure keeps the existing empty-repair fallback. MinGW retains its original
shell parsing and `LocalFree` path.

The compatibility evidence covers Runner's `ProcessSpec` command-line encoding.
It does not establish equivalence between UCRT and shell parsing for every
arbitrary raw command line, an externally preconfigured wildcard-expanding CRT,
other CRT/toolchain versions or MinGW. UCRT startup entry points require review
against the intended compiler/SDK when integrating this candidate.

## Retained checks

The native ARM64 LLVM 23.1.4 build includes the CPU backend, with
`GGML_NATIVE=OFF` and baseline NEON. It uses MSVC 14.51.36231 headers/libraries,
Windows SDK 10.0.26100.0, Release, Ninja and at most four compiler jobs. This
compiler differs from the released runtime's Clang 20.1.8. No optional ISA or
numerical execution is qualified by the build.

Each process uses the ordinary required headless supervisor, a one-process
512 MiB kill-on-close Job, a ten-second deadline, 64 KiB per output stream and
verified artifact handles retained through teardown. The control's fixed version
probe fails with unsigned exit `3221225794`; the candidate's probe succeeds.
Seven named parsing-only cases also succeed:

- Empty prompt and a nonexistent Unicode model path.
- Japanese, accented and supplementary-plane prompt characters.
- Spaces, embedded quotes, trailing backslashes and backslashes before a quote.
- Tabs/newlines, literal wildcard text and leading/trailing spaces.

Every case queries and requires effective Win32k denial, extension-point denial,
remote-image denial and low-integrity-image denial. It repeats real parsing with
`argc=7` and synthetic parsing with `argc=5`, checks exact prompt/model bytes,
and verifies that the CRT table pointer stays unchanged. The fixture disables
the separate optional prompt-escape transform so it checks argument bytes.
The model paths do not exist; only `common_params_parse` is called, with no model
or backend initialization. Earlier fixture mistakes (missing required model
option and default prompt escaping) retain their bounded failure receipts in the
external evidence directory named in the JSON.

The final patch was applied to pristine pinned files with explicit LF settings;
all three resulting file hashes match the tested source. The directory's
`.gitattributes` preserves patch bytes. Build logs, original controls and bounded
raw receipts remain in the external evidence directory. The checked JSON contains
no credentials, inherited environment, hostname or user path.

## Reproduce

Verify the exact source archive, patch length and SHA-256 against the manifest.
From native ARM64 developer PowerShell with the recorded MSVC/SDK environment,
use the recorded LLVM compiler and separate absolute source/build directories.
Build a control before applying the patch if reproducing the paired result.

```powershell
git -c core.autocrlf=false -c core.eol=lf -C $source apply --check $patch
git -c core.autocrlf=false -c core.eol=lf -C $source apply $patch
cmake -S $source -B $build -G Ninja -DCMAKE_BUILD_TYPE=Release `
  "-DCMAKE_C_COMPILER=$llvm/bin/clang-cl.exe" `
  "-DCMAKE_CXX_COMPILER=$llvm/bin/clang-cl.exe" `
  "-DCMAKE_LINKER=$llvm/bin/lld-link.exe" `
  -DBUILD_SHARED_LIBS=ON -DGGML_CPU=ON -DGGML_NATIVE=OFF `
  -DGGML_BACKEND_DL=OFF -DGGML_CPU_ALL_VARIANTS=OFF -DGGML_OPENMP=OFF `
  -DGGML_CPU_KLEIDIAI=OFF -DGGML_LLAMAFILE=OFF `
  -DGGML_CUDA=OFF -DGGML_VULKAN=OFF -DGGML_OPENCL=OFF -DGGML_HEXAGON=OFF `
  -DLLAMA_BUILD_TESTS=ON -DGGML_BUILD_TESTS=OFF `
  -DLLAMA_BUILD_EXAMPLES=OFF -DLLAMA_BUILD_SERVER=OFF -DLLAMA_BUILD_APP=OFF `
  -DLLAMA_BUILD_UI=OFF -DLLAMA_OPENSSL=OFF -DLLAMA_SUBPROCESS=OFF `
  -DLLAMA_LLGUIDANCE=OFF -DLLAMA_BUILD_COMMIT=73bfe6c -DLLAMA_BUILD_NUMBER=10787
cmake --build $build --target llama-completion test-headless-argv --parallel 4
```

The fixture deliberately requires worker creation mitigations and is an explicit
build target, outside ordinary CTest. Use Runner's existing checked maintenance
tool and `Invoke-BonsaiTool -Command completion` from an external directory, with
`ciOnly=false`, no runtime override, all generated EXE/DLL files listed as exact
`artifacts`, the limits above and no model artifacts. Probe `llama-completion.exe`
with only `--version`. For `test-headless-argv.exe`, pass
`@('--model','模型/test.gguf','--seed','1','--prompt','日本語 🪴 café')` for the
Unicode case; the retained evidence lists all seven exact argument arrays.
Each successful fixture prints its observed mitigation values. Other invocations
must retain absolute parent lifetime and owned teardown through the same tool.
Do not treat an inspection-policy launch as this check.

Next, review/integrate this source in the owning runtime, build a reviewed CPU
release and qualify actual-model loading, Unicode paths, continuation, numerical
state, adapters and cancellation under production containment. Only then update
Runner's immutable runtime lock and repeat the X2 device CI preflight and thread
screen on settled `main`. No model, benchmark or full suite ran locally, and this
pilot supplies no throughput, memory-saving, energy or profile-promotion verdict.
