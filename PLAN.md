# Swift Bonsai 2 on Snapdragon X Elite

**Research and implementation plan — October 22, 2026**

**Status:** planning only. No inference engine has been implemented, models downloaded, drivers changed, benchmarks run, or laptop performance established. Values marked *proposed* or *estimated* are design inputs to validate.

## 1. Recommended direction

Build a native Windows ARM64 distribution around **Prism ML's llama.cpp fork**, with a small PowerShell management layer and its built-in web UI. Establish CPU correctness first, qualify **Vulkan on the Adreno X1-85** next, then select the best CPU/GPU configuration using measurements from this laptop. Treat direct Hexagon NPU execution as a separate research milestone.

The objective is the fastest useful, correct local assistant within the laptop's memory and thermal limits. Simultaneous CPU, GPU, and NPU utilization is a candidate configuration, not an acceptance requirement: extra processors can introduce synchronization, shared-memory contention, and power competition.

| Decision | Initial choice | Reason |
| --- | --- | --- |
| Runtime | Prism ML llama.cpp fork, pinned by full commit | Swift's published tensor formats need this fork |
| Main model | Current `Swift-Bonsai-2-PQ2_0.gguf` | Current merged Swift weights; preserve this as the semantic reference |
| Baseline | Native ARM64 CPU, FP16 KV, one conversation | Establish correctness with a simple execution path |
| First accelerator | Native ARM64 Vulkan build | Released fork support exists for both Swift packings and Bonsai transforms |
| First user harness | llama-server's built-in web UI and loopback API | Open source, light installation, direct control over the engine |
| Optional coding harness | Aider against the same API | Add only after coding and reasoning compatibility checks |
| Cache policy | Hybrid KV + recurrent state + bounded checkpoints | This model is not an ordinary all-attention transformer |
| NPU | Time-boxed feasibility investigation | No demonstrated native Swift packed-weight path established here |
| Speculation | Disabled initially | Bonsai 2 drafter and hybrid-state compatibility remain separate gates |
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
| RAM | 32 GB installed, 31.6 GB reported usable | Budget weights, caches, checkpoints, staging buffers, Windows, and applications together |
| Storage | Approximately 292 GB free from supplied figures | Enough for initial packs/builds; avoid uncontrolled model and cache duplication |

The memory figures below use **GiB/MiB** for runtime allocation and **GB** for publisher download sizes. Shared physical memory does not guarantee zero-copy operation, a single resident weight copy, or unrestricted GPU allocation. Discover actual limits before setting offload or context size.

Optimize these objectives in order:

1. Preserve the selected model's behavior and correct conversation state.
2. Improve total request time, first visible answer latency, and sustained generation for one active user.
3. Keep Windows responsive and avoid sustained paging or GPU-memory eviction.
4. Support the useful context length for the task without silently discarding information.
5. Offer an efficient battery profile as well as maximum plugged-in performance.

Record both first streamed reasoning token and first final-answer token. A reasoning model can appear responsive while taking much longer to produce its useful answer. Report tokens/second alongside completion length and end-to-end task time.

There is no credible laptop tokens/second forecast yet. Apple, NVIDIA, or other Snapdragon results are not substitutes for measurements of this exact build, model, driver, context, and power mode.

## 3. Model identity and runtime compatibility

### Preserve the actual Swift release

| Artifact | Publisher size | Role |
| --- | --- | --- |
| `Swift-Bonsai-2-PQ2_0.gguf` | 7.206 GB, approximately 6.71 GiB | Primary current Swift model |
| `Swift-Bonsai-2-PTQ1_0.gguf` | 5.947 GB, approximately 5.54 GiB | Smaller alternate; earlier Swift merged weights |

Both files are complete models. PQ2_0 contains updated merged weights relative to PTQ1_0. Consequently their comparison changes **model version as well as packing**. A faster PTQ1_0 result cannot establish a lossless optimization of current PQ2_0. Keep quality and performance results separately attributed. [Publisher release details][swift-page]

Create a manifest before implementation validation with:

- Hugging Face repository, immutable model revision, exact filename, byte length, and SHA-256.
- Tensor-type histogram, architecture, recurrent/full-attention split, head dimensions, context metadata, and rotation/sign metadata.
- Tokenizer identity, embedded chat-template hash, special token IDs, and optional MTP/NextN tensors.
- Runtime commit, any local patch commits, compiler, build flags, binary architecture, and matching runtime libraries.
- Driver versions, device capabilities, effective configuration, and calibration-file identity where applicable.

