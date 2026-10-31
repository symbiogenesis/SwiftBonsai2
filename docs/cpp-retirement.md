# C++ retirement contract and inventory

The chosen cross-repository milestone is to remove C++ from SwiftBonsai2Runner
and prism-inference, including tests, reference controls, generated source,
inference dependencies and their compiler/build requirements. [PLAN.md](../PLAN.md)
is the single backlog; this document defines inventory, replacement evidence and
retirement conditions. It does not declare the full migration complete.

The scope includes downloaded or bundled C++ inference libraries and DLL-backed
reference tools, not only files found by an extension search. Native Rust UI and
HTTP/JSON/SSE contracts remain unchanged. Vendor system/driver ABI boundaries need
an explicit dependency assessment; moving C++ behind an FFI or process does not
retire it. Existing Git history is not rewritten. Retain source identities,
licenses/notices and legally required attribution, plus qualification evidence;
they do not require retaining operative C++ source or compiler invocations.

## Replacement and deletion rules

Each file or inseparable family needs a recorded disposition: Rust replacement,
immutable observations, or removal of functionality through its existing
acceptance contract. Deleting a control alone does not qualify its replacement.
Before deletion:

1. Bind the replaced behavior to exact upstream revision, source/patch hashes,
   license, target, toolchain, inputs, configuration and expected observations.
   Capture all approved assertions and failure/lifecycle cases that it supplies.
2. Preserve independent controls through immutable fixtures, mathematical known
   answers or separately implemented Rust references. Fixtures retain complete
   input/output identity, schema, counts and digests. They are historical captures,
   not fresh execution of the retired engine. Reports must identify that provider
   honestly rather than relabeling a capture as a newly passing C++ oracle.
3. Pass focused replacement checks before source removal. Owning hosted consumer
   builds and assertions remain integration completion gates; record them pending
   when only local checks ran. Preserve numerical ordering/tolerances, signed and
   exceptional bits, ISA/target boundaries, subset errors, source IDs, approved green
   floors and exclusions. Source removal alone does not close those gates.
4. Remove operative source includes/generators, compiler/link/download/cache paths,
   feature/dependency edges, workflow/control dispatch and stale documentation for
   that file. Retain evidence and notices as data where needed.
5. Keep full-model, actual-device, quality, resource, isolation and distribution
   gates pending until independently satisfied. Synthetic replay does not close
   them. Owner-device work shares the five-minute total budget; complete suites
   and benchmarks remain CI-only.

Current [runtime conformance](prism-upstream-tests.md), [native Rust](native-rust-engine.md),
[speech](../src/speech-tts/README.md) and [validation](validation.md) contracts remain
the acceptance inputs. Conformance-provider changes must preserve every approved
green ID and the complete catalog/schedule, including 22,817 ordered backend
evaluation/support geometries and separately selected performance workloads.
Missing captures or unimplemented assertions remain visible pending/red outcomes.

## Runner checked-in native files

Paths below are relative to Runner. Nine C++ source/header files remain from an
initial eleven; the first two rows record removed controls and retained evidence.
Generated private-source C++ and external dependencies are tracked separately.

