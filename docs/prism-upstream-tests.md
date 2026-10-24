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

All original C++ test executables and fixture exporters are retired. Their
importer and replay build were removed after Windows ARM64 CI proved the safe
Rust replacements for every previously required C++ control. Native builds
disable upstream tests and expose production operations through bounded bridges.
A Rust-owned reference failure still fails CI independently of the runtime floor.

The migration inventories **all 71 catalog sources**, including model and
benchmark cases. Acceptance targets **Snapdragon X Elite on Windows ARM64**.
The Apple-only wired-memory source is explicitly outside that target and never
counts as a pass. Porting all catalog sources is not complete. Every previously passing source
remains required after replacement with safe Rust assertions against pinned Prism.
Unported sources remain visible as unsupported.
The [migration inventory](../tools/prism/upstream/migration.json) lists every
source's assertions and prerequisites, bound to its pinned digest. Its port flags
must match the executable adapter list; they are not test verdicts.

Fifty-three source contracts currently have safe Rust adapters in
[upstream_prism.rs](../src/prism-runtime/examples/upstream_prism.rs):

- `test-barrier.cpp`: all three CPU threadpool schedules, including the
  2,000-node graph, 10,000 changing-thread iterations and 1,000 two-graph
  iterations. The source has zero numerical output assertions. Its workload
  completion, additional API success guards and timings are reported explicitly;
  every execution of this timed source is restricted to CI.
- `test-llama-archs.cpp`: all 151 source variants, the production architecture
  inventory and capability skips, backend comparisons, and model export/load
  roundtrips. Rust creates metadata and tensor weights, retains the original
  numeric predicates, and records seed 1234 as a repeatable replacement for the
  source's unpinned random seed. The export phase runs separately from backend
  comparison and preserves saver-capability and temporary-file skips.
- `test-recurrent-state-rollback.cpp`: all three generated architecture fixtures and
  their full, partial and dirty restores, two-sequence replay, independent tails,
  and pending-fork byte equality. Fixtures are generated within this case's
  evidence directory; it does not depend on an earlier test having run.
- `test-chat.cpp`: all 14 default groups and 431 cases, including every byte
  prefix, final-message updates and reconstruction checks. A separate diagnostic
  retains source filters and forced reconstruction, including the filtered-only
  regression. All 59 template assets retain their original bytes.
- `test-arg-parser.cpp`: all 27 example inventories, option and environment
  uniqueness, repeated valid/invalid parses, load modes, LoRA CSV settings and
  the original HTTP cases. POSIX-only environment mutations remain excluded on
  Windows, as in the source.
- `test-model-resolution.cpp`: all 14 cases, 94 deterministic reorderings and
  13 assembly cases. A bounded Rust loopback server and private cache provide
  the source responses; the child invokes the production resolver.
- `test-kv-mean-center.cpp`: all three generated-model groups and original
  numeric predicates. Rust supplies GGUF metadata, Windows random inputs and
  tensor bytes through bounded file callbacks to production model construction.
- `test-opt.cpp`: all 118 AdamW and 46 SGD result subtests, including dataset
  batching, shuffling, accumulated gradients, epoch/fit equivalence and training
  regressions. The original diagnostic-only SGD tolerance policy is explicit.
- `test-double-float.cpp`: the complete binary32 domain is a CI-only arithmetic
  proof. This source calls no engine API. Rust compares defined floating-point
  results and reports the original out-of-range integer-cast domain explicitly.
- `test-gbnf-validator.cpp`: the manual validator retains Windows text reading,
  acceptance tracing and source highlight behavior. Its zero fixed source
  assertions are explicit; 119 independent source grammar vectors add regression
  coverage. A CI-only diagnostic accepts grammar and input paths.
- Both tokenizer roundtrip sources traverse every token and the entire original
  Unicode domain in two SPM and eight BPE fixtures, including fixtures upstream
  comments out for runtime. Per fixture this retains 456,703 Unicode calls and
  456,702 equality predicates, source exceptions, worker strides and shared stop.
  The BPE rewrite restores the source's intended invalid-UTF-8 skip with safe
  Rust validation: the pinned native decoder consumes errors that the original
  test expected to catch. Skip counts and this correction are reported explicitly;
  every valid token piece and original Unicode predicate remain required.
- `test-c.c`: the empty C main is retired. A C17 adapter includes the actual
  public header and exposes raw ABI layouts to seven additional Rust guards.
  The original contract is compile-only with zero behavioral assertions.
- `test-chat-analysis.cpp`: the full 37-template diagnostic, all paired render
  probes and undefined-variable analysis. Original accepted exceptions and zero
  correctness assertions are explicit; additional Rust diff guards are separate.
