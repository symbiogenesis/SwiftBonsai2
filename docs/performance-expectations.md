# Performance expectations and benchmark context

The [README](../README.md#performance-expectations) uses publisher benchmarks
to explain the model's tradeoffs. Its charts use zero-based axes and reproduce
reported values, not measurements from this project.

## Published reference results

The speed chart uses base Bonsai 2 PQ2 `llama-bench` TG128: 128 generated tokens,
batch 1, depth 0, no vision tower. The quality chart uses the publisher's
14-benchmark thinking-mode average, evaluated with EvalScope/vLLM on H100;
it includes vision tasks. Neither chart evaluates Swift or our Qwen Q3 reference.
Source: [Prism's model card](https://huggingface.co/prism-ml/Ternary-Bonsai-2-27B-gguf#cross-platform-throughput),
checked October 23, 2026.

Do not infer Snapdragon speed from other hardware, or prose quality from a
general benchmark average. Compare time to a useful answer, including prompt
processing, thinking, retries and revisions, rather than decode throughput alone.

## Current Swift evidence

Swift's publisher reports current PQ2 results for IFBench and AIME; the older
GPQA/C-Eval runtime-correction results were not rerun on current merged weights.
Small score differences are not established gains, and token-count changes do
not measure latency. The publisher describes uneven instruction following,
tool use and open-ended coding. See [Swift's evaluation and limitations](https://ukisai.com/swift-bonsai-2#evaluation).

The pinned Prism CPU runtime completes a bounded 16-token Swift decode check.
Current-weight PTQ1 also decodes and saves 1.173 GiB of file payload, but uses
different activation arithmetic. The experimental Rust engine has selected
NEON kernels and a hybrid graph. See the [memory assessment](swift-memory.md)
and [runtime contract](native-rust-engine.md) for bounded source evidence.
Outstanding release/profile qualification lives in the [central TODO](../PLAN.md).

Snapdragon TPS, sustained comfort, creative quality and accepted GPU/NPU profiles
remain unqualified. The [pure-Rust native UI](ui-architecture.md) supplies a chat
workspace with conversation recovery and search; daily-service isolation and
real-model acceptance remain separate requirements. Follow
[validation policy](validation.md) for focused checks, CI-only benchmarks and
the separate standard LoRA performance/quality gates.

## X1/X2 configuration evidence

The [hardware profile plan](hardware-profiles.md) targets dynamic selection of
the best qualified configuration for the detected Snapdragon X1 or X2 device
and workload. There is no measured X2 speedup or accepted automatic selector
today. Family names, core counts and NPU TOPS do not establish model throughput,
energy savings or a placement winner.

Treat prefill and decode separately. Batched prefill can reuse packed weight
tiles across prompt tokens and benefit from matrix compute. Single-conversation
decode has less weight reuse and is sensitive to memory bandwidth, unpacking,
transforms and dispatch costs. Preserve packed weights in the memory ledger;
expanding the whole model to a supported wider type can erase its traffic and
capacity advantage. A bandwidth calculation is an explanatory bound under its
stated assumptions, not a token-rate forecast.

Qualify CPU, Adreno, Hexagon and viable phase-specific combinations independently
on each supported family/SKU and driver/runtime compatibility record. Include
load/repack/compile, state migrations, prefill, first useful answer, sustained
decode, complete-task latency, peak memory and energy when valid telemetry is
available. Compare interactive and battery objectives with the same artifact,
adapter and numerical contracts. More simultaneous processors can compete for
the same memory and power budget.

The future selector will choose among those qualified entries using actual
capabilities, available RAM and workload bounds, with deterministic supported
CPU fallback or admission refusal. Cross-generation extrapolation does not
qualify an entry. All benchmarks and timing/profile captures remain target-device
CI work under the existing validation policy; focused untimed local checks can
verify capability and selection behavior.

## 2026 literature: measurement and comparison

Source review: 2026-10-29. The
[research register](research-register.md) separates paper mechanisms from
qualified profiles. The following sources refine what an experiment should
measure; they supply no Swift throughput forecast.

| Source | Useful comparison practice | Scope limit |
| --- | --- | --- |
| [MLPerf Client v2.0 documentation](https://mlcommons.org/benchmarks/client/), reviewed 2026-10-29 | Separate TTFT from subsequent-token throughput, vary prompt/output lengths and retain accuracy gates. Includes creative-writing workloads and Windows Arm/Snapdragon X/X2 coverage. | Different models, quantizers and acceleration paths. Neither its scores nor its MMLU/IFEval checks establish Swift creative quality or qualify Runner. Power tooling is labelled experimental. |
| [Sequential Isolation Protocol](https://arxiv.org/abs/2610.09778v1), 2026-10-07 preprint | Control system/execution state and repeat comparisons; report variation rather than one favorable run. | Three models on A100/vLLM with multiple concurrency levels. Its variance figures and saturation point are not target-laptop thresholds. |
| [Optimizing Deployment Configurations, MLSys 2026](https://proceedings.mlsys.org/paper_files/paper/2026/hash/97dc07f1253ab33ee514f395a82fa7cc-Abstract-Conference.html) | Search configurations subject to latency constraints, separating prefill/decode, architecture and hardware effects. | Production-scale Llama/GPU configurations. Its design-space findings motivate per-device evidence, not a universal parallelism or batch choice. |
| [Edge-continuum measurement study](https://arxiv.org/abs/2609.08307v1), 2026-09-08, paper lists WIMS 2026 acceptance | Evaluate quality, prefill/decode, energy and delivery overhead together on a Pareto frontier. | Jetson/server QA study; weight-file size and parameter count were weak predictors. Its CPU result does not establish that CPU inference is dominated on Snapdragon. |
| [Energy per completed task](https://arxiv.org/abs/2610.32417v2), 2026-10-26 revision | Track completed-task energy and output/retry counts alongside energy per token. | Agent/GPU experiment, not a creative-writing score. A cheaper token can still produce a more expensive task. |

For `N >= 2` output tokens, define the interval rate as
`r = (N-1)/(t_last-t_first)` over a positive measured interval at the same
observation boundary. Then `last-token latency = TTFT + (N-1)/r` is an identity;
arithmetic averages of per-token or per-run rates need not satisfy it. For one
token, last-token latency is TTFT. Terminal framing, flushing and teardown may
add time afterward and have separate endpoints. Reasoning tokens, buffered delivery and
speculative bursts make *first output*, *first visible answer* and *first useful
answer* distinct observations. A client burst rate is not the device decode rate.

Retain paired repeated trials with order reversal or balanced ordering, clear
cold/warm/prefix-cache state, identical artifacts/adapter/sampler, and complete
output or explicitly labelled truncation. Profiled runs explain attribution;
unprofiled runs establish the corresponding request time. Include initialization,
repack/compile and state-migration costs in the workload that actually pays them.

Memory reports distinguish persistent payload, temporary peak, private/commit,
file-backed residency, device memory and machine headroom. Compression papers
often report only persistent KV/state and omit reconstruction scratch or the
original high-precision copy. Record those exclusions before comparing ratios.
Desktop responsiveness and idle power also matter: the
[mobile bandwidth research](hexagon-npu-plan.md) explains why stable model TPS
can coexist with worse foreground behavior. Actual profiles retain the current
memory admission and [CI-only validation](validation.md) boundaries.