Use actual GGUF metadata as the implementation authority. The marketing name Qwen3.8 and GGUF architecture `qwen35` are not by themselves a mismatch: the upstream [Qwen configuration][qwen-config] uses the Qwen3.5 architecture family.

### Candidate runtime pins

- **Publisher compatibility reference:** revision `1a07bfa5f`, cited by UkisAI. Resolve its full commit before retaining it as a control.
- **Newer accelerator candidate:** release `prism-b10787-73bfe6c`, commit `73bfe6cc485554822f80c473a3c0c4f25e80f41b`, pinned by the current Bonsai demo. Its backend audit records Vulkan PQ2_0 support shipping from `prism-b10735-842b188`. [Release][runtime-release] · [Backend audit][backend-support]

Build the newer candidate natively and compare focused correctness against the reference. A documented compatibility reference is not an ARM64 performance certification. Capture release assets and source status at implementation time; the observed release has an ARM64 Windows CPU package, while the Windows Vulkan package is x64, so native ARM64 Vulkan needs its own build qualification.

Never mix fork binaries with unrelated stock ggml DLLs. Do not rename a tensor type, edit GGUF type IDs to bypass a loader, or substitute base Bonsai/Qwen weights for Swift. A future repacker is acceptable only if decoded values, group scales, transforms, tokenizer, and template are preserved and verified. Requantization is a new quality tradeoff.

## 4. Backend feasibility and acceleration order

| Route | Evidence available now | Implementation gate | Priority |
| --- | --- | --- | --- |
| ARM64 CPU | Fork supports Swift packings and Bonsai transforms | Native build, actual SIMD dispatch, focused model correctness | Required baseline |
| Vulkan GPU | Native PQ2_0/PTQ1_0 and FWHT paths are released | ARM64 build, Adreno driver capabilities, full graph correctness, useful offload | First acceleration candidate |
| OpenCL GPU | Exact X1-85 and Windows ARM64 are documented | Custom Swift packing support and transform/graph audit still needed | Contingency / development lane |
| Hexagon ggml backend | Windows Snapdragon route exists, experimentally | Custom packed matmul, full graph, state placement, SDK and driver access | Research lane |
| ONNX Runtime + QNN | Alternative NPU stack to investigate | Faithful export, quantization, supported graph, cache I/O, resource limits | Separate conversion project |
| DirectML / generic ONNX GPU | Potential alternative after conversion | Exact model conversion and custom ternary execution are unestablished | Lower priority |

The [Vulkan audit][backend-support] describes scalar/coopmat1 and integer-dot paths; device and driver choose usable kernels. It does not promise direct coopmat2 decoding or a particular Adreno speed. The [OpenCL guide][opencl-guide] explicitly lists X1-85, but its datatype list does not establish Swift PQ2_0/PTQ1_0 support. These are different compatibility questions.

### CPU implementation

Use a Release build with the selected revision's Windows ARM64 LLVM toolchain. Verify the PE architecture, dependency architectures, CPU features, and effective kernel dispatch. Inspect NEON/dot-product/i8mm support for the **actual** tensor shapes and packing in that revision; do not infer selected kernels from source-file names or assume extensions present on another ARM processor.

Begin with a single resident model. Tune generation threads independently from prefill threads; proposed candidate counts are 4, 6, 8, and 12, restricted to discovered cores. More threads can lose on a shared-memory-bound decode path. Avoid realtime process priority and leave capacity for the UI and operating system.

Only write custom CPU kernels after profiling identifies the cost. Candidate work includes packed dot products, decode into registers rather than persistent FP16 expansion, fused transforms, cache-friendly tiling, and projection fusion. Every optimized kernel needs a portable reference and adversarial shape/value checks.

### Adreno Vulkan implementation

1. Enumerate actual Vulkan devices, API/features/extensions, subgroup behavior, allocation limits, and memory budgets using native tools.
2. Compile host code and runtime libraries for ARM64, with a compatible shader toolchain. A working x64 executable under emulation does not qualify the native build.
3. Check packed matmul, embeddings/output, FWHT/sign handling, normalization, RoPE, full attention, Gated Delta Net, and convolution/state updates.
4. Log effective per-layer and per-operation placement. “GPU detected” and a high offload-layer count do not prove all expensive operations run there.
5. Qualify CPU-only, a few contiguous partial-offload placements, and maximum feasible offload. Keep attention/cache state close to its consumers; count synchronization and migration costs.
6. Qualify flash attention and quantized KV for the selected shape/type. Unsupported paths use an explicit supported profile rather than silently changing settings.

