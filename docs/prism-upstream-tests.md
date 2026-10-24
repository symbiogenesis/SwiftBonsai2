# Safe Rust Prism conformance migration

[Foundation](../.github/workflows/foundation.yml) imports tests from the exact
Prism archive in [source.lock.json](../tools/prism/source.lock.json), verifies its
length and SHA-256, and extracts it into fresh external storage. The upstream
engine sources are used unchanged. No engine sources or build trees are vendored here.
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

The requested retirement covers **all 71 catalog sources**, including model,
platform and benchmark cases. It is not complete. The 17 original C++ controls
remain required until their equivalent safe Rust tests have passed against
pinned Prism. Unported sources remain visible as unsupported.
The [migration inventory](../tools/prism/upstream/migration.json) lists every
source's assertions and prerequisites, bound to its pinned digest. Its port flags
must match the executable adapter list; they are not test verdicts.

Two source contracts currently have safe Rust adapters in
[upstream_prism.rs](../src/prism-runtime/examples/upstream_prism.rs):

- `test-pq2-row-shapes.cpp`: a committed input-only corpus preserves the
  original generated packed weights and activations for all 15 shapes. Rust
  independently decodes the packed weights, calculates reference products and
  checks the original relative-RMS threshold. The corpus contains no expected
  outputs; its provenance identifies the original archive and source digests.
  This path exercises pre-existing encoded weights; per-run quantizer checks
  remain in the retained `test-quantize-fns.cpp` control.
- `test-ptq1_0-element-map.cpp`: Rust generates the original seed-12345 LCG
  inputs across all 20,000 arbitrary-byte blocks, and independently computes
  the original CPU traversal. It compares all 2,560,000 positions against each
  engine's production decoder. Rejection of any input is red, including
  encodings that the Rust decoder intentionally does not admit.

For each port, the same safe Rust inputs and assertions run against pinned Prism
first, then against the Rust runtime. A native process bridge performs bounded
serialization and calls production ggml functions. It imports no test sources
and contains no random generators, expected outputs, reference arithmetic,
tolerances or test assertions. The C++ exporters and two original C++ test
executables are no longer built by this adapter lane.

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
The two paired reference results now come from safe Rust assertions calling
the engine through the process bridge; the other 17 still run original C++.
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
separate C++ and Rust results, bridge requests/responses and compiler/test logs.
The job summary shows the full inventory with independent Rust and C++ statuses.
Git history records changes to the approved baseline; Actions artifacts retain
per-run observations under the repository's normal artifact retention policy.

CI caches C/C++ compiler objects separately from Cargo builds. The native cache
is scoped to the runner image, compiler and ccache identity; source contents and
compiler options still determine each object hit. Both builds normalize paths
under their common evidence root so a fresh extraction can reuse those objects.
Every run still verifies and extracts the pinned archive, configures fresh build
trees, executes the 17 unported controls and runs both safe Rust ports against
both engines.
`ccache.log` retains hit/miss statistics with the other evidence. Release builds
explicitly retain assertions. A focused adapter builds only the production
bridge and the safe Rust harness.

## Growing the baseline

Add an adapter using the original inputs, assertions and tolerances. Keep input
generation, reference calculations and assertions in safe Rust; extend the
native bridge only with production calls and serialization. Prove the
shared assertions on pinned C++ before applying them to Rust. Add its stable ID
to `AdapterIds`, so it cannot disappear into the unsupported backlog. Update
catalog identity and baseline binding together when this changes the catalog.

After a complete hosted run, review its retained report and increase
`MinimumPassed` and `RequiredPassedIds` to include the newly passing files.
Record the tested revision and run URL in `Provenance`. CI never rewrites the
baseline. Keep every existing required green ID. Review upstream pin changes
and any exception explicitly instead of deleting difficult cases.

Retire the remaining C++ importer only after all 71 sources have Rust-owned
equivalents and their upstream validation evidence is retained. Model tests need
their actual model/vocabulary/reference assets; platform tests need the relevant
host; embedded benchmarks need CI execution lanes. Missing prerequisites and
unsupported Rust features are red outcomes. Current-weight Swift artifacts
cannot substitute for unrelated upstream model fixtures.

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
