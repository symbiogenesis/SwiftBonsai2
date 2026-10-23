# Native Rust Prism CPU reference

`bonsai-prism-runtime` provides an experimental, safe Rust implementation of the
dense Qwen35 text path used by Swift/Bonsai. It links no C/C++ inference library.
The pinned publisher Prism runtime remains the assessment and fallback engine.
This implementation has **not passed full-model parity or device qualification**.

The crate contains:

- A loader factored from `bonsai-gguf`, verifying file length and SHA-256 before
  parsing, retaining exact metadata and reading payloads through that same file
  handle. Windows denies write/delete sharing while loading. On other platforms
  the caller must keep the verified file immutable throughout loading.
- Immutable Rust-owned F32/F16/BF16/PQ2/PTQ1 tensors, checked dimensions and a
  cumulative retained-storage budget. The existing bounded LoRA B sharing policy
  can share byte-identical rank-one payloads within a model.
- Scalar packed matrix operations preserving PQ2's Q8_K activation quantization
  and PTQ1's distinct Q8_0 activation quantization. No persistent expanded float
  matrix is created. Float accumulation and SIMD reduction order can differ from
  the publisher CPU implementation; no bit-identical full-model claim is made.
- An opt-in baseline NEON PQ2 integer kernel using safe core Fearless SIMD APIs.
  Scalar remains the default on ARM64 and other targets. Exact synthetic
  differential checks preserve scalar quantization and floating block order;
  target-device speed and whole-model parity remain unqualified.
- Embeddings, explicit Hadamard/sign transforms, RMS normalization, full causal
  nonflash attention (including FP16 query/probability conversion), text
  interleaved MRoPE, Gated Delta Net/convolution state, gated feed-forward
  layers and output projection. KV entries round through FP16; their current
  backing storage is F32, so retained cache bytes exceed a native FP16 layout.
  Flash attention has a distinct reduction/rounding contract and is not implemented.
- A bounded single-owner conversation with exact prefix checkpoint restoration,
  greedy token streaming, backpressure, cancellation between decoder operations,
  edited prompts, reset and rollback after failed evaluation or stream delivery.
  Failed rollback closes admission until reset succeeds. Streamed tokens are
  provisional until the request returns successfully. A failed backend restore
  still leaves the public token view at the previous committed prefix.
- Reusable caller-owned logits through `Decoder::evaluate_into`, with the owned
  `evaluate` API retained for compatibility. The Qwen35 graph retains bounded
  workspace, per-layer state deltas and quantized activation scratch; the runtime
  consumes logits before the next mutation and validates their shape/finiteness.
  Reset and prefix restoration invalidate prior output without discarding the
  reusable workspace. Checkpoints still own independent committed state/logits;
  scratch never substitutes for a rollback snapshot. Errors clear direct output,
  and runtime cancellation/failure restores the prior committed conversation.

The graph/quantizer reference is Prism commit
`73bfe6cc485554822f80c473a3c0c4f25e80f41b`, matching the assessment runtime.
The newer source-overlay build lock is a separate identity. Audited source paths
and hashes are recorded in `kernels.rs`; the MIT terms are retained in the root
`NOTICE` and `PRISM-LICENSE.txt`. No Camelid or Orangu code is incorporated.

## Build and use

The default `cli` feature enables the JSON token driver shown below. Library
consumers can disable default features, or build with `--no-default-features
--lib`, to omit JSON dependencies. The inference API uses native typed GGUF
metadata and compact arrays, and the safe Prism sharing policy needs no C-header
generator. These feature choices do not change the model's numerical contract.

Use Rust 1.99.1. Run Cargo from a unique system-temp directory, specifying absolute
manifest and target paths. For example, after fetching locked dependencies:

```powershell
$repo = 'C:\Users\symbi\source\repos\SwiftBonsai2'
$lane = Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-Native-' + [guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($lane) | Out-Null
Push-Location $lane
try {
    cargo +1.99.1-aarch64-pc-windows-msvc build --locked --offline --release `
        --manifest-path (Join-Path $repo 'src/prism-runtime/Cargo.toml') `
        --target-dir (Join-Path $lane 'build')
    & (Join-Path $lane 'build/release/bonsai-prism-runtime.exe') --help
} finally { Pop-Location }
```

