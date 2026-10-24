# Safe Rust assertions against pinned Prism

The `upstream_prism` example owns test inputs, references and assertions in
safe Rust (`#![forbid(unsafe_code)]`). It uses the same test code with two
targets: production Prism through a native process bridge, and this repository's
production Rust runtime. Upstream runs first and must pass before Rust is tested.

The current ports preserve two source contracts from
`PrismML-Eng/llama.cpp@b87ca7225456b06b0975b8239c66b5aecf07e37c`:

- `tests/test-pq2-row-shapes.cpp`: all 15 shapes, `K` in
  `{128,256,384,512,640}`, `M=4`, `N` in `{1,4,16}`, and the original
  `rel_rms(computed, dequantized-reference) < 0.05` assertion. The frozen
  input-only corpus preserves original packed weights and activations from
  the recorded hosted baseline. Rust independently decodes weights and computes
  references; it never reads a saved native output as the engine under test.
- `tests/test-ptq1_0-element-map.cpp`: the original seed-12345 LCG, all 20,000
  arbitrary-byte blocks and all 2,560,000 element equality assertions. Rust
  generates the bytes and computes the independent CPU traversal. Scale is
  exactly one because the original integer test leaves it unused. Noncanonical
  inputs remain in the test; the Rust runtime's rejection remains a failure.

The native bridge contains only serialization, input validation and production
ggml calls. It does not include upstream test sources or contain generators,
references, expected values, assertions or tolerances. The C++ fixture exporters
and original two test mains have been retired from this adapter build.
`reference/` still imports the 17 unported C++ correctness controls. Removing
those before their replacements pass would lose existing coverage.

The runtime's vector API runs once for each original activation column,
preserving the complete `M*N` comparison. This tests numerical compatibility,
without claiming batched execution or throughput equivalence.

From a verified, unmodified source extraction, configure this directory with
CMake and `-DPRISM_SOURCE_DIR=<absolute upstream path>`, then build the
`prism-upstream-bridge` target. Run the safe Rust suite with:

```text
cargo run -p bonsai-prism-runtime --example upstream_prism -- \
  --target cpp --case test-pq2-row-shapes \
  --bridge <absolute native bridge executable> \
  --evidence-directory <fresh absolute evidence directory> \
  --output <absolute report path>
```

Repeat with `--target rust`, omitting the bridge options. The PTQ1 selector is
`test-ptq1_0-element-map`. `tests/Test-PrismUpstream.ps1` supervises both targets,
retains bridge requests/responses and logs, and checks the upstream report before
retargeting. Full catalogs and all benchmarks run only in CI.

Reports preserve `SchemaVersion: 1`, `UpstreamCommit`, `Target`, and `Cases`
containing `Id`, `Status` and `Detail`. Measured conformance failures are data for
the baseline gate; infrastructure failures exit nonzero. Unported sources remain
unsupported in the 71-source denominator.

The full requested 71-source migration is tracked in the
[baseline guide](../../../docs/prism-upstream-tests.md). These derivatives retain
the upstream MIT attribution in [LICENSE](LICENSE).
