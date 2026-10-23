# Swift Bonsai pretrained LoRA assessment

The owner-approved OrcaRouter adapter is pinned and provisioned, and a bounded
native ARM64 load/decode check completed. It is **not yet qualified as an effective
or quality-preserving uncensor adapter for Swift Bonsai**. No training or
fine-tuning is required to use this existing artifact. Default profiles remain
unchanged; behavioral and performance acceptance require separate evidence.

## Artifact and published evidence

[swift-lora.lock.json](../config/swift-lora.lock.json) pins
`Continuum-AI-Corp/OrcaBonsai-27B-Uncensored` at
`703028bb7fba03496e796b144839b161cf0f5409`, path
`gguf/bonsai-abliterate-lora.gguf`. The file is 9,682,464 bytes, with SHA-256
`f1669534803d340a496015f5c45125f3437b4d13ec764f40e34488ce83967f42` and Apache-2.0
licensing. Its size and digest were independently verified after the approved
download on October 23, 2026. The repository's [pinned README](https://github.com/Continuum-AI-Corp/OrcaBonsai-27B-Uncensored/blob/703028bb7fba03496e796b144839b161cf0f5409/README.md#gguf--llamacpp)
describes rank 1, alpha 1, and original Ternary Bonsai **PTQ1** inference testing;
it explicitly leaves **PQ2** untested. Those published tests do not establish
transfer to Swift's modified weights. Its `swift/` directory is an Apple
programming-language implementation, not evidence about the Swift Bonsai model.

Provision or reverify only this fixed artifact:

```powershell
./scripts/Get-Adapter.ps1 -DataDirectory "$env:LOCALAPPDATA/SwiftBonsai2"
./scripts/Get-Adapter.ps1 -DataDirectory "$env:LOCALAPPDATA/SwiftBonsai2" -VerifyOnly
```

The destination is `adapters/<pinned-source-commit>/bonsai-abliterate-lora.gguf`
under the existing external data directory. The command uses the bounded
size/hash-verified downloader, has no alternative lock or arbitrary URL option,
and does not start inference or enable a profile.

Other public candidates have different evidence:

