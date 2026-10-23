# Initial model choice

**Priority update, October 23:** the owner subsequently prioritized Swift Bonsai 2
for memory feasibility and BITCOS assessment. See [the new assessment](swift-memory.md).
This document retains the earlier Qwen literature decision and its limitations;
it does not override that later direction or establish Swift creative quality.

Decision recorded October 23, 2026, before building or running an inference engine. Select **Qwen3.8-27B UD-Q3_K_XL**, revision `4ca720788d1e01f1bff70c033e0d0028fd02e502`, for the first native ARM64 CPU implementation. Nominate **Gemma 4 12B IT QAT Q4_0** as fallback. Only the selected model is provisioned by the scripts. No local candidate benchmark informed this decision.

Confidence is moderate for the base-model creative preference, low for the exact quantized artifact and laptop comfort. Published creative-writing results favor Qwen overall, its native CPU route requires no new kernels, and an 8K reference configuration plausibly fits the 18 GiB service allowance. This is an initial engineering choice, not a creative-quality or performance acceptance verdict.

## Literature gate

Sources below were inspected October 22–23, 2026. EQ-Bench is independent of the model publishers but uses LLM judging; its leaderboard entries do not identify these exact local GGUF packs.

| Published identity | Creative Writing v3 Elo | Raw rubric /20 | Longform score | Interpretation |
| --- | ---: | ---: | ---: | --- |
| Qwen/Qwen3.8-27B | 1671.3 | 15.50 | 53.3 | Strongest aggregate creative evidence among the feasible candidates reviewed |
| google/gemma-4-12B-it | 1288.9 | 15.71 | 48.7 | Smaller fallback; the raw rubric slightly favors it over Qwen |
| google/gemma-4-26B-A4B-it | 1304.6 | 16.03 | 50.7 | More resident weights than the 12B fallback; no compelling advantage over Qwen in these aggregate results |
| Swift PQ2_0 / base Bonsai 2 / Qwen3.6-35B-A3B | No entry found | — | — | Missing relevant public evidence; not a zero score |

