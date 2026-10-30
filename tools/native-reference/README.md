# Fixed PTQ1 numerical reference

This diagnostic compares two supplied IDs, `[248045, 872]`, against the unchanged
released native DLLs. The application and inference service remain Rust.
The native CLI's `llama-common`/shell startup dependency is avoided by resolving
the exact public C API directly. The reference collector is C++17 so function
pointer types and by-value parameter structures come from the pinned public
header instead of copied FFI declarations. It compiles no inference source.

`pins.json` binds both source archives and the native header. The wrapper builds
the exact Rust checker source, verifies the installed 63-file native package,
and retains ordinary worker ownership, artifact handles, fixed memory admission,
Job limits, physical reserve and deadlines. A ten-second model-free ABI/default-
parameter probe must pass before model admission or execution. Every model arm
uses a fresh process with 8K context, FP16 K/V, F32 recurrent/convolution state,
128/64 batch/microbatch and four native CPU threads.

```powershell
./scripts/Invoke-NativePtq1Numerics.ps1 -DataDirectory "$env:LOCALAPPDATA/SwiftBonsai2" -PlanOnly
```

Actual execution belongs only to the manually dispatched main-only
`native-ptq1-numerics.yml` workflow on the owner-managed native ARM64 Snapdragon
runner. It uses the shared physical-runner concurrency group and omits Actions
credentials from the complete capture process tree. Already provision the exact
current-weight PTQ1 export, approved adapter, native package, LLVM 23.1.4 and
Visual Studio 2026 ARM64 headers/libraries. No model download or fallback occurs.

The fixed budget is three native arms (absent adapter, loaded scale zero, loaded
1.3) and the same arms for Rust scalar, automatic and explicit NEON requests:
12 processes, each with a ten-minute deadline and one active model at a time.
All model arms and comparisons share a two-hour elapsed budget; each supervised
operation is capped by the remaining allowance. Backend requests alone do not
provide operation-level dispatch counters. Binary snapshots occupy approximately
3.53 GiB before metadata and compression; the wrapper requires at least 8 GiB
external disk headroom for snapshots, source and build outputs.
The upload selection includes only synthetic numerical
payloads, receipts and text diagnostics, excluding models, adapters and build
caches. Preserve it in durable owner storage before temporary artifacts expire.

`swift_ptq1_reference.cpp` saves every logits row and the complete native
`llama_state_seq_get_data` serialization for sequence zero. Its expected hashes
are meaningful together with the wrapper's retained artifact guards; the standalone
collector does not attest file authenticity. All writes are create-new. A single
state snapshot is capped at 512 MiB. It calls no sampler and generates no tokens.

`compare.py` decodes only the pinned single-stream text IMRoPE, non-replay hybrid
state format: 16 FP16 K/V layers and 48 F32 convolution/recurrent layers. It
handles ordinary and transposed V storage, requires exact positions, types,
extents and complete consumption, checks the Rust file inventory and rejects
nonfinite values. Each value retains `1e-3 + 1e-3 * abs(reference)` tolerance;
greedy-ID equality cannot override a value failure. Native capture, raw state,
Rust controls and every compared file receive SHA-256 bindings in the result.
Loaded-zero payload identity and changed nonzero logits are separate controls.

No actual-model capture is established by adding this tool. Until successful
retained device CI, its collector and decoder remain unqualified against the
complete artifact. A passing two-token comparison would qualify only that bounded
trajectory. Grouped SSM authoring basis needs native writer inputs/corrections,
which this tool does not capture; long context/output, service, resources and
independent quality remain separate. No default or admission change is authorized.

Focused model-free checks:

```text
python -B tools/native-reference/test_compare.py NativeStateTests.test_transposed_v_and_complete_recurrent_inventory_match_logical_rows
python -B tools/native-reference/test_compare.py NativeStateTests.test_state_decoder_rejects_truncation_trailing_bytes_and_changed_schema
python -B tools/native-reference/test_compare.py NativeStateTests.test_comparator_enforces_finite_values_exact_extents_and_original_tolerance
python -B tools/native-reference/test_compare.py NativeStateTests.test_control_binding_rejects_changed_adapter_scale_tokens_and_backend
```