- Five model-lifecycle sources preserve cancellation, same-thread destruction,
  state-file continuation, sequence isolation and host/device state copies,
  fragmented-cache restoration, and concurrent model/context decoding. Their
  TinyStories fixture has pinned length and SHA-256 in
  [model-assets.lock.json](../tools/prism/upstream/model-assets.lock.json).
  Rust verifies its bytes before each schedule and controls individual native
  operations and progress callbacks through a bounded persistent process.
- `test-json-schema-to-grammar.cpp`: all 269 mandatory conditions and the
  original optional independent Python path, which adds 156 conditions when
  available. Python availability and skips are reported explicitly. Production
  Python conversion remains upstream code; inputs and comparisons are Rust.
- `test-tokenizer-0.cpp`: all 694 independent token-vector predicates across
  fifteen bundled vocabularies. Rust preserves Windows text input, map ordering,
  shared-context hardware workers and the original worker-zero comparisons.
  Optional timed text export is a separate CI-only diagnostic.
- `test-chat-auto-parser.cpp`: all seventeen groups and 524 executed conditions,
  including the source quote normalizer and 42 role-template rows.
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
- `test-alloc.cpp`: all 13 scenarios and 14 graph allocations, preserving 468
  unconditional predicates and actual overlap-dependent lifetime checks.
- `test-gguf.cpp`: all 34 crafted variants, all CPU file/buffer/callback
  roundtrips and KV-copy/merge checks. Source-only input parity was independently
  verified against MSVC for every crafted file, two generated contexts and RNG state.
- `test-sampling.cpp`: all 343 correctness conditions, preserving stateful
  candidate backing storage across filters, distributions, copying and resetting.
- `test-chat-template.cpp`: all 73 automated conditions, including 36 legacy
  renders and 35 Jinja renders, extra conversations and Windows newline handling.
- `test-chat-peg-parser.cpp`: all seven groups, 197 fixed assertions, conditional
  short-name checks and 105 diff crash-resistance conditions.
- `test-mtmd-c-api.c`: all 46 original conditions. The pinned library's public
  opaque fixture factory supplies chunk contents, as in the original source.
  Rust owns the invocation, traversal, serialization requests and comparisons.

The Rust runtime tokenizer adapter loads actual supported model metadata and
uses its production tokenizer and vocabulary objects. Unsupported vocabulary
constructors remain measured failures. The Rust runtime has no equivalent
generic regex, MTP, Jinja, MTMD or grammar APIs.
Those target results explicitly fail rather than testing substitute helpers.

For each port, the same safe Rust inputs and assertions run against pinned Prism
first, then against the Rust runtime. A native process bridge performs bounded
serialization and calls production Prism functions. It imports no test sources
and contains no random generators, expected outputs, reference arithmetic,
tolerances or test assertions. The C++ exporters and original test executables
are no longer built. A separate C17 production ABI adapter proves public-header
compilation without retaining the original empty C test main.

Catalog runs include the pinned model-lifecycle workloads and the CI-only
barrier timing source. Foundation's
manual `prism_benchmarks` input executes separately reported benchmark workloads
after correctness. PEG serialization retains all 100 deserializations. Its
reported wall time includes native process transport and does not qualify native
kernel performance or affect the correctness denominator. Sampling retains five
32-iteration workloads; chat PEG retains complete/incremental parse workloads.
Their reports state which native dispatch and metadata handling is timed.
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
The ported reference results now come from safe Rust assertions calling
the engine through the process bridge. The original GGUF qualification replay
was retired after its replacement passed native acceptance.
That initial run left 52 sources without an executed C++ control. The MTMD
Rust port adds another upstream control; it is separate from the original 19.

The first native Windows ARM64 acceptance run passed on revision `84ad97d`,
[Foundation run 40116792959](https://github.com/symbiogenesis/SwiftBonsai2/actions/runs/40116792959).
Its ClangCL bridges and Rust harness were verified as ARM64 PE files. All eight
ports published at that revision and 12 retained original controls passed on
Prism. Rust recorded one passing source, seven failed ports and 63 unsupported
sources. Later port registrations require their own hosted reference acceptance.

The next [Windows ARM64 run, 40118410264](https://github.com/symbiogenesis/SwiftBonsai2/actions/runs/40118410264),
passed on `18d2c18`: all 25 ports registered at that revision and two original
controls passed. Rust recorded one passing source, 24 failed ports and 46
unsupported sources. The baseline retains the original green source and all 19
required reference identities independently of these increasing port counts.

[Windows ARM64 run 40120815419](https://github.com/symbiogenesis/SwiftBonsai2/actions/runs/40120815419)
passed on `2daa94c`: all33 Rust-owned source contracts passed on upstream,
including all694 tokenizer predicates and the complete GGUF replacement.
Rust recorded one passing source,32 failed ports and38 unsupported sources.
This provided the final evidence needed to retire the original C++ replay.

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
