# Swift Bonsai 2 memory and BITCOS assessment

On October 23, 2026 the owner explicitly prioritized **Swift Bonsai 2** for the
memory work and BITCOS assessment. This supersedes the Qwen-first implementation
priority for this investigation. The existing Qwen lock/profile is retained for
reproducibility; it does not silently become a compatible Swift launch profile.
No local candidate benchmark informed this choice. Daily chat remains disabled.

The assessment pins the current **Swift-Bonsai-2-PQ2_0.gguf**, rather than the
earlier PTQ release whose merged weights differ, in
[swift-assessment.lock.json](../config/swift-assessment.lock.json). Its published
size is 7,206,168,928 bytes (6.71 GiB), about 5.53 GiB smaller than the previously
provisioned Qwen file before runtime overhead. This file-size difference is not
a measured peak-RAM or speed result. [Publisher release](https://ukisai.com/swift-bonsai-2),
[immutable model metadata](https://huggingface.co/api/models/ukisai/Swift-Bonsai-2-GGUF/revision/a3bdac086bbb6b04d87d0108d18943b5f63868f7?blobs=true).

Prism's released native ARM64 CPU package is pinned at
`73bfe6cc485554822f80c473a3c0c4f25e80f41b`. Stock upstream binaries do not support
these PQ2/PTQ tensor types. Prism includes packed ARM NEON dot-product kernels;
the runtime's transforms and graph support matter in addition to its codec.
Its source-build ARM route requires Clang, so the existing upstream MSVC builder
must not be reused unchanged. Publisher package integrity and architecture checks
do not independently attest source-to-binary correspondence or model correctness.
[Backend support](https://github.com/PrismML-Eng/Bonsai-demo/blob/main/BACKEND-SUPPORT.md).

BITCOS is relevant to these ternary weights: it stores a presence bitmap plus
signs only for nonzeros. With the same 16-bit scale per 128 weights, the ideal
cost is `2.125 - z` bits/weight, versus PQ2's `2.125`, before indexes/alignment.
Actual savings depend on this exact pack's zero fraction `z`; the paper's older
Bonsai 27B census is not a Swift census. The released PQ2 codec can encode a
fourth `+2` symbol, so a lossless three-symbol converter must inspect and reject
unsupported data rather than trust the model's name. Copy scales without
requantization and preserve tensor transforms, tokenizer and template.
[BITCOS paper](https://arxiv.org/html/2609.16338v1).

First steps are immutable provisioning, bounded metadata inspection and an actual
streaming symbol census. Report lower-bound and block-rounded serialized sizes
separately from unknown indexing overhead. A smaller archive alone does not
reduce live weights if inference expands it into the original layout. A useful
BITCOS runtime needs a qualified ARM64 unpack-and-dot path, exact weight parity
and subsequent CI correctness/performance evidence. The paper's x86/Intel GPU
results do not establish a Snapdragon speedup. No weights, sampler or KV precision
will be silently changed to manufacture a memory result.
