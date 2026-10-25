# Prism migration TODO

Original handoff at code revision `9332b01cd8b7102ce1f05d6858dc9d75ccf48a03`.
Implementation and qualification are tracked independently below.
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

- [x] Collect the result of [run 40150402106](https://github.com/symbiogenesis/SwiftBonsai2/actions/runs/40150402106),
  which tests `9332b01` with benchmarks enabled and optional gradients disabled.
  Its verdict was pending at handoff. The run was subsequently cancelled during
  the ordinary source job, before backend plan joining or numerical execution.
  It provides no complete-source or benchmark qualification.
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

- [x] Triage the retained `rust-results.json`: all 65 failures are indexed in
  [runtime-triage.json](../tools/prism/upstream/runtime-triage.json), bound to
  the retained artifact hash and tested revision. Sixty failures report
  unavailable production APIs or wrapper failures; five are measured
  decoder/loader defects. The inventory preserves the original failed verdicts.
- [ ] Implement the remaining required
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

## Focused implementation evidence

The following production changes have passed focused local source checks. They
are not a complete hosted run and do not ratchet the accepted baseline.

| Production capability | Original focused scope |
| --- | --- |
| Native PTQ byte decoding and packed activation dots | 20,000 arbitrary-byte blocks, 2,560,000 element predicates, and all 14,096 production decoder/dot checks; zero rejected blocks or mismatches |
| GGUF vocabulary tokenizer families | All 15 bundled vocabularies and 694 token-vector predicates; all six immutable repository corpora |
| MTP batch bounds | All six source conditions, plus widened-sum boundary regression |
| Unicode regex splitting | Original regex/input/piece assertion; reversible GPT byte encoding regression |
| Multimodal tiling and temporal grouping | All 29 source assertions, including ties-to-even alignment and mutation of remaining parts |
| Concurrent logger | Eight workers, 8,000 messages, format changes, joins, flush and explicit close; source has no output-content assertions |
| Reasoning-budget sampler | All 71 original semantic conditions, including clone/reset and UTF-8 boundaries |
| Owned batch allocator | All 26 source cases and 198 original assertions |
| GBNF parser | All 23 variants and 274 original symbol/rule-element assertions |
| Grammar stack execution | All 174 original stack-pointer/candidate-rejection conditions; independently owned clone storage |
| CPU tensor graph and manual arena storage | Original col2im geometries at all three precisions, all three DFly contracts, all five RoPE modes, and all 48 MoE alias layouts |

The CPU backend now has typed production tensor storage, strided views, graph
operations and a source-controller adapter. Advanced operations and unsupported
packed formats still return explicit failures. Complete backend qualification,
the 1,530 performance workloads, and the remaining ordinary API families above
remain open. Persistent Rust vocabulary sessions now call the same production
tokenizer, without a separate tokenizer implementation in the test harness.

The scalar and native runtime lanes in
[run 40153248553](https://github.com/symbiogenesis/SwiftBonsai2/actions/runs/40153248553)
tested `e62b81556e266b927ad11632c93ee73805d3badb` and failed compilation/analyzer
checks. The retained logs identify backend test naming/value-argument lints, an
unwired reasoning adapter, and extra trailing whitespace in `lib.rs`; no runtime
assertion verdict was obtained. Those defects were repaired through `8aca3a1`.
Release all-target analyzers pass locally for ARM64 and x64, formatting passes,
and the runtime builds without default features. A new hosted verdict is still
required.

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
