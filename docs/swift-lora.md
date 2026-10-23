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

## Allocation-preserving optimization candidates

The current check logs do not report compute-buffer allocations, so the roughly
560 MiB counter difference has **not been attributed to a specific allocation**.
Record allocator high-water marks, CPU workspace, persistent adapter buffers,
and current/peak process commit around context creation, adapter activation,
graph reservation and first decode before claiming to have recovered it.

Two source observations constrain that diagnosis. The
[existing graph allocator](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/ggml/src/ggml-alloc.c)
already reuses suitable `SCALE`/`ADD` parent storage when it is owned, has the
same layout, is at its last use, has no live views and is not a graph output.
Forcing explicit in-place calls cannot be assumed to save additional memory.
The [CPU matrix workspace calculation](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/ggml/src/ggml-cpu/ggml-cpu.c)
does not need activation-type conversion for F32 factors multiplying F32 inputs.
These adapter targets also produce only 5120 output channels: at the recorded
64-token microbatch, one rank-1 projection temporary is 256 bytes and one expanded
correction is 1.25 MiB. The embedding branch performs its rank-1 row lookup before
the expansion; it does not construct a dense vocabulary-by-embedding correction.
Do not multiply that per-site bound by 128 as though all corrections remain live;
actual overlap and storage recycling are determined by the graph allocator.

The following designs preserve the stored adapter and intended operator. None is
implemented or accepted yet:

1. **Reserve the configured graph once.**
   [Common initialization](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/common/common.cpp)
   loads adapter objects before creating the context, but activates them after
   context creation. The [context](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/src/llama-context.cpp)
   has already reserved a base-only graph; activation requests another reserve.
   An internal initialization path receiving the initial adapter/scales map before
   the constructor's first reserve could avoid this redundant work. Preserve
   existing public initialization, scale-zero filtering, load-without-apply,
   fit/no-allocation estimation and dynamic adapter changes. The current allocator
   frees an old compute buffer before replacing it; double reservation alone
   does not prove that two complete buffers coexist or explain the counter delta.
   Startup allocation churn may affect peak accounting without establishing a
   persistent steady-state increment.

2. **Share identical immutable B storage.** A read-only payload audit found all
   129 B tensors byte-identical, each F32 `[1,5120]`, with SHA-256
   `1cdf553ae2623501140ec2e52c3dfa6a80d33fcfe6f91a6d3c4dcd5f887eeca0`.
   During adapter loading, key candidates by backend buffer type/device, dtype,
   exact shape/layout and content; confirm byte equality, then give each target
   its own metadata view over one retained read-only B allocation. Do not alias
   mutable tensors or across independent adapter lifetimes. This removes exactly
   **2,621,440 bytes (2.5 MiB)** of duplicate payload for this artifact. It changes
   no arithmetic and is a modest saving, not a solution to the larger diagnostic.

3. **Fuse rank-1 accumulation without changing A's reduction.** Keep the existing
   `t = mul_mat(A, original_cur)` node and backend dot-product order. A CPU-only
   rank-1 accumulation operation can consume `res`, `B`, `t` and scale, replacing
   the second matrix multiplication, scale and addition. Reuse `res` only when
   the allocator proves last-use and view/output constraints; otherwise retain
   a separate destination. This removes the expanded correction buffer and
   intermediate traffic while preserving the small first projection. The same
   pattern can consume the embedding's existing row lookup. Keep all F32
   rounding stages and adapter application order; do not reassociate scale into
   A/B or introduce a multiply-add contraction. Signed zero, subnormals and
   compiler contraction make bit identity a test requirement, not a consequence
   of the rank-1 algebra. Start with strict CPU reference/output comparisons,
   then native ARM64 logits and deterministic generations in CI before making
   quality or performance claims.

Quantizing adapter factors, lowering rank, pruning sites or reducing scale changes
the correction and is not a lossless overhead reduction. Merging a dense edit into
the ternary base would also change its weights or storage assumptions. Keep those
separate from storage sharing, allocation reuse and validated execution fusion.

## Required standard-profile qualification

- [ ] **Mandatory TODO: establish that the model with the approved abliteration
  LoRA has decode tokens per second and output quality each on par with or
  better than the same model without it.** The owner intends this LoRA to be standard, rather than
  an optional daily-profile feature. Acceptance requires the paired correctness,
  capability, creative quality, responsiveness and resource checks below on the
  target hardware through CI. Record predeclared parity tolerances and individual
  regressions; do not equate fewer refusals, successful loading or one faster
  metric with overall parity. Tokens per second and output quality are the two
  primary acceptance criteria; memory, total tokens, retries and completed-work
  time are supporting measurements and cannot mask regression in either primary
  criterion. The no-LoRA configuration remains the required
  baseline/control. Until this gate passes, the current explicit development
  assessment switch does not certify or enable the intended standard profile.

The owner accepts a modest, bounded RAM premium for this standard LoRA. Require
the complete profile to fit its memory/comfort envelope rather than requiring
the adapter to use exactly the baseline's RAM. Continue validated lossless
storage/allocation improvements. Judge overall parity through useful completed
work: output quality, elapsed time, generated/reasoning tokens and retries,
including tokens spent on refusals and discarded generations. Fewer refusals
may improve quality or real-world token usage; this is a testable possible
benefit, not an assumed gain or a substitute for general capability checks.

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
