# Pinned Prism test baseline

[Foundation](../.github/workflows/foundation.yml) imports tests from the exact
Prism archive in [source.lock.json](../tools/prism/source.lock.json), verifies its
length and SHA-256, and extracts it into fresh external storage. The upstream
sources are used unchanged. No engine sources or build trees are vendored here.
The upstream MIT terms remain in [PRISM-LICENSE.txt](../tools/prism/PRISM-LICENSE.txt).

The [catalog](../tools/prism/upstream/catalog.json) records all **71** recursive
`tests/**/test-*.cpp`, `.c`, `.py` and `.sh` source files, their full relative
identities and SHA-256 digests, plus the test-registration CMake digest. This is
a source-file coverage measure: files contain different numbers of assertions,
and some are helpers, manual tests, model tests or benchmarks. It must not be
read as an assertion-level pass rate or full-model qualification.

## What runs

The raw C++ lane builds and runs 17 original, model-free correctness executables:
allocation and batch handling, col2im, dfly fusion, GGUF, grammar
integration/parser, Jinja, llama grammar, logging, MoE reduction, MTP batching,
PTQ1 CUDA-dot host logic, quantization functions, reasoning budgets, RoPE and
Unicode. The allowlist is explicit in
[Invoke-PrismCppSuite.ps1](../scripts/Invoke-PrismCppSuite.ps1). Model downloaders,
Python environment tests and embedded benchmarks need separate lanes; they
remain visible in the inventory. A C++ control failure fails CI independently of
the Rust coverage floor.

Two source contracts currently have Rust adapters in
[upstream_prism.rs](../src/prism-runtime/examples/upstream_prism.rs):

- `test-pq2-row-shapes.cpp`: the original C++ suite runs first. Exporters then
  capture its generated packed weights, activations, dequantized reference and
  C++ matrix output for all 15 original shapes. The same Rust assertions first
  validate the C++ outputs, then retarget to the production Rust matrix kernel.
  The original relative-RMS threshold is preserved.
- `test-ptq1_0-element-map.cpp`: exporters preserve the original LCG inputs and
  both original element traversals across 20,000 blocks. The Rust harness first
  checks the C++ outputs, then checks the production Rust decoder on the same
  bytes. Rejection of an upstream input is a red result, including encodings
  that the Rust decoder intentionally does not admit.

No benchmark or model inference is run by this lane. The original sampling test
includes a benchmark in its default main, so it is deliberately absent from the
raw correctness allowlist.
The argument parser test makes a live HTTP request, so it is also excluded.
The GGUF test uses its supported explicit seed `12345` for repeatable inputs.

## Floor and retained evidence

The initial measured Rust baseline is **1 green / 71 source files (1.41%)**:
PQ2 row shapes pass, PTQ1 element mapping fails, and 69 source files are
unported. Equivalently the green:red count is **1:70**. The failed PTQ1 port
reports 19,434 rejected blocks out of the original 20,000; admitted positions
match exactly. The baseline retains this original domain instead of narrowing
the test to canonical bytes.

The initial [hosted Foundation run](https://github.com/symbiogenesis/SwiftBonsai2/actions/runs/40112751630)
passed on revision `afe66d881b8828183b9ffe7ef3119463b5d58bc2`. All **19 selected
C++ controls** passed (17 original suites plus the two paired adapter controls).
Their IDs are recorded in the baseline, so none can disappear from later runs.
The other 52 source files have no executed C++ control in this lane.

[baseline.json](../tools/prism/upstream/baseline.json) holds the recorded minimum
green count and every required green source ID. `RequiredReferencePassedIds`
also preserves each already-passing C++ control independently of Rust progress.
Its integer fraction is the
minimum green ratio. The exact catalog digest and upstream revision bind the
baseline to the denominator. Each run:

1. Verifies that the source inventory is complete and unchanged.
2. Requires both implemented adapters to emit one Rust and one C++ result.
3. Expands every remaining catalog file to `unsupported` for Rust.
4. Counts `fail`, `skip` and `unsupported` as red; only `pass` is green.
5. Requires every previously green ID to remain green, as well as the minimum
   fraction. A new pass cannot compensate for a regression in an existing pass.

Missing implemented adapters, duplicate/unknown IDs, malformed reports,
compilation errors, process failures and timeouts fail the pipeline. Expected
Rust conformance failures are recorded and tolerated only within the committed
floor. C++ reference failures always fail the gate.

Every run uploads `report.json`, `gate.json`, the exact source/runtime receipt,
separate C++ and Rust results, shared binary fixtures and compiler/test logs.
The job summary shows the full inventory with independent Rust and C++ statuses.
Git history records changes to the approved baseline; Actions artifacts retain
per-run observations under the repository's normal artifact retention policy.

CI caches C/C++ compiler objects separately from Cargo builds. The native cache
is scoped to the runner image, compiler and ccache identity; source contents and
compiler options still determine each object hit. Both builds normalize paths
under their common evidence root so a fresh extraction can reuse those objects.
Every run still verifies and extracts the pinned archive, configures fresh build
trees, regenerates fixtures and executes all 19 controls and both Rust adapters.
`ccache.log` retains hit/miss statistics with the other evidence. Release builds
explicitly retain assertions. A focused adapter builds only its own two targets.

## Growing the baseline

Add an adapter using the original inputs, assertions and tolerances. Prove the
shared assertions on pinned C++ before applying them to Rust. Add its stable ID
to `AdapterIds`, so it cannot disappear into the unsupported backlog. Update
catalog identity and baseline binding together when this changes the catalog.

After a complete hosted run, review its retained report and increase
`MinimumPassed` and `RequiredPassedIds` to include the newly passing files.
Record the tested revision and run URL in `Provenance`. CI never rewrites the
baseline. Keep every existing required green ID. Review upstream pin changes
and any exception explicitly instead of deleting difficult cases.

## Focused local checks

Full catalogs and all benchmarks remain CI-only. A focused policy check is:

```powershell
./tests/Test-PrismBaseline.ps1 -Case Ratchet
```

One imported adapter can run locally with the pinned archive, locked Rust
dependencies and an available C++ toolchain. All paths below must be expanded
absolute paths outside Git:

```powershell
./tests/Test-PrismUpstream.ps1 -SourceArchive $archive `
    -EvidenceDirectory $freshEvidence -TargetDirectory $externalCargoTarget `
    -Case Pq2RowShapes -Generator 'Visual Studio 18 2026' -NativePlatform x64
```

The x64 C++ control can run under Windows ARM64 emulation; this is correctness
evidence and makes no target-device performance claim. Hosted execution uses
Linux ARM64. For Linux or a configured compiler environment, use `Ninja`.
