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
and [Rust roadmap](native-rust-engine.md) for bounded source evidence and remaining
release/profile acceptance.

Snapdragon TPS, sustained comfort, creative quality and accepted GPU/NPU profiles
remain unqualified. Daily chat also needs UI and isolation acceptance. Follow
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