| File | Replacement and exit condition |
| --- | --- |
| `src/speech-tts/tests/reference/philox.h` | Source removed: normalized-source SHA-256 replaces the anchor. Licensed provenance, 4,411 ordered scalar observations and independent block/rounding answers remain. Hosted owning-crate integration remains pending. |
| `src/speech-tts/tests/philox_oracle.cpp` | Source removed: Rust validates immutable records and fixture identity. Its compilation/launch and indirect sampler-oracle inclusion are removed; provenance is retained as data. |
| `src/speech-tts/tests/reference/sampling.h` | Freeze existing suppression/repetition bit cases; qualify the remaining probability pipeline separately, replace source anchoring with identity metadata, then remove header use. |
| `src/speech-tts/tests/sampling_oracle.cpp` | Replay complete ordered logits/history/parameter/output records in Rust, preserving duplicate/invalid IDs, signed zero and exceptional floats. Remove native compilation. |
| `src/speech-tts/tests/reference/bpe.h` | Freeze scanner chunks, encoded IDs, metadata/special precedence and actual pinned vocabulary captures; retain exact source/license identity as data and qualify the Rust tokenizer consumer. |
| `src/speech-tts/tests/tokenizer_oracle.cpp` | Preserve complete fixture and capture identities plus scanner/merge/fallback/EOS assertions in Rust; remove compile/run path. |
| `src/speech-tts/tests/tokenizer-gguf.h` | Replace model-free GGUF metadata handoff observations with Rust-owned loader/boundary checks when the generated tokenizer shim retires. |
| `src/speech-tts/tests/tokenizer-error.h` | Preserve registration/encode failure and cleanup observations without a generated C++ exception/logging fixture. |
| `src/speech-tts/tests/tokenizer_lifecycle.cpp` | Move metadata intake, replacement, failure cleanup and result ownership checks onto the final Rust loading path; preserve all existing RAII assertions. |
| `src/speech-tts/tests/decode_graph_cache.cpp` | Move decode-workspace selection/eviction into the Rust engine and preserve allocation, batch/window changes, failure cleanup and completed-work lifetime checks. |
| `tools/native-reference/swift_ptq1_reference.cpp` | Preserve pinned logits/state/control captures and artifact/adapter identities; qualify a Rust collector/engine replacement, then retire native DLL collection and its build workflow. |

## Prism checked-in native files

All paths below are under `prism-inference/tools/prism/upstream/common/`.
There are **40 C++ files**: `bridge.cpp` and 39 `.hpp` files. The two public-C-ABI
files are coupled native cleanup, not additional C++ files. Each row must preserve
its existing source-level assertions and contracts through the process above.

| Family | File | Required retained behavior |
| --- | --- | --- |
| Text/API | `vocab_session.hpp` | Stateful vocabulary operations and token/piece buffer contracts. |
| Text/API | `vocab.hpp` | Vocabulary traits, IDs and byte spellings. |
| Text/API | `jinja.hpp` | Template evaluation, capabilities and failure behavior. |
| Text/API | `grammar_parser.hpp` | Grammar parsing and rejection cases. |
| Text/API | `grammar.hpp` | Grammar state, candidate filtering and lifecycle. |
| Text/API | `peg.hpp` | PEG construction, serialization and parsing. |
| Text/API | `json_schema.hpp` | Schema-to-grammar conversions and diagnostics. |
| Text/API | `chat_templates.hpp` | Exact template bytes and selection. |
| Text/API | `chat_parser.hpp` | Incremental content/reasoning parsing. |
| Text/API | `chat_auto.hpp` | Automatic chat-format/capability decisions. |
| Text/API | `chat_analysis.hpp` | Message analysis and source-tested conversions. |
| Text/API | `chat.hpp` | Chat message/request conversions. |
| Text/API | `arg_parser.hpp` | CLI defaults, precedence and rejected options. |
| Text/API | `model_resolution.hpp` | Acquisition/cache/sidecar plans and handler decisions. |
| Text/API | `reasoning_budget.hpp` | Budget lifecycle, forced tokens, clone/state and UTF-8 observations. |
| Text/API | `sampling.hpp` | Complete sampler ordering, draws, candidates and lifecycle. |
| Numerical/ownership | `tensor_graph.hpp` | Tensor construction and graph operation observations. |
| Numerical/ownership | `arena_graph.hpp` | Arena graph ownership, bounds and reuse. |
| Numerical/ownership | `batch_allocator.hpp` | Batch/sequence allocation and failure cases. |
| Numerical/ownership | `allocation.hpp` | Allocation topology, byte counts and cleanup. |
| Numerical/ownership | `threadpool.hpp` | Plans, persistent workers, barriers and failures. |
| Numerical/ownership | `backend_ops.hpp` | Full ordered operation geometry/support/evaluation schedule. |
| Numerical/ownership | `backend_session.hpp` | Backend session state, export and lifetime. |
| Numerical/ownership | `backend_sampler.hpp` | Backend sampler/candidate ownership and source observations. |
| Numerical/ownership | `context_graph.hpp` | Context graph state, construction and execution. |
| Numerical/ownership | `codec_buffers.hpp` | Owned aligned buffers, traces, ranges and primitive codec calls. |
| Numerical/ownership | `codec.hpp` | Codec traits, exact arithmetic and supported layouts. |
| Numerical/ownership | `gguf.hpp` | GGUF metadata/tensor read-write and bounds. |
| Numerical/ownership | `quantization.hpp` | Model quantization decisions and type/shape policies. |
| Numerical/ownership | `optimizer.hpp` | Optimization state, updates and source numerical assertions. |
| Model/multimodal | `model_runtime.hpp` | Actual model/session load, state, inference and recovery contracts. |
| Model/multimodal | `generated_model.hpp` | Generated model metadata and loading observations. |
| Model/multimodal | `dspark_runtime.hpp` | DSpark model/session execution and state. |
| Model/multimodal | `dspark.hpp` | DSpark serialization, adapter and approved correction controls. |
| Model/multimodal | `mtmd_chunks.hpp` | Multimodal chunk ownership and metadata. |
| Model/multimodal | `mtmd.hpp` | Multimodal input, model and API contracts. |
| Model/multimodal | `llguidance.hpp` | Guidance candidate/template behavior and exact external identity. |
| Transport/concurrency | `action_file.hpp` | Bounded file extent/EOF checks and Windows publication sharing. |
| Transport/concurrency | `concurrency.hpp` | Concurrent logger schedules, finalization and cleanup. |
| Dispatch | `bridge.cpp` | Remove each retired family route; retire final dispatcher after all provider migrations. |
| Coupled C ABI | `public_c_abi.h`, `public_c_abi.c` | Preserve target-specific layout/compile observations and negative-call contracts as fixtures/Rust assertions before removing native linkage. |

