# Swift Bonsai 2

A project to run a local AI assistant on Windows ARM64 laptops, targeting
Snapdragon X Elite. It focuses on creative writing, brainstorming, and art
direction using the Swift Bonsai 2 language model.

**Early development:** model setup, CPU checks, and a development API are
available. Everyday chat and a user interface are still being built.

## Memory requirements

The development target is a **32 GB Snapdragon X Elite laptop**. The current
CPU profiles allow one conversation with an 8K-token context and FP16 attention caches:

| Model | Model file | Development memory allowance | Available RAM required to start |
| --- | ---: | ---: | ---: |
| Swift Bonsai 2 `PQ2_0` | 6.71 GiB | 12 GiB | 16 GiB |
| Qwen3.8-27B `UD-Q3_K_XL` | 12.24 GiB | 18 GiB | 22 GiB |

Swift's pinned model file is **about 45% smaller** than the Qwen reference.
The startup requirements include a 4 GiB safety reserve; they refer to
**currently available physical RAM**, so a machine with 16 GB installed does not
meet the current Swift requirement. The allowances are configured development
budgets, not measured peak usage. Longer conversations, caching, and other
applications need additional headroom. See the [Swift memory assessment](docs/swift-memory.md)
and [Qwen reference accounting](docs/model-selection.md#source-only-compatibility-and-memory-sanity-check).

## Performance expectations

- **Response speed:** smaller packed weights may speed up generation when
  [memory bandwidth is the bottleneck](https://huggingface.co/prism-ml/Ternary-Bonsai-2-27B-gguf#cross-platform-throughput).
  This is an expectation to test, and
  unpacking costs and runtime kernels can offset the saving. Snapdragon
  tokens/second and a direct speed comparison with Qwen3.8 have not been
  measured. Swift has passed a bounded 16-token CPU decode check; sustained
  response speed and GPU/NPU acceleration remain to be qualified.
- **Answer quality:** [Prism's published evaluation](https://huggingface.co/prism-ml/Ternary-Bonsai-2-27B-gguf#benchmarks)
  gives the base Bonsai 2 model an average of 84.78 versus 86.32 for
  full-precision Qwen3.8-27B across 14 thinking-mode benchmarks. These results
  concern the base model, not Swift or the Q3 pack above. Creative-writing
  quality and instruction following still need evaluation on this project's
  exact build; [Swift's publisher](https://ukisai.com/swift-bonsai-2#research-status-and-limitations)
  describes its general-purpose behavior as uneven.

## Documentation

- [Development guide](docs/development-guide.md) — requirements, setup, and development commands.
- [Validation](docs/validation.md) — code standards, focused tests, and CI.
- [Native Rust CPU reference](src/prism-runtime/README.md) — experimental token-level Prism execution and remaining parity gates.
- [CPU SIMD plan](docs/simd-plan.md) — staged packed NEON kernels, numerical contracts, safe dispatch, and qualification.
- [Hexagon NPU plan](docs/hexagon-npu-plan.md) — signed runtime access, exact Swift kernels/state, CPU/GPU/NPU placement, and device qualification.
- [Model and memory research](docs/swift-memory.md) — the current Swift assessment and experiments.
- [Implementation plan](PLAN.md) — goals, design, and remaining work.
- [Implementation research](docs/implementation-research.md) — technical decisions and evidence.

## License

Original project code, documentation, and configuration are licensed under the
[Apache License 2.0](LICENSE). Third-party code retains its own terms, including
the MIT-licensed Prism/ggml portions identified in [NOTICE](NOTICE).

Model weights and adapters are separate downloads governed by their publishers'
licenses. See the [license decision and dependency review](docs/licensing.md)
for scope, attribution, and redistribution requirements.
