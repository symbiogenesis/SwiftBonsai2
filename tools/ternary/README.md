# Validated ternary codecs

The unpublished [`bonsai-ternary` package](Cargo.toml) supplies the shared PQ2/PTQ1
codecs and numerical utilities used by the GGUF exporter and Prism runtime. It
has no dependencies, forbids unsafe code, uses checked Release arithmetic and
returns typed encoding errors.

PQ2 stores a little-endian FP16 scale and 128 two-bit symbols (`0 = -1`, `1 = 0`,
`2 = +1`). The native 28-byte PTQ1 layout stores 26 packed trit bytes followed by
the unchanged scale. Packing rejects reserved symbol 3 and zero or nonfinite
weight scales. Finite negative and subnormal scale bits are preserved. Unpacking
also rejects noncanonical PTQ bytes: repacking must reproduce every input byte.

`half_to_float` converts every IEEE 754 binary16 bit pattern, preserving signed
zero and shifting NaN payload bits exactly. `Q8KBlock` preserves the native
292-byte activation layout used by the runtime, including its group-sum fields.
The consuming runtime kernel validates activation scales. This crate provides
no inference kernels or model loader.

The source contract is [Prism commit 73bfe6cc](https://github.com/PrismML-Eng/llama.cpp/tree/73bfe6cc485554822f80c473a3c0c4f25e80f41b).
The upstream MIT notice remains in [PRISM-NOTICE.txt](PRISM-NOTICE.txt).
Independent regressions cover wire bytes, symbol positions, every symbol byte,
all nonzero counts, malformed encodings, binary16 edges and the activation ABI.
The [native C++ reference fixture](tests/fixtures/README.md) additionally retains
161 captured PQ2/PTQ cases and 26 binary16 conversion cases.

Run native Rust commands from a unique external system-temp working directory,
using absolute manifest and target paths:

```powershell
$repo = (Get-Location).Path
$scratch = Join-Path ([IO.Path]::GetTempPath()) ('SwiftBonsai-Ternary-' + [guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($scratch) | Out-Null
$manifest = Join-Path $repo 'tools/ternary/Cargo.toml'
$target = Join-Path $scratch 'build'
$previous = [Environment]::CurrentDirectory
Push-Location $scratch
try {
    [Environment]::CurrentDirectory = $scratch
    cargo +1.99.1-aarch64-pc-windows-msvc build --locked --offline --release --manifest-path $manifest --target-dir $target
    if ($LASTEXITCODE) { throw 'Rust ternary codec build failed.' }
    cargo +1.99.1-aarch64-pc-windows-msvc test --locked --offline --release --manifest-path $manifest --target-dir $target --test cpp_reference cpp_reference_ptq_parity -- --exact
    if ($LASTEXITCODE) { throw 'C++ codec parity failed.' }
} finally {
    [Environment]::CurrentDirectory = $previous
    Pop-Location
}
```

Full suites and all benchmarks remain CI-only. Foundation runs formatting,
strict Clippy across all targets, documentation checks and correctness tests.