| Candidate | Evidence boundary |
| --- | --- |
| [AtomicChat original-Bonsai LoRAs, revision `bfb7f8a`](https://huggingface.co/AtomicChat/Ternary-Bonsai-2-27B-Uncensored-LoRA-GGUF/blob/bfb7f8a74dd01625bf30ff63d798786fe27719b4/README.md) | Publisher reports original PTQ1 and PQ2 tests, including grouped SSM input-order checks. Its own42/own38 adapters remain unqualified on this Swift model and host; none was downloaded here. |
| [Swift KATANA, revision `fe9f2da7`](https://huggingface.co/ayshinko/Swift-Bonsai-2-27B-Katana-MTP-GGUF/blob/fe9f2da74ee3d47b2455158461172c2561ef57c3/README.md#experimental-compatibility-status) | A separate model integration, not a new adapter. Its Orca compatibility statement covers matching structure and explicitly lacks Swift inference validation. Its GPU/MTP examples are not this native ARM64 CPU configuration. |

## What matches, and what still needs verification

A read-only audit of the verified Swift GGUF and approved adapter found all 129
target names and rank-1 shapes compatible: 258 F32 adapter tensors,
`general.architecture=qwen35`, `adapter.type=lora`, and alpha 1. The base contains
851 tensors. These are actual descriptor checks, rather than an inference from
the shared model name. Dimensions below use GGUF order, first dimension fastest.

| Target family | Sites | Base shape | LoRA A | LoRA B |
| --- | ---: | --- | --- | --- |
| `blk.N.ffn_down.weight` | 64 | `[17408,5120]` | `[17408,1]` | `[1,5120]` |
| `blk.N.attn_output.weight` | 16 | `[6144,5120]` | `[6144,1]` | `[1,5120]` |
| `blk.N.ssm_out.weight` | 48 | `[6144,5120]` | `[6144,1]` | `[1,5120]` |
| `token_embd.weight` | 1 | `[5120,248320]` | `[1,248320]` | `[1,5120]` |

The [pinned Prism graph](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/src/llama-graph.cpp)
keeps the original activation `cur` for LoRA and transforms a separate `cur_mm`
for folded base weights. Adapter A must therefore consume the graph's original
input order; applying the base's Hadamard transform to both branches would be
incorrect. Embedding correction is added after inverse-Hadamard lookup. The
[adapter loader](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/src/llama-adapter.cpp)
checks shapes and supplies backend buffers; the
[Qwen35 graph](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/src/models/qwen35.cpp)
routes the projection families through LoRA-aware operations.

There is an unresolved **SSM head-order question**. Swift records
`prism.hadamard.gdn_v_grouped=true`. The
[pinned model loader](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/src/llama-model.cpp)
sets a three-way tiled/grouped permutation for these base-weight inputs, while
the LoRA branch still receives `cur`. Orca's
[exporter](https://github.com/Continuum-AI-Corp/OrcaBonsai-27B-Uncensored/blob/703028bb7fba03496e796b144839b161cf0f5409/scripts/export_gguf_lora.py)
builds A from an unfolded checkpoint and asserts that no SSM permutation is
needed. AtomicChat's card instead reports that an explicit order correction is
necessary. These statements may refer to differently ordered source matrices;
the presence or absence of a final permutation alone does not settle correctness.
The exporter accepts an external checkpoint directory, and the shipped adapter's
four metadata scalars contain no checkpoint revision or input-order declaration.
The audit therefore did not independently establish that checkpoint's basis.
Confirm the actual intended operator at the native residual writers before
calling either adapter mathematically validated on this Swift pack. The approved
file was not rewritten. Even a correctly ordered original-Bonsai correction is
not automatically the same projection for Swift's changed weights.

## Memory and performance impact

The artifact is about **9.23 MiB on disk**; its audited F32 payload is 9,664,512
bytes. This is not the complete runtime increment. The native graph retains
compressed base weights and adds the adapter branch without merging a dense
replacement checkpoint. Each of the 128 linear projection sites adds two rank-1
matrix multiplications plus scaling/addition. The embedding site uses a row lookup
and one matrix multiplication. Additional backend buffers, graph nodes,
intermediates, and changed allocation lifetimes also consume memory. Low rank
reduces arithmetic but does not eliminate scheduling, memory-traffic or small
operation overhead; no throughput percentage follows from the file size.

The integrator's October 23 fixed trusted-input check loaded this adapter with
the pinned Swift/native ARM64 runtime and exited 0 within a 16-token completion
cap. Windows reported peak Job-memory counters of **2,074,329,088 bytes** with the
adapter and **1,487,458,304 bytes** for the recorded base check: a difference of
586,870,784 bytes, about 559.68 MiB. These are Job committed-memory diagnostics,
not total physical residency or an isolated measurement of adapter allocation.
They already rule out promising that the complete added budget is merely 9 MiB.
The check establishes bounded loading/decoding only; it measures neither reliable
uncensoring nor accepted prose quality or performance. No inference benchmark was
run locally.

## Paired qualification on CI

Keep all benchmarks on CI. Pin model, adapter, runtime, template, sampler, context,
batching, threads and deterministic seed; record the complete configuration.
First compare no adapter against loaded adapter at scale zero, checking token
identity and logits within a documented numerical tolerance. Investigate a zero
scale mismatch before interpreting nonzero effects. For the SSM basis question,
verify residual-writer corrections against the intended operator; simple loading
and changed final text are insufficient.

Then compare a bounded, predetermined set of adapter scales using paired benign
fiction and art prompts: dialogue, continuity, requested tone, visual composition,
and sensitive but permitted creative themes. Score coherence, instruction
following, unwanted refusal, repetition and usable completion independently.
Preserve regressions and unsuccessful generations. Do not select a scale solely
because one prompt changes behavior.

Report prompt processing, first-token latency, decode throughput, actual generated
tokens, complete response time, peak working set/private bytes, and Job counters.
Compare equal token budgets and explain differences in reasoning and answer
length. Also report time and tokens to a useful answer: fewer refusals or retries
could improve that outcome despite slower per-token execution, while a fast,
incoherent answer is not an improvement. Paired quality and end-to-end usefulness
must accompany throughput before choosing an adapter for daily use.
