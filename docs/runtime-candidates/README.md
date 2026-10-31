# Runtime source candidates

Each patch has its own base, manifest and evidence. These source-review artifacts
neither change Runner's release lock nor qualify a production runtime.

## Row-parallel candidate

The [patch](oxibonsai-row-parallel.patch) retains Prism commit
`5ff5f3c4719362da6efcbb58114b702db282e5b2` for upstream review; the
[manifest](oxibonsai-row-parallel.json) binds base archive and patch length/SHA-256. It
is source delivery, without release qualification or a Runner lock override.

The base is Prism main `ff3d54de662ddd951dc0ca51d9581129b55239bf`, newer than Runner's
pinned `02c8a24b31ac1c35f2ffd3530f6a753ce440bdc0`. The candidate retains that newer
base's arithmetic, including native PTQ alias handling. Applying it directly to the
older released source is unsupported. Review the intervening source changes
independently before any release/pin update.

### Implementation

The opt-in `parallel-rows` feature adds a private persistent Rayon 1.12.0 pool.
`--row-workers` selects an explicit budget of 1 through 16 for the Swift JSONL worker or
private HTTP child. One retains serial execution and creates no pool; requesting
multiple workers without the feature rejects before loading a model. Each worker
requests a 512 KiB stack. Pool creation, stack commitments and scheduling overhead
remain part of device resource/performance qualification.

The live prepared-projection path borrows original packed weight rows and shared Q8
activations, partitions pre-reserved output into disjoint contiguous chunks, and retains
at most one reusable error slot per worker. It preserves each row's scalar/NEON
arithmetic, selects the earliest failing row deterministically and clears failed output.
Unsupported floating projections use their existing serial path. Graph reset/checkpoint
recovery retain the chosen scheduling policy and existing cooperative cancellation
boundaries. There is no stronger preemption claim or measured throughput gain.

### Reproduce the source change

Verify patch length and SHA-256 against the manifest. In a clean Prism checkout at the
exact base, apply it with:

```powershell
git switch --detach ff3d54de662ddd951dc0ca51d9581129b55239bf
git am C:/absolute/path/to/oxibonsai-row-parallel.patch
```

`git am` reproduces the tested commit's entire tree in an independent worktree. The patch
includes independently authored Rust, focused checks, CI feature variants and build/CLI
docs; runtime/docs attribute OxiBonsai's scheduling idea and pin.

### Focused validation

Passing validation covers formatting, Release Clippy for enabled runtime/server
library/binary/tests, scalar-only library Clippy and Release rustdoc. Nine test
executions cover seven cases; worker policy and server options run with the feature
both enabled and disabled:

| Target | Focused case |
| --- | --- |
| `parallel_rows` | `enabled::parallel_rows_preserve_packed_arithmetic_and_prepared_siblings` |
| `parallel_rows` | `enabled::parallel_rows_clear_failed_output_and_recover_with_serial_float_fallback` |
| runtime library | `kernels::parallel_rows::tests::pinned_parallel_rows_preserve_both_activation_contracts` |
| runtime library | `kernels::parallel_rows::tests::row_tasks_borrow_original_storage_and_return_first_row_failure` |
| `graph` | `graph_parallel_rows_preserve_logits_checkpoints_and_recovery` |
| `parallel_rows` | `row_worker_policy_is_explicit_and_bounded` |
| server library | `native_child::tests::native_child_rejects_unknown_duplicate_relative_and_unbounded_options` |

Use the pinned toolchain from an external working/build directory, with an absolute
manifest and target directory. For example:

```powershell
cargo +nightly-2026-10-22 test --release --locked --offline `
  --manifest-path C:/absolute/prism/Cargo.toml --target-dir C:/external/build `
  --package prism-inference --features parallel-rows --test parallel_rows `
  enabled::parallel_rows_preserve_packed_arithmetic_and_prepared_siblings -- --exact
