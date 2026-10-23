# Safe Rust experimental ternary packing

The unpublished [`bonsai-bitcos` package](Cargo.toml) implements PQ2
validation, exact native PTQ packing, compact BITCOS matrix storage, and scalar
Q8_K dot products. It has no dependencies, forbids unsafe code, and uses checked
Release arithmetic, fallible allocation, and typed errors at public boundaries.
It does not load a model, register a ggml type, or enable an inference profile.

Rust is the default for new repository-owned systems components. The pinned
external Prism/llama.cpp engine and its existing LoRA storage overlay remain
separate migration work; their C/C++ trust boundary is unchanged.

## Representation

The source contract is [Prism commit 73bfe6cc](https://github.com/PrismML-Eng/llama.cpp/tree/73bfe6cc485554822f80c473a3c0c4f25e80f41b).
The [BITCOS paper](https://arxiv.org/html/2609.16338v1) motivates presence plus
compacted signs; this experiment's block/row layout is its own encoding.
The upstream MIT notice remains in [PRISM-NOTICE.txt](PRISM-NOTICE.txt).

Each independently indexed row contains a positive even number of 128-weight
groups. A group stores two unchanged little-endian FP16 scale bytes, sixteen
presence bytes, and `ceil(nonzero / 8)` sign bytes. Bits are ordered low first.
Absent positions restore original PQ2 code 1; compact signs 0 and 1 restore
codes 0 and 2 respectively. Unused high sign bits must be zero. Finite negative
and subnormal scale bits are preserved. Zero/nonfinite weight scales and PQ2
code 3 are rejected. PTQ uses the same symbols/scales in its native 28-byte layout.

A matrix owns one compact payload and a uint32 offset per row plus a sentinel.
The serialized byte count is exactly:

```text
4 * (rows + 1) + sum[groups](18 + ceil(nonzero / 8))
```

Packing validates and counts before allocating the final payload. Loading
validates every offset, extent, scale, sign length, padding bit and exact row
termination. No on-disk container or whole-model conversion is supplied.
The current Swift census's conditional row-index accounting retains about
0.956 GiB of estimated weight savings; this is not measured process RAM.

Each scalar integer dot covers 128 weights and the matching half of a native
256-element Q8_K block. The computation applies
`(FP16_weight_scale * F32_activation_scale) * integer_sum` per group and adds
in source order without fused multiply-add. Compact dots consume the bitmap
and signs directly without allocating an expanded row. `unpack_row` is a
correctness helper. The native 292-byte Q8_K layout is retained; its sums are
unused. PTQ/Q8_K is an experimental comparator, not Prism's released PTQ/Q8_0 path.

Public Rust dot APIs additionally reject invalid symbols/scales, noncanonical
PTQ bytes and nonfinite activation scales. Zero activation scales are valid.
This deliberately strengthens the retired raw C++ dot boundaries. Finite
inputs can still produce nonfinite floating-point results through overflow.

## C++ removal and comparison evidence

Before removal, the original C++ implementation at revision
`d7bab354d0848215f0a9bb4d6699b218cc1e90f7` was compiled with native ARM64 MSVC,
strict floating point, NEON and ARM dot-product instructions. Its PQ2 scalar,
PQ2 NEON, PTQ NEON/Q8_K, compact scalar and compact NEON paths agreed on all
161 captured cases and 169 rows. The Rust implementation then matched the
captured PTQ bytes, compact offsets/payloads, round trips and exact result bits.
This is tested numerical/encoding parity, not a formal proof for every input.

The [immutable fixture and provenance](tests/fixtures/README.md) retain
those values after deletion of the standalone C++ library, tests, CMake builder
and benchmark workflow. Foundation verifies the fixture digest and runs the
Rust regression collection. Additional tests cover malformed inputs and
independent expected wire bytes/arithmetic.

The Rust implementation is scalar. ARM64 SIMD optimization and Rust throughput
remain unqualified. The [historical C++ synthetic measurements](../../config/bitcos.synthetic-cobalt100.json)
remain available with their original revisions/run URLs; they do not measure
Rust. No current synthetic timing executable or benchmark workflow is supplied.

## Build and focused correctness

Run native Rust commands from a unique external system-temp working directory,
using absolute manifest and target paths:

```powershell
$repo = (Get-Location).Path
$scratch = Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-BitcosRust-' + [guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($scratch) | Out-Null
$manifest = Join-Path $repo 'tools/bitcos/Cargo.toml'
$target = Join-Path $scratch 'build'
Push-Location $scratch
try {
    cargo +1.98.0-aarch64-pc-windows-msvc build --locked --offline --release --manifest-path $manifest --target-dir $target
    if ($LASTEXITCODE) { throw 'Rust BITCOS build failed.' }
    cargo +1.98.0-aarch64-pc-windows-msvc test --locked --offline --release --manifest-path $manifest --target-dir $target --test cpp_reference cpp_reference_parity -- --exact
    if ($LASTEXITCODE) { throw 'C++ reference parity failed.' }
} finally { Pop-Location }
```

Full suites and all benchmarks remain CI-only. Foundation runs Rust formatting,
strict Clippy across all targets, documentation checks, reference parity and the
full correctness collection. Model behavior, laptop latency, RAM, thermals and
runtime integration require their own qualification.
