# Swift Bonsai 2 on Snapdragon X Elite

**Research and implementation plan — October 22, 2026**

**Current priority — October 23, 2026:** the owner explicitly prioritized Swift
Bonsai 2 and BITCOS memory assessment. [The assessment](docs/swift-memory.md)
pins and verifies the current 6.71 GiB PQ2 artifact and Prism's native ARM64 CPU
package. Actual symbol census finds 32.76% zeros, no fourth symbols and finite
nonzero scales; block-rounded BITCOS accounting suggests about 0.98 GiB less
serialized data before new indexes/alignment. A reference block codec preserves
exact source bytes in focused regressions. Swift's explicit 8K CPU check uses
12 GiB service allowance plus the unchanged 4 GiB physical reserve and passes
admission and completes the fixed 16-token decode after a template-compatible
system-preamble repair. An exact current-weight PTQ1 exporter offers 1.173 GiB
of payload savings; standalone compact BITCOS dots pass focused numerical
parity. The verified current-weight PTQ export completes bounded native decode,
including the separately pinned pretrained adapter. BITCOS's initial C++ ARM
synthetic performance failed promotion even after a vector decoder improvement.
The standalone implementation is now safe Rust with scalar computation; exact
C++ byte/result comparisons passed before its C++ sources were removed. Rust
SIMD and performance remain unqualified. New systems components default to Rust;
replacement of the pinned native engine and its LoRA overlay is separate work.
Native optimized-model performance, creative/comfort and
daily isolation qualification remain pending. No new training is planned.
The approved abliteration LoRA is intended for the standard profile, with
mandatory baseline performance and quality qualification before acceptance.
The paired CI capture/scorer and a pinned immutable-B-sharing source overlay
are implemented; native storage sharing saves a verified 2.5 MiB in the focused
backend regression. Target-device TPS/quality and the replacement Windows build
remain unqualified. The earlier Qwen
reference below is retained for reproducibility.

**Status — October 23, 2026:** implementation started. [M0a chooses Qwen3.8-27B UD-Q3_K_XL from literature](docs/model-selection.md), with Gemma 4 12B QAT as fallback. M0 inventory, pinned provisioning, full GGUF inspection and the native ARM64 CPU source build are implemented. The external model is size/SHA-256 verified; metadata confirms 866 tensors across 14 types and the expected template/dimensions. M1 now includes a nonlaunching fixed-policy model preflight, [bounded Rust HTTP admission/orchestration](docs/broker-policy.md), an explicit development broker executable with bounded shutdown/drain, and separate owner-private credential creation. Authentication, Host/Origin/raw routes, strict JSON and canonical sampling/output caps, exact-count admission, bounded framing, one active request with bounded waiting, absolute deadlines, frontend disconnect cancellation and fail-closed generation admission have focused synthetic coverage. The fixed CPU check uses atomic Windows Job ownership with kill-on-close, a job-wide committed-memory cap and one-process limit. Strict local builds/analyzers and focused native regressions pass. After the owner stopped WSL, available RAM rose to about 20.1 GiB; the unchanged 22 GiB admission requirement still refused model launch. The Rust broker now has a separate explicit Windows reference-CPU development launcher with pinned artifact verification, protected credentials, atomic Job ownership, authenticated readiness, listener PID checks and resource monitoring. Daily chat and UI remain disabled. The read-only launch check verifies installed artifacts even when memory admission refuses launch; synthetic lifecycle regressions cover ownership and shutdown. Real model execution through this launcher, same-user backend bypass prevention, real tokenization/cancellation behavior, restricted identity/filesystem/egress and creative/comfort qualification remain pending. A Job commit cap does not bound all resident mapped model bytes. No drivers, benchmarks, or inference-performance verdict changed. See the [development guide](docs/development-guide.md) and [implementation evidence](docs/implementation-research.md). Values marked *proposed* or *estimated* remain design inputs to validate.

**Model selection comes first, through literature searching only.** Choose the model up front using already-published, publicly accessible online benchmarks, evaluations, model documentation, and samples. Model selection will **not** be based on local benchmarking. Record sources, dates, exact model versions/quantizations, relevant settings, and evidence gaps, then document the choice before building the inference engine. The next phase is to **build, validate, and optimize the inference engine for that chosen model**. Engine correctness, creative-workflow checks, and latency, throughput, memory, energy, and thermal measurements remain in scope under the repository's CI rules; they validate and improve the implementation rather than serve as a candidate-model selection campaign.

## 1. Recommended direction