```

Checks cover actual Swift widths 5120/17408/6144, odd-group activation fallback, generic
PQ2's fourth symbol, the base's PTQ aliases, malformed/nonfinite payload recovery,
pinned accumulation, shared-storage identity and complete synthetic graph
checkpoint/reset behavior. Full suites, benchmarks, actual-model/device qualification
and release publication remain separate work under [PLAN.md](../../PLAN.md#cpu-kernels)
and the [runtime lock-update
procedure](../development-guide.md#prepare-a-runtime-lock-update).

## Headless UCRT argv candidate

The [patch](headless-ucrt-argv.patch) fixes reproduced CPU source-build completion
startup on Snapdragon X2 Elite Extreme: same-source control exits `0xC0000142`,
candidate `--version` exits zero under unchanged headless policy. The
[manifest](headless-ucrt-argv.json) and [model-free
evidence](headless-ucrt-argv.evidence.json) bind
source/compiler/binary/dependency/supervision identities. This upstream review artifact
leaves Runner's lock and defaults unchanged.

The source is the MIT-licensed Prism revision
[`73bfe6cc485554822f80c473a3c0c4f25e80f41b`](https://github.com/PrismML-Eng/llama.cpp/tree/73bfe6cc485554822f80c473a3c0c4f25e80f41b).
Its `common/arg.cpp` uses `CommandLineToArgvW` for Unicode argument repair. Read-only PE
inspection confirms that this imports `SHELL32.dll` through `llama-common.dll`. The
candidate replaces that MSVC/clang-cl path with UCRT wide argv initialization. The
rebuilt DLL imports `_configure_wide_argv`, `__p___wargv` and `__p___argc` from
`api-ms-win-crt-runtime-l1-1-0.dll`, and no longer imports `SHELL32.dll`. The paired
result fixes this source-build failure, without diagnosing every possible failure of the
independently built released binary.

UCRT configuration is guarded by a thread-safe function-local static, reusing a
preexisting wide startup table. Reconfiguring UCRT would overwrite its pointer without
freeing its old allocation; repeated parsing instead borrows one table. Wildcard
expansion is disabled when this helper initializes it. Existing UTF-8 conversion and the
argument-count repair guard remain in place. Configuration failure keeps the existing
empty-repair fallback. MinGW retains its original shell parsing and `LocalFree` path.

Compatibility covers Runner's `ProcessSpec` encoding, not arbitrary raw command lines,
externally configured wildcard-expanding CRTs, other CRT/toolchains or MinGW.
Integration must review UCRT entry points against its compiler/SDK.

### Retained checks

The native ARM64 Release/Ninja build uses LLVM 23.1.4, MSVC 14.51.36231, SDK
10.0.26100.0, CPU enabled, `GGML_NATIVE=OFF`, baseline NEON and at most four jobs. The release
used Clang 20.1.8. No optional ISA or numerical execution is qualified.

Each process uses the ordinary required headless supervisor, a one-process 512 MiB
kill-on-close Job, a ten-second deadline, 64 KiB per output stream and verified artifact
handles retained through teardown. The control's fixed version probe fails with unsigned
exit `3221225794`; the candidate's probe succeeds. Seven named parsing-only cases also
succeed:

- Empty prompt and a nonexistent Unicode model path.
- Japanese, accented and supplementary-plane prompt characters.
- Spaces, embedded quotes, trailing backslashes and backslashes before a quote.
- Tabs/newlines, literal wildcard text and leading/trailing spaces.

Every case queries and requires effective Win32k denial, extension-point denial,
remote-image denial and low-integrity-image denial. It repeats real parsing with
`argc=7` and synthetic parsing with `argc=5`, checks exact prompt/model bytes, and
verifies that the CRT table pointer stays unchanged. The fixture disables the separate
optional prompt-escape transform so it checks argument bytes. The model paths do not
exist; only `common_params_parse` is called, with no model or backend initialization.
The JSON-named external evidence also retains bounded fixture-failure receipts for a
missing model option and default prompt escaping.

With explicit LF settings, applying the patch to pristine pinned files reproduces all
three tested source hashes; `.gitattributes` preserves patch bytes. External evidence
retains build logs, controls and bounded raw receipts. Checked-in JSON excludes
credentials, inherited environment, hostname and user paths.

### Reproduce

Verify the exact source archive, patch length and SHA-256 against the manifest. From
native ARM64 developer PowerShell with the recorded MSVC/SDK environment, use the
recorded LLVM compiler and separate absolute source/build directories. Build a control
before applying the patch if reproducing the paired result.

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

The fixture deliberately requires worker creation mitigations and is an explicit build
target, outside ordinary CTest. Use Runner's existing checked maintenance tool and
`Invoke-BonsaiTool -Command completion` from an external directory, with `ciOnly=false`,
no runtime override, all generated EXE/DLL files listed as exact `artifacts`, the limits
above and no model artifacts. Probe `llama-completion.exe` with only `--version`. For
`test-headless-argv.exe`, pass `@('--model','模型/test.gguf','--seed','1','--prompt','日本語
🪴 café')` for the Unicode case; the retained evidence lists all seven exact argument
arrays. Each successful fixture prints its observed mitigation values. Other invocations
must retain absolute parent lifetime and owned teardown through the same tool. Do not
treat an inspection-policy launch as this check.

The owning runtime must review/integrate/build a CPU release and qualify model loading,
Unicode paths, continuation/state, adapters and cancellation under production
containment before a Runner lock update and settled-main X2 CI preflight/thread screen.
The retained checks are model-free and establish no speed, memory, energy or promotion
verdict. Benchmarks and full suites remain CI-only.

### Hosted headless comparator preparation

The explicit [hosted candidate workflow](../../.github/workflows/publisher-headless-candidate.yml)
uses [Build-HeadlessPublisherCandidate.ps1](../../scripts/Build-HeadlessPublisherCandidate.ps1)
to apply this exact patch to the verified source archive. It builds separate
`llama-completion`, `llama-server`, `llama-bench` and argument-fixture executables.
The existing installed publisher bundle, Runner runtime locks, app selection,
worker mitigations and filesystem grants are unchanged. The workflow has no
owner-device job and downloads no model artifacts.

The compiler archive and its ARM64 executables reuse the retained LLVM 23.1.4
identities. The build records its hosted image, installed MSVC/SDK, compiler,
supervisor, CMake flags/cache and all packaged file digests. It uses four compile
jobs and restores the publisher toolchain's global `armv8.7-a` and floating-point
flags. Shared CPU libraries remain enabled; dynamic backend loading, OpenMP,
optional accelerators, UI, TLS fetching and subprocess support are disabled.
This differs from the released Clang 20 bundle and the earlier baseline-NEON
argument pilot; numerical and performance qualification remain pending.

A bounded static PE audit checks every candidate EXE/DLL, including normal and
delay imports, then follows mandatory system imports through the hosted image's
API-set mapping. GUI modules reject the candidate. Optional Windows delay imports
are retained in the report rather than described as absent: core Windows DLLs
themselves expose optional GUI APIs. Export forwarders and runtime `LoadLibrary`
calls are outside this static audit. The separate effective-mitigation probes
remain required; a named-import graph alone is not startup qualification.

On the hosted runner, completion/server receive only `--version` and the benchmark
tool receives only `--help` (its pinned CLI has no version switch); the seven
argument fixtures call parsing without loading their nonexistent model paths.
The unchanged supervisor retains all candidate artifact handles through a
one-process 512 MiB Job, ten-second deadline and 64 KiB output bounds. The fixture
requires effective Win32k, extension-point and unsafe-image denial. Actions
credentials are removed from the preparation child before these probes.

The [2026-10-30 hosted result](../evidence/headless-publisher-startup.json) records
all ten probes passing at Runner `e11c5a289528bc435b0a1339071a26d6063f0745`:
completion/server version, benchmark help and seven argument fixtures under the
unchanged mitigations. Independent review verified all 14 packaged file digests
and 13 ARM64 binaries. The full public candidate, exact source ZIPs, bounded logs,
probe receipts and API proofs are durably archived with a digest manifest.
Initially unavailable job metadata later recovered; the archived run and prepare
job API snapshots both report success. This establishes hosted model-free startup.
Model loading, numerical and inference performance qualification and production
runtime promotion remain pending.

The resulting artifact is a source-bound candidate for review, not an installed
runtime or a model speed result. A passed hosted preparation still needs a
separately authorized owner-device trial within the existing five-minute total
budget and the independent numerical, adapter, resource and quality gates before
any release or lock update.

### Bounded X2 model-free startup

The [2026-10-30 owner-device receipt](../evidence/x2-owner-startup-20261030.json)
records all ten probes passing on X2E94100 with the exact hosted bundle and final
source-bound supervisor. Independent review checked all fourteen bundle digests,
supervision sidecars and seven explicit mitigation confirmations. The final capture
took 2,306 ms; it is startup evidence, without an inference performance verdict.
The exact executed supervisor, Runner source ZIP, raw receipts, earlier capture and
review are retained in the digest-verified owner qualification archive. Model loading,
numerical/state/adapter qualification, release integration and production pin updates
remain pending.

[Invoke-HeadlessPublisherStartup.ps1](../../scripts/Invoke-HeadlessPublisherStartup.ps1)
checks an already extracted copy of that exact hosted candidate using a prebuilt
maintenance tool. It accepts only the checked-in hosted package identities, rejects
extra/missing or redirected bundle entries, and supplies every pinned file to native
verification with handles retained through worker teardown. It runs the same ten fixed
version/help/argument probes through the ordinary required headless supervisor, without
model artifacts or runtime overrides. The owner X2 identity, supervisor digest, source,
hosted artifact, exact probes and bounded supervision receipts are retained separately.

```powershell
./scripts/Invoke-HeadlessPublisherStartup.ps1 `
  -CandidateDirectory 'C:/external/archived-candidate/candidate' `
  -ToolPath 'C:/external/prebuilt/bonsai-tools.exe' `
  -OutputDirectory 'C:/external/fresh-startup-evidence' -WorkBudgetSeconds 60
```

The aggregate allowance includes setup, file verification, probes and owned teardown,
reserving ten seconds for reporting. Each worker has a ten-second deadline, a
one-process 512 MiB Job and 64 KiB per-stream output bounds. Budget exhaustion or a
failed probe stops the remaining probes and retains their pending status. No downloads,
local builds, production bundle replacement, model inference or benchmark runs occur.
Charge this capture together with all other owner-device work against the same
five-minute total. A pass establishes only these exact model-free startup invocations;
model loading, Unicode model paths, numerics, adapters, cancellation, release integration
and the settled-main thread screen remain separate gates.
