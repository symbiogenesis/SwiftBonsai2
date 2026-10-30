# Swift artifacts, memory and PTQ1

**Swift is the implementation priority; PQ2 remains the app default.** Bounded
native CPU decode passes, while creative quality, performance and daily-service
isolation remain unqualified. The [Qwen reference](model-selection.md) has a separate
lock/profile. No local training or fine-tuning is planned. [PLAN.md](../PLAN.md) owns
outstanding qualification; [memory options](swift-memory-options.md) owns the
allocation audit and separately labelled approximation research.

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

The verified output is **5,946,648,928 bytes**, saving **1,259,520,000 bytes (1.173019409
GiB)**. Its [receipt](../config/swift.ptq1-export.json) records SHA-256
`e658f0f0736a03a4549535a439f9b07254ebf8ab88ae27cbe6e52d098ca5d1f5`, 209,920,000 verified
blocks and 258,720 working-buffer bytes. Native pinned PQ2/PTQ dequantizers also agree
bit-for-bit on 853,760 synthetic values.

PTQ1 uses Q8_0 activations; PQ2 uses Q8_K. Equal weights do not prove equal logits,
text or speed. [Selection](#supported-selection-and-resource-policy) and
[numerical evidence](#numerical-and-admission-boundaries) below keep those claims
separate from packing. The [packing decision](swift-memory-options.md#packing-decision)
excludes BITCOS.

The pretrained [LoRA](swift-lora.md) changes behavior and adds runtime cost; it has
independent standard-profile qualification. Either packing can use it experimentally
at app scale 1.3.

## Supported selection and resource policy

`scripts/Start-Chat.ps1 -UseRepackedPtq1` selects only this verified artifact on the pinned
publisher Prism CPU server; direct app/broker launches use `--use-repacked-ptq1`. The
artifact handle stays verified/held through owned teardown. Arbitrary paths, checkpoint
substitutions and missing-file fallback are unsupported. The experimental Rust service
still rejects PTQ1; codec/operator support does not establish service parity.

Both packings retain the [CPU resource policy](#native-cpu-status), context, retry
and recovery. Serialized savings alone do not lower admission or promote PTQ1.

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

This owner-selected budget is 512 MiB below the original 12/16 GiB development
budget; allocation changes did not establish the reduction. Full-context/4K-output
peak memory remains unqualified. Qwen retains its separate 22 GiB admission requirement.

PQ2 and current-weight PTQ1 both complete the bounded decode, with and without the
approved adapter. The recorded base check reported 1,487,458,304 bytes peak Job commit.
Job commit excludes mapped weight residency and is not total RAM; working-set/private-byte
and available-memory watchdogs remain required. Sixteen tokens establish loading and
decode only, not a finished answer, creative quality, throughput or daily isolation.

## Numerical and admission boundaries

The distinct activation routes require full-model comparisons bound to exact
artifact/reference, graph arithmetic, scalar/NEON backend and complete live
KV/convolution/recurrent state, with tolerance `1e-3 + 1e-3 * abs(reference)`. Matching
token IDs cannot override a logits/state failure.

| Retained evidence | Result and limit |
| --- | --- |
| Rust **0.1.6**, no adapter, automatic ARM64, supplied `[248045, 872, 198]` | All **483 files / 118,611,456 logits/live-state values** match native bytes; max absolute error zero. Greedy IDs `[198, 198, 220]` match. [Repair receipt](evidence/ptq1-arithmetic-evidence.json) binds source/checker/reference/comparison/supervision hashes and earlier failures, including 430,526 out-of-tolerance one-token values. |
| Q8_0 repair | Matches pinned DLL scale `maximum * rounded-binary32(1/127)` and inverse `127/maximum`; resolves halfway-code divergence first localized to block 13 FFN down. Generic/PQ2 preparation is unchanged. |
| Offline comparator | Reconstructs every KV/convolution/recurrent value from original hash-bound serialized native state; exact inventory and finite values required. Both historical failure controls were reproduced exactly first. |
| Diagnostic resource envelope | **12 GiB admission / 8 GiB Job / 4 GiB reserve**; peak commit 6,275,977,216 bytes (5.845 GiB), minimum available physical 8,027,590,656 (7.476 GiB). Direct Rust supervision retains caller lifetime, artifact verification, output cap and deadline. Three tokens do not qualify service admission; Job commit excludes mapped weights/total RAM. |
| Scalar control | Two supplied tokens before 180-second deadline: incomplete, not accepted. Fresh 360-second attempt rejected before launch at 9,289,871,360 available bytes versus 12 GiB required. Completed scalar/explicit NEON controls remain pending. Default-feature ARM64 build establishes NEON eligibility, without observed per-operation counters. |

Exact runtime main `02c8a24` passed
[Foundation](https://github.com/symbiogenesis/prism-inference/actions/runs/40449357453)
and
[conformance](https://github.com/symbiogenesis/prism-inference/actions/runs/40449357329).
Runner pins the verified Windows bundles from
[0.1.6](https://github.com/symbiogenesis/prism-inference/releases/tag/v0.1.6).
Longer/full-context trajectories, adapters, completed app-setting answers, recovery
and resource/comfort remain pending.
The scale-one LoRA harness does not prove PTQ1/PQ2 answer parity at app scale 1.3.

Subtracting the exact packing difference suggests **15,383,478,272 bytes** candidate
admission only if suitability and resource evidence justify it; it is not enforced.
Mapped residency and private commit are distinct. File savings cannot lower the Job
ceiling/admission by themselves; the physical reserve stays mandatory.

## Bounded decision-screen interface

The manual main-only `swift-ptq1-screen.yml` workflow uses already provisioned artifacts
on the native ARM64 Snapdragon runner. Its fixed workload is one creative prompt, both
packings, adapter off/on at 1.3 and three interleaved repetitions: **12 timed 64-token
prefixes plus two zero-scale controls**. All benchmark execution is CI-only.

Inspect its plan without loading a model:

```powershell
./scripts/Invoke-Ptq1Screen.ps1 -DataDirectory "$env:LOCALAPPDATA/SwiftBonsai2" -PlanOnly
```

Inspect a capture offline:

```powershell
./scripts/Get-Ptq1Screen.ps1 -EvidencePath C:/external/screen-evidence.json
```

The screen verifies artifacts/resources, retains failures and compares each
configuration's median decode throughput. Rejection blocks the candidate; continue
means only that this small prefix screen did not reject it. It cannot establish
completed-answer quality, long-context/statistical equivalence, total physical RAM,
default promotion or acceptance under the separate scale-one/512-token LoRA contract.
