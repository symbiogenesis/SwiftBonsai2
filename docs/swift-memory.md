# Swift Bonsai 2 memory assessment

**Swift Bonsai 2 is the current memory and implementation priority.** The
[Qwen reference](model-selection.md) remains reproducible through its default
lock; it is not a compatible Swift profile. Swift's bounded native CPU decode
passes, but creative quality, performance and daily isolation remain unqualified.
Daily chat is disabled. No new training or fine-tuning is planned on this machine.

## Pinned artifacts

[swift-assessment.lock.json](../config/swift-assessment.lock.json) pins
`Swift-Bonsai-2-PQ2_0.gguf` at revision
`a3bdac086bbb6b04d87d0108d18943b5f63868f7`: **7,206,168,928 bytes (6.71 GiB)**.
This is the current merged-weight release, distinct from the earlier PTQ
checkpoint. It is about 5.53 GiB smaller than Qwen's file; file size does not
measure peak RAM or speed. Sources: [publisher](https://ukisai.com/swift-bonsai-2)
and [immutable metadata](https://huggingface.co/api/models/ukisai/Swift-Bonsai-2-GGUF/revision/a3bdac086bbb6b04d87d0108d18943b5f63868f7?blobs=true).

The native ARM64 CPU package is Prism `prism-b10787-73bfe6c`, commit
`73bfe6cc485554822f80c473a3c0c4f25e80f41b`. Stock upstream binaries do not support
its PQ2/PTQ types. Prism supplies packed ARM NEON kernels and the required
transforms/graph support. Full source builds require Clang; the upstream MSVC
builder cannot be reused unchanged. Package integrity and architecture checks
do not attest source-to-binary correspondence or model correctness. See
[backend support](https://github.com/PrismML-Eng/Bonsai-demo/blob/main/BACKEND-SUPPORT.md).

## Actual artifact census

The complete file passed size/SHA-256 verification. Its
[metadata](../config/swift.metadata.json) records 851 tensors across PQ2/F32/BF16,
64 main blocks, no MTP block, and tokenizer/template identities. The streamed
[census](../config/swift.pq2-census.json) used a 64 KiB payload buffer and found
402 PQ2 tensors, 209,920,000 blocks and 26,869,760,000 encoded symbols:

| Symbol | Count |
| --- | ---: |
| −1 | 9,042,814,485 |
| 0 | 8,802,947,576 (32.76154%) |
| +1 | 9,023,997,939 |
| +2 | 0 |

Every scale is finite and nonzero. All 402 tensors qualify for lossless
three-symbol repacking under the census rules; no floating-point weight
conversion is involved.

## Current-weight PTQ1 option

Prism **PTQ1_0** uses group-128 base-3 packing. It is distinct from the
weight-changing PTQ1.61 research algorithm and the older published PTQ model.
The [Rust exporter](https://github.com/symbiogenesis/prism-inference/blob/main/tools/gguf/README.md), exposed through
[Export-SwiftPtq1.ps1](../scripts/Export-SwiftPtq1.ps1), preserves current symbols,
raw scale bits, other tensor bytes and tokenizer/template/transform metadata.
It verifies byte-identical PQ2 reconstruction for every converted batch.

The verified output is **5,946,648,928 bytes**, saving **1,259,520,000 bytes
(1.1730 GiB)**. Its [receipt](../config/swift.ptq1-export.json) records SHA-256
`e658f0f0736a03a4549535a439f9b07254ebf8ab88ae27cbe6e52d098ca5d1f5`,
209,920,000 verified blocks and 258,720 working-buffer bytes. Native pinned
PQ2/PTQ dequantizers also agree bit-for-bit on 853,760 synthetic values.

PTQ uses Q8_0 activations while PQ2 uses Q8_K. Equal weights therefore do not
prove equal logits, text or speed. `-UseRepackedPtq1` selects only this exact
derived artifact and retains the same memory allowance. Bounded 16-token native
decode passes with and without the approved adapter. See the
[allocation audit](swift-memory-options.md); the
[packing decision](swift-memory-options.md#packing-decision) excludes BITCOS.

A pretrained [LoRA](swift-lora.md) is a separate behavioral change with runtime
cost, not lossless memory savings. Its explicit development switch does not
qualify the intended standard profile.

## Reproduce the assessment

Use native ARM64 PowerShell 7.6, the existing owner-selected external data
directory and a new external report path. These explicit locks leave the Qwen
default unchanged:

```powershell
$data = Join-Path $env:LOCALAPPDATA 'SwiftBonsai2'
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

Optional exact export and adapter assessment:

```powershell
$derived = Join-Path ([IO.Path]::GetDirectoryName($model)) 'Swift-Bonsai-2-current-repacked-PTQ1_0.gguf'
./scripts/Export-SwiftPtq1.ps1 -ModelPath $model -OutputPath $derived
./scripts/Invoke-CpuCheck.ps1 -DataDirectory $data -Profile swift-pq2-assessment -UseRepackedPtq1
./scripts/Get-Adapter.ps1 -DataDirectory $data
./scripts/Invoke-CpuCheck.ps1 -DataDirectory $data -Profile swift-pq2-assessment -UseRepackedPtq1 -UseAbliterateLora
```

Inspection and export use the same Rust CLI and can run in one PowerShell
session. Finish hashing/census/export and close their buffers before inference.
The wrappers can use a prebuilt CLI through `-GgufToolPath`; otherwise they
build pinned sources in external storage.

## Native CPU status

The fixed check uses an 8K context, FP16 K/V, F32 recurrent/convolution state,
batch 128/microbatch 64, four threads, a
synthetic prompt, a 16-token cap and a ten-minute deadline. It retains Swift's
embedded template/default reasoning and omits the explicit system preamble,
which the template cannot render alone without a user query.

Known allocation accounting includes the entire PQ2 file, live KV/recurrent
state, shared rotation and deduplicated signs/metadata: **7,904,356,704 bytes**.
The **12 GiB** Job commit allowance leaves 4,980,545,184 bytes for remaining
runtime/scratch/temporary allocations. A separate **4 GiB** physical reserve
requires **16 GiB available** for admission. These are development limits;
they do not lower Qwen's 22 GiB admission requirement.

PQ2 and current-weight PTQ1 both complete the bounded decode, with and without
the approved adapter. A recorded base check reported about 1.49 GB peak Job
commit. Job commit excludes mapped weight residency and is not total RAM;
working-set/private-byte and available-memory watchdogs remain required.
Sixteen tokens establish loading and decode only, not a finished answer,
creative quality, throughput or daily isolation.
