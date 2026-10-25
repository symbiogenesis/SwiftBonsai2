# Prism migration TODO

Original handoff at code revision `9332b01cd8b7102ce1f05d6858dc9d75ccf48a03`.
Implementation and qualification are tracked independently below.
Wrap-up update: completed in-flight work is being committed to local `main`.
The user explicitly requested **no further push**. Remote `main` was last
published at `5ede838409461f6d8385ad20167288a44f519946`; subsequent local commits
have no hosted verdict. The migration is **not complete**.
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
  Planning is now demonstrated: run
  [40153890429](https://github.com/symbiogenesis/SwiftBonsai2/actions/runs/40153890429)
  at `8aca3a1` passed all 16 planning partitions and the plan join. Compact
  `prism-backend-plan-*` records and separate diagnostic artifacts were retained.
  Execution stopped before producing all compact execution records, so this
  combined handoff item remains open.
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
| Grammar/schema integration | All 504 grammar integration conditions and all 269 JSON-schema source assertions |
| PEG parsing and serialization | All six inherited groups, 241 source assertions; owned captures and atomic/partial parsing added |
| Jinja execution | All 278 literal renders, 56 original observations, 16 additional streaming-hash checks and original no-crash fuzz calls |
| Chat templates and chat PEG | All 36 legacy and 35 common Jinja renders; all seven chat-PEG correctness groups, 30 subgroups and 105 incremental prefixes; embedded benchmarks not run locally |
| Multimodal chunks | All 46 source factory/traversal/serialization conditions |
| Stateful sampling | All 343 original correctness conditions; 20 independent native seeded RNG/XTC observations and clone/reset/offset regressions |
| Owned GGUF contexts | All 34 crafted source variants, 96 source conditions and 27 malformed-input guards |
| Graph allocator | All 13 scenarios/14 graphs; 468 unconditional predicates and 136 triggered overlap checks |
| CPU threadpool | Focused worker-switching, dependency/barrier, independent-plan and error-draining checks; original timed workload remains CI-only |
| Optimizer/datasets | All 164 original CPU subtests, including AdamW and SGD; additional generic gradient/classification/ownership checks |
| Quantization selection | All 11 unchanged real metadata models and 363 expected sections |
| HF model resolution | All 94 listing variants/13 handler assemblies, 757 original predicates, 358 guards and 228 live loopback GETs; complete CLI parser remains separate |
| Legacy, K, IQ4, TQ and FP4 codecs | Independent native reference/importance encodings, arbitrary packed decoders and generic packed activation dots; complete source/benchmark qualification remains open |

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
required. Subsequent native failures are tracked below; this paragraph records
the earlier failure rather than the final session status.

## Latest CI failures and committed repairs

- [Run 40155168704](https://github.com/symbiogenesis/SwiftBonsai2/actions/runs/40155168704)
  tested `a2d085d`. Scalar fallback passed. Native execution passed its runtime
  tests, then the no-default-features analyzer attempted to compile the JSON
  example with its schema/PEG modules disabled. `03fd07b` declares the example's
  `cli` requirement. The exact no-default all-target analyzer passed locally.
- [Run 40155820247](https://github.com/symbiogenesis/SwiftBonsai2/actions/runs/40155820247)
  tested `03fd07b`. Its native suite failed only the Jinja observation-count
  guard: all observations passed, but the guard still expected 56 after 16
  additional hash checks were added. `97a0dda` expects 72; the focused test passed.
- Both recent `prism-upstream / 3` failures stopped at Rust
  `test-tokenizer-1-bpe.cpp`. The retained earlier diagnostic identifies the
  original Aquila GGUF v2 vocabulary being rejected by the v3 reader.
  `d563f38` adds a verified vocabulary-only v2 reader while preserving strict v3
  model execution. The real unchanged Aquila vocabulary now passes the focused
  persistent tokenization/detokenization session; the complete roundtrip source
  still needs hosted completion.
- Backend execution in `40153890429` stopped in C++ geometry **15450**,
  `test_argsort`, because repeated immutable shape queries exhausted the bounded
  transport action count. `41a6f79` reuses actual typed production observations
  until metadata changes. The exact geometry passed against Rust and production
  C++, preserving geometry seed, CRT offset **7,748,238**, **262,400** draws,
  original inputs and comparisons. This does not qualify the remaining matrix.
- [Run 40156509200](https://github.com/symbiogenesis/SwiftBonsai2/actions/runs/40156509200)
  tests the last published `5ede838`; it was pending when inspected. No passing
  verdict is claimed. Later local wrap-up commits are intentionally unpublished.

## Remaining implementation work

Each item below requires production safe Rust functionality and thin adapters
that retain the imported source's inputs, assertions, tolerances and stopping
rules. Existing constructors or a passing focused check do not close an item
requiring full source execution. The historical 65-failure triage remains a
record of its tested revision, not a current capability inventory.

- [ ] **Complete backend numerical operations.** Construction dispatch exists
  for 172 of 173 imported API names. Numerical execution is still missing for
  `ADD1` (4), `IM2COL_BACK` (53), `POOL_2D_BACK` (62), `FLASH_ATTN_BACK` (75),
  `WIN_PART`/`WIN_UNPART` (78/79), `GET_REL_POS`/`ADD_REL_POS` (80/81), and
  custom/map callbacks (92–95). Implement their actual shape, stride, alias and
  arithmetic contracts. `ggml_build_backward_expand`, gradient snapshots and
  reset remain incomplete; keep these separate from default eval/support
  acceptance and from the excluded GEGLU gradient diagnostic.
- [ ] **Finish packed formats.** Remaining type IDs are 16 `IQ2_XXS`, 17 `IQ2_XS`,
  18 `IQ3_XXS`, 19 `IQ1_S`, 21 `IQ3_S`, 22 `IQ2_S`, 29 `IQ1_M`, 41 `Q1_0`,
  and 42 `Q2_0`. Supply checked byte layouts, arbitrary-byte decoding, reference
  and CPU encoders where native traits expose them, actual importance semantics,
  and native packed-activation dots. Preserve each trait's unavailable entries.
  IQ4_NL's native zero-scale reference path packs uninitialized payload nibbles;
  Rust canonicalizes those unused nibbles and preserves numeric zero. Do not
  claim byte parity for undefined native bytes or copy native stack addresses.
- [ ] **Expose complete codec traits and persistent buffers.** Implement the
  production trait inventory and owned `qb-*` buffer/call/model-tensor APIs used
  by `test-quantize-fns.cpp`, `test-quantize-perf.cpp` and
  `test-quantize-stats.cpp`. Preserve all 22 required eligible type identities,
  scalar input order, block boundaries, alignment offsets, native dot operand
  layouts, per-call schedules, statistical accumulation and source thresholds.
  Statistics must read the actual pinned model's F32/F16 tensors. Implement real
  clocks for supported counters; unavailable operations must remain explicit.
  Run the entire performance source and its timing paths only on CI.
- [ ] **Complete chat APIs beyond templates/PEG.** `test-chat-analysis.cpp`,
  `test-chat-auto-parser.cpp` and `test-chat.cpp` still need full source
  qualification and production direct-apply/statistics, differential template
  analysis, format detection, generated parser construction and reasoning/tool
  mapping. Complete server request/response conversion, tool-call/content-part
  JSON handling, delimiter splitting, regex behavior, streaming diffs and
  grammar validation. Reuse the new Jinja, PEG and typed message APIs; do not
  replace these with source-specific expected text. The auto-parser source's
  513 native assertions plus 11 normalization conditions remain due. Chat
  analysis retains 37 diagnostic templates/19 variable probes and zero original
  expected-output assertions. General chat retains all 14 groups, 431 parser
  cases, 363 fixed checks and every original byte-prefix parse/diff/reconstruction.
  Analysis also needs the full six capability flags, supplied-time direct apply
  and undefined-variable statistics; typed/string capability probing alone is
  insufficient. Server conversion must retain request reasoning budgets,
  template kwargs, Responses conversion, developer-role and model-specific
  thinking/tool-order workarounds. Byte-regex captures/offsets and malformed
  UTF-8 retry/partial-input handling are distinct from Unicode regex splitting.
- [ ] **Finish argument parsing and downloads.** Implement complete production
  option registration/negation/environment handling for all 27 example contexts,
  actual parameter mutation, numeric/CSV validation and speculative output-limit
  conversion. Preserve inventory uniqueness and original invalid-option cases.
  Perform bounded real HTTP retrieval with the supplied headers/timeouts/body
  limits and original 200/404/oversize/cancellation behavior. Model-resolution
  option handlers alone do not constitute the full CLI parser.
- [x] **Implement and locally qualify model resolution.** Preserve live listing/commit retrieval,
  listing order, quant tag matching, split-first-shard selection, ancestor
  sidecar precedence, cache/offline behavior and explicit-path/preset/draft
  handler precedence. Run all 94 listing variants and 13 handler assemblies
  (757 predicates) against the production Rust resolver and original loopback
  HF inputs. The final in-flight implementation passed that complete focused
  source, 358 guards, 228 live GETs and five additional production checks;
  integrated locally as `24a3f0e`. Hosted qualification remains open below.
  Its model/draft option handlers do not implement the complete CLI parser.
- [ ] **Provide the shared model/context/session APIs.** Build an owned model
  loader, generated-tensor/progress callbacks, encoder/decoder contexts,
  vocabulary/logit ownership, graph reservation/export, sequence/KV state,
  independent checkpoints and file/buffer save/restore. Implement a persistent
  Rust model-session transport with concurrent ownership and callback errors.
  The current Qwen35 runtime and vocabulary-only sessions do not cover these
  generic model contracts.
- [ ] **Implement architecture graph builders.** `test-llama-archs.cpp` contains
  149 catalog names/151 variants, with 128 source-supported variants. Preserve
  original unsupported distinctions. Required behavior includes real generated
  tensor callbacks, hparam/tensor shape validation, context creation, encode for
  T5/DREAM/LLADA/LLADA_MOE/RND1, original 128-token decode/logit checks, CPU/backend
  NMSE, graph export, saver/file-pointer reload and exact restored logits.
  Partition work into metadata/tensor inventory+saver, architecture-family
  forward graphs, recurrent/hybrid memory, and the shared session transport.
  A copied architecture name table cannot satisfy this source.
- [ ] **Complete model lifecycle and backend-dependent sources.** Run every
  original model/progress/state/concurrency contract in `test-autorelease.cpp`,
  `test-model-load-cancel.cpp`, `test-save-load-state.cpp`,
  `test-state-restore-fragmented.cpp` and `test-thread-safety.cpp` using their
  unchanged stories15M assets and complete schedules. Implement the real model
  behavior behind `test-recurrent-state-rollback.cpp`, `test-backend-sampler.cpp`,
  `test-export-graph-ops.cpp` and `test-kv-mean-center.cpp`; standalone kernel
  snapshots, graph allocator support or greedy helpers do not qualify them.
- [ ] **Complete DSpark and LoRA.** The four DSpark sources require production
  metadata/log-SNR, forward/loop/speculative state and real-eval APIs using their
  original synthetic references and complete 24-prompt AR/verification schedule.
  Retain both approved upstream production corrections. LoRA requires actual
  conversion, tensor merge/application and inference against the original source
  contract, rather than a successful file conversion alone.
- [ ] **Provide LLGuidance and the public C surface.** Implement the actual
  LLGuidance/model-session grammar contract used by `test-grammar-llguidance.cpp`;
  forcing the existing GBNF converter is not equivalent. Supply the public C
  header/ABI needed by `test-c.c`, preserving safe Rust boundaries. That source
  is compile-only with zero behavioral assertions; record that scope honestly.
- [ ] **Finish tokenizer qualification.** Rerun the five originally measured
  decoder/loader failures and every tokenizer source on settled `main`, including
  file-wrapper/generator behavior, randomized HF references, all immutable
  external corpora, complete SPM/BPE token and Unicode roundtrips, and the
  original ignore-merges branch. Preserve the explicit invalid-UTF-8 skip,
  Falcon diagnostic exclusion and hash-verified Llama3 production correction.
- [ ] **Complete hosted acceptance and provenance.** After integration, publish
  only `main` when authorized, run the full native/scalar lanes and all 70
  applicable unified source contracts, verify compact execution handoff plus
  separate full diagnostics, and complete all 22,817 eval/support geometries
  and 1,530 performance workloads. Record tested revision, URLs, report hashes,
  hardware and actual verdicts. Only then ratchet the baseline while preserving
  its required green IDs. Generic hosted Windows ARM64 is not physical
  Snapdragon X Elite performance evidence. Full suites/benchmarks remain CI-only.

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