## External engines, generators and build dependencies

Retirement also covers the Qwen/GGML TTS core and `transcribe-cpp`/`transcribe-cpp-sys`
Canary ASR core, including their separate GGML versions. Native Rust replacements
must retain model verification, audio/token contracts, cancellation, state, output
quality and actual-device resource acceptance. Reusing a C++ library's Rust binding
does not satisfy this milestone.

Expand each external family to exact files from its pinned source manifest before
porting it. Current boundaries include Qwen `src/bpe.h`'s remaining GGUF shim,
`src/prompt-builder.h`, both sampling headers, pipeline/codec/speaker graphs and
audio transforms. A Rust policy callback or removed test oracle does not retire
the downloaded header, generated shim or linked engine that still contains it.

Remove retired paths from `speech-tts/build*.rs`, native-speech provisioning/cache
actions and scripts, `native_startup.rs`/`speech_build.rs`, compiler/source locks,
Cargo edges and package notices where no longer required. Retire operative publisher
routes in `runtime.lock.json`, `swift-assessment.lock.json`, `Build-Engine.ps1`,
`Build-HeadlessPublisherCandidate.ps1`, native capture tooling and candidate patches
only after their replacement/fallback and distribution gates pass.

Prism's coupled cleanup includes its three `CMakeLists.txt` projects,
`RustBridge.cmake`, `Apply-ReferencePatch.cmake` and production patches; native loader
and LoRA-export linkage; LLGuidance/native codec control downloads; and foundation,
focused/conformance compiler caches, packaging and provider dispatch. Retain safe
Rust functionality and evidence rather than deleting the assertions with the build.

Final acceptance requires source/generated-source inventory, dependency and binary
linkage assessment, clean hosted consumer builds without a project C++ toolchain,
preserved qualification contracts, and no operative references to retired controls.
Historical capture metadata and legally required notices remain distinguishable
from live compiler, download, linkage or execution requirements.
