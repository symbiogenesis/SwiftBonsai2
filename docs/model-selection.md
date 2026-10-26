# Qwen reference model

**Swift Bonsai 2 is the current implementation priority.** See the
[Swift assessment](swift-memory.md). Qwen remains the reproducible reference in
[runtime.lock.json](../config/runtime.lock.json); that default lock is not a
Swift launch profile. Daily chat is disabled for both.

The reference is **Qwen3.8-27B UD-Q3_K_XL**, revision
`4ca720788d1e01f1bff70c033e0d0028fd02e502`, with **Gemma 4 12B IT QAT Q4_0**
as a literature-nominated fallback. Qwen was selected from published evidence,
not a local candidate benchmark. Confidence is moderate for the base-model
creative preference and low for the exact quantized artifact and laptop comfort.
Only Qwen is provisioned by the default scripts; the fallback is not selected.

## Literature gate

The October 22–23, 2026 review found the following published results. EQ-Bench
is independent of the publishers but uses LLM judging. Its entries do not
identify the exact GGUF packs pinned here.

| Published identity | Creative Writing v3 Elo | Raw rubric /20 | Longform score |
| --- | ---: | ---: | ---: |
| Qwen/Qwen3.8-27B | 1671.3 | 15.50 | 53.3 |
| google/gemma-4-12B-it | 1288.9 | 15.71 | 48.7 |
| google/gemma-4-26B-A4B-it | 1304.6 | 16.03 | 50.7 |
| Swift PQ2_0 / base Bonsai 2 / Qwen3.6-35B-A3B | No entry found | — | — |

Sources: [Creative Writing v3](https://eqbench.com/creative_writing),
[versioned data](https://eqbench.com/creative_writing.js?v=1.0.91),
[Longform Writing](https://eqbench.com/creative_writing_longform.html), and
[versioned longform data](https://eqbench.com/creative_writing_longform.js?v=1.0.9).
Missing entries are missing evidence, not zero scores.

Qwen leads aggregate creative scores; Gemma 12B has a slightly higher raw
rubric score and more weight-memory headroom. The
[published Qwen samples](https://eqbench.com/results/creative-writing-v3/Qwen__Qwen3.8-27B.html)
and [Gemma samples](https://eqbench.com/results/creative-writing-v3/google__gemma-4-12B-it.html)
show weaknesses in both continuity and constraint following. Longform chapter
scores decline for both. Neither model has established local creative quality.

The [CW methodology](https://github.com/EQ-bench/creative-writing-bench) uses
32 prompts, three iterations and pairwise comparisons truncated to 4,000
characters. Longform v1.11 names Sonnet 4.6 as judge while its repository names
Sonnet 4; per-entry provenance remains unclear. Hardware, quantization, thinking
controls and template revisions are not established by the rows. Typical
sampling (temperature 0.7, min-p 0.1) also differs from publisher guidance.
These English-prose scores cannot be assigned to UD-Q3_K_XL or QAT Q4_0, or used
to establish art direction, image critique, speed or thermal comfort.

[Swift's publisher](https://ukisai.com/swift-bonsai-2) describes an experimental
release with uneven general-purpose behavior. Its reasoning benchmarks do not
establish creative quality. Historical runtime-correction results do not apply
automatically to its updated merged PQ2 artifact. The
[Gemma QAT artifact](https://huggingface.co/google/gemma-4-12B-it-qat-q4_0-gguf)
is a conventional publisher-produced fallback; its benchmark evidence also uses
a different serving representation. No reviewed exact-artifact X Elite result
settles creative quality or laptop comfort for these choices.

## Source-only compatibility and memory sanity check

The [verified metadata](../config/model.metadata.json) describes 866 tensors
across 14 types, architecture `qwen35`, 65 blocks including one MTP block,
four KV heads and 256-dimensional K/V. The main graph has 16 full-attention
and 48 recurrent blocks. Full size/SHA-256, dimensions and embedded-template
identity are verified; actual CPU loading remains pending.

The reference runtime is llama.cpp **b11840**, commit
`36e36e63a54fc8f85b95e9c499a022b597e478dd`. This daily release is an upstream
prerelease. Its [Qwen graph](https://github.com/ggml-org/llama.cpp/blob/36e36e63a54fc8f85b95e9c499a022b597e478dd/src/models/qwen35.cpp)
handles MTP separately; its
[CPU backend](https://github.com/ggml-org/llama.cpp/tree/36e36e63a54fc8f85b95e9c499a022b597e478dd/ggml/src/ggml-cpu)
supports the artifact's tensor types, SSM convolution and Gated Delta Net.
The [release](https://github.com/ggml-org/llama.cpp/releases/tag/b11840) includes
a native Windows ARM64 CPU archive. These are source-compatibility findings,
not execution or performance qualification.

For one sequence, 8,192 context tokens, FP16 K/V, speculation off, no prompt
cache and no retained checkpoints:

```text
weights                            13,146,393,504 bytes = 12.2435 GiB
KV = 8192 × 16 × 4 × 256 × 2 × 2       536,870,912 bytes = 0.5000 GiB
recurrent state (F32, including conv)   156,893,184 bytes = 0.1461 GiB
subtotal                           13,840,157,600 bytes = 12.8897 GiB
remaining under 18 GiB               5,487,195,232 bytes = 5.1103 GiB
```

Batch 256 / microbatch 128 must also fit scratch, graph buffers and allocation
overhead. This accounting is not peak residency. Reuse/checkpoints are disabled
because the server defaults exceed this estimate. The immutable file retains
its MTP block, but speculation and projector/vision allocations are not admitted.
The guarded CPU check requires 22 GiB available and has refused launch on the
recorded host; the profile remains development-only and chat-launch-disabled.

Preserve and verify the embedded template. The
[pinned quantizer instructions](https://huggingface.co/unsloth/Qwen3.8-27B-GGUF/blob/4ca720788d1e01f1bff70c033e0d0028fd02e502/README.md)
document thinking control. A future non-thinking creative candidate uses
temperature 0.7, top-p 0.8, top-k 20, min-p 0, presence penalty 1.5 and repetition
penalty 1; that does not establish that a lower-reasoning switch works.

## Stop and reopen conditions

Reject file/hash/header mismatches and unsupported graphs or types. Limit the
first compatibility investigation to one working day and one narrow,
upstream-supported repair. Custom quantization and backend kernels are outside
this reference bring-up.

Reopen the reference choice if that route cannot produce correct bounded output,
exceeds 18 GiB at 8K with reuse off, or later fails creative, interactive-wait or
owner-accepted heat/fan requirements after bounded tuning. Record the failure
and a fresh literature decision before provisioning the fallback. Keep candidate
benchmarks on CI.

No daily reference profile is accepted. Outstanding qualification work lives in
the [central TODO](../PLAN.md). Use the [development guide](development-guide.md)
for commands and [licensing](licensing.md) for redistribution terms.
