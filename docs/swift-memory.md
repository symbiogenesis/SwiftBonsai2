# Swift artifacts, memory and PTQ1

**PQ2 remains the app default.** Bounded native CPU decode passes; creative
quality, performance and daily-service isolation remain unqualified. The [Qwen reference](model-selection.md) has a separate
lock/profile. No local training or fine-tuning is planned. [PLAN.md](../PLAN.md) owns
outstanding qualification. This guide covers artifacts, resource policy, lossless
packing/allocation and separately labelled approximation research.

## Pinned artifacts

[swift-assessment.lock.json](../config/swift-assessment.lock.json) pins
`Swift-Bonsai-2-PQ2_0.gguf` at revision `a3bdac086bbb6b04d87d0108d18943b5f63868f7`:
**7,206,168,928 bytes (6.71 GiB)**. The merged-weight release is distinct from the
earlier PTQ checkpoint and about 5.53 GiB smaller than Qwen; file size is not peak RAM
or speed. Sources: [publisher](https://ukisai.com/swift-bonsai-2) and [immutable
metadata](https://huggingface.co/api/models/ukisai/Swift-Bonsai-2-GGUF/revision/a3bdac086bbb6b04d87d0108d18943b5f63868f7?blobs=true).

The publisher ARM64 package is Prism `prism-b10787-73bfe6c`, commit
`73bfe6cc485554822f80c473a3c0c4f25e80f41b`: packed NEON, transforms and graph support
for PQ2/PTQ, which stock upstream binaries lack. Full builds require Clang; the upstream
MSVC builder needs changes. Integrity/architecture checks do not attest source-to-binary
correspondence or model correctness. See [backend
support](https://github.com/PrismML-Eng/Bonsai-demo/blob/main/BACKEND-SUPPORT.md).

## Actual artifact census

The size/SHA-256-verified artifact’s [metadata](../config/swift.metadata.json) records 851 tensors across PQ2/F32/BF16, 64
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

Prism **PTQ1_0** losslessly repacks these weights; it is distinct from the
weight-changing PTQ1.61 research algorithm and older published PTQ model.

The verified output is **5,946,648,928 bytes**, saving **1,259,520,000 bytes (1.173019409
GiB)**. Its [receipt](../config/swift.ptq1-export.json) records SHA-256
`e658f0f0736a03a4549535a439f9b07254ebf8ab88ae27cbe6e52d098ca5d1f5`, 209,920,000 verified
blocks and 258,720 working-buffer bytes. Native pinned PQ2/PTQ dequantizers also agree
bit-for-bit on 853,760 synthetic values.

PTQ1 uses Q8_0 activations; PQ2 uses Q8_K. Equal weights do not prove equal logits,
text or speed: see [numerical evidence](#numerical-and-admission-boundaries).
The [packing decision](#packing-decision) excludes BITCOS.

Either packing can use the experimental [LoRA](swift-lora.md) at app scale 1.3;
its behavior, runtime cost and standard-profile qualification are independent.

## Release decision

**2026-10-30: no-go for shipping PTQ1 as the app default; diagnostic use only.**
The lossless representation is not rejected. The historical [decision receipt](evidence/ptq1-release-decision.json)
binds the local and CI X2 startup failures, exact runtime/build identities and retained raw
evidence digests. Rust service [source `27e4860`](https://github.com/symbiogenesis/prism-inference/pull/2)
introduced PTQ1/approved-adapter profiles and passed its owning
[Foundation](https://github.com/symbiogenesis/prism-inference/actions/runs/41009394716)
and [conformance](https://github.com/symbiogenesis/prism-inference/actions/runs/41009394718).
The [one-token device smoke](evidence/ptq1-rust-smoke.json) passes complete numerical
comparison at scale 1.3 for that historical source. Runner now pins Rust **0.1.9**,
source `7aca2251f09acb345d4afb53d5ce411f81017745`; its [release receipt](evidence/prism-019-release.json)
binds both complete upstream CI gates, public bundles, source/helper archives and
the reviewed pin. Release integration is complete. Actual service evidence remains
versioned, and the independent qualification gates below remain open.

| Gate | Evidence and decision |
| --- | --- |
| Current-weight packing | Fresh export matches all 209,920,000 reconstructed blocks, the pinned 5,946,648,928 bytes and SHA-256 `e658f0f0736a03a4549535a439f9b07254ebf8ab88ae27cbe6e52d098ca5d1f5`. The 1,259,520,000-byte saving is real. |
| Publisher runtime startup on X2 Elite Extreme | The exact 63-file verified package at `73bfe6cc485554822f80c473a3c0c4f25e80f41b` exits `3221225794` (`0xC0000142`) on `llama-completion.exe --version` under the ordinary required worker policy. No model option or model artifact is supplied. The 512 MiB Job and ten-second deadline retain the shared 15.5 GiB admission and 4 GiB reserve; the local minimum available physical memory is 21,811,826,688 bytes. This is a startup failure, not a capacity or PTQ1/PQ2 performance comparison. |
| Historical Rust 0.1.6 app integration | This version accepts original PQ2 without LoRA; PTQ1 and adapter service support are absent. The earlier three-token diagnostic remains valid only for its no-adapter scope. |
| Rust replacement source | Source `27e4860` implements explicit PTQ1/approved-adapter service and sealed scale/reset semantics. A single supplied token at scale 1.3 on X2 matches all native logits and live state exactly. This does not establish actual service, quality or resource qualification. |
| Rust 0.1.8 required-sandbox smoke | [X2 main-only application CI](https://github.com/symbiogenesis/SwiftBonsai2Runner/actions/runs/41020705958) passes exact readiness, one scale-1.3 streamed response and owned Job/listener cleanup in a three-minute owner job. The four-token cap yields one reported output token and three visible bytes with `stop`; zero-scale and invalid-scale/reuse checks are skipped. [History](evidence/ptq1-native-service.json) retains the earlier 0.1.7 startup failure. This does not qualify broader lifecycle, answers, quality, resources or default promotion. |
| Current Rust 0.1.9 release | [Exact published artifacts and reviewed pin](evidence/prism-019-release.json) include optional row-worker support and retain one worker by default. Artifact verification does not transfer the 0.1.8 service smoke to these binaries or establish PTQ1 quality, performance or default promotion. |
| Complete correctness, answers and sustained use | Scalar/explicit NEON trajectories, PTQ1 LoRA basis/forward execution, 8K context/4K output, recovery, independent answer quality, target-device benefit and owner-accepted comfort remain unqualified. No throughput or quality verdict was collected in this session. |

The startup failure is shared by the publisher executable, so it also blocks that
PQ2 diagnostic on this host. It is not evidence that PTQ1 arithmetic is worse.
The supervisor's generic `ModelProcessStarted` field records executable launch;
the fixed `--version` command did not load model weights. The corrected
[settled-main CI run](https://github.com/symbiogenesis/SwiftBonsai2Runner/actions/runs/41007117743)
built the maintenance tool successfully and reproduced the same startup failure,
with `ModelLoaded=false` and `ModelTrialsRequested=false`. Its retrieved build and
startup artifacts are archived with exact file digests in the receipt. Both checks
are model-free; neither establishes a performance or quality comparison. The first
[owning CI attempt](https://github.com/symbiogenesis/SwiftBonsai2Runner/actions/runs/41005833161)
stopped in compiler discovery before capture; that build failure is not a PTQ1 verdict.

Further app qualification must use a reviewed runtime serving the intended format
and adapter under unchanged containment. For the publisher path, review and release a
startup fix; the [headless UCRT source candidate](runtime-candidates/README.md#headless-ucrt-argv-candidate)
is one lead, not a diagnosis of the independently built release. Then pass the exact
worker preflight in settled-main device CI. For the Rust path, qualify the current
published service and matching scalar/NEON logits and complete live state; the
historical numerical diagnostic alone does not qualify service behavior.
Then run the stable-condition prefix screen, the separate
[scale-1.3 complete-response contract](swift-lora.md#standard-profile-qualification),
full-context/output and cancellation/recovery trials, independent quality review,
and sustained resource/comfort and installation acceptance with PQ2 rollback.
All independent [promotion criteria](validation.md#promotion-criteria) still apply.

A reproducible correctness/security/recovery regression rejects the tested
configuration. A performance no-go must use the predeclared device/settings and
accepted latency/quality limits, including any capacity/energy route to promotion.
Unknown conditions or missing evidence cannot establish an intrinsic format failure.
No default, runtime pin, admission or reserve changed with this decision.

## Exact PTQ1 packing

PTQ1 stores each 128-symbol group in **28 bytes**, versus PQ2's **34**: 24 five-trit
bytes, two four-trit bytes and the unchanged two scale bytes. The pinned codec uses
ceiling-scaled base-3 codes, 16-lane then 8-lane order, and a canonical zero unused tail
trit. Conventional group-256 TQ1 cannot retain both original group-128 scales.

Independent 128-trit blocks require `ceil(128 log2(3)) = 203` bits, or 26 whole
code bytes: PTQ1 meets this bound before its scale. [Cross-block
compression](inference-algorithms.md#compression-and-approximation-constraints),
physical FP16 KV in Rust and checkpoint storage have separate contracts.

The [native
layout](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/ggml/src/ggml-common.h)
and
[codec](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/ggml/src/ggml-quants.c)
define the representation. The [Rust ternary
crate](https://github.com/symbiogenesis/prism-inference/blob/main/tools/ternary/README.md)
and [GGUF
exporter](https://github.com/symbiogenesis/prism-inference/blob/main/tools/gguf/README.md)
preserve symbols, raw scale bits, other tensor bytes and tokenizer/template/transform
metadata without floating-point quantization. Bounded caller-owned buffers and
batchwise byte-identical PQ2 reconstruction checks avoid a complete expanded model.

All converted tensor block counts are divisible by sixteen, retaining 32-byte alignment.
Non-tensor bytes occupy the 11,120,992-byte header/data-offset region; there are no
source inter-tensor or trailing gaps. Retain the original source on disk, but avoid
holding complete old/new buffers simultaneously.

The [ARM dot
kernel](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/ggml/src/ggml-cpu/arch/arm/quants.c)
decodes PTQ in place without retaining an expanded model. [CPU type
traits](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/ggml/src/ggml-cpu/ggml-cpu.c)
select the distinct activation routes documented in [numerical boundaries](#numerical-and-admission-boundaries).
Preserving PQ2's Q8_K route would require a separately qualified PTQ1×Q8_K kernel.

## Packing decision

Current-weight PTQ1 is the supported lossless packing option.
[BITCOS](https://arxiv.org/abs/2609.16338v1) (2026-09-14 preprint) is not supported.
Swift's 32.76154% zero-symbol rate falls below BITCOS's 37.5% ideal same-scale PTQ1
break-even before offsets, padding or indexes. Its proposed layout would occupy
6,158,339,606 bytes before container/index overhead, **211,690,678 bytes more** than the
verified 5,946,648,928-byte PTQ1 export.

The Cobalt 100 ARM64 synthetic prototype is also slower than PQ2, without establishing
a universal BITCOS limit, Snapdragon behavior or full-model performance. This artifact’s
serialized-size disadvantage alone excludes the proposed layout.

## Supported selection and resource policy

`scripts/Start-Chat.ps1 -UseRepackedPtq1` selects only this verified artifact on the pinned
publisher Prism CPU server; direct app/broker launches use `--use-repacked-ptq1`. The
artifact handle stays verified/held through owned teardown. Arbitrary paths, checkpoint
substitutions and missing-file fallback are unsupported. The explicit experimental
`-NativeEngine` route implements this selection and the approved adapter in Rust
[0.1.9 prerelease](evidence/prism-019-release.json); service implementation alone does
not establish release qualification. Row workers remain one unless explicitly
selected; separate source-checker evidence does not qualify app TPS.

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

Inspection/export share one Rust CLI and release buffers before inference.
`-GgufToolPath` selects a prebuilt CLI; otherwise wrappers build pinned sources
in external storage.

## Native CPU status

The fixed check uses 8K context, FP16 K/V, F32 recurrent/convolution state, batch
128/microbatch 64, four threads, a synthetic prompt and a 16-token cap. New owner-device
checks require prebuilt tools, allow at most three minutes in the worker and share a
four-minute setup/verification/execution budget, leaving cleanup inside the five-minute
total ceiling. Historical captures used a ten-minute worker deadline. The check uses
the embedded template/default reasoning without a system preamble:
the template cannot render that preamble alone without a user query.

| Accounting domain | Current bound |
| --- | ---: |
| PQ2 file + live KV/recurrent state + shared rotation + deduplicated signs/metadata | 7,904,356,704 bytes |
| Job commit allowance | 11.5 GiB |
| Remaining allowance for runtime/scratch/temporary allocations | 4,443,674,272 bytes |
| Running physical reserve | 4 GiB |
| Available physical memory required at admission | 15.5 GiB |

The budget is owner-selected, not justified by measured allocation reductions.
Full-context/4K-output peak memory remains unqualified. Qwen retains its separate 22 GiB admission requirement.

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
| Rust **0.1.7 source `27e4860`**, approved adapter **1.3**, automatic ARM64, supplied `[248045]` | [Main-only X2 CI](https://github.com/symbiogenesis/SwiftBonsai2Runner/actions/runs/41012887460) matches all **129 files / 39,504,384 logits/live-state values** byte for byte; maximum absolute error zero and greedy ID matches. Work takes 153.285 seconds; earliest recorded setup through job completion is 196 seconds, below the five-minute ceiling. Uses unchanged **15.5 GiB admission / 11.5 GiB Job / 4 GiB reserve**; Rust peak Job commit is 6,283,157,504 bytes and minimum available physical memory 17,766,457,344 bytes. The [receipt](evidence/ptq1-rust-smoke.json) binds exact tools, sources, controls and retained raw evidence. No scalar/explicit NEON, zero-scale identity, changed-nonzero effect, writer-basis, service, quality or throughput qualification. |
| Rust **0.1.6**, no adapter, automatic ARM64, supplied `[248045, 872, 198]` | All **483 files / 118,611,456 logits/live-state values** match native bytes; max absolute error zero. Greedy IDs `[198, 198, 220]` match. [Repair receipt](evidence/ptq1-arithmetic-evidence.json) binds source/checker/reference/comparison/supervision hashes and earlier failures, including 430,526 out-of-tolerance one-token values. |
| Q8_0 repair | Matches pinned DLL scale `maximum * rounded-binary32(1/127)` and inverse `127/maximum`; resolves halfway-code divergence first localized to block 13 FFN down. Generic/PQ2 preparation is unchanged. |
| Offline comparator | Reconstructs every KV/convolution/recurrent value from original hash-bound serialized native state; exact inventory and finite values required. Both historical failure controls were reproduced exactly first. |
| Diagnostic resource envelope | **12 GiB admission / 8 GiB Job / 4 GiB reserve**; peak commit 6,275,977,216 bytes (5.845 GiB), minimum available physical 8,027,590,656 (7.476 GiB). Direct Rust supervision retains caller lifetime, artifact verification, output cap and deadline. Three tokens do not qualify service admission; Job commit excludes mapped weights/total RAM. |
| Scalar control | Two supplied tokens before 180-second deadline: incomplete, not accepted. Fresh 360-second attempt rejected before launch at 9,289,871,360 available bytes versus 12 GiB required. Completed scalar/explicit NEON controls remain pending. Default-feature ARM64 build establishes NEON eligibility, without observed per-operation counters. |

The [Rust runtime contract](native-rust-engine.md#swift-cpu-supported-subset)
owns the provisioned release, source revision and current CI status; the table
above retains this diagnostic's exact version and evidence limits.
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
The workflow omits Actions credentials from the evaluation process tree. Before
model admission or artifact hashing, the verified completion executable must pass
a retained, model-free `--version` preflight under the ordinary headless worker
policy: ten seconds, 512 MiB Job, unchanged admission and physical reserve.
Failure stops the screen and records no PTQ1 suitability verdict; the pinned
publisher runtime's known X2 DLL startup failure is tracked under
[headless startup](cpu-kernels.md#ci-availability-and-headless-startup).

Inspect its plan without loading a model:

```powershell
./scripts/Invoke-Ptq1Screen.ps1 -DataDirectory "$env:LOCALAPPDATA/SwiftBonsai2" -PlanOnly
```

Inspect a capture offline:

```powershell
./scripts/Get-Ptq1Screen.ps1 -EvidencePath C:/external/screen-evidence.json
```

The screen verifies artifacts/resources, retains failures and compares median decode
throughput. Schema 2 retains native CPU identity, topology and power observations
before/after all fourteen processes and compares adjacent boundaries. Changed or
unknown conditions make timing **inconclusive**, requiring a new capture; legacy
schema-1 captures remain inspectable with the same timing limitation. Boundary
observations do not establish steady clocks, thermal stability or an idle machine.
Rejection blocks the candidate only for the declared screen; continue means only this
prefix screen did not reject it. Neither verdict qualifies completed answers, long
contexts, statistical equivalence, total physical RAM, defaults or the
scale-one/512-token LoRA contract.

## Allocation audit

This audit targets Prism `73bfe6cc485554822f80c473a3c0c4f25e80f41b`.
Storage reductions are not measured RAM/throughput gains. Lossless changes preserve
weights, KV precision, checkpoints, adapters and the [resource policy](#native-cpu-status).

### Mapping and startup

Runner's shared Windows artifact verifier streams complete SHA-256 checks through
[Windows CNG](../src/windows-credentials/src/hash.rs), avoiding the pinned
RustCrypto ARM64 detector's software fallback on Windows. Verification still reads
every byte, checks the exact length and retains the same read-only file guards;
it does not trust timestamps or a cached model digest. This also applies to runtime,
adapter and speech artifact verification. Native readiness speed remains to be
measured: the separately pinned Rust inference runtime performs its own complete
model hash using RustCrypto before loading, and that additional pass remains.

The [CPU check](../scripts/Invoke-CpuCheck.ps1) selects mmap, no repack, no warmup, CPU
buffers and 128/64 batch/microbatch. The
[loader](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/src/llama-model-loader.cpp)
can point supported CPU tensors into the read-only mapping; other buffer types may copy
them. Load receipts determine which allocations count as savings. Hashing/census/export
use bounded buffers rather than whole-file arrays or model-wide float conversion and
precede inference.

The pinned [Windows mapping
code](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/src/llama-mmap.cpp)
prefetches the complete mapping and cannot unmap fragments. Windows
[prefetch](https://learn.microsoft.com/en-us/windows/win32/api/memoryapi/nf-memoryapi-prefetchvirtualmemory)
populates the physical cache, not the process working set. Disabling it could avoid
eager startup caching, but does not reduce tensor bytes or prove sustained savings once
inference touches the weights. No current CLI switch exposes it.

### Graph and output buffers

The [context
allocator](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/src/llama-context.cpp)
reserves prompt/decode graphs and grows the persistent output buffer to the largest
requested count. With 248,320 vocabulary entries, one F32 logits row is 993,280 bytes;
reducing a retained 64-row buffer to one row would save **62,576,640 bytes**. Completion
normally samples one row and may already retain only that row; graph logits and
persistent host output have distinct lifetimes.
Single-row output, optional embeddings, capture layers, MTP and backend sampling each
have separate allocation contracts.

Microbatch 32 preserves context length but changes execution grouping, so the 64-token
profile's numerical/device evidence does not transfer to it. One F32 17,408-wide
activation occupies 4,456,448 bytes at 64 tokens and 2,228,224 at 32. These are
component sizes; do not multiply them by all layers because the planner reuses storage.
The [CPU
backend](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/ggml/src/ggml-cpu/ggml-cpu.cpp)
retains a maximum-sized work buffer. It has no current post-prefill trimming contract;
releasing it could cause later allocations.

### Other allocation bounds

The [Hadamard
implementation](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/src/llama-model.cpp)
shares one 1024² F32 rotation per CPU buffer type and signs by width/type:
4,423,680 retained rotation/sign/metadata bytes (including 114,688 sign-metadata bytes)
and a temporary 4 MiB initialization vector. Charging per transformed weight would
double-count shared storage. A butterfly rewrite changes floating-point order.

Original F32/BF16 weights are retained: 10.09 MiB F32 and 45 MiB BF16, totalling
57,767,936 bytes. Narrowing all F32 to sixteen bits could save at most 5,291,008 bytes
and would require exactness proofs. BF16 already has a [native dot
route](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/ggml/src/ggml-cpu/ggml-cpu.c);
there is no demonstrated complete F32-expanded copy to remove. FP16 can lose BF16 range.
The 8K FP16 KV/recurrent estimates remain 536,870,912 and 156,893,184 bytes; lowering
context or precision changes the contract.

[Matching thread
settings](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/common/common.cpp)
avoid a separate batch pool. Four threads mean three workers plus the caller in the
non-OpenMP path. Windows [stack
reservation](https://learn.microsoft.com/en-us/windows/win32/procthread/thread-stack-size)
is not committed/resident memory. PE headers and actual stack depth determine whether
stack settings represent a material allocation bound.

The [pinned Rust runtime's atomic checkpoint restore](native-rust-engine.md#swift-cpu-supported-subset)
stages fallible KV copies and reuses live recurrent/convolution storage, avoiding
149.625 MiB of transient Swift payload. This is neither measured peak RAM nor a
steady-state saving.

## 2026 state and cache research outside the lossless scope

The [research register](research-register.md) owns versions and proof limits for
sources reviewed through **2026-10-29**. These candidates do not change current
packing, FP16 KV/FP32 recurrent state or admission. Account separately for Swift's
16 attention and 48 recurrent layers: KV compression affects neither GDN state nor
weights.

### Persistent checkpoint compression versus live-state quantization

| Candidate ([register Q18–Q21](research-register.md#approximation-quantization-theory-and-numerical-analysis)) | Swift boundary |
| --- | --- |
| DASC | Omits checkpoint GDN heads/KDA channels, with zero-fill or approximate refresh; live state stays full precision and convolution uncompressed. Compare exact sparse checkpoints plus suffix replay first. |
| DAMP | Mixed FP16/INT8 live state is approximate. Its 9.9-bit average and batch-16–256 results establish neither exact continuation, batch-one gain nor Swift/LoRA quality. |
| Preserving Updates | Low-rank bypass and directional rounding address small-update loss. Charge factors/scales, merge scratch and checkpoint/rollback lifetimes; per-step MSE does not bound sequence error. |
| Minima W4A4 | Quantizes projections/gates, retaining FP32 recurrence and per-module fused scales. Its larger NVFP4 Qwen artifact is neither four-bit state nor lossless Swift repacking; uniform stability remains unproved. |

### Low-rank full-attention KV and reconstruction costs

| Candidate ([register Q25–Q26](research-register.md#approximation-quantization-theory-and-numerical-analysis), [H23](research-register.md#hybrid-recurrence-prefix-caching-and-speculative-decoding)) | Representation and accounting |
| --- | --- |
| iS-KV | Exact recent window, approximate older SVD history; basis updates transform historical coordinates. Count bases, coefficients, protected prompt/window, FP32 updates, reconstruction and simultaneous old/new factors; persistent-KV ratios exclude weights/scratch. |
| JoLT | Tucker cache plus rotated low-bit residual, jointly budgeted. "Near-lossless" describes empirical quality; retained batch-one A100 decode is **7.5–65.9× slower** than dense. Count decomposition, reconstruction and residual scratch. |
| VeriCache | Compressed drafts plus full verification cache, qualified for greedy output. Count both caches, transfers, verification and hybrid state: GPU-residency savings can increase total storage, and one shared-memory request lacks the evaluated resource overlap. |

An exact recent window does not make older low-rank history exact. Basis and
coefficient/checkpoint identities must commit or roll back together. None of these
replaces exact FP16-bit storage without separate qualification.

Report `unique weights + unique KV + live recurrent state + retained checkpoints +
pending factors/bypass + rollback + peak scratch + metadata`, with mappings/device
allocations in separate counter domains. Reserve overlapping old/new representations
and asynchronous users. [Algorithm constraints](inference-algorithms.md#2026-state-traffic-and-verification-research)
cover buffered/speculative state. Approximation needs its own artifact or numerical
profile, full-continuation/quality checks and target-device CI evidence.

## 2026 entropy coding and alternative weight artifacts

Lossless byte coding and new quantized weights have different contracts. Smaller files
or lower reconstruction error do not establish fewer executable bytes or faster
generation. Source versions and results live in the register's [compression](research-register.md#packed-projections-cpu-kernels-and-compression)
and [quantization](research-register.md#approximation-quantization-theory-and-numerical-analysis) catalogs.

| Candidate / register ID | Memory and arithmetic boundary |
| --- | --- |
| Shannonic / L12 | Lossless ANS can preserve symbols/scales; charge table state, independent-stream indexes and decode work. Communication wins do not prove local inference savings. |
| QTEA / L13 | New ternary/residual artifact; charge residuals/indexes. GPU/architecture estimates do not establish CPU benefit. |
| Tetra / L16 | Distinguish serialized bits from streamed codebook/scale/layout bytes; new weights have measured quality losses. |
| CoS-VQ / Q27 | Shared-scale/codeword search on the Qwen base family; surrogate optimality needs true-error verification and proves no global quality/memory optimum or Swift compatibility. |
| YAQA / Q28; FastKron / Q29 | Offline rounding/quantization: charge Hessians/factors and preprocessing. Work/depth gains do not imply faster generation. |
| Search Your Block Floating Point Scales! / Q30 | Exhaustive legal-scale optimality differs from a heuristic short window; changing scales changes Q8_K/Q8_0 arithmetic. |

Cross-block entropy coding does not contradict the independent fixed-block bound.
Changing codebooks, scales, rotations, sparsity or residual weights is not lossless
export. New artifacts require their own length/hash, activation, quality, compatibility
and runtime evidence. The [SIMD
review](cpu-kernels.md#cpu-kernel-research-reviewed-in-october-2026) and register also
retain unsupported and withdrawn lattice/rotation/hardware leads.