Prefer an intact, supported execution graph over frequent CPU/GPU crossings. Add backend-specific fusion or shader improvements only after an end-to-end profile identifies a bottleneck. Keep a CPU fallback that reports why the requested GPU profile could not start.

### Hexagon NPU feasibility gate

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

## 5. Engine and local harness design

### Keep the native engine upstream-based

Use **Prism llama-server plus its built-in web UI** as the first complete local harness. It provides a browser interface, streaming API, monitoring, and structured-output support without requiring a second model loader. The runtime is MIT-licensed; Swift weights carry Apache-2.0 terms and retained notices. [Server documentation][server-docs] · [Runtime license][runtime-license] · [Swift model][swift-model]

```text
Browser UI / optional coding client
                 |
       loopback HTTP + streaming
                 |
     Prism llama-server, one model
                 |
      ggml execution and memory
          /          |         \
     ARM64 CPU   Adreno Vulkan  Hexagon (future, gated)

PowerShell manager: verify -> select profile -> launch -> health -> stop
CI harness: same binaries + explicit profiles -> results + provenance
```

The repository should own reproducibility, profiles, lifecycle, focused tests, and any justified small upstream patches. Avoid a replacement scheduler, attention engine, or serving stack until evidence shows a concrete gap.

The manager should:

- Verify manifest hashes and architecture; resolve absolute paths and reject missing or incompatible dependencies.
- Select a named, validated profile. Show effective backend, context, KV types, checkpoint/cache budget, and any fallback reason.
- Start one model process, check readiness, expose its local UI, and stop only the process it owns.
- Queue additional requests by default, support streaming cancellation, and recover from process failure without repeated restart loops.
- Reject requests that exceed token/memory limits with an actionable message.
- Keep private prompts out of routine logs; retain technical configuration and error details separately.

Bind to `127.0.0.1` initially. Preserve an offline path once artifacts exist, without automatic cloud fallback or surprise model downloads. Keep optional tool execution in the client, with bounded tool output and visible file diffs.

### Optional coding harness

Use **Aider** as the first coding-client candidate because it can use an [OpenAI-compatible local endpoint][aider-api]. Pin its version, confirm ARM64 dependency installation, set the local model alias/context budget explicitly, and test its handling of reasoning and edit formats before recommending it for daily work. Start with ask/review and small edits.

Do not treat API connectivity as proof of useful coding-agent behavior. Swift's publisher reports uneven instruction following, tool use, and open-ended coding. Keep this a separate client acceptance gate. A richer chat frontend or another agent can later reuse the same endpoint without changing the engine.

### Model behavior contract

Preserve the embedded tokenizer/template and the publisher's generation settings: temperature 1.0, top-p 0.95, top-k 20, min-p 0, repetition penalty 1, presence penalty 0, and no added logit penalty. Preserve the documented thinking/template controls. [Publisher generation contract][swift-page]

For an initial interactive profile, propose a 4,096-token completion cap and make truncation visible; long reasoning tasks need a separately budgeted profile. Count reasoning tokens toward the total limit. Require:

```text
rendered prompt tokens + maximum generated tokens + template margin
    <= validated context capacity for this profile
```

The client must preserve roles/tool messages, stream reasoning and answer content correctly, and report finish reasons. Use deterministic settings only for diagnostics; do not silently replace the publisher's sampling defaults in quality evaluations.

## 6. Hybrid attention and cache design

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

Start with **eight context checkpoints and a 1,024 MiB server prompt-cache budget**, both proposed. Cap their combined observed footprint explicitly. `--cache-ram` does not cap the live KV arena or all checkpoint/rollback allocations. Compare smaller/larger checkpoint counts and spacing only after correctness, then in CI for latency/memory tradeoffs. Include a checkpoints-off profile for unrelated short requests: disabling request prefix reuse does not necessarily disable checkpoint work.

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

## 7. Memory and storage budgets

Use a provisional **20 GiB maximum service footprint** for the first daily profile, reserve roughly 8 GiB for Windows and other applications, and leave the remainder as headroom. Adjust after recording idle usage; these are policy budgets, not discovered device limits.

