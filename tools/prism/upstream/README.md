# Retargeted upstream Prism assertions

The adapter ports two actual test sources from
`PrismML-Eng/llama.cpp@b87ca7225456b06b0975b8239c66b5aecf07e37c`:

* `tests/test-pq2-row-shapes.cpp`: all 15 original shapes, `K` in
  `{128,256,384,512,640}`, `M=4`, `N` in `{1,4,16}`, and the original
  `rel_rms(computed, dequantized-reference) < 0.05` assertion.
* `tests/test-ptq1_0-element-map.cpp`: the original seed 12345 LCG, all 20,000
  arbitrary-byte blocks and all 2,560,000 element equality assertions.

The original C++ tests compile directly from the verified source checkout.
Exporters include that same source to reuse its helpers and write bounded,
little-endian fixtures. The Rust `upstream_prism` example runs identical
assertions against exported C++ results (`--target cpp`) and the current
production Rust `kernels::matvec`/`kernels::row` (`--target rust`). This proves
the Rust harness passes the C++ engine before retargeting its assertions.
The PQ2 standard-library Gaussian generator is never reimplemented in Rust.
Its generated activations, encoded weights, C++ decoded weights and C++ graph
results are carried into the same adapter. C++ and Rust reports use identical
source-path IDs.

The runtime has a vector CPU API. For each upstream `N`, the adapter invokes
that API once for each of the original activation columns and checks the same
complete `M*N` output. This covers numerical shape compatibility; it does not
claim a batched Rust kernel or throughput equivalence.

The PTQ1 upstream test initializes only trit bytes, because scale is irrelevant
to its integer mapping. The exporter initializes the unused scale to exactly
one to exercise the production Rust float decoder. It never filters random
blocks into a canonical subset. Rust intentionally rejects noncanonical PTQ1
encodings; that difference is recorded as a failed upstream source, including
the number of rejected blocks and first error. A failed result is data, and
does not suppress the remaining fixtures.

From a verified unmodified checkout, configure this directory with CMake and
`-DPRISM_SOURCE_DIR=<absolute upstream path>`. Explicit targets are
`prism-upstream-pq2-row-shapes`, `prism-upstream-ptq1-element-map`,
`prism-upstream-pq2-fixtures`, and `prism-upstream-ptq1-fixtures`. Each fixture
exporter takes one absolute output path. The Rust invocation is:

```text
cargo run -p bonsai-prism-runtime --example upstream_prism -- \
  --target cpp --case test-pq2-row-shapes \
  --pq2-fixtures <absolute fixture path> --output <absolute report path>
```

Repeat with `--target rust`. The PTQ1 case selector is
`test-ptq1_0-element-map` and its fixture option is `--ptq1-fixtures`.
Omitting `--case` executes both sources and is reserved for CI. Reports have
`SchemaVersion: 1`, `UpstreamCommit`, `Target`, and `Cases` entries containing
`Id`, `Status` (`pass` or `fail`) and `Detail`. A measured failure exits zero
so the aggregate baseline gate can assess it; malformed/missing fixtures and
other infrastructure failures exit nonzero. Unsupported upstream sources are
counted by the surrounding catalog gate, never silently declared passing.

These derivatives retain the upstream MIT attribution in [LICENSE](LICENSE).
