# Performance expectations and benchmark context

The [README charts](../README.md#performance-expectations) reproduce publisher results on
zero-based axes. They are reference points, not Runner measurements.

## Published reference results

The speed chart uses base Bonsai 2 PQ2 `llama-bench` TG128: 128 generated tokens, batch 1, depth 0,
no vision tower. The quality chart uses the publisher's 14-benchmark thinking-mode average,
evaluated with EvalScope/vLLM on H100; it includes vision tasks. Neither chart evaluates Swift or
our Qwen Q3 reference. Source: [Prism's model
card](https://huggingface.co/prism-ml/Ternary-Bonsai-2-27B-gguf#cross-platform-throughput), checked
October 23, 2026.

Other-hardware throughput and general benchmark averages predict neither Snapdragon speed
nor prose quality. Measure useful-answer time under the protocol below.

## Current Swift evidence

| Publisher evidence | What it establishes |
| --- | --- |
| Current merged PQ2 | IFBench and AIME were refreshed; AIME final-answer-only ties the base. |
| Historical runtime correction | GPQA/C-Eval were not rerun on current merged weights. |
| Evaluation configuration | Supports the [sampler baseline](inference-defaults.md#applied-defaults), without a sampler/effort/prompt sweep or creative-writing comparison. |

Small score differences are not established gains; token counts do not measure latency.
The publisher reports uneven instruction following, tool use and open-ended coding. See
[Swift's evaluation and limitations](https://ukisai.com/swift-bonsai-2#evaluation) and its
[evaluation artifact](https://huggingface.co/ukisai/Swift-Bonsai-2-GGUF/blob/main/benchmark_results.json).

The [Swift artifact/memory contract](swift-memory.md) records bounded 16-token
Prism CPU decode and current-weight PTQ1 results; the [Rust runtime contract](native-rust-engine.md)
owns its NEON/hybrid-graph evidence. These checks do not establish complete-answer
performance. [PLAN.md](../PLAN.md) owns outstanding release/profile qualification.

Snapdragon TPS, sustained comfort, creative quality, GPU/NPU profiles, daily-service isolation
and actual-model application acceptance remain unqualified. Follow [validation policy](validation.md)
and the independent [LoRA performance/quality gates](swift-lora.md#standard-profile-qualification).

## X1/X2 configuration evidence

[Hardware policy](hardware-capabilities.md#automatic-profile-selection) owns selection, admission, phase handoff and qualified CPU
fallback/refusal. No X2 speedup or automatic selector is accepted. Family/core counts, NPU TOPS and
bandwidth bounds cannot forecast TPS or energy: prefill can reuse weight tiles, while single-chat
decode pays bandwidth, unpack/transform and dispatch costs with little reuse. Whole-model expansion
can erase packing benefits; simultaneous processors share bandwidth and power.

Compare CPU/Adreno/Hexagon and phase combinations separately for each SKU/driver/runtime, using
the measurement protocol below. Interactive/battery rankings require matching-device evidence;
focused untimed discovery/selection checks cannot establish them.

## 2026 literature: measurement and comparison

The [systems catalog](research-register.md#2026-systems-measurement-and-output-budget-work) records
sources reviewed **2026-10-29** and their platform limits:

| Source | Measurement lesson and limit |
| --- | --- |
| MLPerf Client v2.0 | Separate TTFT/token rate, vary prompt/output lengths and retain accuracy gates. Windows Arm/X/X2 coverage uses other models/quantizers; experimental power tools and general accuracy do not qualify Swift creative quality. |
| Sequential Isolation Protocol; MLSys deployment search | Repeat controlled phase-specific comparisons. A100/vLLM variance and deployment results supply neither laptop thresholds nor a universal batch size. |
| Edge-continuum study | Compare latency/energy tradeoffs; its Jetson/server frontier is not a Snapdragon CPU verdict. |

For `N >= 2` output tokens, define the interval rate as `r = (N-1)/(t_last-t_first)` over a positive
measured interval at the same observation boundary. Then `last-token latency = TTFT + (N-1)/r` is an
identity; arithmetic averages of per-token or per-run rates need not satisfy it. For one token,
last-token latency is TTFT. Terminal framing, flushing and teardown may add time afterward and have
separate endpoints. Reasoning tokens, buffered delivery and speculative bursts make *first output*,
*first visible answer* and *first useful answer* distinct observations. A client burst rate is not
the device decode rate.

| Comparison requirement | Record |
| --- | --- |
| Paired repeated trials | Reverse or balance order; identify cold/warm/prefix-cache state; hold artifacts, adapter, sampler and numerical contracts fixed. Retain complete output or label truncation. |
| Phase and task costs | Load/initialization, repack/compile, migration, prefill, first useful answer, decode and complete task. Charge each cost to the workload that pays it. Use profiled runs for attribution and unprofiled runs for request timing. |
| Memory | Persistent payload, temporary peak, private/commit, file-backed residency, device memory and machine headroom. Identify excluded reconstruction scratch or retained high-precision copies before comparing compression ratios. |
| Energy and comfort | Observable energy per useful completed answer, output/retry/revision counts, desktop responsiveness and idle power. Cheaper tokens can cost more per useful answer. |

Stable TPS can coexist with worse foreground behavior under shared bandwidth/power contention;
see [mobile bandwidth research](hexagon-npu-plan.md). Keep current admission limits;
timing/profile captures and benchmarks remain [CI-only](validation.md).

## Q4 speech resource comparison

The [paired hosted ARM64
run](https://github.com/symbiogenesis/SwiftBonsai2Runner/actions/runs/40445129576) at Runner
revision `90db7f71033adeb2e262ec6f97f7dc360fceb9c8` completed all eight fresh-process sessions and
sixteen bootstrap/warm WAV records on a four-core Cobalt 100. Independent framing checks passed for
every clip. No spontaneous worker exits occurred; all termination codes were observed after
supervisor stop.

Positive differences below mean lower Q4 usage. Counter domains are separate.

| Threads | Repetition | Q8 minus Q4 peak Job commit | Q8 minus Q4 peak working set |
| ---: | ---: | ---: | ---: |
| 2 | 1 | 0.734196 GiB | 1.532074 GiB |
| 2 | 2 | 0.734276 GiB | 1.532158 GiB |
| 4 | 1 | 0.734226 GiB | 1.532124 GiB |
| 4 | 2 | 0.734653 GiB | 1.532566 GiB |
| Overall median | | **0.734251 GiB** | **1.532141 GiB** |

The active talker file difference is 860,203,008 bytes (0.801126 GiB). Design and Base talkers are
sequential, so their reductions count once for active weights. The Q8 codec is unchanged. The larger
observed working-set reduction does not identify its allocation cause. Job commit excludes mapped
residency; neither counter is added to the other or treated as complete system RAM.

Phase private/resident maxima were sampled every 25 ms and can miss brief allocations. The lowest
sampled system availability was 7.015308 GiB, above the unchanged 5 GiB reserve. Admission remained
9 GiB with a 16 GiB commit ceiling. These results describe this hosted CPU and short public
fixtures. Snapdragon, long-clip and independent listening qualification remain outstanding; Q8 stays
the default. Work items remain in [PLAN.md](../PLAN.md).

The report SHA-256 is `9b9aecf03a2c0a2467a3b3c8f316c4ee0c2c237dbe67e22975fa5c9b07285eb4`. The TTS
executable was 1,914,368 bytes, SHA-256
`2ac52dddc913ca4e193a2614262d154bd63632cc12a23521c8f2cdfc9e4999d8`. Artifact `12708687596` retains
the exact model identities, individual observations, failure diagnostics and synthetic listening
clips.

## 2026 speech inference research

The [mobile/speech catalog](research-register.md#2026-mobile-acceleration-and-speech) records
sources reviewed **2026-10-29**. Their implications below retain the
[speech contract](../tools/qwen-tts/README.md): sequential VoiceDesign/Base, reused conditioning,
bounded playback and existing admission/reserve. No model, quantization, thread or accelerator
change is qualified by these papers.

| Lead | Decision boundary |
| --- | --- |
| Qwen3-TTS report | Separate talker, residual-code predictor and causal codec invocation rates/first-packet prerequisites. Publisher 97 ms is not this 1.7B GGML CPU path, fixed-reference bootstrap or audible playback; 12Hz/25Hz tokenizers differ. |
| RVQ position-aware speculation | Frozen heads plus 3,072 learned offsets/sparse verification introduce artifacts. Reported 2–2.2× RVQ-token speed is 0.6B CustomVoice/Apple, not 1.7B VoiceDesign/Base GGML/full readout; distributional equality is not seeded-audio identity. |
| One Megakernel Is Not Enough | Producer/codec rates, registers, retained resources and first-audio dependencies guide fusion boundaries. H100/CUDA measured-plan choices are not a portable kernel or laptop forecast. |
| On-device streaming ASR | Chunk/right-context, caching, selective quantization/fusion trade latency/quality/storage. 32-core EPYC, Nemotron/Canary-1B-v2 results are not Canary-Qwen-2.5B; streaming needs compatible trained state, and smaller files do not prove smaller processes. |
| Qwen3-TTS conditioning/failure study | Listening must evaluate timbre, timing, continuity and prosody; automatic metrics/WER can disagree. Cloning results do not qualify description-designed references. Keep reference/seed fixed while isolating optimization. |

Attribute voice bootstrap, warm talker prefill, residual-code steps, codec chunks and playback
before optimizing. Conditioning reuse and bounded retained decode graphs already exist.
Workspace/fusion changes retain cancellation, PCM framing and voice identity; speculation needs
accepted-prefix state, sampling correction and bounded scratch. Even a tiny trained drafter is a
separate artifact.

Measure first generated PCM separately from first audible output, chunk supply gaps/underruns,
long-read peak memory and combined chat/speech use in target-device CI. Average
faster-than-real-time synthesis can still stall individual chunks, while overlap contends with
Swift/native rendering. Framing/ownership checks do not replace these measurements or independent
listening.