Sources: [Creative Writing v3](https://eqbench.com/creative_writing), [versioned CW data](https://eqbench.com/creative_writing.js?v=1.0.91), [Longform Writing](https://eqbench.com/creative_writing_longform.html), [versioned longform data](https://eqbench.com/creative_writing_longform.js?v=1.0.9). The [CW methodology](https://github.com/EQ-bench/creative-writing-bench) uses 32 prompts and three iterations, with pairwise comparisons truncated to 4,000 characters. Longform v1.11 identifies Claude Sonnet 4.6 as judge; its repository still names Sonnet 4, and exact per-entry judge provenance remains unclear. Typical generation settings are temperature 0.7 and min-p 0.1. These differ from publisher-recommended samplers. Exact run-specific hardware, quantization, thinking controls, and template revision are not established by the leaderboard rows, so the scores cannot be assigned to UD-Q3_K_XL or QAT Q4_0. This evidence covers English prose, not art direction or image critique.

Actual published [Qwen samples](https://eqbench.com/results/creative-writing-v3/Qwen__Qwen3.8-27B.html) and [Gemma12 samples](https://eqbench.com/results/creative-writing-v3/google__gemma-4-12B-it.html) temper Qwen's Elo lead: in the first Azra scene Qwen postpones the requested meeting, whereas Gemma 12B completes an offer scene but uses generic descriptions. Neither is uniformly superior prose. Longform chapter scores decline for both (Qwen 13.45 to 11.26; Gemma 12B 13.16 to 9.97). Continuity and constraint-following require subsequent chosen-engine qualification. The fallback remains useful even though Qwen leads the aggregate rankings.

[Swift's publisher](https://ukisai.com/swift-bonsai-2) calls it an experimental release with uneven general-purpose behavior. Its current PQ2_0 IFBench/AIME results and historical GPQA/C-Eval results do not establish prose quality or laptop efficiency. Historical runtime-correction results cannot be attributed to the updated merged PQ2_0 artifact. No exact-artifact X Elite creative/thermal comparison was found that would justify choosing Swift on those grounds.

The [Google QAT artifact](https://huggingface.co/google/gemma-4-12B-it-qat-q4_0-gguf) offers a publisher-produced conventional Q4_0 fallback with substantially more weight-memory headroom. Its benchmark evidence still concerns a different serving representation. MoE active-parameter counts do not remove resident expert memory, and no published laptop comfort result resolves these choices.

## Source-only compatibility and memory sanity check

The selected [artifact metadata](https://huggingface.co/api/models/unsloth/Qwen3.8-27B-GGUF/revision/4ca720788d1e01f1bff70c033e0d0028fd02e502?blobs=true) supplies an immutable revision, 13,146,393,504-byte length, and LFS SHA-256. A bounded 1 MiB HTTP range read inspected header metadata without downloading tensor payloads. It confirms GGUF architecture `qwen35`, 65 blocks including one MTP block, full-attention interval four, four KV heads, 256-dimensional K/V, and recurrent state dimensions recorded in [runtime.lock.json](../config/runtime.lock.json). The 64-block main graph contains 16 full-attention and 48 recurrent blocks. The complete tensor histogram, tokenizer identity, template hash, and file digest remain download-time checks.

The October 23 dependency update pins upstream llama.cpp daily release b11840, commit `36e36e63a54fc8f85b95e9c499a022b597e478dd`, to include published fixes after stable v0.7.0 (b11669). Daily releases are marked prerelease upstream; model execution and performance remain unqualified. Its [Qwen graph](https://github.com/ggml-org/llama.cpp/blob/36e36e63a54fc8f85b95e9c499a022b597e478dd/src/models/qwen35.cpp) handles MTP separately from the main graph. Its [CPU implementation](https://github.com/ggml-org/llama.cpp/tree/36e36e63a54fc8f85b95e9c499a022b597e478dd/ggml/src/ggml-cpu) implements the needed conventional quantization types, SSM convolution, and Gated Delta Net. The [b11840 release](https://github.com/ggml-org/llama.cpp/releases/tag/b11840) publishes a native Windows ARM64 CPU archive with a GitHub-provided SHA-256. This is source-level feasibility, not proof of this laptop's execution correctness. Model terms are Apache-2.0; runtime terms are MIT. Retain upstream notices when distributing artifacts.

At one sequence, 8,192 context tokens, FP16 K/V, speculation off, no prompt cache, and no retained checkpoints:

```text
weights                            13,146,393,504 bytes = 12.2435 GiB
KV = 8192 × 16 × 4 × 256 × 2 × 2       536,870,912 bytes = 0.5000 GiB
recurrent state (F32, including conv)   156,893,184 bytes = 0.1461 GiB
subtotal                           13,840,157,600 bytes = 12.8897 GiB
remaining under 18 GiB               5,487,195,232 bytes = 5.1103 GiB
```

This leaves plausible room for scratch, graph buffers, and allocation overhead with batch 256 / microbatch 128, but does not establish peak residency. Checkpoints and reuse are deliberately disabled for the first reference: the pinned server's large defaults are unsuitable for this admission estimate. The MTP block remains in the immutable file but is not enabled for speculation. No projector or vision allocation is admitted.

The exact embedded template remains authoritative. The [pinned quantizer instructions](https://huggingface.co/unsloth/Qwen3.8-27B-GGUF/blob/4ca720788d1e01f1bff70c033e0d0028fd02e502/README.md) document thinking control; a future non-thinking creative candidate uses temperature 0.7, top-p 0.8, top-k 20, min-p 0, presence penalty 1.5, repetition penalty 1. The initial runtime must preserve and verify template behavior rather than assume a lower-reasoning switch works.

## Stop and reopen conditions

Before engine use, reject a file/hash/header mismatch or unsupported graph/type. Limit the first compatibility investigation to one working day and at most one narrow, upstream-supported repair; custom quantization or backend kernels are outside this bring-up. Reopen the literature decision if the pinned native CPU route fails that limit, duplicates/expands weights enough to exceed the 18 GiB ceiling at 8K with reuse off, or cannot produce correct bounded output.

During later self-hosted qualification, reopen the decision if useful creative output, acceptable interactive waits, or owner-accepted sustained heat/fan behavior cannot be achieved with bounded profile tuning. Record the failure and a fresh literature decision before provisioning the fallback. Do not convert engine tuning into a local candidate benchmark campaign.

## Implementation gates

M0a is complete at source level. M0 provides pinned provisioning, host inventory, and [full artifact metadata](../config/model.metadata.json). Full size/SHA-256, dimensions, and embedded-template identity were verified after provisioning on October 23. The actual mixed pack contains 14 tensor types; pinned CPU source provides inference operations for each. Actual CPU loading remains pending because the guarded check refused admission at about 8 GiB available against its conservative 22 GiB requirement. The reference profile remains **development-only and chat-launch-disabled**. Authenticated chat, lifecycle, origin/route controls, restricted inference policy, creative/comfort evidence, Vulkan, and all performance promotion remain later gates. No benchmark has been run and no daily profile is accepted.
