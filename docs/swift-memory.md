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

## Actual artifact census

The complete file passed size/SHA-256 verification, then the inspector streamed
its PQ2 payload through a 64 KiB buffer. [Metadata](../config/swift.metadata.json)
records 851 tensors across PQ2/F32/BF16, 64 main blocks, no MTP block, and the
embedded template/tokenizer identities. The [portable census](../config/swift.bitcos-census.json)
records 402 PQ2 tensors, 209,920,000 blocks and 26,869,760,000 symbols:

| Symbol | Count |
| --- | ---: |
| −1 | 9,042,814,485 |
| 0 | 8,802,947,576 (32.76154%) |
| +1 | 9,023,997,939 |
| +2 | 0 |

No zero or nonfinite scales were found. Under the conservative census rules all
402 PQ2 tensors are eligible for three-symbol lossless repacking. These counts
describe encoded symbols; the census never converts weights to floating point.

| Layout accounting | Bytes | GiB |
| --- | ---: | ---: |
| Existing complete PQ2 file | 7,206,168,928 | 6.7113 |
| Tensor-packed sign lower bound, retaining other bytes | 6,105,800,658 | 5.6865 |
| Signs rounded to bytes within each 128-weight block | 6,158,339,606 | 5.7354 |

The block-rounded candidate saves 1,047,829,322 bytes (0.9759 GiB, about 14.54%)
before new indexing/alignment/container overhead. It retains the original scale
bytes and existing non-PQ tensors/header allowance. This is a serialized-size
estimate from actual data, not an existing converted GGUF or a RAM/speed result.

`Pq2BitcosCodec` supplies a bounded reference proof for one block: two unchanged
scale bytes, sixteen presence bytes and `ceil(nonzero/8)` sign bytes. Decoding
recreates the exact original 34 bytes, including scale bits and code order.
Focused regressions cover sparse/dense/sign boundaries, signed and subnormal
scales, fourth-code rejection, truncation, excess bytes and noncanonical padding.
The [standalone ARM64 prototype](../tools/bitcos/README.md) now feeds compact
blocks directly into NEON integer dots, preserving PQ2's Q8_K inputs for its
single-row path. Strict native builds and focused dot parity pass. A row index
adds up to about 21 MB under the documented width condition, retaining about
0.956 GiB of estimated savings. It has no ggml/runtime integration. Full native
correctness and manually dispatched synthetic timing comparisons run in CI;
hosted results do not qualify Snapdragon end-to-end speed.

## Existing ternary packing alternative

Prism's **PTQ1_0** is group-128 base-3 ternary packing, distinct from the
unrelated weight-changing **PTQ1.61** research algorithm. A bounded
[exporter](../scripts/Export-SwiftPtq1.ps1) repacks the current Swift symbols
and raw scales without training or requantization. Each batch reconstructs
the original PQ2 bytes exactly; other tensor payloads and raw metadata remain
unchanged except the file/tensor type and offsets. Native pinned PQ2/PTQ
dequantizers also agree bit-for-bit on 853,760 synthetic values.

The verified complete PTQ file is 5,946,648,928 bytes, saving 1,259,520,000
bytes (1.1730 GiB). Its [portable export receipt](../config/swift.ptq1-export.json)
records SHA-256 `e658f0f0736a03a4549535a439f9b07254ebf8ab88ae27cbe6e52d098ca5d1f5`
and verification of all 209,920,000 converted blocks using 258,720 buffer bytes.
This uses the current merged weights, not the older
published PTQ checkpoint. PTQ1 and BITCOS are alternative encodings; their
savings are not additive. At Swift's measured zero fraction PTQ1 is smaller.
The native PTQ backend uses Q8_0 activations while PQ2 uses Q8_K, so equal
weights do not prove equal logits, output or speed. An explicit development
`-UseRepackedPtq1` switch selects only this exact derived artifact and retains
the same memory allowance. Its bounded 16-token native decode passes. See the
[quantified allocation audit](swift-memory-options.md).

The owner excludes new training on this machine; compatible pretrained LoRAs
may be qualified separately. They are behavior changes with additional runtime
cost, rather than lossless memory savings. The approved adapter remains an
explicit development assessment option.

## Reproduce the assessment

Use native ARM64 PowerShell, an existing owner-selected external data directory,
and new external report paths. The explicit assessment lock does not replace
the retained Qwen default or its serving policy:

