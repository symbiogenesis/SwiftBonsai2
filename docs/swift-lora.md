# Swift LoRA: artifact, basis and qualification

The native app enables the pinned OrcaRouter adapter experimentally at scale **1.3**;
`Start-Chat.ps1 -NoAbliterateLora` opts out. Settings accepts 0 to 2 in 0.1 steps for the
next response and every Extra effort pass, without reloading; zero disables correction
but retains the adapter. Bounded native ARM64 loading/decode passes, but **Swift basis
correctness, effectiveness, quality and standard/daily-service acceptance are unqualified**.
No optimal scale is established. The broker has an explicit development option;
no local training is needed.
[PLAN.md](../PLAN.md#creative-quality-adapters-and-memory) owns remaining work.

## Artifact and published evidence

[swift-lora.lock.json](../config/swift-lora.lock.json) pins
`Continuum-AI-Corp/OrcaBonsai-27B-Uncensored`, revision
`703028bb7fba03496e796b144839b161cf0f5409`, path `gguf/bonsai-abliterate-lora.gguf`:
**9,682,464 bytes**, SHA-256
`f1669534803d340a496015f5c45125f3437b4d13ec764f40e34488ce83967f42`, Apache-2.0.
Downloaded size/digest are verified. The [pinned
README](https://github.com/Continuum-AI-Corp/OrcaBonsai-27B-Uncensored/blob/703028bb7fba03496e796b144839b161cf0f5409/README.md#gguf--llamacpp)
describes rank 1, alpha 1 and testing on original Ternary Bonsai **PTQ1**; it leaves
**PQ2** untested. Those tests do not establish transfer to Swift's modified weights. The
repository's `swift/` directory is Apple-language code, not evidence about the Swift
Bonsai model.

Provision or reverify the fixed artifact:

```powershell
./scripts/Get-Adapter.ps1 -DataDirectory "$env:LOCALAPPDATA/SwiftBonsai2"
./scripts/Get-Adapter.ps1 -DataDirectory "$env:LOCALAPPDATA/SwiftBonsai2" -VerifyOnly
```

The destination is `adapters/<pinned-source-commit>/bonsai-abliterate-lora.gguf` in the
existing external data directory. The bounded downloader verifies size/hash; it accepts
no alternative lock or URL and enables no profile.

Other candidates are not qualified alternatives:

| Candidate | Evidence boundary |
| --- | --- |
| [AtomicChat original-Bonsai LoRAs, `bfb7f8a`](https://huggingface.co/AtomicChat/Ternary-Bonsai-2-27B-Uncensored-LoRA-GGUF/blob/bfb7f8a74dd01625bf30ff63d798786fe27719b4/README.md) | Publisher reports original PTQ1/PQ2 and grouped SSM-order tests. Its own42/own38 artifacts remain unqualified on this Swift pack/host. |
| [Swift KATANA, `fe9f2da7`](https://huggingface.co/ayshinko/Swift-Bonsai-2-27B-Katana-MTP-GGUF/blob/fe9f2da74ee3d47b2455158461172c2561ef57c3/README.md#experimental-compatibility-status) | A separate model integration. Orca structural compatibility is claimed without Swift inference validation; GPU/MTP examples differ from this CPU profile. |

## Structural compatibility and unresolved basis

The verified adapter has 258 F32 tensors, `general.architecture=qwen35`,
`adapter.type=lora` and alpha 1. All 129 target names and rank-1 shapes match the actual
851-tensor Swift base. Dimensions use GGUF order, first dimension fastest:

| Target family | Sites | Base shape | LoRA A | LoRA B |
| --- | ---: | --- | --- | --- |
| `blk.N.ffn_down.weight` | 64 | `[17408,5120]` | `[17408,1]` | `[1,5120]` |
| `blk.N.attn_output.weight` | 16 | `[6144,5120]` | `[6144,1]` | `[1,5120]` |
| `blk.N.ssm_out.weight` | 48 | `[6144,5120]` | `[6144,1]` | `[1,5120]` |
| `token_embd.weight` | 1 | `[5120,248320]` | `[1,248320]` | `[1,5120]` |

The [Prism
graph](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/src/llama-graph.cpp)
feeds original activation `cur` to LoRA and separately transformed `cur_mm` to folded
base weights. Do not apply the base Hadamard transform to both branches. Embedding
correction follows inverse-Hadamard lookup. The [adapter
loader](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/src/llama-adapter.cpp)
checks shapes; the [Qwen35
graph](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/src/models/qwen35.cpp)
uses LoRA-aware operations for the target families.

**SSM input order remains unresolved.** Swift records
`prism.hadamard.gdn_v_grouped=true`. Its
[loader](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/src/llama-model.cpp)
applies a three-way tiled/grouped permutation to base-weight inputs while the adapter
receives `cur`. Orca's
[exporter](https://github.com/Continuum-AI-Corp/OrcaBonsai-27B-Uncensored/blob/703028bb7fba03496e796b144839b161cf0f5409/scripts/export_gguf_lora.py)
builds A from an unfolded checkpoint and asserts no SSM permutation is needed;
AtomicChat reports an explicit order correction. Different source-matrix orders could
explain that disagreement. Orca accepts an external checkpoint directory, and the
shipped metadata declares neither its revision nor input order.

The exporter's optional `--check` does not resolve this gate. It checks only layer
0, compares inverse-Hadamard stored columns without the native SSM input
permutation, and imposes no absolute SSM-error ceiling. An identity mapping that
scores best in that comparison is not proof of the native writer's input basis.
The producer [does not ship the checkpoint](https://github.com/Continuum-AI-Corp/OrcaBonsai-27B-Uncensored/blob/703028bb7fba03496e796b144839b161cf0f5409/README.md#gguf--llamacpp).
Resolving this needs an immutable checkpoint/conversion identity bound to the
approved adapter hash, an explicit channel mapping and independent verification
of all 48 shipped SSM A vectors under that mapping. Reproduction can run on
hosted infrastructure; matching native inference alone cannot replace it.

Native residual-writer evidence must establish the correction's basis on Swift;
loading or changed text cannot. Even correct input order does not establish that an
original-Bonsai correction suits Swift's modified weights. The approved file is unmodified.

## Memory and execution cost

The adapter contains **9,664,512 F32 payload bytes** (about 9.23 MiB on disk). Prism
retains packed base weights and adds adapter branches rather than merging a dense
replacement. Each of 128 linear sites adds two rank-1 multiplications, scale and
addition; the embedding uses lookup plus multiplication. Backend buffers, graph nodes,
intermediates and changed lifetimes add overhead beyond the file's payload.

Bounded 16-token checks pass with peak Job commit of **2,074,329,088 bytes**
with the adapter and **1,487,458,304 bytes** for the base: a diagnostic difference of
**586,870,784 bytes (559.68 MiB)**. These separate process counters exclude mapped
weights and do not isolate adapter allocation. They establish bounded loading/decode,
not reliable refusal changes, quality or performance.

The difference does not isolate persistent buffers, workspace high-water allocations
or graph lifetimes. The
[allocator](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/ggml/src/ggml-alloc.c)
already reuses eligible SCALE/ADD parent storage; explicit in-place calls do not
guarantee savings. F32 factor/input multiplication needs no activation-type conversion
in the [CPU
workspace](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/ggml/src/ggml-cpu/ggml-cpu.c).
At microbatch 64, one rank-1 projection temporary is 256 bytes and one expanded
5120-channel correction is 1.25 MiB. The embedding does not construct a dense vocabulary
correction. Do not multiply per-site buffers by 128 without proving their lifetimes
overlap.

## Allocation semantics

Adapter activation reserves the graph again after base initialization, freeing old
compute buffers before replacement. The commit difference does not prove coexistence.
Prism Inference owns the immutable B-storage overlay,
native regression and
[builder](https://github.com/symbiogenesis/prism-inference/blob/main/scripts/Build-PrismEngine.ps1).
Its [overlay guide](https://github.com/symbiogenesis/prism-inference/blob/main/docs/prism-storage-overlay.md)
records byte-identical operations and a 2.5 MiB storage saving. The installed assessment
runtime does not automatically include the source overlay; Runner consumes reviewed pins.

All 129 B tensors are byte-identical F32 `[1,5120]`, SHA-256
`1cdf553ae2623501140ec2e52c3dfa6a80d33fcfe6f91a6d3c4dcd5f887eeca0`. The overlay shares
storage only after matching buffer type/device, shape/layout and bytes, preserving each
target's metadata and adapter lifetime. Its saving is modest and does not resolve the
larger counter difference.

Lossless allocation changes preserve public initialization, scale-zero filtering,
load-without-apply, fit/no-allocation estimates, dynamic changes, A's reduction, all F32
rounding stages and adapter order. Base-result reuse depends on last-use/view/output
constraints. Reassociation or FMA contraction can change signed zero, subnormals and
results; bit identity is part of this contract.

Quantizing factors, reducing rank/scale, pruning sites or merging into the ternary base
changes the correction or weights; these are outside lossless allocation work.

## Standard-profile qualification

Standard-profile acceptance requires **decode throughput and output quality each to
match or improve on the same model without the adapter**. Target-device evaluation,
native correctness evidence and independent human review remain pending. The fixed
scale-one/512-token harness below does not qualify app scale 1.3, broader creative
quality, comfort or daily service.

A separate opt-in [app-scale contract](../config/swift-lora.app-qualification.json)
uses **scale 1.3** with the same four prompts, 24 paired samples, four zero-scale
controls, greedy sampler and **512-token output cap**. Its distinct contract digest
binds captures, nonzero-logits proof and independent complete-answer reviews;
scale-one evidence cannot satisfy it. Select `qualification: app-scale-1.3` in the
manual workflow, or inspect it without inference:

```powershell
./scripts/Invoke-LoraQualification.ps1 -DataDirectory "$env:LOCALAPPDATA/SwiftBonsai2" -UseRepackedPtq1 -AppSettings -PlanOnly
./scripts/Get-LoraQualification.ps1 -EvidencePath C:/external/evidence.json -ReviewPath C:/external/review.json -AppSettings
```

`AppSettingGatesPassed` reports only these matched LoRA gates.
`StandardProfileMayEnable` stays false for this separate contract. The 512-token cap
does not prove answer completeness: every exact response still requires human review.
Thinking, sampler changes, full 4K output, long context, broad creative quality,
comfort and daily service remain separate qualification. Omitting `AppSettings`
preserves the original scale-one contract.

### Fixed acceptance contract

[swift-lora.qualification.json](../config/swift-lora.qualification.json) pins four
general/creative prompts, three repetitions, alternating baseline/adapter order, fresh processes,
seed 42, greedy sampling, a 512-token cap, 8K FP16 context, 128/64 batch/microbatch,
four CPU threads and exact artifact identities.

Every prompt must independently pass all three requirements:

| Measure | Requirement |
| --- | --- |
| Decode throughput | Median ratio **≥ 1.0** against no LoRA. |
| Human-reviewed quality | Every applicable dimension **≥ 3/4**, with median no lower than baseline; any unwanted adapter refusal rejects quality. |
| Memory | Median peak Job-commit premium **≤ 768 MiB**, within the unchanged **11.5 GiB** Job cap and **4 GiB** available-physical-memory floor. |

Reduced refusals, shorter answers and memory savings cannot offset either primary
failure. Job commit excludes mapped weights; the one-second physical watchdog is not a
continuous peak-RAM measurement. The memory premium is a policy allowance, not measured
adapter allocation.

The standard scorer and [PTQ1 prefix screen](swift-memory.md#bounded-decision-screen-interface)
validate the Job cap against compiled Swift process policy. Contract-hash checks
reject captures under the obsolete 12 GiB scoring allowance; recapture rather than
rewrite evidence or review hashes.

### Capture on the target device

The manually dispatched [workflow](../.github/workflows/swift-lora-qualification.yml)
runs only on `main`, using a Windows ARM64 self-hosted Snapdragon X Elite runner
labelled `swift-bonsai-snapdragon` and a read-only token. The owner must provide the
runner and already provisioned external artifacts; the workflow downloads no model
weights. PQ2 is the default. The exact current-weight PTQ1 export is a separate fixed
comparison because its activation quantization differs.

Inspect the plan locally without model loading or benchmarking:

```powershell
./scripts/Invoke-LoraQualification.ps1 -DataDirectory "$env:LOCALAPPDATA/SwiftBonsai2" -PlanOnly
```

Actual capture rejects local execution. The legacy matrix requires 24 paired samples
plus four loaded-at-zero controls, including completed samples before any later batch
failure. Redesign it to fit the owner's five-minute whole-job limit before scheduling
it; do not split the matrix across repeated owner jobs.

| Capture component | Contract |
| --- | --- |
| Integrity and lifetime | Verify the complete publisher package/model/adapter, retain write/delete-denying handles and copy the verified adapter into a unique external temp lane. Every process enters a one-process kill-on-close Windows Job at creation, with a three-minute worker deadline and bounded diagnostics. All owner-device setup, samples, comparison and cleanup must fit five minutes total. |
| Receipts | Rust parses timings, strictly decodes UTF-8 and hashes exact responses. Each sample has `capture.json`, raw stdout and `diagnostics.json`; completed supervision is written last. PowerShell assembles paired evidence. Upload controls, stdout/logs/arguments/counters/failures; exclude model/adapter files. |
| Scale | The pinned [`--lora-scaled` parser](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/common/arg.cpp) splits every colon in `FNAME:SCALE`; relative filenames avoid drive-colon ambiguity. [Zero scale](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/src/llama-context.cpp) loads the adapter without its active graph branch. |
| Throughput | Native single-token **eval-run count / eval time**, from the pinned [stderr timing printer](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/common/sampling.cpp), invariant decimals, missing/duplicate/invalid counters rejected. `decodeTokens` counts eval runs: final sampled tokens normally are not decoded, and a one-token prompt remainder may count. [Completion diagnostics](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/tools/completion/completion.cpp) cross-check sampler accepts minus verbose-prompt tokens, including EOG; visible text is not a token counter. |
| Limits | Exact stdout preserves reasoning, EOG and trailing newlines; EOG is not an authoritative finish reason. Wall time includes startup; first-token latency, power and thermals are absent. This small greedy corpus proves neither broad creative quality nor statistical equivalence. |

Use the [complete-task measurement protocol](performance-expectations.md#2026-literature-measurement-and-comparison)
for broader useful-completion comparisons under equal budgets.

### Independent review and correctness evidence

`evidence.json` binds the LF-normalized UTF-8 contract and matched controls by hash. The
capture reports SSM basis verification, zero-scale logits and nonzero logits application
as pending: completion exposes no logits or generated token IDs. Native writer-level SSM
basis, loaded-at-zero response/logits identity and nonzero application are prerequisites
for interpreting effects; response changes or successful exits cannot satisfy them.
Capture identity binds the exact model, adapter, runtime, template, sampler, context,
batching, threads and seed. Failed generations and individual regressions remain
visible.

After external human review, score the capture without loading a model:

```powershell
./scripts/Get-LoraQualification.ps1 -EvidencePath C:/external/evidence.json -ReviewPath C:/external/review.json
```

Supply a genuine review of every sample in this form:

```json
{
  "schemaVersion": 1,
  "source": "external-human-review",
  "attestation": "I reviewed the exact captured responses without automated scoring.",
  "runId": "COPY_FROM_CAPTURE",
  "contractSha256": "COPY_FROM_CAPTURE",
  "reviews": [{
    "sampleId": "s0001",
    "responseSha256": "COPY_EXACT_RESPONSE_HASH",
    "reviewerId": "REVIEWER",
    "humanReviewed": true,
    "reviewedAtUtc": "2026-10-23T18:00:00Z",
    "answerComplete": true,
    "refused": false,
    "scores": { "instructionFollowing": 3, "correctness": 3, "coherence": 3 },
    "rationale": "Replace with a specific assessment of this exact response."
  }]
}
```

Creative prompts also require `creativeUsefulness` under the fixed 0-4 rubric.
Broader fiction/art acceptance separately covers dialogue, continuity, tone,
composition, sensitive permitted themes, coherence, instruction following, refusal,
repetition and usable completions. Example scores illustrate the schema, not actual
reviews; one response cannot settle the scale.
The scorer rejects malformed/hash-mismatched reviews but cannot authenticate reviewers
or attestations. Missing reviews/native evidence stay pending; known regressions reject.

`StandardProfileMayEnable` becomes true only when all implemented mandatory gates pass.
The scorer enables no serving profile, and daily security and broader creative/comfort
gates remain separate.
