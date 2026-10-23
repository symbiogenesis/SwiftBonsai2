# Swift Bonsai 2 on Snapdragon X Elite

Research and implementation plan for a native Windows ARM64 inference engine and local open source harness.

**Status:** planning only; no runtime, model download, driver change, or performance experiment has been performed.

**Target:** Snapdragon X Elite X1E-78-100, Adreno X1-85, 32 GB shared system RAM.

**Research date:** October 22, 2026.

## Direction

Build on Prism ML's llama.cpp fork, establish a native ARM64 CPU correctness baseline, and qualify Adreno GPU and Hexagon NPU execution against the exact Swift GGUF tensor formats before enabling offload. Choose the fastest correct configuration for the actual workload rather than requiring all processors to run simultaneously.

The completed plan will specify runtime and harness choices, backend feasibility gates, hybrid attention and KV-cache policy, memory budgets, staged delivery, and reproducible validation under this repository's CI-only benchmark policy.
