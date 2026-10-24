# Native Rust Prism CPU reference

`bonsai-prism-runtime` is an experimental safe Rust implementation of the dense
Qwen35 text path used by Swift/Bonsai. It links no C/C++ inference library.
**Full-model parity, broker integration and target-device qualification are
pending.** Pinned publisher Prism remains the assessment and fallback engine.

## Implemented behavior

- Verified bounded GGUF loading through `bonsai-gguf`: file length/SHA-256 are
  checked before parsing, and payloads use the same retained handle. Windows
  denies write/delete sharing; other platforms require caller-enforced immutability.
- Immutable F32/F16/BF16/PQ2/PTQ1 tensors, checked dimensions, retained-storage
  budgets and optional byte-identical rank-one LoRA B sharing. Storage sharing
  does not implement adapter forward execution.
- Packed scalar matrix operations with PQ2 Q8_K and PTQ1 Q8_0 activation
  contracts, plus automatic baseline NEON on eligible Windows ARM64 operations.
  Scalar remains the reference/fallback; no expanded float matrix is retained.
  Synthetic integer/matvec parity does not establish whole-model parity or speed.
- Embeddings, Hadamard/sign transforms, RMS normalization, causal nonflash
  attention, text-interleaved MRoPE, GDN/convolution state, gated feed-forward
  layers and output projection. Attention retains FP16 query/probability
  conversion and exact FP16 KV bits; convolution/GDN state stays F32.
- Single-owner bounded conversations with greedy token streaming, backpressure,
  exact prefix checkpoints, edited prompts, reset, cancellation between decoder
  operations and rollback after evaluation or delivery failure. Streamed tokens
  are provisional until success. Failed restore preserves the public committed
  token prefix and closes admission until reset succeeds.
- `Runtime::generate_until` accepts an absolute monotonic deadline and rolls
  back expiry through the same transaction. Checks surround decoder operations
  and stream callbacks; synchronous work must return before expiry is detected,
  and rollback itself must complete before the method returns.
- Stable `Decoder::evaluate_into` reuses caller-owned logits; `evaluate` remains
  available. Qwen35 retains bounded graph/kernel scratch and stages per-layer
  state deltas. Outputs are shape/finiteness checked; errors clear direct output.
  Checkpoints own independent committed state/logits, never scratch aliases.
- `Decoder::ingest` skips output-only normalization/projection for intermediate
  uncached prompt tokens while validating computed state. Its compatibility
  fallback computes/validates then discards logits. Final prompt tokens and
  decode steps produce logits; exact prefix hits reuse saved output.
- `Decoder::ingest_block` provides bounded 1–256-token reference prefill.
  `Runtime::generate_with_prefill` and `generate_until_with_prefill` accept
  `Prefill { block_tokens }`; existing generation calls retain one-token blocks.
  The default block decoder preserves exact sequential arithmetic and checks
  cancellation/deadlines between tokens. Errors roll back the whole request.
  Optimized tiled graph execution and prefill performance remain unqualified.
- Full-attention checkpoints share sealed 64-token FP16 KV pages. Each snapshot
  copies the private mutable tail and complete F32 convolution/recurrent state;
  later append, reset or restoration cannot modify saved pages.
- Scoped prepared matvecs share activation preparation across compatible FFN,
  full-attention and recurrent sibling projections. Groups require the same
  exact input, transform/permutation, width and activation contract; mixed
  contracts prepare separately. Prepared storage cannot outlive its borrowed
  input or scratch. Exact synthetic checks do not establish device speed gains.
- Optional checkpoint replacement preserves older prefixes when capture fails
  or the replacement alone exceeds the available snapshot budget. Count-based
  eviction follows successful capture; byte-pressure eviction still precedes
  allocation to keep the hard snapshot bound.
- Failed decoder resets keep the public committed history and saved prefixes
  while closing admission. A successful reset clears them and restores admission.

The graph/quantizer reference is Prism
`73bfe6cc485554822f80c473a3c0c4f25e80f41b`, matching the assessment runtime.
The source-overlay build lock is a separate identity. Audited source paths/hashes
are in [kernels.rs](src/kernels.rs); MIT terms are in [NOTICE](../../NOTICE) and
[PRISM-LICENSE.txt](PRISM-LICENSE.txt). No Camelid/Orangu code is incorporated.

## Build and use

Default features are `cli` (JSON token driver) and `experimental-neon`.

| Cargo feature selection | Result |
| --- | --- |
| Defaults | Token driver/library with eligible NEON selection |
| `--no-default-features --features experimental-neon --lib` | NEON-enabled library without JSON driver |
| `--no-default-features --lib` | Scalar-only library |
| `--no-default-features --features cli` | Scalar-only token driver |

Use Rust 1.99.1 and an absolute manifest/target path from a unique system-temp
working directory. After fetching locked dependencies:

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

