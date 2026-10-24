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

The raw C++ lane builds and runs two original, model-free correctness executables:
allocation and GGUF. The allowlist is explicit in
[Invoke-PrismCppSuite.ps1](../scripts/Invoke-PrismCppSuite.ps1). Model downloaders,
Python environment tests and embedded benchmarks need separate lanes; they
remain visible in the inventory. A C++ control failure fails CI independently of
the Rust coverage floor.

The migration inventories **all 71 catalog sources**, including model and
benchmark cases. Acceptance targets **Snapdragon X Elite on Windows ARM64**.
The Apple-only wired-memory source is explicitly outside that target and never
counts as a pass. Retirement is not complete. The two original C++ controls
remain required until their equivalent safe Rust tests have passed against
pinned Prism. Unported sources remain visible as unsupported.
The [migration inventory](../tools/prism/upstream/migration.json) lists every
source's assertions and prerequisites, bound to its pinned digest. Its port flags
must match the executable adapter list; they are not test verdicts.

Twenty-five source contracts currently have safe Rust adapters in
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
- `test-unicode.cpp`: the exact original regex, input and two-piece expected
  vector are checked against production `unicode_regex_split`.
- `test-mtp-catchup-batch.cpp`: all six first-decode bounds assertions,
  including negative catch-up and zero-batch rejection, target the production
  speculative-decoding helper.
- `test-jinja.cpp`: all 19 default groups, including 278 literal renders,
  stateful hash properties, statistics, capabilities, string parts and all
  1,122 crash-resistance fuzz calls. Rust generates the inputs and checks the
  results. The Windows random stream was independently matched against MSVC;
  expected strings come from the original source. The optional Python mode is
  an upstream diagnostic cross-check, outside its default correctness suite.
- `test-mtmd-impl.cpp`: all five tiling cases and 12 temporal-grouping cases,
  with both group-size and mutated remaining-part assertions (29 in total).
- `test-grammar-parser.cpp`: all 23 variants, including six rejected grammars,
  ordered symbol IDs and flattened element types/values (274 original checks).
- `test-reasoning-budget.cpp`: all 71 original conditions for basic sequences,
  cloning, forcing, end matching and UTF-8 boundaries. Raw candidate float bits
  preserve finite-logit checks. Static transport traces preserve the original
  analysis break and may add an unobserved accept on a completed sampler before
  freeing it; no extra assertion coverage is claimed.
- `test-col2im-1d.cpp`: all 11 geometries across three precisions, with 33
  original NMSE checks and 22 shape checks.
- `test-dfly-fusion.cpp`: all three aggregate contracts, including full and
  layer-sliced outputs and eight layer separation norms.
- `test-rope.cpp`: the original five modes and ten relative-L1 checks over
  complete tensor extents, retaining the continuous Windows random stream.
- `test-llama-grammar.cpp`: all 174 original stack-frame and rejected-candidate
  comparisons. The source's unused expected-row tails remain recorded.
- `test-grammar-integration.cpp`: all 11 groups and 504 conditions, including
  408 literal string expectations, parser/build errors and 86 clone checks.
- `test-quantize-fns.cpp`: all 1,108 original predicates. The source's 144 enum
  slots, 37 active formats and 22 eligible quantizers are pinned independently of
  observed outputs, preserving genuine skips without hiding missing formats.
- `test-ptq1_0-cuda-dot.cpp`: all 53,192 original host-side predicates plus 14,096
  production decoder/CPU-dot checks. Unit scales preserve exact packed integer
  observations, while Rust retains the original floating scales and diagnostics.
  These host transcriptions do not qualify GPU execution.
- `test-log.cpp`: eight concurrent workers and all 8,000 messages, random
  timestamp/prefix changes, joins, flush and explicit logger destruction.
- `test-moe-weighted-reduce.cpp`: all 48 shapes/layouts, including weights inside
  the destination and destination inside the input. Rust owns manual placement
  and bitwise comparison against plain CPU allocation. CPU fallback is tested;
  CUDA fusion is not qualified.
- All six nested `peg-parser` sources: 170 builder workloads and 241 executed
  original conditions across basic parsing, grammar generation, JSON, Python
  dictionaries, serialization and Unicode. Malformed UTF-8 remains byte data.
  The umbrella `test-peg-parser.cpp` preserves group order and summary semantics.
- `test-batch-alloc.cpp`: all 26 named cases and 198 original assertions, covering
  initialization, splitting, tail retention and multidimensional positions.

The Rust runtime has no equivalent generic regex, MTP, Jinja, MTMD or grammar APIs.
Those target results explicitly fail rather than testing substitute helpers.

For each port, the same safe Rust inputs and assertions run against pinned Prism
first, then against the Rust runtime. A native process bridge performs bounded
serialization and calls production Prism functions. It imports no test sources
and contains no random generators, expected outputs, reference arithmetic,
tolerances or test assertions. The C++ exporters and all twenty-five ported C++ test
executables are no longer built by this adapter lane.

Ordinary correctness runs execute no benchmark or model inference. Foundation's
manual `prism_benchmarks` input executes separately reported benchmark workloads
after correctness. PEG serialization retains all 100 deserializations. Its
reported wall time includes native process transport and does not qualify native
kernel performance or affect the correctness denominator. The original sampling test
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
The twenty-five ported reference results now come from safe Rust assertions calling
the engine through the process bridge; the other two still run original C++.
That initial run left 52 sources without an executed C++ control. The MTMD
Rust port adds another upstream control; it is separate from the original 19.

The first native Windows ARM64 acceptance run passed on revision `84ad97d`,
[Foundation run 40116792959](https://github.com/symbiogenesis/SwiftBonsai2/actions/runs/40116792959).
Its ClangCL bridges and Rust harness were verified as ARM64 PE files. All eight
ports published at that revision and 12 retained original controls passed on
Prism. Rust recorded one passing source, seven failed ports and 63 unsupported
sources. Later port registrations require their own hosted reference acceptance.

[baseline.json](../tools/prism/upstream/baseline.json) holds the recorded minimum
green count and every required green source ID. `RequiredReferencePassedIds`
also preserves each already-passing C++ control independently of Rust progress.
Its integer fraction is the
minimum green ratio. The exact catalog digest and upstream revision bind the
baseline to the denominator. Each run:

1. Verifies that the source inventory is complete and unchanged.
2. Requires all implemented adapters to emit one Rust and one C++ result.
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

The Windows ARM64 lane caches the pinned archive and Cargo artifacts. Native production libraries are configured and built afresh with ClangCL. The receipt records configure arguments, executable hashes and verified ARM64 PE machine values. Every run verifies and extracts the archive, runs all eight safe Rust ports against pinned Prism first and the Rust runtime second, and retains all 12 unported controls.

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

Retire the remaining C++ importer only after the Windows-applicable sources have
Rust-owned equivalents and their upstream validation evidence is retained.
Keep the full 71-source provenance inventory, including the explicit Apple
exclusion. Model tests need
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
evidence and makes no target-device performance claim. Hosted acceptance uses native Windows ARM64. For Linux or a configured compiler environment, use `Ninja`.