| Component | Proposed first daily-profile allowance | Notes |
| --- | ---: | --- |
| Packed weights plus unavoidable layout overhead | Up to 8 GiB | PQ2_0 file alone is about 6.71 GiB; detect duplicated/repacked copies |
| Live full-attention KV | Up to 2 GiB | Fits the conditional 32K FP16 estimate |
| Recurrent state and retained checkpoints | Up to 2 GiB | Begin with one sequence and eight checkpoints |
| Server prompt-cache storage | 1 GiB | Separate from live KV and checkpoints |
| Compute, staging, graph, and process overhead | Up to 3 GiB initially | Actual peak depends on backend and microbatch |
| Additional service headroom | About 4 GiB | Detect unexpected allocations before exhausting the machine |

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
| `long-context` | 64K first; larger only after acceptance / 1 | Q8 candidate; Q4 separately experimental | Best supported placement within budget | Documents and extended reasoning |
| `battery` | 8K–16K / 1 | Bounded reuse, no speculation | Selected by energy/task and responsiveness | Lower power use |
| `npu-experimental` | Explicitly bounded / 1 | Backend-specific validated state path | Only after NPU gate | Research, never silent default |

Initial batch/microbatch candidates are 256/64 for bring-up and 512/128 for qualification. Increase only with memory evidence; prefill and decoding have different bottlenecks. Explicitly set one slot rather than inheriting an automatic concurrency default.

Retain GPU kernel/shader caches supported by the chosen backend and invalidate them on relevant source/toolchain/device changes. Separate cold startup, warm model, and warm prompt-cache results. Do not count shader compilation as steady-state decode or hide it from startup measurements.

## 9. Speculative decoding and advanced optimization

Keep speculation off until ordinary execution, caching, and rollback are correct. The current [Bonsai speculative guide][speculation] says Bonsai 2 27B has no official DSpark drafter; previous-generation drafters and their speedups do not establish compatibility with Swift Bonsai 2.

Possible later experiments, in this order:

1. Confirm whether the exact Swift GGUF contains usable MTP/NextN tensors; base configuration alone is insufficient.
2. Qualify runtime/backend support and checkpoint/rollback semantics for each rejected draft.
3. Consider a compatible small external drafter only if tokenization and verification are sound.
4. Measure accepted tokens per verification, draft cost, target cost, state-copy cost, peak memory, and total latency together.
5. Recheck prefix-cache behavior and cross-request state after enabling the speculative path.

Preserve target sampling semantics where the algorithm claims exactness. Early-exit/approximate methods require a separately labeled quality-changing profile. A second model sharing the same memory bandwidth can slow the laptop even when its draft-token acceptance looks good.

Other potential work belongs behind profiles: packed CPU kernels, Vulkan fusion, graph reuse, persistent buffers, and carefully selected prefill/decode placement. Do not simultaneously vary packing, sampling, context, and device assignment and attribute the result to one optimization.

## 10. Validation and performance evidence

### Repository policy

Local builds, analyzer checks, and **focused correctness tests** are allowed. Full test suites run only in CI. **Every benchmark runs only in CI**, including single-case, smoke, debugging, short, and dry runs. Do not approximate a full suite with batches of local filters or disguise a benchmark as a diagnostic.

To measure this laptop, create a manually triggered self-hosted CI workflow on the Yoga, labeled for Windows ARM64 and this hardware. Running that job through CI preserves the benchmark-location rule while testing the real device. Generic hosted runners cannot supply its Adreno/NPU/thermal evidence.

The repository currently has no remote configured. Remote/runner setup is an implementation prerequisite for hosted workflows and laptop benchmark publication; no workflow dispatch is part of this planning delivery. Keep the runner available only to trusted code, with inference caches and credentials scoped appropriately.

### Focused local correctness checks

- Verify a pinned model loads with the intended tensor types and chat template; generate a short bounded response.
- Exercise streaming, cancellation, context rejection, orderly shutdown, and process restart.
- Compare a selected packed kernel to a dequantized/reference calculation, including zero/extreme inputs, block boundaries, and awkward shapes.
- Compare the same request with fresh prefill versus prefix reuse, including recurrent-state restoration after an edited turn.
- Check one targeted long-prompt regression or backend failure at a time when its change requires it.

Numerical comparisons use explicit dtype-appropriate tolerances. Greedy token agreement is useful but insufficient; small floating-point changes can alter a sampled sequence. Compare selected logits, finite outputs, and task correctness. New quantized cache/weight representations need quality evaluation, not a demand for bit-identical output.

### CI workload matrix

