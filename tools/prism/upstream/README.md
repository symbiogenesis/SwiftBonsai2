# Safe Rust assertions against pinned Prism

The `upstream_prism` example owns test inputs, references and assertions in
safe Rust (`#![forbid(unsafe_code)]`). It uses the same test code with two
targets: production Prism through a native process bridge, and this repository's
production Rust runtime. Upstream runs first and must pass before Rust is tested.

The current ports preserve twenty-five source contracts from
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
- `tests/test-unicode.cpp`: the exact original regex, input and expected pieces.
- `tests/test-mtp-catchup-batch.cpp`: all six original first-decode bounds,
  including zero-batch and negative catch-up rejection.
- `tests/test-jinja.cpp`: every default literal, hash, stats, caps, string-parts
  and fuzz group. Expected literals are source data; all generators and
  assertions are Rust. Stateful hash traces preserve intermediate digest calls.
- `tests/test-mtmd-impl.cpp`: five tiling cases and 12 grouping cases, including
  both grouping outputs and the mutation of the remaining input parts.
- `tests/test-grammar-parser.cpp`: all 23 parse/reject variants and 274 original
  symbol-map and rule-element checks, including token and token-not elements.
- `tests/test-reasoning-budget.cpp`: all 71 original sequence, cloning, forcing,
  end-match and UTF-8 conditions, retaining candidate float bits and the original
  analysis break. Static transport may accept one unobserved completed token
  before freeing a sampler, without claiming additional assertion coverage.
- `tests/test-col2im-1d.cpp`: all 11 geometries at three precisions, preserving
  the 33 original NMSE thresholds and 22 shape checks.
- `tests/test-dfly-fusion.cpp`: all three aggregate contracts, covering the full
  output, layer slices and eight layer separation norms.
- `tests/test-rope.cpp`: all five modes and ten relative-L1 checks over the
  original tensor extents and continuous Windows random stream.
- `tests/test-llama-grammar.cpp`: all 174 original stack-frame and rejected
  candidate pair comparisons. Unused expected-row tails remain source data.
- `tests/test-grammar-integration.cpp`: all 11 groups and 504 original conditions,
  including schema conversion, parser failures, recursive roots and cloning.
- `tests/test-quantize-fns.cpp`: all 1,108 original predicates, preserving
  source-pinned quantizer eligibility, F32 tails and packed ternary dot patterns.
- `tests/test-ptq1_0-cuda-dot.cpp`: all 53,192 host-side predicates, with 14,096
  additional checks against production CPU decoding and packed dots. This source
  uses host transcriptions; it establishes no GPU execution coverage.
- `tests/test-log.cpp`: all eight concurrent workers and 8,000 messages,
  timestamp/prefix changes, joining, flushing and explicit singleton destruction.
- `tests/test-moe-weighted-reduce.cpp`: all 48 cases, preserving manual aliased
  buffer layouts and bitwise comparison against plain CPU allocation. Acceptance
  uses the source's CPU fallback; CUDA fusion remains unqualified.
- All six `tests/peg-parser/` sources preserve their 170 builder workloads and
  241 executed original conditions, including malformed UTF-8 byte inputs. The
  umbrella `tests/test-peg-parser.cpp` preserves its group order and summary.
- `tests/test-batch-alloc.cpp`: all 26 named cases and 198 assertions, including
  invalid inputs, sequence splitting, tail retention and multidimensional positions.

Unicode, MTP, Jinja, MTMD and grammar use a [common bridge](common/PROTOCOL.md).
The Rust runtime has no equivalent production APIs; their Rust results fail.

The native bridge contains only serialization, input validation and production
ggml calls. It does not include upstream test sources or contain generators,
references, expected values, assertions or tolerances. The C++ fixture exporters
and all twenty-five ported test mains have been retired from this adapter build.
`reference/` still imports the two unported C++ correctness controls. Removing
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
`test-ptq1_0-element-map`. For `test-unicode` or `test-mtp-catchup-batch`, use
`--common-bridge` with the executable built from `common/` instead of `--bridge`.
`tests/Test-PrismUpstream.ps1` supervises both targets,
retains bridge requests/responses and logs, and checks the upstream report before
retargeting. Full catalogs and all benchmarks run only in CI.
The Foundation manual input `prism_benchmarks` runs the ported benchmark paths
after correctness. PEG deserialization retains its 100-iteration workload;
reported wall time includes process transport and has no correctness threshold.

Reports preserve `SchemaVersion: 1`, `UpstreamCommit`, `Target`, and `Cases`
containing `Id`, `Status` and `Detail`. Measured conformance failures are data for
the baseline gate; infrastructure failures exit nonzero. Unported sources remain
unsupported in the 71-source denominator.

Native acceptance targets Windows ARM64 for Snapdragon X Elite. The Apple-only
wired-memory source remains inventoried as an explicit exclusion. The complete
71-source migration inventory is tracked in the
[baseline guide](../../../docs/prism-upstream-tests.md). These derivatives retain
the upstream MIT attribution in [LICENSE](LICENSE).
