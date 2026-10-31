# Documentation

Start with the [development guide](development-guide.md) for setup, launch and
developer commands, or [portable packages](chat-packages.md) to run without a
checkout. [PLAN.md](../PLAN.md) is the single backlog; these guides own current
behavior, technical contracts and retained evidence.

The current application uses a native Rust UI, Windows ARM64 CPU inference and
a strict local HTTP/JSON/SSE subset. Experimental runtimes, formats and
accelerators require separate qualification. Evidence scopes below distinguish
implemented behavior from actual-model, device, quality and isolation acceptance.

## Use and develop

| Guide | Contents |
| --- | --- |
| [Development](development-guide.md) | Requirements, provisioning, app/settings/speech, developer commands, build caches and runtime lock updates |
| [Portable packages](chat-packages.md) | Assembly, installation, update, rollback and installed launch |
| [Validation](validation.md) | Local-check policy, CI ownership, standards, regression coverage, acceptance and promotion |
| [Licensing](licensing.md) | Project, dependency, model and font terms; notices and redistribution |

## Application contracts

| Contract | Contents |
| --- | --- |
| [UI architecture](ui-architecture.md) | Rust/native boundary, launch ownership, conversation state and speech responsibilities |
| [Broker](broker-policy.md) | Public/private protocol, authentication, framing, admission, completion and recovery |
| [Streaming resources](streaming-resources.md) | Text/Find, HTTP, recovery and speech/PCM storage, work bounds and ownership |
| [Windows sandboxing](sandboxing.md) | Worker authority, Jobs/handles, files/locks, threads, power and containment limits |
| [Dependencies](dependency-audit.md) | Dated security/footprint evidence and parser/runtime selection decisions |

## Models, memory and runtime

| Guide | Contents |
| --- | --- |
| [Swift memory and PTQ1](swift-memory.md) | Artifacts, census, budgets, lossless packing/allocation, numerical evidence, decision screen and approximation boundaries |
| [Swift LoRA](swift-lora.md) | Artifact, basis, cost, storage overlay and paired device/human qualification |
| [Qwen reference](model-selection.md) | Selection rationale, compatibility/memory checks and stop/reopen conditions |
| [Native Rust engine](native-rust-engine.md) | Supported graph/API, numerical/state ownership, runtime overhead and promotion evaluator |
| [C++ retirement](cpp-retirement.md) | File-by-file inventories, independent replacement controls, dependency/build cleanup and remaining integration gates |
| [Prism conformance](prism-upstream-tests.md) | Source fidelity, broader upstream acceptance and pinned source-reuse review |
| [OxiBonsai review](oxibonsai-reuse.md) | Pinned source/license, reusable regressions, arithmetic/security transfer limits |
| [Runtime candidates](runtime-candidates/README.md) | Independent row-parallel and headless-startup patches, manifests, evidence and reproduction |

## Hardware and inference

| Guide | Contents |
| --- | --- |
| [Hardware discovery and profiles](hardware-capabilities.md) | Implemented inventory, evidence identities, selection eligibility, phase placement and fallback |
| [CPU kernels and X2 screen](cpu-kernels.md) | Packed arithmetic, ISA dispatch, thread-screen controls and device evidence |
| [Adreno and OpenCL](adreno.md) | Vulkan/compute access, placement, native OpenCL query, X2 observations and source audit |
| [Hexagon](hexagon.md) | Signed deployment, packed execution/state, memory and failure contracts |
| [Inference algorithms](inference-algorithms.md) | LUT/prefill/GDN, attention, hybrid cache and speculation constraints |
| [Inference defaults](inference-defaults.md) | Sampler/thinking/output settings, prompt harness and quality limits |
| [Performance and speech](performance-expectations.md) | Publisher benchmark context, paired speech results and latency/memory/energy measurement |
| [Research register](research-register.md) | Dated sources, mathematical proof obligations, applicability, negative results and experiment review |

Retained JSON receipts live in [evidence/](evidence/), candidate manifests and
patches in [runtime-candidates/](runtime-candidates/README.md), and charts in
[assets/](assets/). Owning guides explain their scope and reproduction; file
presence alone is not qualification. Add prose to its owning topic, link shared
requirements and put outstanding work in PLAN.