The driver requires an absolute model path, expected length/SHA-256, retained-model,
context/checkpoint budgets, prompt token IDs and generation limit. It validates
identity syntax, positive bounds, prompt-plus-generation capacity and the
256-stop-token limit before model I/O; vocabulary checks follow verified loading.
Supply exact stop IDs with `--stop-tokens` and token IDs from the reference
tokenizer. Output is one JSON token object per line followed by accounting.
Optional `--timeout-seconds <1..3600>` bounds generation from immediately before
the runtime request, including prefill, delivery and checkpoint work. Model
verification/loading and graph construction precede that deadline. Omit the
option to retain cancellation-only behavior. A timeout fails the command;
already streamed tokens are provisional and no success record follows.
Optional `--prefill-block-tokens <1..256>` selects the reference block boundary;
the default is one. `prefill_block_tokens` records the selected size. This changes
scheduling boundaries while retaining sequential graph operations.
The driver does not tokenize, implement stochastic sampling, launch the broker
or promote a profile.

## Memory and ownership

`--memory-bytes` bounds retained model storage, not process memory. Allow additional
headroom for parser/conversion scratch, graph workspace, active state, checkpoints
and allocator overhead. Optional `--state-bytes` sets the admitted maximum
live KV/convolution/recurrent element payload, from 1 byte through the default
4 GiB. `Qwen35::with_state_budget` exposes the same bound to library callers.
Admission charges the complete configured context using FP16 K/V and F32
convolution/recurrent state before state allocation; KV storage remains lazy.
The completion record includes `state_budget_bytes`. This is a payload bound,
not a bound on vector headers, allocator overhead, graph scratch or process RAM;
`--checkpoint-bytes` covers saved snapshots including in-flight rollback.
Process/hardware admission remains the caller's responsibility pending broker
integration.

Graph workspace retains high-water allocations across evaluations/reset. KV
pages grow within the context bound, sealing only complete pages. Checkpoint
admission charges every referenced immutable page per snapshot, page-table
capacity, copied private tails and recurrent storage. This conservatively
overcharges shared payload rather than depending on current reference counts;
the bound covers worst-case independent copies. Tight budgets can reject rollback
capture or retain fewer prefixes. RoPE reuse lasts one
evaluation attempt. Neither retained-model nor checkpoint budgets cover workspace.
Synthetic capacity reuse does not establish full-model allocation, peak-memory or
speed benefit; [R4.2](../../docs/native-rust-engine.md#ordered-work) remains open.

## SIMD and validation

[SIMD S0–S6](../../docs/simd-plan.md) owns kernel status and acceptance.
`MatvecBackend::Auto` selects baseline NEON for complete PQ2/PTQ1 rows when built
for Windows ARM64 MSVC with `experimental-neon` and ambient NEON. PQ2 widths
divisible by 256 retain Q8_K; odd-group PQ2 and PTQ1 retain four Q8_0 scales per
128 weights. Unsupported cases fall back to scalar, which also runs natively
on ARM64. Force it with `MatvecScratch::with_backend(MatvecBackend::Scalar)`;
`selected_backend()` reports actual selection.

Use `Qwen35::with_backend` or the token driver's `--backend auto|scalar|neon`
to apply that policy to the graph's retained projection scratch. The completion
record's `backend_requested` reports the requested policy. It does not claim
every projection used that backend; unsupported operations still use scalar.
`backend_last_operation` reports only the latest matrix attempt: actual selection,
build eligibility, scalar fallback reason, dispatch and success flags. Fallback
reasons distinguish explicit scalar selection, unsupported tensor kinds, disabled
NEON features and unsupported targets. `MatvecScratch::diagnostics` and
`Qwen35::backend_diagnostics` expose the same record. Reset, restore and graph
rejection before matrix preparation leave it unchanged; it can describe
provisional work. A final F32 output projection reports scalar even when earlier
packed projections used NEON.

`backend_statistics` reports cumulative preparation attempts/failures, matrix
attempts/successes, rejection before dispatch, scalar/NEON dispatches and each
scalar fallback reason. `MatvecScratch::statistics` and
`Qwen35::backend_statistics` expose these counts. Prepared siblings share a
preparation and each count as a matrix attempt. Counts include failed/provisional
work and survive graph reset/restore; they are not committed-token accounting.
Counters saturate at `u64::MAX` and retain no input data.

Target-only `fearless_simd` 1.1.0 uses safe core APIs under unchanged
`forbid(unsafe_code)`. Direct packed decode preserves scalar quantization and
ordered scales. Optional ISA detection, per-path memory and
performance qualification remain open.

[Foundation](../../.github/workflows/foundation.yml) runs the complete synthetic
native suite, formatting, Release Clippy/rustdoc, feature variants and non-ARM
fallback checks. Fixtures cover bounded loading, exact codecs/rounding, graph
state, output reuse, checkpoint isolation and transactional recovery. They do not
load the full model. Locally run focused named regressions, from the same external
working directory convention, for example:

```powershell
cargo +1.99.1-aarch64-pc-windows-msvc test --locked --offline --release `
    --manifest-path <repo>/src/prism-runtime/Cargo.toml --target-dir <temp>/build `
    --test runtime edited_turns_restore_complete_recurrent_prefix_and_failed_stream_rolls_back -- --exact
```

The [R0–R7 roadmap](../../docs/native-rust-engine.md) still requires profiling,
full-model logits/states/tokens, tokenizer/template and stochastic RNG parity,
adapter execution, broker/OS ownership, broader SIMD/Adreno support and device
correctness/TPS/quality/security acceptance. Full suites and every benchmark
remain CI-only under [validation policy](../../docs/validation.md).
