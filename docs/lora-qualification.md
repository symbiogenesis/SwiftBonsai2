# Standard LoRA qualification

The approved adapter is intended for the standard profile only after **decode throughput
and output quality each match or improve on the same model without it**. Bounded
loading/decode passes; target-device evaluation, native correctness evidence and
independent human review remain pending.

## Fixed acceptance contract

[swift-lora.qualification.json](../config/swift-lora.qualification.json) pins four
general/creative prompts, three repetitions, alternating arm order, fresh processes,
seed 42, greedy sampling, a 512-token cap, 8K FP16 context, 128/64 batch/microbatch,
four CPU threads and exact artifact identities.

Every prompt independently requires:

- Median decode-throughput ratio of at least **1.0** against no LoRA.
- Every applicable quality dimension at least **3/4** and no lower median than
  baseline; an independently reviewed unwanted adapter refusal rejects quality.
- Median peak Job-commit premium at most **768 MiB**, within the unchanged
  **11.5 GiB** Job cap and **4 GiB** available-physical-memory floor.

Reduced refusals, shorter answers and memory savings cannot offset either primary
failure. Job commit excludes mapped weights; the one-second physical watchdog is not a
continuous peak-RAM measurement. The memory premium is a policy allowance, not measured
adapter allocation.

The contract validator binds the Job cap to the compiled Swift process policy; both the
standard scorer and PTQ1 prefix screen use that validated cap. The corrected contract
hash rejects captures made against the former 12 GiB scoring allowance. Recapture under
the current contract rather than rewriting historical evidence or review hashes.

## Capture on the target device

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

Actual capture rejects local execution. CI retains 24 paired samples plus four
loaded-at-zero controls, including completed samples before any later batch failure.

| Capture component | Contract |
| --- | --- |
| Integrity and lifetime | Verify the complete publisher package/model/adapter, retain write/delete-denying handles and copy the verified adapter into a unique external temp lane. Every process enters a one-process kill-on-close Windows Job at creation, with ten-minute deadline and bounded diagnostics. |
| Receipts | Rust parses timings, strictly decodes UTF-8 and hashes exact responses. Each sample has `capture.json`, raw stdout and `diagnostics.json`; completed supervision is written last. PowerShell assembles paired evidence. Upload controls, stdout/logs/arguments/counters/failures; exclude model/adapter files. |
| Scale | The pinned [`--lora-scaled` parser](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/common/arg.cpp) splits every colon in `FNAME:SCALE`; relative filenames avoid drive-colon ambiguity. [Zero scale](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/src/llama-context.cpp) loads the adapter without its active graph branch. |
| Throughput | Native single-token **eval-run count / eval time**, from the pinned [stderr timing printer](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/common/sampling.cpp), invariant decimals, missing/duplicate/invalid counters rejected. `decodeTokens` counts eval runs: final sampled tokens normally are not decoded, and a one-token prompt remainder may count. [Completion diagnostics](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/tools/completion/completion.cpp) cross-check sampler accepts minus verbose-prompt tokens, including EOG; visible text is not a token counter. |
| Limits | Exact stdout preserves reasoning, EOG and trailing newlines; EOG is not an authoritative finish reason. Wall time includes startup; first-token latency, power and thermals are absent. This small greedy corpus proves neither broad creative quality nor statistical equivalence. |

Broader useful-completion comparisons separately account for prompt processing,
first-token latency, decode throughput, generated/reasoning tokens, response time,
retries and resources under equal budgets. Fewer refusals alone cannot prove improvement.

## Independent review and correctness evidence

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

Creative prompts also require `creativeUsefulness`. The 0-4 rubric and prompt checks
cover fiction/art dialogue, continuity, tone, composition, sensitive permitted themes,
coherence, instruction following, refusal, repetition and usable completions. Example
scores are schema illustrations, not reviews; one response cannot settle the scale.
The scorer rejects malformed/hash-mismatched reviews but cannot authenticate reviewers
or attestations. Missing reviews/native evidence stay pending; known regressions reject.

`StandardProfileMayEnable` becomes true only when all implemented mandatory gates pass.
The scorer enables no serving profile, and daily security and broader creative/comfort
gates remain separate.