Use native `llama-bench` where appropriate plus a small repository-owned streaming HTTP runner for user-visible latency/cache behavior. Add [EleutherAI's evaluation harness][lm-eval-api] through its local API adapters for compatible tasks; validate templates, scoring, tokenization, and required log probabilities before relying on it.

| Dimension | Initial representative cases |
| --- | --- |
| Prompt size | 512, 2K, 8K, 16K; then 32K/64K for accepted profiles |
| Output | Fixed-length 128/512-token performance cases; separate natural task completions |
| Cache | Fresh process, warm model/no reusable prompt, repeated prefix, edited prefix, conversation switch |
| Backend | CPU; selected partial/full Vulkan placements; NPU only after its correctness gate |
| KV | FP16; accepted Q8; experimental Q4 with exact calibration provenance |
| Concurrency | One active request first; two only as a separate throughput experiment |
| Power | Plugged-in performance and a separately recorded battery profile |
| Thermal | Short interactive bursts plus 15–30-minute sustained runs |

Avoid a full Cartesian sweep. Establish a baseline, screen thread/batch choices, compare feasible device placements, then qualify cache choices on the best candidates. Keep CPU/GPU comparisons on the same Swift artifact; measure alternate-weight PTQ separately.

Record:

- Load/readiness time, shader/kernel compilation, cold and warm time to first token, and first answer latency.
- Prompt tokens processed versus reused, prefill rate, decode rate at several occupied-context lengths, and total request time.
- Median and p95 across sufficient repetitions; increase samples when variance prevents a conclusion. Randomize/interleave candidates and separate first-run warmup.
- Peak and sustained memory, commit, hard faults, GPU evictions, and allocation/repack sizes.
- Power state, clocks/thermal throttling where observable, fan behavior notes, and energy per completed task where valid telemetry exists. Mark missing telemetry as unavailable.
- Quality, finish reasons, error/cancellation behavior, speculative acceptance if enabled, and client-visible truncation.
- Every artifact/version/configuration identity plus CI run URL, tested revision, and verdict.

Use held-out reasoning, instruction-following, coding/edit, JSON-format, and long-context retrieval cases. Include evidence at early/middle/late context positions and multi-turn edits. Benchmark correctness includes failures and truncation; do not discard them from the score.

### Promotion criteria

Promote a configuration only when:

1. Focused state/streaming/cancellation checks and the required CI suites pass.
2. It stays within memory budgets without sustained paging or eviction and remains usable through the sustained workload.
3. It has no unresolved critical task failure and its quality difference is within a predeclared evaluation tolerance. Report uncertainty when sample size cannot establish this.
4. Paired CI measurements establish a useful improvement. Use **10% lower end-to-end latency** as an initial promotion target, or a documented capacity/energy benefit without unacceptable interactive regression.
5. The manifest and effective profile reproduce the outcome on the settled `main` revision.

Choose among a small Pareto set: fastest interactive, best long-context capacity, and most efficient battery behavior. No finite tuning pass proves a global optimum. A source-supported accelerator without correct device results remains experimental; an NPU no-go decision can still leave a complete, useful CPU/GPU product.

## 11. Proposed repository implementation layout

This is a future layout; only this Markdown plan is being delivered now.

```text
PLAN.md
README.md                         installation and daily use
config/runtime.lock.json          runtime/model/toolchain identities
config/profiles/*.json            explicit validated settings
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
```

Keep third-party source at pinned revisions without copying an untracked engine into the repository. Model packs and personal conversation caches stay outside Git. Build outputs use documented artifact lanes; disposable diagnostics live in temp. A tool should fail clearly if a required native component is absent rather than installing arbitrary replacements.

During implementation, publish only `main`. Give every delegated worker a dedicated worktree, retain useful evidence, merge all session changes back to local/remote `main`, run required validation on the settled revision, and remove eligible session worktrees/branches. Preserve other sessions' work.

## 12. Delivery milestones and stop/go decisions

Effort ranges below are planning estimates for one engineer after dependencies are available. Driver, SDK, or kernel gaps can expand them; there is no implied delivery date.

| Milestone | Approximate effort | Concrete output | Exit condition |
| --- | --- | --- | --- |
| M0: inventory and reproducibility | 0.5–1 day | Device report, GGUF metadata, immutable manifests, toolchain/CI design | Exact architecture and dependency gaps known |
| M1: CPU + usable local harness | 1–3 days | Native engine, web UI, launcher, pinned model, focused regressions | Correct bounded chat, cancellation, offline restart |
| M2: Vulkan qualification | 2–5 days if existing kernels work | Native GPU build, placement audit, CI CPU/GPU comparison | Correct graph and demonstrated benefit, or documented no-go |
| M3: cache and context qualification | 2–4 days | Hybrid-state tests, memory estimator, bounded reuse, accepted KV types | Fresh/reused continuations agree; selected contexts fit and retain quality |
| M4: profiles and performance report | 2–4 days | Daily/long/battery profiles, CI provenance, installation guide | Sustained and task-level acceptance on settled main |
| M5: NPU feasibility | About 2 days initially | Operation/packing/memory gap report and decision | Feasible bounded implementation or explicit deferral |
| M6: advanced kernels/speculation | Separately estimated | Focused patch or experimental profile | Correctness and CI benefit justify maintenance cost |

M0 -> M1 -> M2/M3 -> M4 is the main delivery path. Investigate M5 after the CPU/GPU path exposes realistic bottlenecks; do not make everyday usability depend on a speculative NPU port. A substantial custom ternary NPU backend is a multiweek project to scope after the feasibility report, not a two-day integration.

The first future implementation step should be **M0 plus M1**: pin the model/runtime, inspect native capabilities, establish an honest CPU reference, and make the local UI usable. That produces the correctness reference needed to assess every later optimization.

## 13. Risk register and explicit open questions

| Risk / question | Resolution |
| --- | --- |
| Does current Swift metadata exactly match the upstream dimensions? | Read the selected GGUF before allocating caches or accepting the estimates |
| Does native ARM64 Vulkan work correctly on the installed Adreno driver? | Native dependency check, operation tests, whole-model comparison, device CI |
| Is CPU or GPU faster for the actual context/task? | Paired self-hosted CI; retain both validated profiles |
| Do generic backend types omit a Bonsai transform or force CPU fallback? | Audit full graph and placement, not just the quantization decoder |
| Can custom ternary weights execute efficiently on Hexagon? | Time-boxed format/operator/memory investigation; separate port decision |
| Does the smaller PTQ release alter quality? | Attribute it as a different Swift version; compare held-out tasks separately |
| Does prefix reuse restore incorrect recurrent state? | Fresh-prefill comparison after edits, cancellation, eviction, and slot switching |
| Do checkpoint/rollback copies overwhelm RAM? | Separate caps, actual allocation accounting, one sequence initially |
| Does Q8/Q4 KV impair long-context reasoning or retrieval? | Held-out task evaluation with exact calibration and runtime identity |
| Does an accelerator win briefly but throttle or stall Windows? | Sustained device CI and responsiveness/memory criteria |
| Does the client silently change templates, sampling, or token limits? | Capture rendered prompts/effective request settings in controlled fixtures |
| Are coding/tools reliable enough to recommend? | Separate client task acceptance; start with reviewable small edits |
| Is speculative decoding actually compatible and beneficial? | Exact tensors/drafter identity, rollback tests, cache tests, CI end-to-end gain |
| Can the result be reproduced after an upstream update? | Pin all artifacts, retain baseline profile, qualify upgrades deliberately |

## 14. Primary sources and freshness

Sources were checked on October 22, 2026. Moving branch documentation is research evidence; implementation must retain immutable revisions and recheck device support. This plan distinguishes publisher statements, source-level backend support, conditional arithmetic, and proposed settings; none is a measured laptop result.

- [UkisAI release page][swift-page] and [Swift artifact/model instructions][swift-model].
- [Prism base Bonsai 2 architecture][bonsai-model] and [upstream Qwen configuration][qwen-config].
- [Prism backend/format audit][backend-support] and [candidate runtime release][runtime-release].
- [Windows ARM64 OpenCL guide][opencl-guide] and [Snapdragon/Hexagon guide][snapdragon-guide].
- [llama-server interface/options][server-docs], [Qwen35 graph][qwen-implementation], and [recurrent allocation implementation][recurrent-implementation].
- [Prompt reuse/checkpoints][prompt-cache], [experimental KV4][kv-cache], and [speculative compatibility][speculation].
- [Aider local API connection][aider-api] and [lm-evaluation-harness local API adapters][lm-eval-api].

[swift-page]: https://ukisai.com/swift-bonsai-2
[swift-model]: https://huggingface.co/ukisai/Swift-Bonsai-2-GGUF
[bonsai-model]: https://huggingface.co/prism-ml/Ternary-Bonsai-2-27B-gguf
[qwen-config]: https://huggingface.co/Qwen/Qwen3.8-27B/blob/main/config.json
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
[lm-eval-api]: https://github.com/EleutherAI/lm-evaluation-harness/blob/main/docs/API_guide.md
