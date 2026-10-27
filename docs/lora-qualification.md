# Standard LoRA qualification

The approved adapter is intended for the standard profile only after **decode
throughput and output quality each match or improve on the same model without
it**. Bounded loading/decode passes; target-device evaluation, native correctness
evidence and independent human review remain pending.

## Fixed acceptance contract

[swift-lora.qualification.json](../config/swift-lora.qualification.json) pins
four general/creative prompts, three repetitions, alternating arm order, fresh
processes, seed 42, greedy sampling, a 512-token cap, 8K FP16 context,
128/64 batch/microbatch, four CPU threads and exact artifact identities.

Every prompt independently requires:

- Median decode-throughput ratio of at least **1.0** against no LoRA.
- Every applicable quality dimension at least **3/4** and no lower median than
  baseline; an independently reviewed unwanted adapter refusal rejects quality.
- Median peak Job-commit premium at most **768 MiB**, within the unchanged
  **11.75 GiB** Job cap and **4 GiB** available-physical-memory floor.

Reduced refusals, shorter answers and memory savings cannot offset either
primary failure. Job commit excludes mapped weights; the one-second physical
watchdog is not a continuous peak-RAM measurement. The memory premium is a
policy allowance, not measured adapter allocation.

## Capture on the target device

The manually dispatched [workflow](../.github/workflows/swift-lora-qualification.yml)
runs only on `main`, using a Windows ARM64 self-hosted Snapdragon X Elite runner
labelled `swift-bonsai-snapdragon` and a read-only token. The owner must provide
the runner and already provisioned external artifacts; the workflow downloads
no model weights. PQ2 is the default. The exact current-weight PTQ1 export is a
separate fixed comparison because its activation quantization differs.

Inspect the plan locally without model loading or benchmarking:

```powershell
./scripts/Invoke-LoraQualification.ps1 -DataDirectory "$env:LOCALAPPDATA/SwiftBonsai2" -PlanOnly
```

Actual capture rejects local execution. CI verifies the complete publisher
package, model and adapter, retains handles that deny writes/deletion, and
creates a verified adapter copy in a unique external temp lane. It records
24 paired samples plus four loaded-at-zero-scale controls. Each process belongs
from creation to a one-process, kill-on-close Windows Job, with a ten-minute
deadline and bounded diagnostics. Uploaded artifacts include controls, exact
stdout, logs, arguments, counters and failures; they exclude model/adapter files.

The Rust supervisor owns timing parsing, strict UTF-8 response decoding, exact
response hashes and diagnostics. Each completed sample has one `capture.json`
record, alongside raw stdout and `diagnostics.json`; its completed supervision
receipt is written last. PowerShell assembles paired evidence from those records,
including completed samples retained before a later batch failure.

The pinned `--lora-scaled` parser splits every colon in `FNAME:SCALE`. A relative
filename in the verified working directory avoids Windows drive-colon parsing.
Scale zero loads the adapter but omits its active graph branch. Sources:
[argument parser](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/common/arg.cpp)
and [scale filtering](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/src/llama-context.cpp).

Throughput is native single-token **eval-run count / eval time**. The parser
reads the pinned stderr timing record with invariant decimals and rejects
missing, duplicate or invalid counters. JSON `decodeTokens` is the eval-run
count, not exact generated tokens: the final sampled token normally is not
decoded, and a one-token prompt remainder may count as an eval run. Separate
diagnostics cross-check sampler accepts minus verbose-prompt tokens, including
EOG; displayed text is not a token counter. Sources:
[timing printer](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/common/sampling.cpp)
and [completion loop](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/tools/completion/completion.cpp).

Exact stdout retains reasoning, EOG markers and trailing newlines. An EOG
marker is not an authoritative machine finish reason. Wall time includes
startup. First-token latency, power and thermals are not captured. This small
greedy corpus does not establish broad creative capability or statistical
equivalence; wider creative/comfort checks still apply.

## Independent review and correctness evidence

`evidence.json` binds the LF-normalized UTF-8 contract and matched controls by
hash. The capture reports SSM basis verification, zero-scale logits and nonzero
logits application as pending: completion exposes no logits or generated token
IDs. Response identity cannot replace those native correctness requirements.
Do not mark them verified because text changes or a process exits successfully.

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

Creative prompts also require `creativeUsefulness`. Example scores illustrate
the schema only. The contract defines the 0–4 rubric and prompt-specific checks.
The scorer rejects malformed/hash-mismatched reviews but cannot authenticate
reviewer identity or attestations. Missing reviews/native evidence stay pending;
known regressions reject acceptance.

`StandardProfileMayEnable` becomes true only when all implemented mandatory
gates pass. The scorer enables no serving profile, and daily security and
broader creative/comfort gates remain separate.
