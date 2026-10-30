# Swift pretrained LoRA

The pinned OrcaRouter adapter passes bounded native ARM64 loading/decode;
**effectiveness, correctness and quality on Swift remain unqualified**. The native app
enables it experimentally at scale **1.3**; `Start-Chat.ps1 -NoAbliterateLora` opts out.
Settings selects 0 to 2 in 0.1 steps for the next response and every Extra effort pass
without reloading; zero disables correction while retaining the adapter. The
owner-selected default is not a measured optimum or standard/daily-service acceptance.
The broker exposes an explicit development option; no local training is needed.
[PLAN.md](../PLAN.md#creative-quality-adapters-and-memory) owns remaining work.

## Artifact and published evidence

[swift-lora.lock.json](../config/swift-lora.lock.json) pins
`Continuum-AI-Corp/OrcaBonsai-27B-Uncensored`, revision
`703028bb7fba03496e796b144839b161cf0f5409`, path `gguf/bonsai-abliterate-lora.gguf`:
**9,682,464 bytes**, SHA-256
`f1669534803d340a496015f5c45125f3437b4d13ec764f40e34488ce83967f42`, Apache-2.0.
Downloaded size/digest are verified. The [pinned
README](https://github.com/Continuum-AI-Corp/OrcaBonsai-27B-Uncensored/blob/703028bb7fba03496e796b144839b161cf0f5409/README.md#gguf--llamacpp)
describes rank 1, alpha 1 and testing on original Ternary Bonsai **PTQ1**; it leaves
**PQ2** untested. Those tests do not establish transfer to Swift's modified weights. The
repository's `swift/` directory is Apple-language code, not evidence about the Swift
Bonsai model.

Provision or reverify the fixed artifact:

```powershell
./scripts/Get-Adapter.ps1 -DataDirectory "$env:LOCALAPPDATA/SwiftBonsai2"
./scripts/Get-Adapter.ps1 -DataDirectory "$env:LOCALAPPDATA/SwiftBonsai2" -VerifyOnly
```

The destination is `adapters/<pinned-source-commit>/bonsai-abliterate-lora.gguf` in the
existing external data directory. The bounded downloader verifies size/hash; it accepts
no alternative lock or URL and enables no profile.

Other candidates are not qualified alternatives:

| Candidate | Evidence boundary |
| --- | --- |
| [AtomicChat original-Bonsai LoRAs, `bfb7f8a`](https://huggingface.co/AtomicChat/Ternary-Bonsai-2-27B-Uncensored-LoRA-GGUF/blob/bfb7f8a74dd01625bf30ff63d798786fe27719b4/README.md) | Publisher reports original PTQ1/PQ2 and grouped SSM-order tests. Its own42/own38 artifacts remain unqualified on this Swift pack/host. |
| [Swift KATANA, `fe9f2da7`](https://huggingface.co/ayshinko/Swift-Bonsai-2-27B-Katana-MTP-GGUF/blob/fe9f2da74ee3d47b2455158461172c2561ef57c3/README.md#experimental-compatibility-status) | A separate model integration. Orca structural compatibility is claimed without Swift inference validation; GPU/MTP examples differ from this CPU profile. |

## Structural compatibility and unresolved basis

The verified adapter has 258 F32 tensors, `general.architecture=qwen35`,
`adapter.type=lora` and alpha 1. All 129 target names and rank-1 shapes match the actual
851-tensor Swift base. Dimensions use GGUF order, first dimension fastest:

| Target family | Sites | Base shape | LoRA A | LoRA B |
| --- | ---: | --- | --- | --- |
| `blk.N.ffn_down.weight` | 64 | `[17408,5120]` | `[17408,1]` | `[1,5120]` |
| `blk.N.attn_output.weight` | 16 | `[6144,5120]` | `[6144,1]` | `[1,5120]` |
| `blk.N.ssm_out.weight` | 48 | `[6144,5120]` | `[6144,1]` | `[1,5120]` |
| `token_embd.weight` | 1 | `[5120,248320]` | `[1,248320]` | `[1,5120]` |

The [Prism
graph](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/src/llama-graph.cpp)
feeds original activation `cur` to LoRA and separately transformed `cur_mm` to folded
base weights. Do not apply the base Hadamard transform to both branches. Embedding
correction follows inverse-Hadamard lookup. The [adapter
loader](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/src/llama-adapter.cpp)
checks shapes; the [Qwen35
graph](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/src/models/qwen35.cpp)
uses LoRA-aware operations for the target families.

**SSM input order remains unresolved.** Swift records
`prism.hadamard.gdn_v_grouped=true`. Its
[loader](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/src/llama-model.cpp)
applies a three-way tiled/grouped permutation to base-weight inputs while the adapter
receives `cur`. Orca's
[exporter](https://github.com/Continuum-AI-Corp/OrcaBonsai-27B-Uncensored/blob/703028bb7fba03496e796b144839b161cf0f5409/scripts/export_gguf_lora.py)
builds A from an unfolded checkpoint and asserts no SSM permutation is needed;
AtomicChat reports an explicit order correction. Different source-matrix orders could
explain that disagreement. Orca accepts an external checkpoint directory, and the
shipped metadata declares neither its revision nor input order.

Native residual-writer evidence determines whether the intended correction is
mathematically valid on Swift. Loading and changed text cannot settle this question. A
correctly ordered original-Bonsai correction also need not be the right correction for
Swift's changed weights. The approved file is unmodified.

## Memory and execution cost

The adapter contains **9,664,512 F32 payload bytes** (about 9.23 MiB on disk). Prism
retains packed base weights and adds adapter branches rather than merging a dense
replacement. Each of 128 linear sites adds two rank-1 multiplications, scale and
addition; the embedding uses lookup plus multiplication. Backend buffers, graph nodes,
intermediates and changed lifetimes add overhead beyond the file's payload.

Recorded 16-token checks exited zero with peak Job commit of **2,074,329,088 bytes**
with the adapter and **1,487,458,304 bytes** for the base: a diagnostic difference of
**586,870,784 bytes (559.68 MiB)**. These separate process counters exclude mapped
weights and do not isolate adapter allocation. They establish bounded loading/decode,
not reliable refusal changes, quality or performance.

The delta does not identify persistent buffers, workspace high-water allocations or
graph lifetimes. The
[allocator](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/ggml/src/ggml-alloc.c)
already reuses eligible SCALE/ADD parent storage; explicit in-place calls do not
guarantee savings. F32 factor/input multiplication needs no activation-type conversion
in the [CPU
workspace](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/ggml/src/ggml-cpu/ggml-cpu.c).
At microbatch 64, one rank-1 projection temporary is 256 bytes and one expanded
5120-channel correction is 1.25 MiB. The embedding does not construct a dense vocabulary
correction. Do not multiply per-site buffers by 128 without proving their lifetimes
overlap.

## Allocation semantics

Common initialization reserves the base graph before activating adapters, causing
another reserve. Old compute buffers are freed before replacement; this does not prove
that two buffers coexist or explain the commit delta. Prism Inference owns the immutable B-storage overlay, native regression and
[builder](https://github.com/symbiogenesis/prism-inference/blob/main/scripts/Build-PrismEngine.ps1).
Its [overlay guide](https://github.com/symbiogenesis/prism-inference/blob/main/docs/prism-storage-overlay.md)
records byte-identical operations and a 2.5 MiB storage saving. Runner consumes
reviewed pins; the installed assessment runtime does not automatically include
the source overlay.

All 129 B tensors are byte-identical F32 `[1,5120]`, SHA-256
`1cdf553ae2623501140ec2e52c3dfa6a80d33fcfe6f91a6d3c4dcd5f887eeca0`. The overlay shares
storage only after matching buffer type/device, shape/layout and bytes, preserving each
target's metadata and adapter lifetime. Its saving is modest and does not resolve the
larger counter difference.

Lossless allocation changes preserve public initialization, scale-zero filtering,
load-without-apply, fit/no-allocation estimates, dynamic changes, A's reduction, all F32
rounding stages and adapter order. Base-result reuse depends on last-use/view/output
constraints. Reassociation or FMA contraction can change signed zero, subnormals and
results; bit identity is part of this contract.

Quantizing factors, reducing rank/scale, pruning sites or merging into the ternary base
changes the correction or weights; these are outside lossless allocation work.

## Required standard-profile qualification

The [fixed LoRA contract and paired CI procedure](lora-qualification.md) own the
per-prompt throughput, independent human-quality, memory and native-correctness gates.
Each must pass; reduced refusals or memory use cannot offset a primary regression. The
scale-one/512-token harness does not qualify the app's scale 1.3. Broader
creative/comfort and daily-service acceptance remain separate.