- [ ] **Required: qualify the standard abliteration LoRA against the same model without it.**
  The fixed [paired CI capture and gate scorer](docs/lora-qualification.md) are
  implemented. Device capture, native correctness evidence and human reviews
  remain pending; the TODO is not complete.
  Run matched CI comparisons on the target Snapdragon with the exact model,
  representation, runtime, template, sampler, context and resource limits held
  fixed. Resolve adapter application/basis correctness first. Require overall
  **decode tokens per second and output quality each to be on par with or better
  than the no-LoRA baseline** before accepting the standard profile. Output
  quality includes general capability and creative usefulness. These are the
  primary performance criteria; lower total token use or fewer refusals cannot
  substitute for a tokens-per-second or quality regression.
  Record prefill, first-token latency, decode throughput, tokens/time/retries to
  useful completed output, memory, power/thermals and failures. Predeclare
  workloads, quality rubric and parity tolerances; retain per-metric regressions
  rather than hiding them in an aggregate score. A modest, bounded LoRA RAM
  premium is acceptable within the resource envelope; equal RAM usage is not an
  acceptance requirement. Evaluate useful completed work, including refusal and
  retry tokens/time: improved answers or fewer wasted generations may improve
  overall performance even with added adapter computation. Continue lossless
  overhead reduction without assuming those gains are already demonstrated.
  Abliteration and this comparison
  are part of standard-profile acceptance, not optional enhancements. Keep the
  no-LoRA path as the control and diagnostic reference. Loading alone does not
  complete this TODO. See [required qualification](docs/swift-lora.md#required-standard-profile-qualification).

**Research current Swift Bonsai 2 PQ2_0 first**, alongside the alternatives in section 3, and choose the model from published online evidence before implementation. Its compact weights, inherited hybrid attention, and shorter-reasoning training make it a promising efficiency candidate, not an established creative-quality winner. Review published Swift/base Bonsai comparisons and relevant Gemma/Qwen results; document uncertainty where exact-pack creative or laptop evidence is missing. Then build the inference engine for the chosen model: use Prism ML's pinned llama.cpp fork for Swift/Bonsai or upstream llama.cpp for a selected conventional alternative. Keep the server's built-in web UI for direct chat and engine use. Treat coding agents as an optional later feature: evaluate **OpenCode** and qualify **MXC ProcessContainer** before enabling tools. Use a small Rust policy/launch broker where custom security-sensitive orchestration is needed; retain PowerShell for installation/build management. Establish CPU correctness first, qualify **Vulkan on the Adreno X1-85** next, and consider the documented OpenCL route where the selected model's quantization is supported. Select the CPU/GPU configuration using measurements from this laptop. Treat direct Hexagon NPU execution as a separate research milestone.

The objective is the best useful creative assistant that can run responsively and comfortably within this laptop's memory and thermal limits. Choose creative quality within those constraints; model branding, parameter count, compression ratio, and processor utilization are not objectives. Simultaneous CPU, GPU, and NPU utilization is a candidate configuration, not an acceptance requirement: extra processors can introduce synchronization, shared-memory contention, and power competition.

The owner's primary uses are **creative writing and art**, ahead of coding. Prioritize prose quality, style and voice control, character/scene continuity, revision, brainstorming, art direction, and image-prompt composition. Image critique additionally requires a qualified vision path. Generating images is a separate model/pipeline decision; a text-generating LLM does not become an image generator by accepting images.

The owner also places strong weight on **short waits and comfortable sustained laptop operation**. Use published evidence for these concerns when choosing the model up front. During engine development, require useful creative output and correct behavior; among implementation profiles for the chosen model, favor time to useful prose, time to an acceptable finished piece, and restrained sustained heat/fan activity. Swift-first is a research priority, not a measured efficiency or quality verdict.

| Decision | Initial choice | Reason |
| --- | --- | --- |
| Runtime | Upstream llama.cpp for conventional candidates; Prism fork for Swift; each pinned by full commit | Match the model architecture, tensor types, and backend to an explicit runtime |
| Main model | Source-only compatibility preflight, then literature-based choice | Choose creative usefulness within support/memory constraints before building its engine; record a fallback decision |
| Baseline | Native ARM64 CPU, FP16 KV, one conversation | Establish correctness with a simple execution path |
| First accelerator | Native ARM64 Vulkan; OpenCL candidate for supported conventional types | Swift needs its released packed kernels and transforms; ordinary Q4_0 has an additional documented Adreno route |
| Direct chat / engine inspection | Authenticated, inference-only built-in web UI and loopback API | Restrict browser origins, endpoints, and process access before daily use |
| Full agent harness | Optional later: OpenCode native Windows ARM64 candidate; Aider fallback | Direct creative chat is the first delivery; qualify model behavior and containment before adding tools |
| Tool/agent containment | MXC native ProcessContainer candidate | Explicit Windows ARM64 support; exact policy enforcement remains a gate |
| Security-sensitive custom controller | Small Rust broker using MXC's SDK | Keep policy code small; contain unsafe/native components separately |
| Cache policy | Model-specific KV/state accounting and bounded reuse | Qwen/Swift recurrent state and Gemma sliding-window attention require different estimators |
| NPU | Time-boxed feasibility investigation | No demonstrated native Swift packed-weight path established here |
| Speculation | Disabled initially; model-free prompt lookup first experiment | Copy-heavy revision may benefit without a second model; verification/rollback remain gates |
| Additional CPU optimization | Profile first; consider WoS Porter and T-MAC-style lookup kernels | Target a demonstrated bottleneck in the selected model; Swift adapters remain separate work |
| BITCOS | Gated lossless repacking/kernel experiment | Actual symbol census, native kernels, and end-to-end benefit required |
| NoGraphicsAPI | Do not replace the ggml Vulkan backend with it | Vulkan-based graphics abstraction with narrower target requirements |
| Performance evidence | Manually triggered self-hosted CI on this laptop | All benchmarks are CI-only under the working preferences |

Runtime support and the distinction between the published Swift files are documented in the [Swift model instructions][swift-model]. Vulkan's newer support is documented in Prism's [backend audit][backend-support]; it is source support, not certification of this laptop.

## 2. Hardware envelope and what “optimal” means

### Target supplied by the owner

| Component | Planning input | Design implication |
| --- | --- | --- |
| OS / architecture | Windows on ARM64 | Native ARM64 host binaries and compatible driver libraries |
| CPU | Snapdragon X Elite X1E-78-100, Oryon, reported 3.42 GHz | Tune generation and prompt-processing threads separately |
| GPU | Adreno X1-85; Windows reports 128 MB dedicated graphics memory | Query usable shared-memory budgets; do not treat 128 MB as the model capacity |
| NPU | Snapdragon Hexagon | Verify Windows runtime access, supported operations, packing, and memory limits |
| RAM | 32 GB installed, 31.6 GB reported usable; usable-byte count not yet verified | Resolve the reported units, then budget weights, caches, checkpoints, staging buffers, Windows, and applications together |
| Storage | Approximately 292 GB free from supplied figures | Enough for initial packs/builds; avoid uncontrolled model and cache duplication |

A read-only host check during this plan revision reports **Windows 11 Professional 26H2, ARM64, build 26300.9711**. MXC's current [OS support table][mxc-os-support] lists 26300.9550 for that release family. The host is above that documented floor; this does not establish that the installed MXC package or the requested policy will work. No Windows feature, policy, or software was installed or changed.

The memory figures below use **GiB/MiB** for runtime allocation and **GB** for publisher download sizes. Shared physical memory does not guarantee zero-copy operation, a single resident weight copy, or unrestricted GPU allocation. Discover actual limits before setting offload or context size.

Optimize these objectives in order:

1. Establish useful creative-writing quality, artistic collaboration, and instruction following before specializing the runtime for one model; qualify coding/tools separately if wanted.
2. Preserve each candidate's declared behavior and correct conversation state when evaluating backend or kernel changes.
3. Minimize time to useful prose and an acceptable finished response while keeping sustained heat, fan activity, and energy use acceptable for this laptop.
4. Keep Windows responsive and avoid sustained paging or GPU-memory eviction.
5. Support the useful context length for the task without silently discarding information.
6. Offer efficient battery and plugged-in profiles; peak throughput or full processor utilization is not the default objective.

Record both first streamed reasoning token and first final-answer token. A reasoning model can appear responsive while taking much longer to produce its useful answer. Report tokens/second alongside completion length and end-to-end task time.

There is no credible laptop tokens/second forecast yet. Apple, NVIDIA, or other Snapdragon results are not substitutes for measurements of this exact build, model, driver, context, and power mode.

## 3. Model identity and runtime compatibility

### Select the model through literature before building its engine

The 18 GiB inference-service ceiling in section 7 leaves room for conventional low-bit models as well as Swift. The following are **candidates**, not measured winners. File sizes are publisher download figures converted to GiB; they exclude live caches, runtime copies, and compute buffers.

| Candidate / artifact | Weight file | Role in the upfront literature review |
| --- | ---: | --- |
| Current Swift Bonsai 2 `PQ2_0` | 7.206 GB / approximately 6.71 GiB | First research candidate; seek published prose/style evidence and relevant response-time/thermal reports |
| Base Ternary Bonsai 2 27B `PQ2_0` | Inspect publisher artifact metadata | Published comparator for the Swift adaptation; check runtime, prompts, sampling, and output budgets before attributing differences |
| Gemma 4 12B instruction-tuned QAT `Q4_0` | 6.98 GB / approximately 6.50 GiB | Smaller-model alternative; speed or lower power is not established by parameter count alone |
| Qwen3.8-27B `UD-Q3_K_XL` | 13.1 GB / approximately 12.20 GiB | Alternative creative-quality evidence to review, subject to its larger footprint and published runtime support |
| Gemma 4 26B-A4B instruction-tuned QAT `Q4_0` | 14.4 GB / approximately 13.41 GiB | Optional published general-assistant/MoE comparison with approximately 4B active parameters and less memory headroom |
| Qwen3.6-35B-A3B `UD-IQ3_S` | 13.7 GB / approximately 12.76 GiB | Optional published MoE comparison: 35B total, approximately 3B active per token; seek evidence relevant to creative work |

Sources: [Qwen3.8 model][qwen38-model] and [quantization files][qwen38-quants], [Qwen3.6 architecture][qwen36-model] and [quantization files][qwen36-quants], [Gemma architecture/capabilities][gemma12-model], [official 12B QAT pack][gemma12-qat], [official 26B-A4B QAT pack][gemma26-qat], and [Swift release][swift-page]. MoE active parameters can reduce per-token work; **all resident experts still count toward memory**, and active parameter count is not a laptop speed forecast. Google's [QAT release][gemma-qat-release] describes training for the quantization format; it does not establish exact laptop quality or performance.

Qwen3.8 lineage is a sensible reason to research Bonsai 2, but version numbers do not rank differently compressed or fine-tuned models. Conventional Qwen3.8, ternary Bonsai 2, and Swift have different weights/representations and can differ in voice, detail, instruction following, and long-form consistency. Swift's shorter-reasoning objective has no established advantage for fiction or art collaboration. Inspect published creative-writing results and existing samples rather than transferring math/coding benchmark rankings to this use.

Swift combines three potentially complementary savings: ternary packing reduces stored weight bytes and may reduce decode memory traffic when consumed by efficient packed kernels; Qwen3.8's inherited hybrid attention reduces context-state requirements relative to an all-attention graph; Swift training aims to reduce generated reasoning work. The attention benefit is shared with the conventional Qwen3.8 base. These savings address different costs and do not establish multiplicative speedups, lower device power, or cooler operation. It remains a dense 27B-class language model: a small weight file is not a thermal limit. CPU/Vulkan paths exist, while NPU execution and speculative decoding remain separate gates. [Architecture][bonsai-model] · [Swift evidence][swift-page] · [Backend status][backend-support]

Complete model selection as an upfront literature-search gate. Compare existing public benchmarks, methodologies, and samples for the candidates, prioritizing creative writing and art assistance, then memory fit and relevant performance evidence. Record each source URL/date, benchmark and judge version, exact model/quantization, settings, and hardware where available. Distinguish independent evaluations from publisher reports and exact-pack results from indirect base-model evidence. Where evidence is missing or noncomparable, state the limitation and confidence in the recommendation. Produce a model-choice decision record before engine implementation; local model benchmarking is not an input to this decision.

Before finalizing that choice, perform a **source-only compatibility preflight**: inspect published artifact/configuration metadata, the pinned ARM64 loader/backend and required graph operations, template controls, license terms, and a context-aware peak-memory estimate. Establish at least one credible native execution route without requiring custom kernels. This moves dependency discovery ahead of commitment; it does not involve downloading/running candidate weights or establish laptop performance. Prefer the strongest relevant creative evidence among feasible candidates, including a smaller model at a better-supported quantization when the evidence favors it. Record one literature-nominated fallback without downloading or keeping a second model resident.

The model decision is documented and revisable. If exact-artifact support, correctness, memory fit, useful creative quality, or acceptable responsiveness/comfort cannot be achieved with bounded engineering, retain the failure evidence and reopen the literature review. Record the replacement as a new model-choice decision; do not turn the engine phase into an unreported local candidate-benchmark campaign or indefinitely fund an unsuitable choice. Define the practical limits and the first source/kernel investigation's stop condition in the decision record.

After that decision, build the engine for the chosen model with one resident model, one active slot, **8K context initially, then 16K when needed**, FP16 KV, speculation off, and small bounded caches. Preserve its own template and recommended sampler. Engine validation may compare reference and optimized implementations/profiles, including paired creative samples where useful, and record regeneration/revision count, truncation, peak memory, time to useful prose, time to an **acceptable finished piece** including hidden reasoning, and sustained heat/fan behavior. Run performance and comparative quality workloads in self-hosted CI under section 10; these qualify the implementation of the already-selected model.

The chosen engine's initial creative-workflow validation set should cover:

- A short scene in several distinct voices; judge specificity, rhythm, dialogue, and unwanted generic phrasing.
- A continuation from an outline/character bible; check viewpoint, motivations, world facts, and continuity after editing an earlier turn.
- Revision under explicit constraints, such as preserving plot while changing voice, length, pacing, or emotional tone.
- Art concepts, composition/palette alternatives, and image prompts with controllable details; judge usefulness and responsiveness to feedback.
- Image critique only for a validated vision model/projector pair; distinguish observed image details from inventions.

Sample multiple outputs using a predeclared rubric; creative preference is not a single deterministic right answer. Include a thinking-disabled or lower-reasoning profile when the exact model supports it, since visible prose and collaboration may matter more than a long private reasoning trace.

[EQ-Bench Creative Writing][creative-bench] and its [longform benchmark][longform-bench] supply published scores, methodology, and samples for the upfront literature review, as well as ideas for later engine validation. Record the evaluated version and inspect existing samples. Judge/API settings and full-precision or hosted models differ from these exact quantized files; mark indirect evidence accordingly. An absent model entry is an evidence gap, not a requirement to run that benchmark before selecting a model.

Once the literature decision is recorded, proceed with CPU-first engine bring-up, containment, and cache correctness for the selected model. Keep a reference implementation/profile for optimization comparisons. Defer model-specific NPU kernels, BITCOS, and lookup-kernel adapters until the engine's measured bottlenecks justify them.

### Preserve the actual Swift release

| Artifact | Publisher size | Role |
| --- | --- | --- |
| `Swift-Bonsai-2-PQ2_0.gguf` | 7.206 GB, approximately 6.71 GiB | Primary current Swift model |
| `Swift-Bonsai-2-PTQ1_0.gguf` | 5.947 GB, approximately 5.54 GiB | Smaller alternate; earlier Swift merged weights |

Both files are complete models. PQ2_0 contains updated merged weights relative to PTQ1_0. Consequently their comparison changes **model version as well as packing**. A faster PTQ1_0 result cannot establish a lossless optimization of current PQ2_0. Keep quality and performance results separately attributed. [Publisher release details][swift-page]

### What Swift's token-efficiency evidence establishes

The [publisher's methodology][swift-page] separates historical runtime-correction results from the current merged PQ2_0 results:

| Evaluation | Base / Swift score | Mean token change | Evidence scope |
| --- | --- | ---: | --- |
| GPQA Diamond | 84.18% / 84.34% | -19.0% thinking tokens | Historical correction; not rerun on the current merged pack; GPQA informed training choices |
| C-Eval | 81.37% / 81.47% | -18.4% thinking tokens | Historical correction; not rerun on the current merged pack |
| IFBench | 82.13% / 82.60% | -1.7% thinking tokens | Current PQ2_0; loose scoring |
| AIME 2025 | 92.00% / 93.33% | +4.3% completion tokens | Current PQ2_0; final-answer-only scoring gives 90.67% for both |

The advertised 39.8% reduction is the **historical GPQA median**, not the mean or an established current-file saving. Small score differences are not established gains. UkisAI also reports uneven instruction following, tool use, and open-ended coding. These findings justify a comparison, not a production-quality assumption. [Current model limitations][swift-model]

At unchanged decode speed, 19% fewer generated tokens would cut **generation time** by 19%, equivalent to about `1 / 0.81 = 1.235` times generation-only throughput. That is conditional arithmetic; prefill, tool work, retries, and quality failures can erase the benefit. Current IFBench savings are much smaller, and AIME uses more tokens. Seek published matched Swift/base comparisons before attributing end-to-end savings to the adaptation; missing evidence remains a limitation in the upfront model choice. Later measurements establish the built engine's behavior, not a retroactive model-selection benchmark.

### Artifact manifests and semantic references

Create a separate manifest and correctness reference for every model before implementation validation with:

- Hugging Face repository, immutable model revision, exact filename, byte length, and SHA-256.
- Tensor-type histogram, architecture, recurrent/local/full-attention layout, head dimensions, sliding-window/KV-sharing settings, context metadata, and rotation/sign metadata.
- Tokenizer identity, embedded chat-template hash, special token IDs, and optional MTP/NextN tensors.
- Runtime commit, any local patch commits, compiler, build flags, binary architecture, and matching runtime libraries.
- Driver versions, device capabilities, effective configuration, and calibration-file identity where applicable.

Record where each expected digest came from and the publisher/repository trust decision. A digest computed only after downloading identifies those bytes; it does not independently authenticate their origin or make a model safe. Prefer authenticated publisher metadata and signed release provenance where available. Keep a last-known-good manifest and deliberately review security fixes before accepting a replacement pin. [Runtime security guidance][llama-security]

Use actual GGUF metadata as the implementation authority. The marketing name Qwen3.8 and GGUF architecture `qwen35` are not by themselves a mismatch: the upstream [Qwen configuration][qwen-config] uses the Qwen3.5 architecture family.

### Candidate runtime pins and lightweight bring-up

- **Publisher compatibility reference:** revision `1a07bfa5f`, cited by UkisAI. Resolve its full commit before retaining it as a control.
- **Newer accelerator candidate:** release `prism-b10787-73bfe6c`, commit `73bfe6cc485554822f80c473a3c0c4f25e80f41b`, pinned by the current Bonsai demo. Its backend audit records Vulkan PQ2_0 support shipping from `prism-b10735-842b188`. [Release][runtime-release] · [Backend audit][backend-support]

Build the newer candidate natively and compare focused correctness against the reference. A documented compatibility reference is not an ARM64 performance certification. Capture release assets and source status at implementation time; the observed release has an ARM64 Windows CPU package, while the Windows Vulkan package is x64, so native ARM64 Vulkan needs its own build qualification.

A pinned, hash-verified publisher ARM64 CPU binary can provide the first bounded loading/chat check before custom compilation. Verify its PE/dependency architecture and matching libraries. Inspect the pinned [Bonsai demo setup][demo-setup] before adopting it: its default model download is unnecessary when qualifying an explicitly selected Swift artifact. Download only the intended pack to the external model directory and pass its exact absolute path with `-m`; do not copy an unpinned setup command or assume a release directory layout. Rebuild the accepted source pin for the reproducible distribution.

Pin upstream llama.cpp separately for conventional candidates. Its [Qwen hybrid-MoE graph][upstream-qwenmoe] and [Gemma graph][upstream-gemma] provide implementation evidence; inspect the chosen revision's exact 12B/unified variant, loader, tool template, and backend operations before accepting it. Hugging Face's autogenerated application snippets alone are not runtime compatibility evidence.

Never mix fork binaries with unrelated stock ggml DLLs. Do not rename a tensor type, edit GGUF type IDs to bypass a loader, or substitute base Bonsai/Qwen weights inside a Swift correctness comparison. Model selection may deliberately choose other weights under their own manifest. A future lossless repacker must preserve decoded values, group scales, transforms, tokenizer, and template. Requantization is a new quality tradeoff.

## 4. Backend feasibility and acceleration order

| Route | Evidence available now | Implementation gate | Priority |
| --- | --- | --- | --- |
| ARM64 CPU | Upstream conventional-model graphs; Prism Swift packings and Bonsai transforms | Native build, actual SIMD dispatch, focused model correctness | Required baseline |
| Vulkan GPU | Upstream conventional kernels; Prism native PQ2_0/PTQ1_0 and FWHT paths are released | ARM64 build, exact quantization/graph, Adreno capabilities, correctness, useful offload | First acceleration candidate |
| OpenCL GPU | Exact X1-85 and Windows ARM64 are documented; ordinary Q4_0/K-quants listed | Exact graph/quantization audit; Swift packing and transforms remain unestablished | Conventional-model candidate / Swift development lane |
| Hexagon ggml backend | Windows Snapdragon route exists, experimentally | Custom packed matmul, full graph, state placement, SDK and driver access | Research lane |
| Qualcomm GenieX | Windows ARM64; GGUF llama_cpp plugin and separate QAIRT plugin | Supported exact model/quantization, signed package, placement, state/memory, and API behavior | Conventional-model NPU candidate; no demonstrated Swift pack path |
| ONNX Runtime + QNN | Alternative NPU stack to investigate | Faithful export, quantization, supported graph, cache I/O, resource limits | Separate conversion project |
| DirectML / generic ONNX GPU | Potential alternative after conversion | Exact model conversion and custom ternary execution are unestablished | Lower priority |

The [Vulkan audit][backend-support] describes scalar/coopmat1 and integer-dot paths; device and driver choose usable kernels. It does not promise direct coopmat2 decoding or a particular Adreno speed. The [OpenCL guide][opencl-guide] explicitly lists X1-85, but its datatype list does not establish Swift PQ2_0/PTQ1_0 support. These are different compatibility questions.

For conventional models, the [upstream OpenCL guide][upstream-opencl] lists Q4_0, Q4_K, Q5_K, and Q6_K among its supported types. This makes Gemma's QAT Q4_0 a useful platform-fit candidate. IQ3_S and Q3_K are absent from that advertised list: do not assume the chosen Qwen three-bit packs can use this route. CPU/Vulkan compatibility and actual operation placement need their own checks; format support alone is not full-graph acceleration.

### CPU implementation

Use a Release build with the selected revision's Windows ARM64 LLVM toolchain. Verify the PE architecture, dependency architectures, CPU features, and effective kernel dispatch. Inspect NEON/dot-product/i8mm support for the **actual** tensor shapes and packing in that revision; do not infer selected kernels from source-file names or assume extensions present on another ARM processor.

Check OS-enabled instruction support as well as CPU capability. A July 2026 [community X Elite CPU guide][snapdragon-cpu-history] reports an illegal-instruction failure from native-target SVE selection and a working explicit NEON/dotprod/i8mm build. It concerns an older 8B pack, a different OS/toolchain snapshot, and includes stale format claims; use it to motivate a native loading regression and reviewed ISA flags, not a current speed forecast or a universal flag recipe. Review its PolyForm Noncommercial terms before copying material.

Begin with a single resident model. Tune generation threads independently from prefill threads; screen **4, 6, and 8** first, restricted to discovered cores. Try 12 only if results justify more parallel work. More threads can lose on a shared-memory-bound decode path or increase sustained heat without a useful response-time improvement. Avoid realtime process priority and leave capacity for the UI and operating system. Compare the accepted CPU profiles with Vulkan on finished-response latency and sustained behavior; GPU detection or using more processors is not evidence of a better daily profile.

Include synchronization waiting in that small screen: compare `--poll 0` and disabled batch polling with the pinned binary's defaults. Current fork documentation lists generation polling at 50; obtain exact accepted values from the chosen build. Record generation/prefill thread and polling settings separately. This is an energy/latency experiment, not a claim that a warm idle server continuously spins or that disabling polling always improves speed. Give the battery profile an explicit low-polling candidate. [Runtime options][server-docs]

Only write custom CPU kernels after profiling identifies the cost. Candidate work includes packed dot products, decode into registers rather than persistent FP16 expansion, fused transforms, cache-friendly tiling, and projection fusion. Every optimized kernel needs a portable reference and adversarial shape/value checks.

**Optional profiling aid: Qualcomm WoS Porter.** Its [repository][wos-porter] documents Copilot, Claude Code, and Codex integrations, including `wos-etl-hotspot` for ETL CPU-hotspot/call-graph analysis and suggested ARM64 optimizations. Capture representative performance traces through approved self-hosted CI, then inspect retained traces locally. Restrict suggested instructions to the actual Oryon feature set; review patches and apply the normal correctness/performance gates. Its default local benchmark workflow must not override repository policy. No release-date or speedup claim is needed to use it as an engineering lead.

### Adreno Vulkan implementation

1. Enumerate actual Vulkan devices, API/features/extensions, subgroup behavior, allocation limits, and memory budgets using native tools.
2. Compile host code and runtime libraries for ARM64, with a compatible shader toolchain. A working x64 executable under emulation does not qualify the native build.
3. Check packed matmul, embeddings/output, FWHT/sign handling, normalization, RoPE, full attention, Gated Delta Net, and convolution/state updates.
4. Log effective per-layer and per-operation placement. “GPU detected” and a high offload-layer count do not prove all expensive operations run there.
5. Qualify CPU-only, a few contiguous partial-offload placements, and maximum feasible offload. Keep attention/cache state close to its consumers; count synchronization and migration costs.
6. Qualify flash attention and quantized KV for the selected shape/type. Unsupported paths use an explicit supported profile rather than silently changing settings.

Prefer an intact, supported execution graph over frequent CPU/GPU crossings. Add backend-specific fusion or shader improvements only after an end-to-end profile identifies a bottleneck. Keep a CPU fallback that reports why the requested GPU profile could not start.

A [community ARM64/Adreno report][adreno-history] is useful for build and allocation troubleshooting. It reports about 14.4 CPU versus 12.6 GPU tokens/second for an earlier 8B type-42 Q2_0 model on an X1E-80-100 machine with 64 GB RAM, using a GPU path that dequantized to float. This is historical single-machine evidence, not a Swift 27B forecast or a verdict on current Prism kernels. The guide contains placeholder model/fork identifiers, so inspect the actual pinned sources rather than inheriting its patches or memory flags. Review its PolyForm Noncommercial terms before copying implementation material.

### Hexagon NPU feasibility gate

Qualify a conventional-model route before undertaking custom Swift kernels. Qualcomm's [GenieX SDK][geniex-sdk] documents a `llama_cpp` plugin consuming compatible GGUF files across CPU/Adreno/Hexagon and a **separate** `qairt` plugin consuming QAIRT `.bin` packages. Its [model guide][geniex-models] recommends Q4_0 for the GGUF Hexagon path; other types may fall back. Qwen3.5-9B Q4_0 is a smaller candidate to investigate, not an established whole-model result on this laptop. This route does not establish support for Swift PQ2_0/PTQ1_0 or the three-bit Qwen candidates. Pin runtime/model licenses, exact package, actual placement, memory, and API behavior; do not add another model server until it offers a concrete benefit.

The [GenieX v0.8.0 release][geniex-release] documents a Microsoft-signed HTP catalog for its Windows route. Prefer a compatible signed package. Ordinary GGUF NPU execution and QAIRT conversion are distinct paths; neither should be inferred from the other or from a model-name match.

The Windows [Snapdragon backend guide][snapdragon-guide] supplies a starting point, not proof that this custom model can run on the NPU. Pin the Hexagon SDK, runtime libraries, and drivers required by the selected fork revision; SDK requirements change between guides/releases. The [Hexagon matmul implementation][hexagon-implementation] does not establish native support for Swift's packed types.

Use supported signed HTP libraries where available. The current [Windows Hexagon development guide][hexagon-windows] discusses signing/test-signing requirements; if a development path requires changing boot security or machine signing policy, treat that as a separate explicit owner decision. It is not part of ordinary engine setup or GPU qualification.

Time-box an initial investigation to approximately two engineering days:

- Establish native Windows device access and applicable SDK redistribution terms.
- Audit packed tensor formats and every operation in the actual Swift graph. Existing generic Q4/Q8 support does not imply PQ2_0/PTQ1_0 support.
- Determine whether unsupported operations fall back, whether those boundaries dominate runtime, and where recurrent/KV state resides.
- Inspect allocation/session limits and any repacked copies. The [backend design notes][hexagon-developer] describe per-session mapping constraints, so account for weights, state, and scratch per session rather than treating all shared RAM as one NPU pool.
- Estimate the missing implementation work and its likely benefit before committing to a port.

**Proceed with direct NPU execution only if** faithful packed execution or a clearly bounded alternative is feasible, state movement fits the budget, and self-hosted CI shows an end-to-end benefit. Otherwise retain CPU/GPU as the main engine and document the NPU gap.

An ONNX/QNN route requires an explicit exporter and graph/quantization validation; it is not a provider toggle for a GGUF file. The [ONNX Runtime GenAI QNN guide][qnn-guide] describes a separately prepared model package. Expanding 27B weights toward INT8 is roughly 27 GB before scales, caches, activations, and duplication; FP16 is roughly 54 GB. Those are order-of-magnitude calculations, not viable 32 GB deployment assumptions. Retaining efficient low-bit representations is central to feasibility.

A separate small embedding model could eventually use the NPU for retrieval. That would accelerate an added application feature, not Swift decoding. A small NPU draft model is also conditional on tokenizer alignment, supported speculative verification, and correct hybrid-state rollback.

### Snapdragon literature and GitHub check — October 22, 2026

The quick follow-up search found a concrete ARM kernel lead and partial X Elite NPU work, **not a verified new Swift 27B/X Elite end-to-end speedup**. The most actionable changes are:

| Primary finding | Evidence boundary | Action in this plan |
| --- | --- | --- |
| [ik_llama Prism R8 kernels][ik-prism-r8], followed by [PQ2 ARM NEON][ik-pq2-neon] and [PTQ ARM NEON][ik-ptq-neon], merged September 29, 2026 | Eight-row interleaving and ARM implementations exist; initial performance reports use Intel hardware, and the PTQ follow-up explicitly attributes its gains to AVX paths | Audit a narrow CPU backport before designing a new lookup adapter; do not replace the whole runtime or import the reported speedups |
| [htp-llm][htp-llm] and its [ternary HTP branch][htp-ternary], checked at moving main/branch | Partial ternary codec/repacking/HVX work targets X Elite; DSP Hadamard is unfinished, golden correctness has not passed, and no speed result is established | Source-audit lead inside M5, with immutable pins and codec/transform parity required; keep CPU/Vulkan delivery independent |
| [OpenCL program-binary cache][upstream-opencl], checked at moving master | Documented persistent compiled-kernel reuse; not evidence of ternary graph support or faster decode | Qualify a private cache for supported OpenCL profiles and record cold/warm startup separately |
| [Qualcomm binary OpenCL kernels][adreno-x2-kernels], merged July 1, 2026 | The documented library targets X2-90/X2-85/X2-45 and selected non-ternary MoE formats | Exclude its advertised gains from this X1-85 plan unless explicit X1 support later appears |

For the MIT-licensed R8 experiment, retain notices and start the diff audit at merge `b25a013d0293cfddfa31b39f2dfbb02721cc3422`, PQ2 follow-up `e44285e561f5da288a1cbee0ab37033194218c6d`, and NEON implementation `1f6e2b92e0fd6c3dbf08d9c1d022af3b90927d08`. Check whether equivalent kernels are already in the selected Prism pin before adding work. Q8_K128 activation packing and architecture-side Hadamard handling require numerical/graph validation; repacking weights alone does not prove equivalent model behavior. Verify exact decoded weights, activation quantization, scales, signs/rotations, awkward shapes, creative regressions, and peak repack residency before CI timing. [License][ik-license]

The HTP prototype is a useful gap map, not a deployable accelerator. Inspect its deliberate rejection of unfinished Hadamard-hinted matmul, scale/layout compatibility with the pinned Prism codecs, large-shape HMX failures, and signing requirements before reuse. Do not adopt its test-signing/custom-certificate procedure as ordinary setup, infer that every GenieX route requires static QAIRT conversion, or treat a successful basic NPU session as Bonsai correctness. Revisit these sources only when entering their engineering lane; the quick survey is not an exhaustive literature review or a model-selection verdict.

## 5. Engine, agent harness, and execution isolation

### Keep the native engine upstream-based

Use the selected runtime's **llama-server plus its built-in web UI** for direct creative chat and engine inspection; Swift/Bonsai use the matching Prism distribution. It provides a browser interface, streaming API, monitoring, and structured-output support without requiring a second model loader. It is not by itself a complete coding-agent harness or a security boundary. The Prism runtime is MIT-licensed; Swift weights carry Apache-2.0 terms and retained notices. Inventory every alternate model/runtime's terms under its own manifest. [Server documentation][server-docs] · [Runtime license][runtime-license] · [Swift model][swift-model]

```text
Authenticated browser UI --> Inference-only ARM64 llama-server
                                      |
                             ggml CPU / Vulkan / NPU (gated)
                                      ^
                          bounded inference-only bridge
                                      |
                          Trusted small Rust policy broker
                                      |
                             MXC ProcessContainer
                                      |
                  Agent + tools + plugins + language servers
                  Restricted workspace and scratch; no host secrets

PowerShell: installation and reproducible builds
CI: same binaries/policies/profiles -> results + provenance
```

The repository should own reproducibility, profiles, lifecycle, focused tests, and any justified small upstream patches. Avoid a replacement scheduler, attention engine, or serving stack until evidence shows a concrete gap.

The manager should:

- Verify manifest hashes and architecture; resolve absolute paths and reject missing or incompatible dependencies.
- Select a named, validated profile. Show effective backend, context, KV types, checkpoint/cache budget, and any fallback reason.
- Start one model process, check readiness, expose its local UI, and stop only the process it owns.
- Queue additional requests by default, support streaming cancellation, and recover from process failure without repeated restart loops.
- Reject requests that exceed token/memory limits with an actionable message.
- Keep private prompts out of routine logs; retain technical configuration and error details separately.

Bind inference to `127.0.0.1` initially. Preserve an offline path once artifacts exist, without automatic cloud fallback or surprise model downloads. The sandbox accesses inference only through a qualified, narrowly scoped bridge; its loopback behavior must be established rather than assumed. Keep model-generated commands inside the execution boundary, with bounded tool output and visible file diffs.

### Direct-chat security baseline

The daily profile must be authenticated and inference-only. Bind only to numeric loopback; require an unpredictable owner-private bearer key for inference, private state, and administrative requests. Supply it through the pinned runtime's key-file support rather than URLs, command history, or logs. Qualify built-in UI authentication, streaming, cancellation, and offline restart. Loopback scopes network reachability; it does not authenticate a browser page or another local process.

Set the UI's exact allowed scheme/host/port origin and validate Host and Origin on applicable requests. Reject wildcard or `null` browser origins and unexpected Host values; authentication still applies to clients without Origin. Check unauthenticated requests, hostile origins, preflights, and alternate loopback/IPv6 exposure. CORS alone is not request authorization. Use native enforcement where supported; otherwise a small local guard may validate requests and stream bytes without retokenizing, copying weights, or entering the per-token compute path. If the guard supplies required controls, keep its backend credential private and prevent untrusted callers from bypassing it through raw backend routes; qualify the complete path. Do not claim controls that the pinned implementation cannot enforce.

Current server documentation exposes optional host tools/MCP and separate authentication/CORS settings. Therefore the launcher must enforce a fixed inference profile: `--no-agent`, UI MCP proxy off, no built-in host tools, and no server-hosted MCP definitions. Reject equivalent inherited `LLAMA_ARG_*`, UI, or extra-argument overrides while preserving Windows system/profile variables. Model function-call parsing for a contained client remains distinct from enabling host execution inside llama-server. Disable router/dynamic loading, arbitrary static-file roots, RPC, and unnecessary slot/config/metrics administration; allow only routes required by the qualified UI, streaming API, and minimal readiness check. Obtain actual flags from the pinned binary; do not invent unsupported negative switches. [Native server options][server-docs]

Enforce bounded request bytes, prompt/completion tokens, image dimensions/decoded bytes, duration, queue length, and concurrency before expensive work. Cancel abandoned generation. Vision initially accepts explicit local uploads; deny server-side remote-media fetching by default. A later fetcher needs a separate egress policy covering private/loopback/link-local destinations, DNS changes, redirects, and download/decode limits. Render model text as untrusted content: qualify Markdown/HTML sanitization and prohibit scripts or automatic external image/resource loads. These are project acceptance requirements, not assumptions about native defaults.

### Inference process, private data, and updates

Model/media parsers and accelerator libraries remain outside the tool sandbox. Require a qualified restricted process token/identity or containment policy for the daily inference service: non-administrator execution, read-only approved runtime/model access, bounded private writable cache/temp directories, no unrelated credentials/projects, and network egress denied after provisioning. Establish permissions during setup so daily offline use needs no elevation. Verify accelerator access, mmap, restart, and actual denied file/network operations. Standard-user execution and same-user ACLs alone do not establish parser isolation. A trusted-input development profile may establish initial CPU correctness with its reduced assurance explicit; it must not be promoted as the isolated daily profile. Keep an unsupported device policy experimental rather than silently widening access. [Runtime security guidance][llama-security]

Keep conversations, artwork, KV/state snapshots, keys, and diagnostics outside Git with access limited to the owner, the intended service identity, and required OS principals. Disable prompt/body logging, bound retention, provide explicit history/state deletion, and exclude personal content from CI artifacts. Separate online artifact provisioning from offline inference: use approved publishers and immutable revisions, verify expected hashes/provenance, and never execute model-supplied installation commands or remote custom code automatically. Review security fixes for the runtime, parsers, broker, agents, and drivers; qualify replacement pins and retain a rollback manifest. No security change here authorizes changes to boot security, signing policy, or global profile directories.

### Agent harness selection

If tool automation is wanted after creative chat works, evaluate an established agent client before building a new agent loop. Keep the inference engine independent so changing clients does not reload or duplicate the model. Harness/containment qualification is required for enabling tools, not a prerequisite for delivering direct creative chat.

| Candidate | Evidence and fit | Qualification / decision |
| --- | --- | --- |
| **OpenCode** | MIT; current TypeScript/Bun implementation; documented llama.cpp endpoint; native Windows ARM64 downloads | First full agent candidate. ARM64 arrived in v2.0.28 on October 13, 2026, so its native-platform history is recent |
| **Aider** | Apache-2.0 coding client, managed Python controller, local compatible API and reviewable edit workflow | Retain as coding fallback; pin and verify ARM64 Python/native dependencies |
| **Goose** | Apache-2.0 agent with Rust core | Reconsider after supported native Windows ARM64 packaging; reviewed Windows workflow builds x64 |

Sources: [OpenCode provider integration][opencode-provider], [license][opencode-license], [release history][opencode-changelog], [Aider local endpoint][aider-api], [Aider license][aider-license], and [Goose Windows build][goose-windows]. Do not select an older project merely because it also used the name OpenCode, or describe the current implementation as Go.

OpenCode's [security policy][opencode-security] explicitly says its permission prompts are not a sandbox. Run the entire agent and its executable dependencies inside the qualified boundary. A tool-call plugin or shell wrapper cannot contain in-process plugins, project configuration hooks, language servers, package scripts, or an alternate execution path. [Whole-agent versus tool-only isolation][pi-security]

For every candidate, verify streamed reasoning/answers, cancellation, sampler forwarding, exact templates, tool-call parsing, malformed/unsupported schemas, tool-result round trips, patch application, and context/output limits with the selected model. If native function calling is unreliable, use a validated simpler edit workflow or direct chat; do not compensate by granting broader tool privileges.

Judge effectiveness by **successful task latency**, retries, prompt growth, tool output volume, and total memory. A faster client language alone does not accelerate model decoding. Pin a supported release and dependencies, review security advisories and update behavior, and retain a working fallback.

“Battle-tested, secure, and memory-safe” remains a set of acceptance requirements, not a badge inferred from stars, a vendor name, Rust, or TypeScript. The custom model fork, native libraries, Bun/Python extensions, GPU/NPU drivers, OS kernel, and broker FFI remain separate trust boundaries. A memory-safe controller does not make the full inference stack memory-safe.

Swift's publisher reports uneven instruction following, tool use, and open-ended coding. Keep client task quality separate from API connectivity and sandbox enforcement. Start with ask/review and small edits.

### Sandbox choices: MXC, LiteBox, and NVX

These projects occupy different layers. Choose one appropriate boundary and add layers only for a demonstrated requirement.

| Project | Role and current target evidence | Decision for this laptop |
| --- | --- | --- |
| **MXC** | Policy/lifecycle SDK with several OS and VM backends; README explicitly lists Windows 11 ARM64 ProcessContainer | First native containment candidate; probe the exact request and selected tier |
| **LiteBox** | Library OS; current Windows userland runner is x86-64 and shares address space with its host | Exclude that path as the required native hostile-code boundary; a library OS would need an appropriate outer boundary |
| **NVX** | OpenVMM-based Linux microVM; architecture explicitly excludes non-x86 guests | No established native Snapdragon path; do not make it a local dependency |

Sources: [MXC backend matrix][mxc-readme], [LiteBox Windows runner][litebox-runner], [LiteBox Windows memory boundary][litebox-platform], and [NVX non-goals][nvx-non-goals]. LiteBox also states it is still working toward a stable release. MXC marks several VM backends experimental; its `microvm` backend naming does not establish integration with the separate NVX repository. [LiteBox status][litebox-readme]

**Proposed implementation:** use MXC's Rust SDK in a small broker that owns sandbox creation and capability grants. Put the agent and every executable child in the boundary. Keep native accelerator inference outside the tool worker under the qualified inference-service policy above. Do not pass GPU/NPU device access to tool workers or expose the broker's administrative capability through the inference bridge.

Host qualification must record the MXC revision, exact OS build, requested policy, chosen backend/tier, native assets, and unsupported capabilities. Use the current V1 request-aware capability APIs and actual containment checks. `available_backends()` alone is not proof of enforcement. [MXC Rust API][mxc-api]

### Containment contract and bypass checks

The default policy should provide one disposable project workspace, separate scratch, and required read-only tools. Deny unrelated user files, credentials, model packs, conversation/state caches, broker control handles, clipboard/GUI access, and networking unless individually required. Keep the OS/profile environment valid while removing explicitly scoped application secrets; never clear or redirect inherited Windows system/profile variables wholesale.

The broker must fail closed if the requested capability cannot be enforced. No automatic unsandboxed shell fallback, broad host ACL mutation, or permissive network mode to make a tool start. Require `allowDaclMutation:false` in the selected MXC policy. Never enable MXC's audit mode for untrusted model-generated execution; audit mode disables its security. [MXC schema][mxc-schema] · [Audit-mode contract][mxc-readme]

Qualify each of these before calling a profile contained:

- Agent filesystem APIs, shell commands, subprocesses, grandchildren, external MCP servers, plugins, and package hooks all remain inside the boundary.
- Traversal, reparse-point/junction escapes, inherited handles, pre-opened files, and unintended writable installation paths do not bypass the workspace policy.
- A restricted child cannot read host credentials, another project, inference caches, or the broker's administrative API.
- Direct sockets, DNS, localhost/private-network access, subprocess networking, and proxy bypass follow the actual requested rules. Proxy environment variables alone are not network enforcement.
- Cancellation/timeouts terminate the owned process tree and release handles; enforce supported memory/process/CPU/output limits and report missing controls.
- The inference bridge authenticates the intended peer, accepts only bounded inference requests, applies token/queue limits, and exposes no arbitrary host file or command execution.

MXC's [networking contract][mxc-network] differs by backend/tier. Private-network capability has bidirectional implications; do not represent it as a narrow outbound endpoint allowlist. Select a supported IPC or constrained network/proxy bridge and test it. If no acceptable bridge exists, use direct chat or keep the client explicitly trusted until the boundary is implemented; do not label a weakened path secure.

ProcessContainer shares the host kernel. Keep a separately qualified VM tier for workloads whose threat model requires a separate kernel. [Windows Sandbox][windows-sandbox] supports ARM64 on eligible Windows editions, but MXC's integration is experimental, features need qualification, and its networking/mapped directories must be configured explicitly. Never assume NVX supplies that ARM64 tier. Optional WSL/Linux-tool support is another lane with its own networking and terminal constraints, not an automatic replacement for native tools.

### Low-overhead sandbox lifecycle

Prefer direct native process containment for ordinary tasks after enforcement passes. Reuse a warm worker only within the same project, policy, identity, and trust domain. Reset or destroy it between domains; do not pool workers with residual secrets, processes, or writable artifacts across projects.

Keep the model resident outside short-lived tool environments. Stream bounded output, reuse immutable tool installations, avoid launching a VM or copying a model per tool call, and use small stable tool schemas. Measure cold/warm startup, broker round-trip, resident memory, cleanup, and successful-task latency separately in self-hosted CI. No millisecond launch claim has been established on this laptop.

### Model behavior contract

Preserve each artifact's embedded tokenizer/template and publisher settings in its reference profile. **Swift's** contract is temperature 1.0, top-p 0.95, top-k 20, min-p 0, repetition penalty 1, presence penalty 0, and no added logit penalty. Preserve its documented thinking/template controls. Base Bonsai, Qwen, and Gemma have their own settings; do not apply Swift's sampler universally. [Swift generation contract][swift-page] · [Base Bonsai settings][bonsai-model]

For an initial interactive profile, propose a 4,096-token completion cap and make truncation visible; long reasoning tasks need a separately budgeted profile. Count reasoning tokens toward the total limit. Require:

```text
rendered prompt tokens + maximum generated tokens + template margin
    <= validated context capacity for this profile
```

The client must preserve roles/tool messages, stream reasoning and answer content correctly, and report finish reasons. Use deterministic settings only for diagnostics; do not silently replace the publisher's sampling defaults in quality evaluations.

Add **explicit creative profiles** after the reference: thinking disabled, or a supported moderate reasoning effort, for drafting, brainstorming, and revision; retain a deeper-thinking profile for difficult planning or analysis. Qwen3.8 documents per-request thinking control. Base Bonsai's card warns that `low` is unsupported and can behave near `xhigh`, while recommending `medium` for shorter reasoning. Verify the actual template controls instead of inventing a universal effort flag. Count hidden thinking in latency, output, and context budgets. Treat shorter caps, thinking changes, and sampling adjustments as behavior/quality tradeoffs; compare creative preferences and finish reasons before promotion. Do not silently truncate a story to manufacture a speedup.

For image critique, pin the matching vision projector/preprocessor and include image tokens, extra weights, and peak preprocessing allocations in admission checks. A text GGUF alone is insufficient evidence of working vision. For future image generation, select a separate qualified image model and schedule it within the same shared-memory budget, unloading/reducing LLM residency when necessary; concurrent full-size pipelines are not an initial requirement.

## 6. Hybrid attention and cache design

The detailed 64-layer calculations below apply to **Swift/Bonsai's Qwen3.8-derived graph**. Derive fresh estimates from each candidate GGUF: Qwen3.6 MoE has different recurrent/full-attention dimensions; Gemma uses local sliding-window plus global attention rather than Gated Delta Net snapshots. Do not copy Swift's checkpoint settings or cache arithmetic into a Gemma profile. Validate both continuity after revisions and actual allocation, including any runtime option that retains a full-size sliding-window cache.

### Account for three kinds of state

The base has 64 language blocks with approximately three linear-attention blocks per full-attention block. Bonsai also applies Hadamard rotations and matching signs; decoding weights without the matching runtime graph is insufficient. [Base architecture][bonsai-model]

The fork's [Qwen35 implementation][qwen-implementation] uses Gated Delta Net in recurrent layers and separate full-attention memory. Plan for:

1. **Full-attention KV:** grows with retained token count and independent sequences.
2. **Recurrent/convolution state:** generally fixed per live sequence for fixed model dimensions; it still occupies memory and must be restored correctly.
3. **Prompt reuse/checkpoints:** retained snapshots and cached slot state, additional to live inference state.

Compressing KV does not automatically compress recurrent state, weights, checkpoints, or scratch buffers. Keep recurrent state at the runtime's validated precision initially.

### Conditional KV memory calculation

The upstream [configuration][qwen-config] specifies 16 full-attention layers, four KV heads, and 256 elements per head. **The following is an estimate conditional on the selected Swift GGUF retaining those dimensions**, with one sequence, no speculative/MTP allocation, and no padding or extra buffers.

```text
KV bytes = token capacity × full-attention layers × KV heads
           × (K head dimension × K bytes/element
              + V head dimension × V bytes/element)

FP16: 16 × 4 × (256 × 2 + 256 × 2) = 65,536 bytes/token
```

Q8_0 stores 32 values with a two-byte scale per block; Q4_0 uses 16 packed bytes plus a two-byte scale. Thus the estimated rates are 64 KiB/token for FP16, 34 KiB/token for Q8_0, and 18 KiB/token for Q4_0 when both K and V use that type. Validate allocation and backend support before using these ratios. [Quantization block definitions][quant-blocks]

| Context capacity | FP16 K + V | Q8_0 K + V | Q4_0 K + V |
| --- | ---: | ---: | ---: |
| 8,192 | 0.50 GiB | 0.27 GiB | 0.14 GiB |
| 16,384 | 1.00 GiB | 0.53 GiB | 0.28 GiB |
| 32,768 | 2.00 GiB | 1.06 GiB | 0.56 GiB |
| 65,536 | 4.00 GiB | 2.13 GiB | 1.13 GiB |
| 131,072 | 8.00 GiB | 4.25 GiB | 2.25 GiB |
| 262,144 | 16.00 GiB | 8.50 GiB | 4.50 GiB |

These are **KV-only** estimates, not total memory promises. Maximum model context metadata does not establish practical quality, acceptable attention cost, or capacity on this laptop. Implement the estimator from GGUF dimensions and ggml block sizes; reconcile it with runtime allocation logs.

### Recurrent state and checkpoints

For the upstream dimensions, the Gated Delta Net matrix state alone is approximately:

```text
48 recurrent layers × 48 value heads × 128 × 128 × 4 bytes
    = 144 MiB per live sequence
```

Convolution state and allocation overhead add to this. Around 150 MiB per saved recurrent state is a planning estimate; 32 full state snapshots could therefore add roughly 4.7 GiB before other cache storage. Actual checkpoint representation, rollback planes, count, and placement must be read from the selected runtime and measured allocations, not assumed identical to this estimate. [Recurrent allocation implementation][recurrent-implementation]

Start the Swift creative profile with **two context checkpoints and a 256 MiB server prompt-cache budget**, both proposed. Eight checkpoints and 1,024 MiB are a later reuse candidate. Cap their combined observed footprint explicitly. `--cache-ram` does not cap the live KV arena or all checkpoint/rollback allocations. Compare smaller/larger checkpoint counts and spacing only after correctness, then in CI for latency/memory/energy tradeoffs. Include a checkpoints-off profile for unrelated short requests: disabling request prefix reuse does not necessarily disable checkpoint work.

### Prefix reuse policy

Use the selected runtime's supported reuse mechanisms before inventing a second cache. Prism distinguishes request prefix reuse, recurrent context checkpoints, server prompt-cache storage, and chunk shifting via `--cache-reuse`. Chunk shifting can be unavailable while ordinary prefix reuse still works. [Prompt-cache documentation][prompt-cache]

Requirements:

- Keep system text and shared instructions stable; append variable material after the reusable prefix.
- Preserve token-level identity and model/template identity. Dynamic timestamps, tool schemas, whitespace changes, or hidden client instructions can invalidate a prefix.
- Restore **both** ordinary KV and the matching recurrent state at the reused position. If no valid checkpoint exists, replay from the last valid point or prefill afresh.
- Bound prompt-cache storage and retained snapshots independently; use eviction rather than unbounded growth.
- Key any saved state by model hash, tokenizer/template, runtime/state-format version, KV types, rotation/calibration settings, and token sequence. Treat state files as private conversation data.
- Exercise edited prompts, regeneration, conversation switching, cancellation, and eviction; an append-only conversation is insufficient validation.

A cache entry is useful only when it saves the complete work needed for a correct continuation. Log cached/prefilled token counts and restoration decisions; do not infer a hit from the client resending the same text.

### Quantized KV policy

1. Establish FP16 K/V as the correctness reference.
2. Qualify Q8_0 K/V for daily and longer-context profiles, with supported flash attention and actual device placement.
3. Consider asymmetric K/V types only where the selected backend supports them and quality/memory results justify them.
4. Keep Q4_0 as an experimental long-context memory option until retrieval, reasoning, and numerical stability pass.

Prism describes Q4 KV as a memory tool and reports slightly slower decode in its documented setup. Its optional mean-centering bias is **model-specific**, with matching K-rotation requirements. Generate/validate calibration for the exact Swift artifact and a separate representative corpus; do not reuse a base Bonsai bias blindly or tune on held-out evaluation tasks. [KV-cache notes][kv-cache]

Choose quantized KV for measured total-request benefit or necessary context capacity, not merely its smaller byte count. Do not silently quantize recurrent state or promise that Flash Attention improves every workload.

### Context exhaustion

Begin with one active slot and no automatic context shifting. If the request will exceed capacity, offer a larger validated profile or an explicit conversation summary/truncation action. Rebuild state from the resulting prompt. Recurrent memory must not be edited as though arbitrary KV rows can simply be deleted.

### Writing context and author-owned project memory

Begin with explicit **scene packets**: the relevant outline, author-approved character/world facts, desired voice/viewpoint, the current scene, and a few relevant source excerpts. Keep stable instructions first and variable excerpts after the reusable prefix. Count the rendered prompt and reserve output space: an 8K context with a 4K completion allowance leaves less than 4K for instructions, history, sources, and template overhead. Offer visible draft/revise/outline output presets; shorten a completion budget only as an explicit behavior choice. Do not resend a whole manuscript or grow context by default when a focused packet answers the brief.

Original manuscripts and an author-edited project bible remain authoritative. Generated summaries are versioned suggestions with chapter/excerpt references; surface contradictions and require acceptance before changing canon. Preserve original text and reviewable edits independently of volatile inference state. Start with manual excerpt selection or simple local lexical search; add embeddings only when relevance/continuity failures justify another model and index. Evaluate omitted facts, conflicting canon, voice drift, and source relevance. Imported text is task data and cannot grant tools or change security policy. Rebuild model state after an accepted context change.

## 7. Memory and storage budgets

With agent/tool containment added, use a provisional **18 GiB inference-service ceiling**, about **2 GiB for an ordinary tool worker**, **0.5 GiB for the controller/client**, and roughly **8 GiB for Windows and other applications**: 28.5 GiB combined. The reserve is about **3.1 GiB only if the reported usable figure means 31.6 GiB** (as Windows commonly labels it). Literal decimal 31.6 GB is about 29.43 GiB, leaving only about 0.93 GiB. M0 must record actual usable bytes and reduce admissions if needed; do not rely on an unverified 3 GiB reserve. These are admission-policy budgets, not discovered or guaranteed OS enforcement limits. A compiler-heavy task or VM may require shrinking/unloading inference rather than admitting both at their normal maxima.

For the primary creative-chat delivery, the optional tool worker need not be resident. Keep the same conservative inference ceiling until actual Windows/application use is known. Budget any vision projector and image preprocessing explicitly; future image generation shares this physical RAM and may require unloading the language model.

The ceiling is a maximum, not a residency target. Before launch and large requests, reconcile available RAM, system commit, device budgets, and other active applications against the requested peak. Keep a visible safety reserve and reject or offer a smaller validated context/cache profile when current headroom is insufficient; an idle image editor can become memory-intensive during an operation. Do not force a second model or a larger context to consume unused RAM. Keep a model warm during active work, with an explicit idle-unload/manual-release option for battery use or an art pipeline; unloading trades later startup time for released residency and is not assumed to save power by itself.

The conventional-model candidates fit **on paper**, with one active sequence, no drafter/MTP allocation, limited reuse, and no persistent expanded/duplicate weight copy:

| Candidate profile | Initial memory reasoning | Proposed service allowance, not a measurement |
| --- | --- | ---: |
| Qwen3.8-27B UD-Q3_K_XL, 16K FP16 KV | Approximately 12.20 GiB weights + 1 GiB live KV + recurrent state/few checkpoints + small prompt cache + compute/staging | Approximately 16–18 GiB |
| Gemma 4 12B QAT Q4_0, 16K | Approximately 6.50 GiB weights, local/global KV, small reuse budget, and compute/staging; no Qwen-style recurrent checkpoints | Approximately 10–13 GiB |
| Gemma 4 26B-A4B QAT Q4_0, 16K | Approximately 13.41 GiB weights; fit depends on bounded sliding-window/global KV and actual backend scratch | Approximately 16–18 GiB |
| Qwen3.6-35B-A3B UD-IQ3_S, 16K–32K | Approximately 12.76 GiB weights; conditional FP16 live KV is 0.31/0.625 GiB and recurrent matrix state about 60 MiB per sequence | Approximately 16–18 GiB |

Derive cache values from the actual GGUF and reconcile logs against the [Qwen3.8][qwen-config], [Qwen3.6][qwen36-config], and [Gemma][gemma12-config] configurations. For conventional Qwen3.8 bring-up, also propose **two checkpoints and 256 MiB prompt-cache storage** while its larger weights are resident; Swift's later eight-checkpoint/1 GiB reuse candidate is not a universal default. Keep 2–3 GiB as a provisional compute/staging allowance and reject unexpected copies. Qwen3.8 at 32K adds another 1 GiB of FP16 KV; longer context, vision, or large batches may require accepted Q8 KV, less cache, or a smaller model. Do not promote a configuration whose measured peak crosses the ceiling.

Do not select a quantization solely by its familiar name: Qwen3.6's published UD-Q4_K_M file is 22.1 GB, approximately 20.6 GiB, exceeding the service ceiling before caches. Model size, active parameters, and backend-supported types must all fit together.

The original **Swift PQ2_0-specific larger-cache daily candidate** retains this allowance; the initial creative profile starts with the smaller context/cache settings in section 8:

| Component | Proposed larger-cache daily-profile allowance | Notes |
| --- | ---: | --- |
| Packed weights plus unavoidable layout overhead | Up to 8 GiB | PQ2_0 file alone is about 6.71 GiB; detect duplicated/repacked copies |
| Live full-attention KV | Up to 2 GiB | Fits the conditional 32K FP16 estimate |
| Recurrent state and retained checkpoints | Up to 2 GiB | One sequence; eight-checkpoint reuse candidate after the initial two-checkpoint creative profile |
| Server prompt-cache storage | 1 GiB | Separate from live KV and checkpoints |
| Compute, staging, graph, and process overhead | Up to 3 GiB initially | Actual peak depends on backend and microbatch |
| Additional inference-service headroom | About 2 GiB | Detect unexpected allocations before exhausting the machine |

Track unique physical residency as well as process private bytes, system commit, available RAM, page faults, and GPU shared-memory allocation/eviction. Mapped weights and GPU allocations can overlap in accounting; summing dashboard numbers can double-count, while ignoring copies can undercount.

Start with supported automatic/mapped loading. Do not force memory locking, disable the Windows pagefile, or change system memory policy to manufacture a result. Reduce context, cache snapshots, or microbatch when allocations exceed the envelope.

Store models in one owner-selected absolute data directory, outside Git. Retain immutable downloaded packs plus a small number of versioned builds; budget approximately 30–50 GB initially for models, toolchains/builds, cache data, and results, then revise from actual usage. Avoid downloading the full-precision source unless a separately justified conversion needs it.

Keep disposable scripts, captures, logs, and diagnostics in a unique session directory under the system temp directory. Launch diagnostics with an explicit absolute working directory, preserve inherited Windows environment variables, and use documented build-output locations. Cleanup must verify ownership and resolved paths.

## 8. Initial profiles to implement

All settings here are **candidates**, not known optima. The launcher must obtain accepted flag names and effective values from the chosen binary; fork/version defaults can change. [Server options][server-docs]

| Profile | Context / active slots | KV / reuse | Execution | Intended use |
| --- | --- | --- | --- | --- |
| `reference-cpu` | 8K / 1 | FP16, speculation off; reuse separately switchable | CPU only | Correctness and regression reference |
| `daily` | 16K initially; promote 32K after validation / 1 | FP16 initially; Q8 candidate; 8 checkpoints, 1 GiB prompt cache | Best validated CPU/Vulkan placement | General interactive use |
| `creative` | Chosen model: 8K initially, then 16K as needed / 1 | FP16 initially; for Swift, propose 2 checkpoints and 256 MiB prompt cache; qualify Q8 if useful | CPU 4/6/8-thread screen, then accepted CPU/Vulkan profile; qualified thinking-off/moderate effort and sampler, explicitly shown | First engine profile for drafting, revision, art direction; useful prose, short waits, and acceptable sustained heat |
| `long-context` | 64K first; larger only after acceptance / 1 | Q8 candidate; Q4 separately experimental | Best supported placement within budget | Documents and extended reasoning |
| `battery` | 8K–16K / 1 | Bounded reuse, no speculation | Screen zero/low CPU polling and phase-specific threads; choose by energy/task and responsiveness | Lower power use |
| `npu-experimental` | Explicitly bounded / 1 | Backend-specific validated state path | Only after NPU gate | Research, never silent default |

Initial batch/microbatch candidates are 256/64 for bring-up and 512/128 for qualification. Increase only with memory evidence; prefill and decoding have different bottlenecks. Explicitly set one slot rather than inheriting an automatic concurrency default.

Qualify the smaller-cache **`creative` profile for the model chosen in the upfront literature review first**. If Swift is chosen, the eight-checkpoint/1 GiB `daily` settings are a later reuse candidate, not the initial comfort-oriented default. Conventional Qwen3.8 can also start with two checkpoints and 256 MiB prompt cache; Gemma requires its own non-recurrent cache policy. Keep the model warm during active use where residency is comfortable. Vision gets a separately admitted variant with its exact projector and image budget; long-form writing gets an explicit longer-context profile only after continuity, memory, and sustained-operation acceptance. Do not force a lower reasoning mode unless the exact template supports it and creative checks pass; unsupported `low` can behave near `xhigh` on base Bonsai.

Retain GPU kernel/shader caches supported by the chosen backend and invalidate them on relevant source/toolchain/device changes. Separate cold startup, warm model, and warm prompt-cache results. Do not count shader compilation as steady-state decode or hide it from startup measurements.

For an accepted OpenCL build supporting the documented program cache, set `GGML_OPENCL_KERNEL_CACHE_DIR` only in the child environment to an absolute private bounded cache directory; never redirect `LOCALAPPDATA`. Validate hit/miss behavior and invalidation by source/options/device/driver/platform identity, and report recompilation when the cache is unavailable. Keep writable cache contents out of runtime/model installation directories. This improves a candidate startup path, not the model's established decode rate. [OpenCL cache documentation][upstream-opencl]

## 9. Speculative decoding and advanced optimization

Keep speculation off until ordinary execution, caching, and rollback are correct. The current [Bonsai speculative guide][speculation] says Bonsai 2 27B has no official DSpark drafter; previous-generation drafters and their speedups do not establish compatibility with Swift Bonsai 2.

The default `bonsai2` demo deliberately runs without speculation even if a drafter file is present. Community Bonsai 2 experiments requiring runtime changes, and reported GB10 gains, are leads to audit rather than released Swift/Snapdragon support. An NPU drafter is a later heterogeneous-inference experiment: tokenizer/template agreement, target verification, hybrid rollback, actual draft acceptance, and cross-device overhead must all pass before enabling it. Do not multiply its hypothetical speedup by token-efficiency or kernel claims.

Possible later experiments, in this order:

1. Qualify target verification, supported sampling, and checkpoint/rollback semantics for each rejected draft.
2. Try **model-free prompt lookup/ngram speculation** for copy-heavy revision before adding a draft model. Current runtime sources expose `ngram-*` paths that propose existing text without another weight pack. Begin with short drafts; check the pinned implementation rather than assuming current branch options are released. [Lookup approach][prompt-lookup] · [Fork implementation][speculative-source]
3. Confirm whether the exact Swift GGUF contains usable MTP/NextN tensors; base configuration alone is insufficient.
4. Consider a compatible small external drafter only if tokenization and verification are sound.
5. Measure accepted tokens per verification, proposal/target cost, recurrent checkpoint/replay cost, peak memory, and finished-task latency together. Prompt lookup has no second model but can still lose to target verification and state copies.
6. Recheck prefix-cache behavior and cross-request state after enabling the speculative path.

Preserve target sampling semantics where the algorithm claims exactness; a greedy-only path cannot silently replace the creative sampler. Prompt lookup is most plausible when much of supplied prose is retained, not as a promised benefit for free invention. It still requires hybrid-state restoration after rejection. Early-exit/approximate methods require a separately labeled quality-changing profile. A second model sharing the same memory bandwidth can slow the laptop even when its draft-token acceptance looks good.

Other potential work belongs behind profiles: packed CPU kernels, Vulkan fusion, graph reuse, persistent buffers, and carefully selected prefill/decode placement. Do not simultaneously vary packing, sampling, context, and device assignment and attribute the result to one optimization.

### NoGraphicsAPI assessment

**Do not replace the ggml Vulkan backend with NoGraphicsAPI for this project.** Its current Vulkan path is a C++20 abstraction built on Vulkan, targeting x86-64; the utility math requires AVX2/FMA. Its feature contract includes Vulkan 1.4, descriptor heaps, mesh shaders, and untyped shader pointers. This is a narrower target than generic Vulkan compute and is not a documented Windows ARM64/Adreno inference backend. It also leaves GPU allocation lifetimes and synchronization with the application. [Project requirements][nographics-readme]

This is a compatibility/engineering decision, not a measured claim that wrappers are inherently slow. The implementation passes a root GPU address without copying the root structure; some fallback commands resolve addresses to backing buffers. Its public interface intentionally omits native operations, and non-Apple ARM builds are unsupported. Extending it or adding native Vulkan escape hatches could expose more functionality, but would not establish performance parity or improve the model's kernels. Reconsider only after a supported ARM64/device path and same-workload CI comparison show no material performance/energy regression; no such result is established here. [Native mapping][nographics-vulkan] · [API limits][nographics-limits] · [Build targets][nographics-build]

The useful ideas are persistent allocations, reducing redundant binding/submission work, and keeping data resident. Investigate those inside the existing backend if profiles show CPU submission overhead. A wrapper does not supply Swift packed kernels, Gated Delta Net, KV management, or model scheduling, and an ARM64 port would add maintenance before establishing a benefit.

Use a simple Amdahl bound to decide whether API work deserves attention. If serial submission/bookkeeping on the request's critical path occupies 5% of its duration, even eliminating it entirely gives at most `1 / 0.95 = 1.053x`; at 10%, the bound is `1.111x`. Do not estimate that fraction by summing CPU/GPU work that overlaps. These are illustrative calculations, not measurements. Improving packed kernels, memory traffic, cache reuse, or unnecessary model work is more promising when those dominate. Do not require graphics-only extensions for a compute optimization.

### BITCOS: a worthwhile conditional compression experiment

BITCOS stores a presence bitmap and signs only for nonzero ternary values: ideal symbol cost is `2 - z` bits/weight for zero fraction `z`. Published implementations/results are Intel AVX2/AVX-512 and Xe2, not Oryon/Adreno; the paper also reports a mobile CPU case that loses despite compression. It does not establish a ready-made Swift/Snapdragon integration. [BITCOS paper][bitcos-paper]

For the **same exact group-128 weights**, retaining one FP16 scale per group, our derived planning comparison is:

```text
PQ2_0:                   2.125 bits/weight
PTQ1_0:                  1.750 bits/weight
BITCOS with same scales:  2.125 - z + delta bits/weight

delta = actual offsets, padding, alignment, and indexing overhead
```

The existing block costs come from [Prism's format definitions][quant-blocks]. Ignoring `delta`, the following compares eligible packed tensors, **not whole model files, RAM use, or tokens/second**:

| Actual zero fraction | Ideal BITCOS bits/weight | Bytes saved vs PQ2_0 | Bytes saved vs PTQ1_0 |
| --- | ---: | ---: | ---: |
| 30% | 1.825 | 14.1% | -4.3%: larger |
| 40% | 1.725 | 18.8% | 1.4% |
| 50% | 1.625 | 23.5% | 7.1% |

BITCOS beats the dense-trit PTQ block cost when `z > 0.375 + delta`, with zero fraction `z` expressed from 0 to 1. It can reduce PQ2 bytes while still losing runtime to unpacking and indexing. Swift PQ2 and the published PTQ release have different merged weights, so this table is an encoding comparison on identical hypothetical data, not an apples-to-apples comparison of those two released artifacts.

Proposed feasibility sequence:

1. Census each eligible tensor of the pinned current Swift PQ2 file: encoded symbols, zeros, group structure, dimensions, and scales. Do not reuse a previous-generation Bonsai sparsity statistic.
2. Prove the data is actually ternary. The [generic PQ2 decoder][prism-quant-codec] permits a fourth `+2` code. A strictly ternary converter must reject such data or use an explicitly designed lossless exception representation; never silently drop or remap it.
3. Estimate **serialized and runtime** size with real tile offsets and padding. Separate archival compression from a layout that supports efficient random row/tile access.
4. Write a reference lossless repacker and exhaustive block round-trip checks. Preserve scales, tensor ordering, rotations/signs, tokenizer, and model metadata. No pruning, changed activation quantization, retraining, or altered Swift weights in this experiment.
5. Develop/qualify an ARM64 NEON fused unpack-and-dot kernel against the current native baseline. Decode directly into useful computation, not a persistent expanded weight copy.
6. Consider an Adreno shader only after CPU feasibility and a GPU-specific access/subgroup design. An Intel instruction sequence is not a portable optimization, and BITCOS does not automatically enable NPU execution.
7. Compare microkernels **and** end-to-end cold/warm prefill, decode, memory, energy, and prefix reuse in CI. Keep the original format/profile if improvement is inconclusive.

This could be a lossless storage optimization, but different accumulation paths can still change floating-point rounding. Require exact decoded-weight parity plus dtype-appropriate logits/task validation. Unstructured zeros and compact storage do not by themselves activate hardware structured-sparsity acceleration or eliminate the arithmetic.

### Other high-value opportunities, in priority order

| Priority | Candidate | Why investigate / constraint |
| --- | --- | --- |
| P0 | Creative model choice and supported reasoning/sampling budgets | Improve usable prose and collaboration before optimizing one model's packed kernels; count revisions and hidden reasoning |
| P0 | Stable minimal system/tool prefixes; bounded tool output; relevant file selection | Reduce prefill, context pressure, and agent retries without changing the engine |
| P0 | Warm resident model, valid hybrid prefix reuse, bounded checkpoints | Avoid repeated loading/prefill while respecting memory and state correctness |
| P0 | Scene packets, author-owned canon, visible prompt/output budgets | Reduce unnecessary context while preserving source facts, voice, and reviewable revisions |
| P1 | Thread/polling profiles and persistent compiled-kernel caches | Cheap existing controls for comfort and startup; qualify phase-specific tradeoffs |
| P1 | Actual ARM packed SIMD dispatch and runtime tiling/repacking | Improve the selected weights' execution before adding a new model format |
| P1 | Reviewed ik_llama Prism-format R8 ARM NEON backport if missing | Concrete existing PQ2/PTQ source lead before inventing an adapter; validate activation/transform semantics |
| P1 | T-MAC-style ARM lookup kernels | Existing Windows/ARM work; adapt exact Swift quantization and graph semantics |
| P1 | Vulkan fused operations, persistent scratch, graph/command reuse | Reduce measured dispatch/intermediate-memory costs without an API rewrite |
| P1 | Separate prefill/decode tuning and small contiguous device partitions | Different phases may favor different hardware; count migration/state-copy costs |
| P1 | Model-free prompt lookup for copy-heavy revisions | No second model; still needs correct target sampling, hybrid rollback, and net CI benefit |
| P2 | BITCOS | Only after actual sparsity, serialization overhead, and native kernel work justify it |
| P2 | Embedding retrieval or an accepted model-based speculative path | Add a model only when simpler context selection/lookup does not meet the task; require compatibility and memory acceptance |
| P3 | NPU ternary port / novel graphics abstraction | Substantial hardware-specific work; require evidence existing paths are the bottleneck |

**T-MAC deserves specific attention.** Microsoft's [T-MAC][tmac] uses lookup-based low-bit matrix kernels and documents Windows ARM/Snapdragon X Elite work. It is more directly relevant to CPU kernel experiments than copying Intel-specific BITCOS assembly. Its older models/baselines do not predict Swift 27B performance. Port a narrow, verified kernel adapter into the pinned runtime rather than assuming the older engine integration understands Swift's custom packs, transforms, or recurrent graph.

The related [T-MAN][tman] NPU demo documents Android Snapdragon 8 Gen 3 and slow prefill; it is research evidence, not Windows X Elite support. Keep approximate aggregation, activation quantization, and changes to group scales as separately labeled quality-changing experiments.

An especially useful control is a **lossless runtime layout change on the current PQ2 weights**. For example, if a supported kernel wants group-64, splitting one group-128 scale into two identical group-64 scales can preserve decoded values while adding scale overhead. Verify symbol order, layout, and every decoded tensor; do not requantize. Compare the native kernel benefit with the extra bytes and repacking cost. This avoids attributing older PTQ weights' behavior to compression alone.

Do not retrofit GQA, a sliding window, KV eviction, or arbitrary recurrent-state compression as though they were lossless switches. The model already has its own attention topology. Paged allocation may help capacity/concurrency, but does not automatically speed a single-user hybrid workload. First exploit the runtime's supported memory management and correct reuse.

## 10. Validation and performance evidence

### Repository policy

Local builds, analyzer checks, and **focused correctness tests** are allowed. Full test suites run only in CI. **Every benchmark runs only in CI**, including single-case, smoke, debugging, short, and dry runs. Do not approximate a full suite with batches of local filters or disguise a benchmark as a diagnostic.

To measure this laptop, create a manually triggered self-hosted CI workflow on the Yoga, labeled for Windows ARM64 and this hardware. Running that job through CI preserves the benchmark-location rule while testing the real device. Generic hosted runners cannot supply its Adreno/NPU/thermal evidence.

The owner requested this planning update remain local. No remote publication, runner setup, or workflow dispatch is required for this Markdown revision. CPU implementation, local builds, and focused correctness checks can proceed before CI exists; performance promotion and full-suite acceptance wait for approved CI setup rather than moving those workloads outside CI.

If CI is later configured, allow the laptop runner only manually approved, reviewed `main` revisions and workflows. Never execute untrusted pull-request/fork code or attacker-controlled checkout commands/inputs, including through `pull_request_target`. Use a dedicated non-administrator account without personal manuscripts, live chat/state caches, or reusable cloud/Git credentials; give workflow tokens minimal read-only permissions unless a specific job needs more. Pin third-party actions to full commits, review dependencies as executable code, serialize GPU/NPU workloads, use explicit private scratch/artifact lanes, and stop the runner when unused. Runner registration alone is not cleanup or isolation. Benchmark fixtures use synthetic or expressly approved content and their own inference instance. [Self-hosted runner security][actions-security]

### Focused local correctness checks

- Verify a pinned model loads with the intended tensor types and chat template; generate a short bounded response.
- Exercise streaming, cancellation, context rejection, orderly shutdown, and process restart.
- Compare a selected packed kernel to a dequantized/reference calculation, including zero/extreme inputs, block boundaries, and awkward shapes.
- Compare the same request with fresh prefill versus prefix reuse, including recurrent-state restoration after an edited turn.
- Check one targeted long-prompt regression or backend failure at a time when its change requires it.
- For sandbox/broker changes, exercise the specific denied access, subprocess, network, or timeout path being changed. Full containment suites and all latency measurements remain in CI.
- For direct-chat security changes, check missing/wrong keys, unexpected Host/Origin, disabled host-tools/MCP/admin routes, request limits, safe rendering, and offline restart. Verify inherited environment variables cannot re-enable a forbidden capability.
- For inference-policy changes, check denied unrelated-file/credential/network access while approved model mmap, private cache writes, device access, streaming, and cancellation still work. Do not infer the boundary from a successful launch.

Numerical comparisons use explicit dtype-appropriate tolerances. Greedy token agreement is useful but insufficient; small floating-point changes can alter a sampled sequence. Compare selected logits, finite outputs, and task correctness. New quantized cache/weight representations need quality evaluation, not a demand for bit-identical output.

### CI workload matrix

This matrix applies **after the literature-based model choice**, to building and qualifying that model's inference engine. It covers engine correctness, creative workflows, performance, and optimization; it is not the model-selection gate.

Use native `llama-bench` where appropriate plus a small repository-owned streaming HTTP runner for user-visible latency/cache behavior. Add [EleutherAI's evaluation harness][lm-eval-api] through its local API adapters for compatible tasks; validate templates, scoring, tokenization, and required log probabilities before relying on it.

| Dimension | Initial representative cases |
| --- | --- |
| Model / creative quality | The artifact chosen up front from literature; compare its reference and optimized runtime/profile outputs for quality regressions |
| Creative workflow | Scene drafting, voice/style changes, constrained revision, multi-chapter continuity, art direction/image prompts; grounded image critique only after vision qualification |
| Project context | Scene packets versus admitted longer context; author-approved facts, omitted/conflicting canon, summary provenance, and source relevance |
| Prompt size | 512, 2K, 8K, 16K; then 32K/64K for accepted profiles |
| Output | Fixed-length 128/512-token performance cases; separate natural task completions |
| Cache | Fresh process, warm model/no reusable prompt, repeated prefix, edited prefix, conversation switch |
| Backend | CPU; selected partial/full Vulkan placements; NPU only after its correctness gate |
| KV | FP16; accepted Q8; experimental Q4 with exact calibration provenance |
| Concurrency | One active request first; two only as a separate throughput experiment |
| Power | Plugged-in performance and a separately recorded battery profile |
| CPU waiting / startup | Phase-specific threads and polling; cold/warm compiled-kernel cache; explicit idle-unload tradeoff |
| Thermal | Short interactive bursts plus 15–30-minute sustained runs |
| Harness / containment | Direct API, accepted agent workflow, cold/warm MXC worker; separate VM tier if qualified |
| Experimental packing | Same current weights/layout control, actual BITCOS serialized size, native kernel, end-to-end results |

Avoid a full Cartesian sweep. Establish a small engine baseline for the selected model, then screen thread/batch choices and feasible device placements; qualify cache choices on the best implementation profiles. Keep backend/kernel comparisons on the same exact model artifact and settings. Attribute weight-version, packing, and sampler changes separately so they cannot be mistaken for an engine optimization. The upfront literature review has already settled model selection.

Record:

- Load/readiness time, shader/kernel compilation, cold and warm time to first token, and first answer latency.
- Prompt tokens processed versus reused, prefill rate, decode rate at several occupied-context lengths, and total request time.
- Median and p95 across sufficient repetitions; increase samples when variance prevents a conclusion. Randomize/interleave candidates and separate first-run warmup.
- Peak and sustained memory, commit, hard faults, GPU evictions, and allocation/repack sizes.
- Power state, clocks/thermal throttling where observable, fan behavior notes, and energy per completed task where valid telemetry exists. Mark missing telemetry as unavailable.
- Quality, finish reasons, error/cancellation behavior, speculative acceptance if enabled, and client-visible truncation.
- Every artifact/version/configuration identity plus CI run URL, tested revision, and verdict.

Use held-out creative briefs, instruction-following/revision, long-form continuity, and long-context retrieval cases first. Include character/world facts at early/middle/late context positions and edits to earlier turns. Track generic phrasing, repetition, viewpoint drift, constraint violations, revision count, and owner preference alongside automated checks. Add coding/edit, tool/JSON, and containment suites only for profiles enabling those capabilities. Benchmark correctness includes failures and truncation; do not discard them from the score. Keep private manuscripts and artwork out of shared CI artifacts unless the owner explicitly chooses otherwise.

### Promotion criteria

Promote a configuration only when:

1. Focused state/streaming/cancellation/security checks and the required CI suites pass. A daily profile enforces the direct-chat authentication, endpoint, privacy, and restricted inference-process contract; an explicitly trusted development profile is not a substitute for that acceptance.
2. It stays within memory budgets without sustained paging or eviction and remains responsive through the sustained workload, with heat/fan behavior acceptable to the owner. Record temperature, throttling, power, and energy per acceptable finished piece where telemetry is valid; mark missing readings unavailable. Do not infer a thermal pass from small weights or a short burst.
3. It has no unresolved critical task failure and its quality difference is within a predeclared evaluation tolerance. For creative work, use the owner's paired preferences and explicit voice/continuity/revision rubric; faster text is not a win if it takes more revisions or misses the brief. Report uncertainty when sample size cannot establish a preference.
4. For same-model performance optimizations, paired CI measurements establish a useful improvement: use **10% lower end-to-end latency** as an initial target, or a documented capacity/energy benefit without unacceptable interactive regression. For creative behavior profiles of the chosen model, owner-preferred prose/continuity can justify slower generation only within accepted interactive-latency, memory, and sustained heat/energy limits. Report that quality/speed/comfort tradeoff. These are engine/profile promotion criteria; model choice was completed through literature before implementation.
5. The manifest and effective profile reproduce the outcome on the settled `main` revision.
6. An agent profile enforces its declared filesystem/network/process boundary, contains all execution paths, and fails closed. A successful model response alone does not qualify the sandbox or the agent.

Keep a short promotion record with the exact model/runtime/profile, creative strengths/limitations, useful-context and output limits, observed interactive/sustained behavior, enforced security boundary, evidence links, and rollback profile. Owner-facing status should expose actionable limits and fallback reasons without surfacing compiler/kernel settings in normal creative work. Stop research lanes that cannot demonstrate a plausible end-to-end benefit within their agreed scope; additional engineering is a separate decision.

Choose among a small Pareto set: fastest interactive, best long-context capacity, and most efficient battery behavior. No finite tuning pass proves a global optimum. A source-supported accelerator without correct device results remains experimental; an NPU no-go decision can still leave a complete, useful CPU/GPU product.

## 11. Proposed repository implementation layout

This is the target layout. The implemented foundation and development commands are described in the [development guide](docs/development-guide.md); remaining work is tracked in the milestones below.

```text
PLAN.md
README.md                         project introduction and documentation links
docs/development-guide.md         setup and development commands
docs/validation.md                standards, focused checks, and CI
config/runtime.lock.json          runtime/model/toolchain identities
config/profiles/*.json            explicit validated settings
config/policies/*.json            versioned MXC requests and capability limits
config/security/*.json            inference launch, route/origin, and process policies
src/broker/                       small Rust policy, launch, and inference bridge
scripts/Doctor.ps1                native tools/devices/memory/dependency report
scripts/Get-Model.ps1             pinned resumable download and hash verification
scripts/Build-Engine.ps1           reproducible ARM64 Release builds
scripts/Start-Bonsai.ps1           profile resolution, startup, readiness
scripts/Stop-Bonsai.ps1            stop owned process
patches/                          minimal justified upstream changes
tests/                            focused correctness fixtures and CI suites
tools/evaluation/                 API runner, task definitions, result schema
.github/workflows/                build/test plus manual laptop performance jobs
docs/results/                     concise reports linking retained CI evidence
docs/model-selection.md           upfront literature sources, comparisons, model choice, confidence
docs/writing-workflow.md          scene packets, canon, summaries, context/output budgets
```

Keep third-party source at pinned revisions without copying an untracked engine into the repository. Model packs and personal conversation caches stay outside Git. Build outputs use documented artifact lanes; disposable diagnostics live in temp. A tool should fail clearly if a required native component is absent rather than installing arbitrary replacements.

Give every delegated worker a dedicated worktree, retain useful evidence, integrate session changes into local `main`, validate the settled revision, and remove eligible session worktrees/branches. Preserve other sessions' work. The original planning revision was explicitly **local-only**. Subsequent implementation follows the owner's current publication instructions: publish only `main`, never work branches or publication tags.

## 12. Delivery milestones and stop/go decisions

Effort ranges below are planning estimates for one engineer after dependencies are available. Driver, SDK, or kernel gaps can expand them; there is no implied delivery date.

| Milestone | Approximate effort | Concrete output | Exit condition |
| --- | --- | --- | --- |
| M0a: compatibility preflight and literature model choice | 0.5–1 day | Source-only ARM64/format/graph/memory preflight; published creative benchmark/sample review; decision with confidence, fallback, and reopen/stop conditions | One feasible model chosen before engine implementation, using literature rather than local model benchmarking |
| M0: inventory and reproducibility | 0.5–1 day | Device report, GGUF metadata, immutable manifests, toolchain/CI design | Exact architecture and dependency gaps known |
| M1: CPU + authenticated direct local chat | 1–3 days for engine bring-up; isolation gaps scoped separately | Native matched engine, inference-only UI/launcher, key/origin/endpoint controls, focused regressions and inference-policy qualification | Correct bounded chat, cancellation, offline restart; restricted daily process policy passes or remains explicitly development-only |
| M1a: chosen engine's creative/comfort qualification | 1–3 days after initial loading paths work | Scene packets/project bible, visible context/output budgets, bounded creative profile, polling/thread screen and sustained CI evidence | Useful prose/art collaboration with acceptable waits, heat, memory, and daily security on the selected model |
| M1b: optional agent + containment qualification | 3–5 days if wanted and supported paths work | Native agent candidate, MXC broker/policy, bounded inference bridge, bypass checks | Successful reviewed task and enforced whole-agent boundary before enabling tools |
| M2: Vulkan qualification | 2–5 days if existing kernels work | Native GPU build, placement audit, CI CPU/GPU comparison | Correct graph and demonstrated benefit, or documented no-go |
| M3: cache and context qualification | 2–4 days | Hybrid-state tests, memory estimator, bounded reuse, accepted KV types | Fresh/reused continuations agree; selected contexts fit and retain quality |
| M4: profiles and performance report | 2–4 days | Creative/daily/long/battery profiles, optional vision/accepted agent profiles, CI provenance, installation guide | Sustained creative-task and memory acceptance on settled main; containment acceptance for enabled tools |
| M5: NPU feasibility | About 2 days initially | Operation/packing/memory/signing gap report, including audited HTP prototype where relevant | Feasible bounded implementation or explicit deferral; no unsafe signing-policy workaround |
| M6: R8 / lookup kernels / speculation / BITCOS | Separately estimated after feasibility | Narrow reviewed native-kernel patch or profile; model-free lookup before a second drafter | Decoded-weight and behavior correctness, native-platform support, and CI benefit justify maintenance cost |

M0a -> M0 -> M1 -> M1a -> M2/M3 -> M4 is the main creative-chat delivery path: preflight source compatibility and choose the model from literature first, then build and qualify its inference engine, with independent work isolated appropriately. M1b is optional; agent automation is accepted only after it passes. M6 is also optional and may follow whichever CPU/cache bottleneck justifies it; a small existing-kernel backport need not wait for NPU research. Investigate M5 after the selected engine's CPU/GPU results expose realistic bottlenecks; do not make everyday usability depend on a speculative NPU port. A substantial custom ternary NPU backend is a multiweek project to scope after the feasibility report, not a two-day integration.

The first step is **M0a: finish the source-only preflight and literature model choice**. Then proceed directly to **M0 plus M1, followed by M1a**: pin the model and matching runtime, inspect native capabilities, establish an honest CPU reference, and make authenticated local chat usable. Qualify its 8K–16K creative profile with scene packets, short waits, sustained comfort, and the daily security contract. Continue CPU/GPU, cache, and justified kernel work on that engine; the build phase is not a prerequisite for model choice. Reopen the literature decision if its documented stop conditions are reached.

## 13. Risk register and explicit open questions

| Risk / question | Resolution |
| --- | --- |
| Does a math/coding benchmark or newer base imply better creative prose? | Upfront literature review of published creative results and samples; distinguish exact quantized artifacts from base-model evidence and record gaps |
| Does Swift's shorter-thinking objective help fiction or art direction? | Seek published current-Swift/base comparisons relevant to creative work; missing evidence remains uncertainty in the upfront choice |
| Is the initial model choice feasible or becoming a sunk-cost commitment? | Source-only compatibility/memory preflight, literature fallback, and explicit reopen/engineering stop conditions |
| Does conventional Qwen3.8 Q3 fit the service ceiling with useful context? | Start 16K, few checkpoints and small cache; inspect peak copies/scratch; qualify longer context or Q8 KV separately |
| Is image input actually enabled and grounded? | Pin matching projector/preprocessor, budget image tokens/allocations, and validate observed details before recommending critique |
| Is image generation being confused with text or image understanding? | Separate image-model pipeline and shared-memory scheduling decision |
| Does an ordinary GGUF NPU route establish Swift support? | GenieX plugin/format/operator audit; require exact packed types and placement rather than inferring from Q4_0 support |
| Does current Swift metadata exactly match the upstream dimensions? | Read the selected GGUF before allocating caches or accepting the estimates |
| Does native ARM64 Vulkan work correctly on the installed Adreno driver? | Native dependency check, operation tests, whole-model comparison, device CI |
| Is CPU or GPU faster for the actual context/task? | Paired self-hosted CI; retain both validated profiles |
| Do generic backend types omit a Bonsai transform or force CPU fallback? | Audit full graph and placement, not just the quantization decoder |
| Can custom ternary weights execute efficiently on Hexagon? | Time-boxed format/operator/memory investigation; separate port decision |
| Does the smaller PTQ release alter quality? | Consult published evidence during model selection and attribute it as a different Swift version; later engine experiments must keep version changes distinct |
| Does prefix reuse restore incorrect recurrent state? | Fresh-prefill comparison after edits, cancellation, eviction, and slot switching |
| Does context selection or a generated summary damage story canon? | Author-owned bible/originals, sourced versioned summaries, contradiction checks, and explicit acceptance of changes |
| Do checkpoint/rollback copies overwhelm RAM? | Separate caps, actual allocation accounting, one sequence initially |
| Does Q8/Q4 KV impair long-context reasoning or retrieval? | Held-out task evaluation with exact calibration and runtime identity |
| Does an accelerator win briefly but throttle or stall Windows? | Sustained device CI and responsiveness/memory criteria |
| Does the client silently change templates, sampling, or token limits? | Capture rendered prompts/effective request settings in controlled fixtures |
| Are coding/tools reliable enough to recommend? | Separate client task acceptance; start with reviewable small edits |
| Is a memory-safe client mistaken for a memory-safe whole stack? | Document native runtime/driver/FFI trust boundaries; isolate processes and keep broker code small |
| Does an execution path bypass MXC? | Contain the whole agent and descendants; test plugins, hooks, direct filesystem/network calls, and failed launches |
| Can direct chat or native server tools bypass the agent boundary? | Authenticated exact-origin inference-only profile; reject host-tools/MCP/router and environment overrides; negative route tests |
| Can a model/media parser read unrelated data outside the tool worker? | Qualified restricted inference process, trusted artifacts, private caches, bounded uploads, denied egress, and security updates |
| Can a self-hosted benchmark job compromise the personal laptop? | Reviewed manual main-only workflows, dedicated account, pinned dependencies, no personal data/credentials, and no untrusted PR inputs |
| Does Windows ARM64 availability imply every policy is supported? | Probe exact request/backend/tier and verify enforcement; no permissive fallback |
| Do LiteBox or NVX meet this host's boundary and architecture needs? | Exclude reviewed x86/shared-address-space or non-ARM paths from the default |
| Would NoGraphicsAPI improve actual inference? | Existing Vulkan path first; measure submission fraction before considering any port |
| Do R8 ARM sources or an HTP prototype imply a proven X Elite improvement? | Exact artifact/graph and device correctness, pinned diff/license review, repack/rollback accounting, then CI; no borrowed x86/X2 speed claims |
| Are actual PQ2 symbols strictly ternary, and is BITCOS smaller/faster? | Per-tensor census, exact round-trip, overhead-aware size, native kernels, CI; reject unsupported symbols |
| Is speculative decoding actually compatible and beneficial? | Exact tensors/drafter identity, rollback tests, cache tests, CI end-to-end gain |
| Can the result be reproduced after an upstream update? | Pin all artifacts, retain baseline profile, qualify upgrades deliberately |

## 14. Primary sources and freshness

Sources were checked on October 22, 2026. Moving branch documentation is research evidence; implementation must retain immutable revisions and recheck device support. This plan distinguishes publisher statements, source-level backend support, conditional arithmetic, and proposed settings; none is a measured laptop result.

- [UkisAI release page][swift-page] and [Swift artifact/model instructions][swift-model].
- [Prism base Bonsai 2 architecture][bonsai-model] and [upstream Qwen configuration][qwen-config].
- [Qwen3.8 model][qwen38-model] and [quantization choices][qwen38-quants]; [Qwen3.6 MoE model][qwen36-model], [quantizations][qwen36-quants], and [configuration][qwen36-config].
- [Gemma 4 architecture][gemma12-model], [configuration][gemma12-config], [12B QAT pack][gemma12-qat], [26B-A4B QAT pack][gemma26-qat], and [Google's QAT explanation][gemma-qat-release].
- [Creative-writing evaluation][creative-bench] and [longform methodology][longform-bench]; these are evaluation leads, not exact local-quantization results.
- [Prism backend/format audit][backend-support] and [candidate runtime release][runtime-release].
- [Windows ARM64 OpenCL guide][opencl-guide] and [Snapdragon/Hexagon guide][snapdragon-guide].
- [Upstream OpenCL support][upstream-opencl], [Qwen MoE graph][upstream-qwenmoe], [Gemma graph][upstream-gemma], and [Bonsai demo setup][demo-setup].
- [Qualcomm WoS Porter][wos-porter], [GenieX SDK][geniex-sdk], [model/quantization guide][geniex-models], [signed Windows release][geniex-release], and [historical community Adreno report][adreno-history].
- [Historical X Elite CPU/ISA report][snapdragon-cpu-history]; [Prism R8 kernels][ik-prism-r8], [PQ2 ARM NEON follow-up][ik-pq2-neon], [PTQ ARM NEON follow-up][ik-ptq-neon], and [license][ik-license].
- [X Elite HTP prototype][htp-llm] and [unfinished ternary branch][htp-ternary]; [X2-specific OpenCL binary kernels][adreno-x2-kernels]. These are scoped research leads, not certified X1 performance.
- [llama-server interface/options][server-docs], [Qwen35 graph][qwen-implementation], and [recurrent allocation implementation][recurrent-implementation].
- [Prompt reuse/checkpoints][prompt-cache], [experimental KV4][kv-cache], and [speculative compatibility][speculation].
- [Model-free prompt lookup][prompt-lookup] and [fork speculative implementation][speculative-source].
- [Inference runtime security guidance][llama-security] and [self-hosted runner security][actions-security].
- [Aider local API connection][aider-api] and [lm-evaluation-harness local API adapters][lm-eval-api].
- [OpenCode releases][opencode-changelog], [provider integration][opencode-provider], and [security boundary][opencode-security]; [Goose Windows build][goose-windows].
- [MXC][mxc-readme], [OS support][mxc-os-support], [Rust V1 API][mxc-api], [schema][mxc-schema], and [networking contract][mxc-network].
- [LiteBox][litebox-readme], its [Windows userland platform][litebox-platform], [NVX architecture exclusions][nvx-non-goals], and [Windows Sandbox][windows-sandbox].
- [NoGraphicsAPI requirements][nographics-readme], [BITCOS paper][bitcos-paper], [Prism codec][prism-quant-codec], [T-MAC][tmac], and [T-MAN][tman].

[swift-page]: https://ukisai.com/swift-bonsai-2
[swift-model]: https://huggingface.co/ukisai/Swift-Bonsai-2-GGUF
[bonsai-model]: https://huggingface.co/prism-ml/Ternary-Bonsai-2-27B-gguf
[qwen-config]: https://huggingface.co/Qwen/Qwen3.8-27B/blob/main/config.json
[qwen38-model]: https://huggingface.co/Qwen/Qwen3.8-27B
[qwen38-quants]: https://huggingface.co/unsloth/Qwen3.8-27B-GGUF/tree/main
[qwen36-model]: https://huggingface.co/Qwen/Qwen3.6-35B-A3B
[qwen36-quants]: https://huggingface.co/unsloth/Qwen3.6-35B-A3B-GGUF/tree/main
[qwen36-config]: https://huggingface.co/Qwen/Qwen3.6-35B-A3B/blob/main/config.json
[gemma12-model]: https://huggingface.co/google/gemma-4-12B-it
[gemma12-config]: https://huggingface.co/google/gemma-4-12B-it/blob/main/config.json
[gemma12-qat]: https://huggingface.co/google/gemma-4-12B-it-qat-q4_0-gguf
[gemma26-qat]: https://huggingface.co/google/gemma-4-26B-A4B-it-qat-q4_0-gguf
[gemma-qat-release]: https://blog.google/innovation-and-ai/technology/developers-tools/quantization-aware-training-gemma-4/
[creative-bench]: https://eqbench.com/creative_writing
[longform-bench]: https://eqbench.com/creative_writing_longform.html
[upstream-opencl]: https://github.com/ggml-org/llama.cpp/blob/master/docs/backend/OPENCL.md
[upstream-qwenmoe]: https://github.com/ggml-org/llama.cpp/blob/master/src/models/qwen35moe.cpp
[upstream-gemma]: https://github.com/ggml-org/llama.cpp/blob/master/src/models/gemma4.cpp
[demo-setup]: https://github.com/PrismML-Eng/Bonsai-demo/blob/main/setup.ps1
[wos-porter]: https://github.com/qualcomm/extension-wos-porter
[geniex-sdk]: https://github.com/qualcomm/GenieX/blob/main/sdk/README.md
[geniex-models]: https://geniex.aihub.qualcomm.com/en/models/supported
[geniex-release]: https://github.com/qualcomm/GenieX/releases/tag/v0.8.0
[adreno-history]: https://github.com/LucRoot/Windows_ARM64_bonsai_adreno_vulkan
[backend-support]: https://github.com/PrismML-Eng/Bonsai-demo/blob/main/BACKEND-SUPPORT.md
[runtime-release]: https://github.com/PrismML-Eng/llama.cpp/releases/tag/prism-b10787-73bfe6c
[runtime-license]: https://github.com/PrismML-Eng/llama.cpp/blob/prism/LICENSE
[opencl-guide]: https://github.com/PrismML-Eng/llama.cpp/blob/prism/docs/backend/OPENCL.md
[snapdragon-guide]: https://github.com/PrismML-Eng/llama.cpp/blob/prism/docs/backend/snapdragon/README.md
[hexagon-implementation]: https://github.com/PrismML-Eng/llama.cpp/blob/prism/ggml/src/ggml-hexagon/ggml-hexagon.cpp
[hexagon-windows]: https://github.com/ggml-org/llama.cpp/blob/master/docs/backend/snapdragon/windows.md
[hexagon-developer]: https://github.com/PrismML-Eng/llama.cpp/blob/prism/docs/backend/snapdragon/developer.md
[qnn-guide]: https://github.com/microsoft/onnxruntime-genai/blob/main/docs/qnn.md
[server-docs]: https://github.com/PrismML-Eng/llama.cpp/blob/prism/tools/server/README.md
[qwen-implementation]: https://github.com/PrismML-Eng/llama.cpp/blob/prism/src/models/qwen35.cpp
[recurrent-implementation]: https://github.com/PrismML-Eng/llama.cpp/blob/prism/src/llama-memory-recurrent.cpp
[quant-blocks]: https://github.com/PrismML-Eng/llama.cpp/blob/prism/ggml/src/ggml-common.h
[prompt-cache]: https://github.com/PrismML-Eng/Bonsai-demo/blob/main/PROMPT-CACHE.md
[kv-cache]: https://github.com/PrismML-Eng/Bonsai-demo/blob/main/KV-CACHE.md
[speculation]: https://github.com/PrismML-Eng/Bonsai-demo/blob/main/SPECULATIVE.md
[aider-api]: https://aider.chat/docs/llms/openai-compat.html
[aider-license]: https://github.com/Aider-AI/aider/blob/main/LICENSE.txt
[lm-eval-api]: https://github.com/EleutherAI/lm-evaluation-harness/blob/main/docs/API_guide.md
[opencode-provider]: https://opencode.ai/docs/providers/#llamacpp
[opencode-license]: https://github.com/anomalyco/opencode/blob/dev/LICENSE
[opencode-changelog]: https://github.com/anomalyco/opencode/blob/dev/CHANGELOG.md
[opencode-security]: https://github.com/anomalyco/opencode/security
[goose-windows]: https://github.com/aaif-goose/goose/blob/main/.github/workflows/bundle-windows.yml
[pi-security]: https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/security.md
[mxc-readme]: https://github.com/microsoft/mxc
[mxc-os-support]: https://github.com/microsoft/mxc/blob/main/docs/backends/process-container/os-version-support.md
[mxc-api]: https://github.com/microsoft/mxc/blob/main/docs/api-reference/rust/v1/api.md
[mxc-schema]: https://github.com/microsoft/mxc/blob/main/docs/schema.md
[mxc-network]: https://github.com/microsoft/mxc/blob/main/docs/backends/process-container/networking.md
[litebox-readme]: https://github.com/microsoft/litebox
[litebox-runner]: https://github.com/microsoft/litebox/blob/main/litebox_runner_linux_on_windows_userland/src/main.rs
[litebox-platform]: https://github.com/microsoft/litebox/blob/main/litebox_platform_windows_userland/src/lib.rs
[nvx-non-goals]: https://github.com/microsoft/nvx/blob/dev/doc/design/1.2-non-goals.md
[windows-sandbox]: https://learn.microsoft.com/en-us/windows/security/application-security/application-isolation/windows-sandbox/
[nographics-readme]: https://github.com/sebbbi/NoGraphicsAPI
[bitcos-paper]: https://arxiv.org/html/2609.16338v1
[prism-quant-codec]: https://github.com/PrismML-Eng/llama.cpp/blob/prism/ggml/src/ggml-quants.c
[tmac]: https://github.com/microsoft/T-MAC
[tman]: https://github.com/microsoft/T-MAC/tree/main/t-man
[snapdragon-cpu-history]: https://github.com/LucRoot/Windows_ARM64_bonsai_snapdragon_cpu
[ik-prism-r8]: https://github.com/ikawrakow/ik_llama.cpp/pull/2550
[ik-pq2-neon]: https://github.com/ikawrakow/ik_llama.cpp/pull/2561
[ik-ptq-neon]: https://github.com/ikawrakow/ik_llama.cpp/pull/2564
[ik-license]: https://github.com/ikawrakow/ik_llama.cpp/blob/main/LICENSE
[htp-llm]: https://github.com/aakar-mutha/htp-llm
[htp-ternary]: https://github.com/aakar-mutha/llama.cpp/tree/ternary-bonsai-htp
[adreno-x2-kernels]: https://github.com/ggml-org/llama.cpp/pull/23042
[prompt-lookup]: https://github.com/ggml-org/llama.cpp/blob/master/examples/lookup/README.md
[speculative-source]: https://github.com/PrismML-Eng/llama.cpp/blob/prism/common/speculative.cpp
[llama-security]: https://github.com/ggml-org/llama.cpp/security
[actions-security]: https://docs.github.com/en/actions/reference/security/secure-use#hardening-for-self-hosted-runners
[nographics-vulkan]: https://github.com/sebbbi/NoGraphicsAPI/blob/main/docs/vulkan-support.md
[nographics-limits]: https://github.com/sebbbi/NoGraphicsAPI/blob/main/docs/no-graphics-api-comparison.md
[nographics-build]: https://github.com/sebbbi/NoGraphicsAPI/blob/main/docs/building.md
