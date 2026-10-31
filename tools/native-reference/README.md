# Five-minute PTQ1 correctness smoke

The owner-laptop trial is one native reference plus one Rust Auto process, each
evaluating the single supplied ID `[248045]` with the approved adapter at scale
1.3. **The entire owner job, including setup, loading, comparison, cleanup and
artifact upload, has a five-minute hard limit.** The wrapper starts its separate
240-second work clock before preparation and caps external calls by the remaining
allowance, reserving the remainder of the job for cleanup/upload. This is one
bounded trial; omitted controls are not split into repeated jobs.

No-adapter, loaded-zero identity, changed nonzero logits, scalar, explicit NEON,
writer authoring basis, quality, performance, full context and service remain
pending. A passing result covers only one Auto-requested token at scale 1.3.
Backend requests do not provide operation-level dispatch counters. No default or
admission change is authorized.

The application, ordinary inference service and reference collector are Rust.
The isolated collector resolves the unchanged released native DLL API without the
CLI's shell dependency. Its private `repr(C)` declarations are bound to the exact
pinned public header. Compile-time guards check all 62 field offsets, three sizes
and alignments before any native structure return is read. The collector compiles
no inference implementation; the reference DLL engine remains native.

`fixtures/collector-abi-v1.txt` retains 369 independently captured native layout,
type, enum, default/configured parameter and returned-aggregate observations.
Adjacent provenance binds the exact headers, compiler, DLLs, original driver and
license. Rust computes observations from its own declarations and the actual DLLs;
expected records are not used to generate observations. The original C++ driver
is removed. Seven focused tests retain success, every native failure phase,
cleanup order, bounded output, create-new collisions and partial evidence.
Returned native errors release acquired resources in order; an unexpected foreign
exception aborts the isolated collector before damaged native state can be reused.

`pins.json` binds native and Rust source archives and the native header. Capture
tools must be built in hosted CI and delivered with `capture-tools.json`:
`SchemaVersion: 2`, `RunnerRevision`, `NativeSource`, `RustSource`,
`CollectorLanguage: rust`, `CollectorSourcePath`, `CollectorSourceSha256`,
`AbiControlSha256`, `AbiProvenanceSha256`, `Compiler`, `RustToolchain`, and exactly two `Files`
entries (`Name`, `Bytes`, lowercase `Sha256`) for `swift-ptq1-reference.exe` and
`swift_model_check.exe`. The wrapper checks the current Runner revision, source
pins, Rust source/control digests, binary hashes/sizes and ARM64 PE architecture. Both binaries
and their receipt are guarded during execution. There is no owner build fallback.
Supply the separately hosted-prepared maintenance executable through `ToolPath`
or `BONSAI_TOOLS_PATH`.

Hosted preparation uses the pinned Rust compiler/LLD for the collector and its
model-free tests; it no longer downloads native source headers or invokes a C++
compiler for this tool. The native source pins remain provenance for the DLL ABI.
Before owner model admission, both the original JSON ABI probe and the complete
live 369-record observation set must match the frozen native control. Duplicate,
missing, extra, changed or overflowing records reject the trial, as do changed
expected control bytes after receipt validation. These checks share the existing
240-second work allowance and do not replace the logits/state comparison.

```powershell
./scripts/Invoke-NativePtq1Numerics.ps1 -DataDirectory "$env:LOCALAPPDATA/SwiftBonsai2" -PlanOnly
```

Actual execution belongs only to the main-only manual
`native-ptq1-numerics.yml` workflow on the existing native ARM64 Snapdragon runner,
with its physical-runner concurrency group and credential-clean child. The exact
PTQ1 export, approved adapter and native package must already be provisioned.
One model runs at a time. The shared policy retains 15.5 GiB admission, 11.5 GiB
Job commit and 4 GiB physical reserve. Source preparation and compilation run in
hosted CI; all owner preparation is included in its 240-second work budget.
Snapshots occupy approximately 301 MiB before metadata/compression; the wrapper
requires 1 GiB external disk headroom. The upload selection excludes weights,
adapters and build caches. Archive credential-free evidence durably before
temporary artifacts expire.

The collector saves one logits row and complete native sequence-zero state. Its
expected artifact identity is meaningful together with the wrapper's retained
guards; the standalone collector does not attest authenticity. All writes are
create-new and each state snapshot is capped at 512 MiB. It samples no token.
The native sequence header must be magic `0xaf143cd8`, then sequence ID zero.
The strict decoder then requires the pinned single-stream text IMRoPE, non-replay
hybrid layout: 16 FP16 K/V layers and 48 F32 convolution/recurrent layers, exact
positions/types/extents and complete consumption. Control JSON, stdout and logits
are bounded before reads. Every value must be finite and satisfy
`1e-3 + 1e-3 * abs(reference)`; greedy-ID equality cannot override a value failure.
The complete one-token comparison is 129 files / 39,504,384 values.

Model-free synthetic checks cover transposed V, recurrent inventory, the native
magic/sequence header, truncated/tailing input, finite values, exact extents,
the unchanged tolerance, control binding and pre-read limits. No actual-model
smoke is established by adding this tool. Writer authoring basis remains pending
even if all numerical values match; native writer inputs/corrections are absent.

```text
python -B tools/native-reference/test_compare.py NativeStateTests.test_transposed_v_and_complete_recurrent_inventory_match_logical_rows
python -B tools/native-reference/test_compare.py NativeStateTests.test_state_decoder_rejects_truncation_trailing_bytes_and_changed_schema
python -B tools/native-reference/test_compare.py NativeStateTests.test_comparator_enforces_finite_values_exact_extents_and_original_tolerance
python -B tools/native-reference/test_compare.py NativeStateTests.test_control_binding_rejects_changed_adapter_scale_tokens_and_backend
python -B tools/native-reference/test_compare.py NativeStateTests.test_capture_pre_read_bounds_reject_oversized_json_and_logits
```
