# Swift pretrained LoRA

The approved OrcaRouter adapter is pinned, provisioned and passes bounded native
ARM64 loading/decode. **Its effectiveness and quality on Swift remain
unqualified.** The owner intends it for the standard profile after independent
throughput and output-quality parity gates pass. Until then it is an explicit
development option in the broker. The native chat app enables it by
default at scale **1.3**; use `Start-Chat.ps1 -NoAbliterateLora` to opt out.
The native Settings pane adjusts the scale from 0 to 2 in 0.1 steps for the next
response, including every Extra effort pass, without reloading the model. Zero
disables the correction for that response while keeping the adapter loaded.
The 1.3 default is an owner-selected experimental setting, not a measured optimum
or evidence that effectiveness, numerical correctness or quality gates pass.
This app default does not promote the qualified standard profile; daily chat
stays disabled. Using this pretrained artifact
requires no local training or fine-tuning.

## Artifact and published evidence

[swift-lora.lock.json](../config/swift-lora.lock.json) pins
`Continuum-AI-Corp/OrcaBonsai-27B-Uncensored`, revision
`703028bb7fba03496e796b144839b161cf0f5409`, path
`gguf/bonsai-abliterate-lora.gguf`: **9,682,464 bytes**, SHA-256
`f1669534803d340a496015f5c45125f3437b4d13ec764f40e34488ce83967f42`,
Apache-2.0. Downloaded size/digest are verified. The
[pinned README](https://github.com/Continuum-AI-Corp/OrcaBonsai-27B-Uncensored/blob/703028bb7fba03496e796b144839b161cf0f5409/README.md#gguf--llamacpp)
describes rank 1, alpha 1 and testing on original Ternary Bonsai **PTQ1**;
it leaves **PQ2** untested. Those tests do not establish transfer to Swift's
modified weights. The repository's `swift/` directory is Apple-language code,
not evidence about the Swift Bonsai model.

Provision or reverify the fixed artifact:

```powershell
./scripts/Get-Adapter.ps1 -DataDirectory "$env:LOCALAPPDATA/SwiftBonsai2"
./scripts/Get-Adapter.ps1 -DataDirectory "$env:LOCALAPPDATA/SwiftBonsai2" -VerifyOnly
```

The destination is `adapters/<pinned-source-commit>/bonsai-abliterate-lora.gguf`
in the existing external data directory. The bounded downloader verifies
size/hash; it accepts no alternative lock or URL and enables no profile.

Other candidates are not qualified alternatives:

| Candidate | Evidence boundary |
| --- | --- |
| [AtomicChat original-Bonsai LoRAs, `bfb7f8a`](https://huggingface.co/AtomicChat/Ternary-Bonsai-2-27B-Uncensored-LoRA-GGUF/blob/bfb7f8a74dd01625bf30ff63d798786fe27719b4/README.md) | Publisher reports original PTQ1/PQ2 and grouped SSM-order tests. Its own42/own38 artifacts remain unqualified on this Swift pack/host. |
| [Swift KATANA, `fe9f2da7`](https://huggingface.co/ayshinko/Swift-Bonsai-2-27B-Katana-MTP-GGUF/blob/fe9f2da74ee3d47b2455158461172c2561ef57c3/README.md#experimental-compatibility-status) | A separate model integration. Orca structural compatibility is claimed without Swift inference validation; GPU/MTP examples differ from this CPU profile. |

## Structural compatibility and unresolved basis

The verified adapter has 258 F32 tensors, `general.architecture=qwen35`,
`adapter.type=lora` and alpha 1. All 129 target names and rank-1 shapes match
the actual 851-tensor Swift base. Dimensions use GGUF order, first dimension
fastest:

| Target family | Sites | Base shape | LoRA A | LoRA B |
| --- | ---: | --- | --- | --- |
| `blk.N.ffn_down.weight` | 64 | `[17408,5120]` | `[17408,1]` | `[1,5120]` |
| `blk.N.attn_output.weight` | 16 | `[6144,5120]` | `[6144,1]` | `[1,5120]` |
| `blk.N.ssm_out.weight` | 48 | `[6144,5120]` | `[6144,1]` | `[1,5120]` |
| `token_embd.weight` | 1 | `[5120,248320]` | `[1,248320]` | `[1,5120]` |

The [Prism graph](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/src/llama-graph.cpp)
feeds original activation `cur` to LoRA and separately transformed `cur_mm` to
folded base weights. Do not apply the base Hadamard transform to both branches.
Embedding correction follows inverse-Hadamard lookup. The
[adapter loader](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/src/llama-adapter.cpp)
checks shapes; the [Qwen35 graph](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/src/models/qwen35.cpp)
uses LoRA-aware operations for the target families.

**SSM input order remains unresolved.** Swift records
`prism.hadamard.gdn_v_grouped=true`. Its
[loader](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/src/llama-model.cpp)
applies a three-way tiled/grouped permutation to base-weight inputs while the
adapter receives `cur`. Orca's
[exporter](https://github.com/Continuum-AI-Corp/OrcaBonsai-27B-Uncensored/blob/703028bb7fba03496e796b144839b161cf0f5409/scripts/export_gguf_lora.py)
builds A from an unfolded checkpoint and asserts no SSM permutation is needed;
AtomicChat reports an explicit order correction. Different source-matrix orders
could explain that disagreement. Orca accepts an external checkpoint directory,
and the shipped metadata declares neither its revision nor input order.

Verify the intended correction at native residual writers before declaring the
adapter mathematically valid on Swift. Loading and changed text cannot settle
this question. A correctly ordered original-Bonsai correction also need not be
the right correction for Swift's changed weights. The approved file remains
unmodified.

## Memory and execution cost

The adapter contains **9,664,512 F32 payload bytes** (about 9.23 MiB on disk).
Prism retains packed base weights and adds adapter branches rather than merging
a dense replacement. Each of 128 linear sites adds two rank-1 multiplications,
scale and addition; the embedding uses lookup plus multiplication. Backend
buffers, graph nodes, intermediates and changed lifetimes add overhead beyond
the file's payload.

Recorded 16-token checks exited zero with peak Job commit of **2,074,329,088
bytes** with the adapter and **1,487,458,304 bytes** for the base: a diagnostic
difference of **586,870,784 bytes (559.68 MiB)**. These separate process counters
exclude mapped weights and do not isolate adapter allocation. They establish
bounded loading/decode, not reliable refusal changes, quality or performance.

The difference is not yet attributed. Record allocator/workspace high-water
marks, persistent adapter buffers and process commit around context creation,
activation, graph reservation and first decode before claiming recovery.
The [allocator](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/ggml/src/ggml-alloc.c)
already reuses eligible SCALE/ADD parent storage; explicit in-place calls do
not guarantee savings. F32 factor/input multiplication needs no activation-type
conversion in the [CPU workspace](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/ggml/src/ggml-cpu/ggml-cpu.c).
At microbatch 64, one rank-1 projection temporary is 256 bytes and one expanded
5120-channel correction is 1.25 MiB. The embedding does not construct a dense
vocabulary correction. Do not multiply per-site buffers by 128 without proving
their lifetimes overlap.

## Allocation-preserving optimization candidates

| Candidate | Status and constraints |
| --- | --- |
| Share immutable B payloads | Implemented [Rust source overlay](prism-storage-overlay.md); focused native test saves exactly 2.5 MiB with byte-identical operations. Not installed in the assessment runtime. |
| Reserve the configured graph once | Unimplemented. Common initialization reserves the base graph before activating adapters, causing another reserve. Old compute buffers are freed before replacement, so this does not prove two buffers coexist or explain the commit delta. |
| Fuse rank-1 accumulation | Unimplemented. Keep the existing A reduction and replace B multiplication/scale/add only after strict numerical and allocator checks. |

All 129 B tensors are byte-identical F32 `[1,5120]`, SHA-256
`1cdf553ae2623501140ec2e52c3dfa6a80d33fcfe6f91a6d3c4dcd5f887eeca0`.
The overlay shares storage only after matching buffer type/device, shape/layout
and bytes, preserving each target's metadata and adapter lifetime. Its saving is
modest and does not resolve the larger counter difference.

An initial-adapter context path must preserve public initialization, scale-zero
filtering, load-without-apply, fit/no-allocation estimates and dynamic changes.
A fused CPU accumulation must retain A's reduction, all F32 rounding stages and
adapter order. Reuse the base result only when last-use/view/output constraints
permit it. Do not reassociate scale or introduce FMA contraction: signed zero,
subnormals and compiler behavior make bit identity a test requirement. Require
strict reference comparisons, then ARM64 logits/generation checks in CI.

Quantizing factors, reducing rank/scale, pruning sites or merging into the
ternary base changes the correction or weights; these are outside lossless
allocation work.

## Required standard-profile qualification

- [ ] **Require decode tokens per second and output quality each to match or
  improve on the same model without LoRA.** The no-LoRA configuration remains
  the control. The [paired CI capture and scorer](lora-qualification.md) implement
  the fixed contract; target-device evaluation, native basis/logits evidence
  and independent human reviews remain pending.

The contract requires every prompt's median throughput ratio to be at least
1.0, and every applicable quality dimension to match the baseline median and
score at least 3/4. Unwanted adapter refusal on a reviewed benign prompt rejects
quality acceptance. Fewer refusals, shorter output, retries or memory savings
cannot compensate for throughput or quality regression.

The owner accepts a modest bounded RAM premium: the fixed gate permits up to
768 MiB median peak Job-commit increment per prompt within the same 12 GiB Job
cap and 4 GiB physical reserve. Keep improving lossless allocation without
requiring identical baseline RAM.

Pin model, adapter, runtime, template, sampler, context, batching, threads and
seed. Verify loaded-at-zero-scale response/logits identity before interpreting
nonzero effects, and verify the SSM basis at residual writers. Preserve failed
generations and individual regressions. Broader paired fiction/art checks must
cover dialogue, continuity, tone, composition and sensitive permitted themes,
with independent coherence, instruction-following, refusal, repetition and
usable-completion scores. Do not choose a scale from one changed response.

Keep benchmarks on CI. Report prompt processing, first-token latency, decode
throughput, generated/reasoning tokens, response time, retries and resource
counters. Compare equal budgets and time/tokens to useful completed work;
fewer refusals may help, but that benefit must be measured. The fixed capture
does not supply first-token, power or thermal measurements, so those remain
separate creative/comfort gates before daily use.
