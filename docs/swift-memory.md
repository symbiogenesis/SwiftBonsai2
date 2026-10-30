# Swift Bonsai 2 memory assessment

**Swift is the implementation priority.** Bounded native CPU decode passes; creative
quality, performance and daily-service isolation remain unqualified. The [Qwen
reference](model-selection.md) has a separate lock/profile. No local training or
fine-tuning is planned.

## Pinned artifacts

[swift-assessment.lock.json](../config/swift-assessment.lock.json) pins
`Swift-Bonsai-2-PQ2_0.gguf` at revision `a3bdac086bbb6b04d87d0108d18943b5f63868f7`:
**7,206,168,928 bytes (6.71 GiB)**. The merged-weight release is distinct from the
earlier PTQ checkpoint and about 5.53 GiB smaller than Qwen; file size is not peak RAM
or speed. Sources: [publisher](https://ukisai.com/swift-bonsai-2) and [immutable
metadata](https://huggingface.co/api/models/ukisai/Swift-Bonsai-2-GGUF/revision/a3bdac086bbb6b04d87d0108d18943b5f63868f7?blobs=true).

The native ARM64 package is Prism `prism-b10787-73bfe6c`, commit
`73bfe6cc485554822f80c473a3c0c4f25e80f41b`: packed NEON, transforms and graph support
for PQ2/PTQ, which stock upstream binaries lack. Full builds require Clang; the upstream
MSVC builder needs changes. Integrity/architecture checks do not attest source-to-binary
correspondence or model correctness. See [backend
support](https://github.com/PrismML-Eng/Bonsai-demo/blob/main/BACKEND-SUPPORT.md).

## Actual artifact census

The complete file passed size/SHA-256 verification. Its
[metadata](../config/swift.metadata.json) records 851 tensors across PQ2/F32/BF16, 64
main blocks, no MTP block, and tokenizer/template identities. The streamed
[census](../config/swift.pq2-census.json) used a 64 KiB payload buffer and found 402 PQ2
tensors, 209,920,000 blocks and 26,869,760,000 encoded symbols:

| Symbol | Count |
| --- | ---: |
| −1 | 9,042,814,485 |
| 0 | 8,802,947,576 (32.76154%) |
| +1 | 9,023,997,939 |
| +2 | 0 |

Every scale is finite and nonzero. All 402 tensors qualify for lossless three-symbol
repacking under the census rules; no floating-point weight conversion is involved.

## Current-weight PTQ1 option

Prism **PTQ1_0** uses group-128 base-3 packing, distinct from the weight-changing
PTQ1.61 research algorithm and older published PTQ model. The [exact-packing
audit](swift-memory-options.md#exact-ptq1-packing) documents the exporter, preserved
symbols/scales/tensors/metadata and batchwise byte-identical PQ2 reconstruction.

The verified output is **5,946,648,928 bytes**, saving **1,259,520,000 bytes (1.1730
GiB)**. Its [receipt](../config/swift.ptq1-export.json) records SHA-256
`e658f0f0736a03a4549535a439f9b07254ebf8ab88ae27cbe6e52d098ca5d1f5`, 209,920,000 verified
blocks and 258,720 working-buffer bytes. Native pinned PQ2/PTQ dequantizers also agree
bit-for-bit on 853,760 synthetic values.

PTQ uses Q8_0 activations; PQ2 uses Q8_K. Equal weights do not prove equal logits,
text or speed. The [PTQ1 contract](ptq1-readiness.md) owns selection, numerical evidence
and unchanged admission; the [packing decision](swift-memory-options.md#packing-decision)
excludes BITCOS.

The pretrained [LoRA](swift-lora.md) changes behavior and adds runtime cost; it has
independent standard-profile qualification.

## Reproduce the assessment

Use native ARM64 PowerShell 7.6, the existing owner-selected external data directory and
a new external report path. These explicit locks leave the Qwen default unchanged:

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

Inspection/export share one Rust CLI. Complete them and release buffers before
inference. `-GgufToolPath` selects a prebuilt CLI; otherwise wrappers build pinned
sources in external storage.

## Native CPU status

The fixed check uses an 8K context, FP16 K/V, F32 recurrent/convolution state, batch
128/microbatch 64, four threads, a synthetic prompt, a 16-token cap and a ten-minute
deadline. It retains Swift's embedded template/default reasoning and omits the explicit
system preamble, which the template cannot render alone without a user query.

| Accounting domain | Current bound |
| --- | ---: |
| PQ2 file + live KV/recurrent state + shared rotation + deduplicated signs/metadata | 7,904,356,704 bytes |
| Job commit allowance | 11.5 GiB |
| Remaining allowance for runtime/scratch/temporary allocations | 4,443,674,272 bytes |
| Running physical reserve | 4 GiB |
| Available physical memory required at admission | 15.5 GiB |

The owner chose this 512 MiB reduction from the original 12/16 GiB development budget;
allocation changes did not establish it. Full-context/4K-output peak memory remains
unqualified. Qwen retains its separate 22 GiB admission requirement.

PQ2 and current-weight PTQ1 both complete the bounded decode, with and without the
approved adapter. A recorded base check reported about 1.49 GB peak Job commit. Job
commit excludes mapped weight residency and is not total RAM; working-set/private-byte
and available-memory watchdogs remain required. Sixteen tokens establish loading and
decode only, not a finished answer, creative quality, throughput or daily isolation.
