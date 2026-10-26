# Deferred Prism migration contracts

Broader runtime work belongs to
[Prism Inference](https://github.com/symbiogenesis/prism-inference) and remains
deferred behind the [Swift-only engine MVP](prism-upstream-todo.md). Reopen only
requirements demonstrated to be necessary for Swift. Implementation and complete
source qualification remain separate; a focused check does not promote a baseline.

The target is Snapdragon X Elite on Windows ARM64. Apple work is excluded.
The [conformance guide](prism-upstream-tests.md),
[71-source catalog](https://github.com/symbiogenesis/prism-inference/blob/main/tools/prism/upstream/catalog.json)
and [migration inventory](https://github.com/symbiogenesis/prism-inference/blob/main/tools/prism/upstream/migration.json)
define the upstream scope. Runner owns application and released-runtime integration.

## Full reference qualification

- [ ] Verify compact plan/execution handoffs and complete diagnostic artifacts
  without extraction limits dropping source results.
- [ ] Complete all backend execution partitions and all 22,817 ordered geometries
  in both evaluation and support modes. Preserve source identities, random
  offsets, input domains, tolerances and result hashes.
- [ ] Complete the 1,530 backend performance workloads separately from correctness.
  Unsupported operations must remain explicit; they cannot yield fabricated timings.
- [ ] Require exactly 70 Windows-applicable source results per target, with the
  Apple-only exclusion explicit in the complete inventory and passing native
  reference results.
- [ ] Ratchet the upstream baseline only after complete reviewed hosted evidence
  proves new passing identities. Preserve existing required green IDs and record
  the tested revision, hardware and actual scope of each acceptance result.

## Deferred implementation contracts

Each item below requires production safe Rust functionality and thin adapters
that retain the imported source's inputs, assertions, tolerances and stopping
rules. Existing constructors or a passing focused check do not close an item
requiring full source execution.

- [ ] **Complete backend numerical operations.** Construction dispatch now
  accompanies numerical kernels for F32/F16/BF16 `ADD1` (4), `IM2COL_BACK` (53),
  `POOL_2D_BACK` (62), `WIN_PART`/`WIN_UNPART` (78/79) and
  `GET_REL_POS`/`ADD_REL_POS` (80/81). These have focused local checks;
  full source qualification remains open. Packed `ADD1` additionally covers
  20 encodable formats with F32 scalars, restricted to one fourth-axis slab;
  auxiliary formats and codecs with unavailable encoders remain rejected.
  `FLASH_ATTN_BACK` (75) and custom/map callbacks (92–95) remain unimplemented.
  The pinned public attention-backward constructor aborts; its dormant CPU code
  is not a passing public reference. Preserve actual
  shape, stride, alias and arithmetic contracts; extraction-source restrictions
  are explicit in the [reuse review](prism-reuse-review.md).
  `ggml_build_backward_expand`, gradient snapshots and reset now support the
  implemented CPU derivative families, with transactional expansion and owned
  accumulators. Unsupported derivatives remain explicit errors. Cross-entropy
  normalizes over every trailing batch axis. Complete gradient qualification
  remains separate from default eval/support acceptance and from the excluded
  GEGLU gradient diagnostic.
- [ ] **Finish packed formats.** Types 16 `IQ2_XXS`, 17 `IQ2_XS`, 18 `IQ3_XXS`,
  19 `IQ1_S`, 21 `IQ3_S`, 22 `IQ2_S` and 29 `IQ1_M` now have checked layouts,
  arbitrary-byte decoding, native packed-activation dots and CPU graph routing,
  with focused native evidence. Types 41 `Q1_0` and 42 `Q2_0` also provide
  encoding. Types 18 `IQ3_XXS`, 21 `IQ3_S` and 22 `IQ2_S` now also implement
  native reference and importance encoding with pinned differential fixtures.
  Full source qualification remains open. Complete the remaining importance
  encoders, including IQ2_XXS/XS and IQ1; preserve the unavailable
  native CPU encoder entries. See the [direct-port evidence](prism-reuse-review.md#direct-port-continuation).
  IQ4_NL's native zero-scale reference path packs uninitialized payload nibbles;
  Rust canonicalizes those unused nibbles and preserves numeric zero. Do not
  claim byte parity for undefined native bytes or copy native stack addresses.
- [ ] **Expose complete codec traits and persistent buffers.** Implement the
  remaining owned `qb-*` transport and model-tensor APIs used
  by `test-quantize-fns.cpp`, `test-quantize-perf.cpp` and
  `test-quantize-stats.cpp`. The production `codec` module now exposes all 144
  pinned trait slots and bounded buffers with checked byte regions, stale-handle
  rejection, overlapping calls and unchanged output on failure. The ordinary
  codec adapter uses this API. Native capabilities and implemented Rust operations
  remain distinct; unported CPU quantizers reject without a reference fallback.
  Persistent model sessions, file transfers, clocks and full source qualification
  remain open. Preserve all 22 required eligible type identities,
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
  The six capability probes are implemented, including object/string argument
  fallback and usage after exceptions. Native comparison matches 31 of 37
  templates; Apertus, Mistral-Small-3.2, Firefunction-v2, IBM Granite 3.3,
  Llama 3.2 and GPT-OSS retain execution gaps, also visible in the existing
  string-content probe. The ignored `jinja_capabilities` native differential
  reproduces those limits. Analysis still needs supplied-time direct apply and
  undefined-variable statistics. Server conversion must retain request reasoning budgets,
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
  The resolver transport now checks complete Content-Length/chunked bodies and
  bounds curl capture; focused malformed-transfer checks and the original
  model-resolution source pass. Full HTTPS/download/cancellation qualification
  remains open; see the [reuse review](prism-reuse-review.md).
- [ ] **Qualify the resolver's remaining input domains.** The original source
  exercises refs/tree listing retrieval and offline finalization. Separate
  production parity checks remain for live HTTPS/curl, actual URL/multi-shard
  downloads, redirect/Content-Length behavior, recursive cached snapshots and
  metadata-inferred draft resolution. Quant-tag patterns currently use Rust
  regex syntax; support or explicitly reject native ECMAScript-only syntax
  rather than claiming complete regex-domain equivalence from these fixtures.
- [ ] **Provide the shared model/context/session APIs.** The owned model loader,
  generated-tensor/progress callbacks and complete streaming GGUF saver now have
  focused local contracts. Decoded nonfinite payloads are rejected even when
  their stored codec scales are finite. Still implement encoder/decoder contexts,
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
- [ ] **Complete full-migration hosted acceptance and provenance (deferred).** After integration, publish
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