The driver requires an absolute model path, expected artifact length and SHA-256,
a retained-model byte budget, context and checkpoint budgets, prompt token IDs,
and a generation limit. It validates artifact identity syntax, positive resource
bounds, prompt plus generation capacity and the 256-stop-token limit before
opening or hashing the model. Vocabulary checks require the verified model.
`--stop-tokens` supplies the exact stop IDs. It emits one
JSON token object per line, followed by completion accounting. It does not infer
tokenization, stop tokens or a sampler from an approximate substitute. Use token
IDs produced by the exact reference tokenizer. It does not install/promote a
profile, launch the broker, or alter the configured fallback.

`--memory-bytes` bounds retained model storage. Parser scratch, transient payload
conversion, graph scratch, active sequence state, checkpoint copies and allocator
overhead need additional headroom. It is not an OS process limit. Graph state has
its own checked bound; `--checkpoint-bytes` bounds all saved snapshots including
the in-flight rollback state. Hardware/process admission remains the caller's
responsibility until the broker ownership integration is qualified.

Graph workspace now retains its high-water allocations across evaluations rather
than releasing temporary vectors after each token. This can remove repeated
allocation while retaining more idle memory. The graph's geometry/context bounds
still apply; the retained-model and checkpoint budgets do not cover workspace.
Synthetic numerical and storage-reuse tests do not measure full-model allocation
counts, peak memory or speed. R4.2 still requires same-model prefill/decode
allocation and memory qualification in CI when requested.

KV backing vectors also grow geometrically up to the validated context size,
reducing repeated prefix copies at the cost of unused reserved capacity between
growth steps. Reset retains that storage. The conservative checkpoint estimate
charges actual vector capacities, so tight checkpoint budgets can reject a
rollback capture or retain fewer reusable prefixes as this reserve grows.
RoPE frequency reuse lasts only for one evaluation attempt; it carries no
position cache across reset, restoration or failed evaluation.

## Validation and remaining work

The [CPU SIMD plan](../../docs/simd-plan.md) records the first implemented S1/S2
work: compile with `--features experimental-neon`, then explicitly request
`MatvecScratch::with_backend(MatvecBackend::ExperimentalNeon)` when calling
`matvec_into`. `selected_backend()` reports the actual per-call selection.
Eligibility requires native Windows ARM64 MSVC with ambient baseline NEON and
PQ2 rows whose column count is a multiple of 256. Other formats, odd-group PQ2,
feature-disabled builds and other targets use scalar. Scalar runs natively on
ARM64 as well; it does not imply x86 emulation. Compilation alone does not
enable NEON: `matvec`, the Qwen35 graph and the token driver still use scalar
defaults. No CLI switch or daily profile is promoted by this kernel experiment.

The safe core-only `fearless_simd` 1.1.0 dependency is target-specific and
optional; no local unsafe allowance or procedural macro is added. Packed
unpack/dot consumes immutable bytes directly, keeps the original scalar
quantizer and ordered scale operations, and creates no expanded matrix.
Three focused raw-integer tests and four bitwise matvec/fallback/error tests
cover the first path. Strict native Release checks and assembly inspection pass;
see the plan for compiler/source identities and limitations. The feature also
works with `--no-default-features --lib`. Separate PTQ1 qualification, optional
ISA detection, fuller dispatch diagnostics, profiling and S6 promotion remain
pending. No throughput or whole-model parity benefit is established.

Focused fixtures cover malformed/truncated/over-budget GGUFs, exact metadata,
immutable B sharing, native activation rounding and packed dot results, synthetic
full/recurrent graph state, and transactional edited-prefix/cancellation behavior.
Foundation CI runs the complete native crate suite and strict formatting,
Release Clippy and documentation checks on Windows ARM64. Locally run only
focused named regressions, for example:

```powershell
cargo +1.99.1-aarch64-pc-windows-msvc test --locked --offline --release `
    --manifest-path <repo>/src/prism-runtime/Cargo.toml --target-dir <temp>/build `
    --test runtime edited_turns_restore_complete_recurrent_prefix_and_failed_stream_rolls_back -- --exact
```

This is a token-level reference, not the accepted daily chat replacement.
The [R0–R7 plan](../../docs/native-rust-engine.md) still requires baseline profiling,
reference full-model logits/states and token comparisons, tokenizer/chat-template
and stochastic RNG parity, adapter forward execution, broker count/completion and
readiness integration, OS resource ownership, broader SIMD/Adreno execution, and paired
device correctness/TPS/quality/security acceptance. Full suites and every
benchmark remain CI-only. No speedup is claimed.
