<div align="center">

# 🌱 Swift Bonsai 2

**A local creative AI assistant for Windows on ARM.**

Creative writing · Brainstorming · Art direction

![Platform: Windows ARM64](https://img.shields.io/badge/Windows-ARM64-0078D4?style=flat-square)
![Status: early development](https://img.shields.io/badge/status-early_development-D99A28?style=flat-square)
[![License: Apache 2.0](https://img.shields.io/badge/license-Apache_2.0-50855B?style=flat-square)](LICENSE)

[Get started](docs/development-guide.md) · [Documentation](#documentation) · [Roadmap](PLAN.md)

</div>

---

Swift Bonsai 2 targets **Snapdragon X Elite laptops**, bringing the Swift Bonsai 2
language model to a local Windows ARM64 assistant.

> [!IMPORTANT]
> **Early development.** Model setup, CPU checks, and a development API are
> available. Everyday chat and a user interface are still being built.

## At a glance

| | Current target |
| --- | --- |
| **Hardware** | Snapdragon X Elite · 32 GB RAM |
| **Model** | Swift Bonsai 2 · `PQ2_0` |
| **CPU profile** | One conversation · 8K-token context · FP16 attention caches |
| **Next milestones** | Everyday chat, a user interface, and quality/performance qualification |

## Memory footprint

| Model | Model file | Development budget | Free RAM to start |
| --- | ---: | ---: | ---: |
| **Swift Bonsai 2 `PQ2_0`** | **6.71 GiB** | **12 GiB** | **16 GiB** |
| Qwen3.8-27B `UD-Q3_K_XL` | 12.24 GiB | 18 GiB | 22 GiB |

Swift's pinned model file is **about 45% smaller** than the Qwen reference. The
budgets above are configured development allowances, not measured peak usage.

> [!NOTE]
> Startup requires **currently available physical RAM**, including a 4 GiB safety
> reserve. A machine with 16 GB installed does not meet the current Swift
> requirement. Longer conversations, caching, and other applications need
> additional headroom.

See the [Swift memory assessment](docs/swift-memory.md) and
[Qwen reference accounting](docs/model-selection.md#source-only-compatibility-and-memory-sanity-check).

## Performance expectations

### Response speed

Compact weights can reduce memory traffic, but response speed also depends on
the hardware, runtime and prompt length. **Swift's Snapdragon tokens/second have
not been measured yet.** For context, these are Prism's published results for
the **base Bonsai 2 model**, using PQ2 weights on other hardware:

![Base Bonsai 2 PQ2 generation speed: Apple M5 Pro 28.1, RTX 4090 81.2 and RTX 5090 129.9 tokens per second.](docs/assets/reference-throughput.svg)

[Source: Prism throughput benchmarks](https://huggingface.co/prism-ml/Ternary-Bonsai-2-27B-gguf#cross-platform-throughput).
These are reference points, not a Swift or Snapdragon speed forecast. Thinking
time and long prompts also affect how soon a useful answer arrives.

### Answer quality

Prism reports **84.78** for base Bonsai 2 versus **86.32** for full-precision
Qwen3.8-27B across 14 thinking-mode benchmarks. The chart shows how that compares
with conventional compressed versions of the same Qwen base:

![Published benchmark averages: Qwen FP16 86.32, Qwen Q4 85.18, base Bonsai 2 84.78 and Qwen IQ2 72.59, on a zero-to-100 scale.](docs/assets/answer-quality.svg)

[Source: Prism benchmark results](https://huggingface.co/prism-ml/Ternary-Bonsai-2-27B-gguf#benchmarks).
These scores cover **base Bonsai, not Swift**, and do not measure creative-writing
quality. Swift's own publisher reports [uneven general-purpose behavior](https://ukisai.com/swift-bonsai-2#research-status-and-limitations).
Expect an experimental writing partner; prose quality on this build still needs
evaluation. See [benchmark context and current limits](docs/performance-expectations.md).

## Documentation

### Start here

| Guide | What you'll find |
| --- | --- |
| [Development guide](docs/development-guide.md) | Requirements, setup, and development commands |
| [Validation](docs/validation.md) | Code standards, focused tests, and CI |
| [Implementation plan](PLAN.md) | Goals, design, and remaining work |

### Runtime and acceleration

| Guide | What you'll find |
| --- | --- |
| [Native Rust CPU reference](src/prism-runtime/README.md) | Experimental token-level Prism execution and remaining parity gates |
| [CPU SIMD plan](docs/simd-plan.md) | Packed NEON kernels, numerical contracts, safe dispatch, and qualification |
| [Adreno GPU plan](docs/adreno-gpu-plan.md) | CPU-only status, native Vulkan bring-up, placement, and device qualification |
| [Hexagon NPU plan](docs/hexagon-npu-plan.md) | Signed runtime access, exact Swift kernels/state, placement, and qualification |

### Research

| Guide | What you'll find |
| --- | --- |
| [Model and memory research](docs/swift-memory.md) | The current Swift assessment and experiments |
| [Inference algorithms](docs/inference-algorithms.md) | LUT/prefill/cache opportunities, mathematical limits, and experimental boundaries |
| [Implementation decisions](docs/implementation-research.md) | Design rationale and evidence limits |

---

## License

Original project code, documentation, and configuration are licensed under the
[Apache License 2.0](LICENSE). Third-party code retains its own terms, including
the MIT-licensed Prism/ggml portions identified in [NOTICE](NOTICE).

Model weights and adapters are separate downloads governed by their publishers'
licenses. See the [license decision and dependency review](docs/licensing.md)
for scope, attribution, and redistribution requirements.
