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
  provisional until the request returns successfully.

The graph/quantizer reference is Prism commit
`73bfe6cc485554822f80c473a3c0c4f25e80f41b`, matching the assessment runtime.
The newer source-overlay build lock is a separate identity. Audited source paths
and hashes are recorded in `kernels.rs`; the MIT terms are retained in the root
`NOTICE` and `PRISM-LICENSE.txt`. No Camelid or Orangu code is incorporated.

## Build and use

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
and a generation limit. `--stop-tokens` supplies the exact stop IDs. It emits one
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

## Validation and remaining work

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

This is a token-level scalar reference, not the accepted daily chat replacement.
The [R0–R7 plan](../../docs/native-rust-engine.md) still requires baseline profiling,
reference full-model logits/states and token comparisons, tokenizer/chat-template
and stochastic RNG parity, adapter forward execution, broker count/completion and
readiness integration, OS resource ownership, SIMD/Adreno execution, and paired
device correctness/TPS/quality/security acceptance. Full suites and every
benchmark remain CI-only. No speedup is claimed.
