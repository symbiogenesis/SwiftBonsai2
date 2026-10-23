# Required standard LoRA evaluation

The approved abliteration LoRA is intended for the standard profile. The
implemented gate independently requires tokens per second and output quality
to match or improve on the same model without the adapter. Loading is already
demonstrated; these performance and quality requirements remain unproven.

The committed [contract](../config/swift-lora.qualification.json) fixes four
general and creative prompts, three repetitions, alternating arm order, seed 42,
greedy sampling, a 512-token output cap, 8K FP16 context, batch 128/microbatch 64,
four CPU threads and the exact model/runtime/adapter identities. Each arm starts
a fresh process. Every prompt must independently meet a median decode-throughput
ratio of at least 1.0. Each applicable quality dimension must independently meet
the baseline median and score at least 3/4. Reduced refusals, shorter responses
and memory savings cannot compensate for failure of either primary criterion.

A modest memory premium is permitted: the median peak Job-commit increment for
each prompt may be up to 768 MiB, within the unchanged 12 GiB Job cap and 4 GiB
available-physical-memory floor. These Job counters exclude mapped weight pages
and are not total physical RAM. Current memory is sampled every second; this
watchdog is not a continuous physical-memory peak measurement. The 768 MiB limit
is a declared policy allowance, not a measurement of the adapter's allocation.

## Capture on the target device

The manually dispatched [paired capture workflow](../.github/workflows/swift-lora-qualification.yml)
requires a Windows ARM64 self-hosted runner labelled `swift-bonsai-snapdragon`.
There was no registered repository runner when checked on October 23, 2026.
The workflow is implemented but has not executed a model evaluation. It runs only
on `main`, with a read-only token and the owner-managed, already provisioned
external model/runtime directory. It does not install a runner or download model
weights. PQ2 is the default; the exact current-weight PTQ1 export is a separate
fixed comparison, because its activation quantization differs.

Inspect the plan locally without model loading or a benchmark:

```powershell
./scripts/Invoke-LoraQualification.ps1 -DataDirectory "$env:LOCALAPPDATA/SwiftBonsai2" -PlanOnly
```

The actual capture rejects local execution. On the qualified CI runner it
verifies the complete publisher package, model and approved adapter; holds
read-denying handles during use; creates one verified adapter copy in a unique
external temp lane; and runs 24 paired samples plus four loaded-at-zero-scale
controls. Each process is owned from creation by a one-process, kill-on-close
Windows Job, with a ten-minute deadline and bounded diagnostic output. Artifacts
include controls, exact stdout, native logs, arguments, sample diagnostics and
failures. Model and adapter files are excluded from uploaded artifacts.

The pinned runtime's `--lora-scaled` parser accepts one `FNAME:SCALE` argument and
splits every colon. A simple relative filename in the verified native working
directory avoids the Windows drive-colon defect. Scale zero loads the adapter
but omits its active graph branch. See the pinned [argument parser](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/common/arg.cpp)
and [scale filtering](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/src/llama-context.cpp).

The parser reads `common_perf_print` prompt/eval timings from stderr using
invariant decimals and rejects missing, duplicate or invalid counters. Throughput
is the native single-token **eval-run count divided by its eval time**; the JSON
field `decodeTokens` denotes that denominator. It is not the exact generated
token count. The final sampled token is normally not decoded; a one-token prompt
remainder can also count as an eval run. Separate diagnostics report sampler
accepts minus verbose-prompt tokens as generated sampled tokens, including EOG.
These counters are cross-checked rather than deriving token counts from displayed
text. See the pinned [timing printer](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/common/sampling.cpp)
and [completion loop](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/tools/completion/completion.cpp).

Exact stdout retains reasoning, native EOG markers and trailing newlines. The
marker is an observation, not an authoritative machine finish reason. Process
wall time includes startup. First-token latency, power and thermals are not
provided by this capture and remain separate measurements. This small fixed
greedy corpus is a qualification input, not proof of broad creative capability
or statistical equivalence. Broader creative/comfort checks remain required.

## Independent review and correctness evidence

`evidence.json` is hash-bound to the contract and matched controls. It deliberately
records pending SSM basis verification, unavailable zero-scale logits and
unverified nonzero-logits application. Completion exposes no logits or generated
token IDs. Response identity is checked, but cannot substitute for those native
correctness requirements. Do not fill in verified fields merely because output
changed or the process exited successfully.

After external human review, score a captured run without loading a model:

```powershell
./scripts/Get-LoraQualification.ps1 -EvidencePath C:/external/evidence.json -ReviewPath C:/external/review.json
```

Review JSON uses this shape; add a genuine record for every captured sample:

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

Creative prompts additionally require `creativeUsefulness`. These example
numbers are schema illustrations, not assessments. Scores range from 0 to 4;
the committed contract defines the rubric and prompt-specific checks. The
scorer binds hashes and rejects malformed/mismatched records; it cannot
authenticate a human identity or the truth of an attestation. Missing reviews
or native correctness evidence stay pending. Known regressions are rejected.
`StandardProfileMayEnable` is true only when all implemented mandatory gates
pass; this reporting tool does not enable a serving profile. The broader daily
security and comfort gates still apply.
