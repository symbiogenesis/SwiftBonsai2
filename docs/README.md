# Documentation

Start with the [development guide](development-guide.md) to install/provision,
launch the native app or run bounded developer checks. [PLAN.md](../PLAN.md) is
the only backlog; these pages describe current behavior, contracts and evidence.

**Current boundary:** native Windows ARM64/Rust UI, CPU execution and a strict
local HTTP/JSON/SSE subset. Experimental runtimes, formats and accelerators retain
separate qualification gates. A working fixture or published base-model result
does not establish actual-model quality, speed, isolation or sustained comfort.

## Use and develop

| Page | Owns |
| --- | --- |
| [Development guide](development-guide.md) | Requirements, provisioning, launch/settings/speech, developer commands and runtime lock updates |
| [Portable packages](chat-packages.md) | Assembly, installation, updates/rollback and launches without a checkout |
| [Validation](validation.md) | Allowed local checks, CI ownership, named regressions and acceptance limits |
| [Licensing](licensing.md) | Project/dependency/model terms, fonts, notices and redistribution |

## Application contracts

| Page | Owns |
| --- | --- |
| [UI architecture](ui-architecture.md) | Lasting Rust/native presentation boundary, state/lifecycle and interoperability responsibilities |
| [Broker policy](broker-policy.md) | Exact public/private protocol, authentication, framing, admission and completion/recovery |
| [Streaming resources](streaming-resources.md) | Bounded text, request/response storage, queues, Find, recovery and PCM lifetimes |
| [Sandboxing and Windows integration](sandboxing.md) | Worker authority, containment, Jobs/handles, files/locks, threads, power and Windows capability limits |
| [Implementation decisions](implementation-research.md) | Design rationale, file-integrity ownership, protocol choices and runtime-overhead research implications |
| [Dependency audit and choices](dependency-audit.md) | Dated security/feature/footprint reviews, parser/runtime comparisons and retained dependency decisions |

## Models, memory and runtime

| Page | Owns |
| --- | --- |
| [Swift memory assessment](swift-memory.md) | Immutable artifacts, actual tensor census, resource budgets and reproducible checks |
| [Memory options](swift-memory-options.md) | Lossless packing/allocation accounting and separately identified approximate research |
| [Current-weight PTQ1](ptq1-readiness.md) | Explicit format selection, arithmetic/evidence limits and bounded decision screen |
| [Swift LoRA](swift-lora.md) | Adapter artifact, basis/application limits, cost and immutable storage overlay |
| [LoRA qualification](lora-qualification.md) | Fixed acceptance settings, paired device captures and independent review |
| [Qwen reference](model-selection.md) | Reference-model decision, compatibility/memory checks and stop/reopen conditions |
| [Native Rust engine](native-rust-engine.md) | Supported graph/API subset, numerical/state ownership and replacement boundaries |
| [Runtime promotion](runtime-promotion.md) | Independent source/release/application identities and offline evidence evaluation |
| [Prism conformance and reuse](prism-upstream-tests.md) | Source fidelity, supported subset, upstream qualification and reuse review |
| [OxiBonsai reuse](oxibonsai-reuse.md) | Pinned source/license review, reusable regressions and arithmetic/security transfer limits |
| [Runtime source candidates](runtime-candidates/README.md) | Reviewable row-parallel patch, manifests and focused evidence |
| [Headless startup candidate](runtime-candidates/headless-ucrt-argv.md) | Separate UCRT argv patch, exact startup evidence and reproduction |

## Hardware and inference

| Page | Owns |
| --- | --- |
| [Hardware capabilities](hardware-capabilities.md) | Implemented bounded discovery/report contracts and retained CPU/GPU/OpenCL observations |
| [Hardware profiles](hardware-profiles.md) | Eligibility, future automatic selection, phase placement and qualified fallback |
| [CPU SIMD](simd-plan.md) | Packed arithmetic, optional ISA dispatch and numerical/resource gates |
| [X2 CPU screen](x2-cpu-optimization.md) | Thread-screen controls, plan inspection, host/power identity and device evidence |
| [Adreno](adreno-gpu-plan.md) | Vulkan access, placement, arithmetic and accelerator qualification |
| [OpenCL feasibility](opencl-feasibility.md) | Bounded native query, retained X2 evidence and immutable source audit |
| [Hexagon](hexagon-npu-plan.md) | Signed deployment, exact packed execution/state, memory and failure contracts |
| [Inference algorithms](inference-algorithms.md) | Runtime semantics, LUT/prefill/GDN, attention, hybrid cache and speculation constraints |
| [Inference defaults](inference-defaults.md) | Sampler/thinking/output settings, prompt harness and quality evidence limits |
| [Performance and speech evidence](performance-expectations.md) | Published benchmark context, measured-versus-unqualified results, paired speech resources and measurement methodology |
| [Research register](research-register.md) | Dated primary sources, derivations, applicability, negative results and experiment review |

## Evidence files

Retained JSON receipts live in [evidence/](evidence/), runtime candidate manifests
and patches in [runtime-candidates/](runtime-candidates/README.md), and SVGs in
[assets/](assets/). Their owning pages explain scope and reproduction; file
presence is not a qualification verdict. New prose belongs in the topic that owns
its contract; link to shared requirements rather than copying them, and put new
work in PLAN.