```powershell
$lockPath = Join-Path $PWD 'config/swift-assessment.lock.json'
$lock = Get-Content -Raw -LiteralPath $lockPath | ConvertFrom-Json -DateKind String
./scripts/Get-Model.ps1 -DataDirectory $data -LockPath $lockPath
./scripts/Get-Engine.ps1 -DataDirectory $data -LockPath $lockPath
$model = Join-Path $data -ChildPath 'models', $lock.model.revision, $lock.model.filename
./scripts/Read-Gguf.ps1 -ModelPath $model -ExpectedBytes $lock.model.bytes -ExpectedSha256 $lock.model.sha256 `
    -Dialect Prism-73bfe6cc485554822f80c473a3c0c4f25e80f41b -CensusPq2 -OutputPath D:\Reports\new-swift-census.json
./scripts/Invoke-CpuCheck.ps1 -DataDirectory $data -Profile swift-pq2-assessment -CheckOnly
./scripts/Invoke-CpuCheck.ps1 -DataDirectory $data -Profile swift-pq2-assessment
```

Run the optional export in a fresh native ARM64 PowerShell process, with the
same external `$data` and source `$model` paths. This loads the complete exporter
type set rather than the metadata-only inspector:

```powershell
# Optional exact current-weight export, published only after verification.
$derived = Join-Path ([IO.Path]::GetDirectoryName($model)) 'Swift-Bonsai-2-current-repacked-PTQ1_0.gguf'
./scripts/Export-SwiftPtq1.ps1 -ModelPath $model -OutputPath $derived
./scripts/Invoke-CpuCheck.ps1 -DataDirectory $data -Profile swift-pq2-assessment -UseRepackedPtq1
# Approved pretrained adapter; explicit and separately qualified.
./scripts/Get-Adapter.ps1 -DataDirectory $data
./scripts/Invoke-CpuCheck.ps1 -DataDirectory $data -Profile swift-pq2-assessment -UseRepackedPtq1 -UseAbliterateLora
```

The verified derived artifact passes bounded native decode both alone and with
the approved adapter. These checks preserve the same 16-token limit; they do not
qualify answer quality, reliable refusal changes, latency or throughput.

The Swift-only CPU check uses 8K FP16 state, batch 128 / microbatch 64 and the
same fixed synthetic prompt, 16-token limit and ten-minute timeout. Source-level
accounting includes the entire file, live KV/recurrent state, one shared rotation
matrix and deduplicated sign vectors/metadata: 7,904,356,704 known bytes. A 12 GiB
service/Job committed-memory limit leaves 4,980,545,184 bytes for remaining
runtime/scratch/temporary allocations; admission retains a separate 4 GiB
physical reserve, requiring 16 GiB available. This is an explicit bounded
development allowance, not a reduction of the Qwen reference's 22 GiB admission.
Windows Job commit limits do not cap all mapped physical residency; the existing
working-set/private-byte and available-memory watchdog remains active.

## Native CPU status

The corrected Swift profile completes its fixed 16-token CPU decode and exits
zero inside the original 12 GiB Job allowance and 4 GiB physical reserve.
Sixteen tokens exercise decode; they do not establish a useful finished answer,
creative quality, performance or daily isolation. Windows reported about
1.49 GB Job peak commit, which excludes mapped weight residency and is not a
total RAM measurement.

Earlier attempts ended in `0xc0000409`. A bounded debugger child localized an
unhandled C++ exception: completion formatted the explicit system message alone
before appending the user, and Swift's template raises `No user query found in
messages.` The Swift-only assessment now omits that system preamble and retains
its embedded template/default reasoning. Qwen reference arguments are unchanged.
No engine, template, model weights or global system setting was patched.

## ARM prototype performance gate

The first [hosted comparison](https://github.com/symbiogenesis/SwiftBonsai2/actions/runs/39985113979)
tested `b41b466` on Cobalt 100 ARM64 with MSVC 19.51.36260.0 and DOTPROD.
The initial scalar-cursor/nibble-LUT BITCOS decoder was about 7.2× slower than
the optimized PQ2 comparator in the hot-footprint case and 10.3× slower in the
44.6 MB PQ2-footprint case, despite smaller weights. This fails promotion.
These are single-thread synthetic kernels, not Snapdragon/end-to-end results;
the footprint name does not establish cache misses. The adapted PTQ/Q8_K
comparator is not Prism's released PTQ/Q8_0 backend. No runtime change follows
from these numbers. A separately tested vector decoder may be assessed next.
