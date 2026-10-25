# Prism migration TODO

Handoff at code revision `9332b01cd8b7102ce1f05d6858dc9d75ccf48a03`.
CI is continuing independently; this handoff does not wait for its verdict.
The target is Snapdragon X Elite on Windows ARM64. Apple work is excluded.
See the [migration guide](prism-upstream-tests.md),
[71-source catalog](../tools/prism/upstream/catalog.json), and
[migration inventory](../tools/prism/upstream/migration.json).

## Completed and measured

- All 70 Windows-applicable sources have shared safe Rust adapters, used against
  upstream production C++ and our Rust runtime. Adapter coverage is not a pass
  verdict. The Apple-only source remains explicit in the 71-source inventory.
- Original C++ test executables, fixture exporters, and the replay importer/build
  are retired. The remaining two C++ translation units and one C ABI translation
  unit expose production operations needed by the reference adapters.
- [Run 40146089128](https://github.com/symbiogenesis/SwiftBonsai2/actions/runs/40146089128)
  on `eb37e263b9852c4fcccb0d1fa8ee69aea977b4d5` passed all 69 ordinary source
  adapters against C++ and all 16 backend planning partitions. Rust recorded
  4 ordinary passes and 65 failures. The run failed during artifact extraction
  with `EMFILE` before backend numerical execution. It did not qualify all 70
  applicable sources.
- `9332b01` changes artifact handoffs to compact records while retaining full
  evidence separately. Suite and runtime source code are unchanged from that run.

## Finish reference qualification first

- [ ] Collect the result of [run 40150402106](https://github.com/symbiogenesis/SwiftBonsai2/actions/runs/40150402106),
  which tests `9332b01` with benchmarks enabled and optional gradients disabled.
  Its verdict was pending at handoff. Do not treat a pending run as passed.
- [ ] Verify the compact plan and execution handoffs avoid the extraction failure,
  and that the separate `prism-backend-evidence-plan-*` and
  `prism-backend-evidence-run-*` artifacts retain complete diagnostics.
- [ ] Complete plan joining and all 16 backend execution partitions. Account for
  all 22,817 ordered geometries in each of evaluation and support modes, preserving
  source identities, random offsets, original tolerances, and result hashes.
  The complete numerical matrix has not yet run successfully.
- [ ] Complete the requested 1,530 backend performance workloads and retain their
  measurements separately from correctness. Unsupported Rust operations remain
  explicit failures or unsupported results; they do not become fabricated timings.
- [ ] If execution exposes another port defect, reproduce the first failing
  geometry with a focused check, repair the Rust controller or production bridge,
  and commit that fix separately. Preserve upstream assertions and input domains.
- [ ] Verify the final unified gate contains exactly 70 applicable source results
  per target and all C++ reference results pass. Preserve the Apple exclusion in
  the full catalog. Record the actual tested revision, run URL, report hashes,
  source verdicts, and benchmark scope in the migration guide and baseline
  provenance only after reviewing the artifacts.

## Rust runtime follow-up

These are production capability gaps exposed by the imported suite, separate
from completing the suite's upstream reference qualification.

- [ ] Triage the retained `rust-results.json`: 60 of the 65 ordinary failures
  report unavailable production APIs or wrapper failures. Implement the required
  safe Rust runtime operations and connect adapters to those operations; do not
  put replacement implementations of the runtime inside the tests.
- [ ] Resolve the five measured decoder/loader failures, retaining their original
  domains: `test-ptq1_0-element-map.cpp`, `test-ptq1_0-cuda-dot.cpp`,
  `test-tokenizer-0.cpp`, `test-tokenizer-0.sh`, and `test-tokenizers-repo.sh`.
  The PTQ element-map run rejects 19,434 original blocks; the packed-codec run
  fails 12,325 of 14,096 checks. Vocabulary loading also rejects original inputs.
- [ ] Provide the missing Rust backend operation API and run the same complete
  backend contracts against it. Backend planning currently reports unsupported
  Rust operations; it supplies no passing numerical qualification.
- [ ] Ratchet [baseline.json](../tools/prism/upstream/baseline.json) after complete
  hosted evidence proves new passing source identities. Preserve the existing
  required green IDs. Of the four observed ordinary passes, PQ2 exercises the
  runtime; the others are a zero-assertion file workload, a model-data helper,
  and pure arithmetic. Keep those scopes explicit when reporting progress.

## Constraints to preserve

- Keep the intended invalid-UTF-8 skip, immutable model/vocabulary inputs,
  synthetic DSpark references, and both documented, hash-verified upstream
  production fixes. The reference includes DSpark saver and Llama 3 decoding
  corrections; it is not an unmodified upstream build.
- Keep gradient mode an explicit diagnostic. Pinned upstream aborts building
  GEGLU backward graphs; the complete gradient matrix is not qualified. The
  original default source mode is evaluation; default acceptance also requires
  support preflight. Do not invent gradient skips or claim gradient success.
- Run full suites and every benchmark only on CI. Focused local tests and builds
  are allowed. Hosted Windows ARM64 evidence does not establish physical
  Snapdragon X Elite performance; record target hardware before making that claim.
- Keep logical changes in individual commits, publish only `main`, and retain
  the required production bridges while keeping original C++ tests retired.
