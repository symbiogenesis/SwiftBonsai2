# Rust default-runtime promotion evidence

`Get-RuntimePromotion.ps1` reports the reviewed requirements for replacing the current
Swift PQ2 standard profile with Rust Prism while retaining its pinned LoRA at scale 1.3.
It reads the compiled release/model/adapter/template locks and requires an explicit full
Runner revision. PTQ1 remains a separate lane.

```powershell
$revision = git rev-parse main
$plan = ./scripts/Get-RuntimePromotion.ps1 -RunnerRevision $revision
$plan.policy.requiredGates
./scripts/Get-RuntimePromotion.ps1 -RunnerRevision $revision -EvidencePath D:\Evidence\promotion.json
```

The evaluator checks offline completeness and byte integrity, without inference,
benchmarks, selection changes, reviewer authentication or network provenance checks.
Independently review CI/source/report/reviewer identities and require `promotionReady:
true`; command success alone cannot authorize promotion.

Required gates cover release provenance, runtime Foundation/conformance, Runner
Foundation, 8K logits/full state, prompts/sampling/stops, 4K-output lifecycle/recovery,
complete-model resources, adapter basis/logits, standard-scale TPS/human quality,
actual-model sandbox/transport, speech/offline recovery, sustained device/creative
acceptance, installation and rollback. Released private-service adapter readiness and
per-request scale/state lifecycle qualify independently of source-graph support or
adapter-disabled smoke checks.

Device gates require the real self-hosted Snapdragon X Elite ARM64 runner. Paired
promotion requires predeclared **10% end-to-end latency benefit**, or reviewed
capacity/energy benefit within accepted limits; same-engine LoRA TPS is independent.
Full suites/benchmarks stay CI-only; focused local correctness checks are allowed.

The 1 MiB UTF-8 JSON evidence manifest must live outside Git alongside its retained
reports. Copy `policySha256` and the entire `policy.identities` object from the plan.
`gates` may omit unfinished work; every omitted gate stays pending. A complete pending
skeleton is also returned as `evidenceTemplate` by the plan. A supplied record has this
shape:

```json
{
  "schemaVersion": 1,
  "policySha256": "<exact plan policySha256>",
  "identities": { "<copy every policy.identities member>": "<unchanged>" },
  "gates": [
    {
      "id": "runner-foundation",
      "status": "passed",
      "note": "Reviewed required jobs, exact revision and retained final verdict.",
      "report": { "file": "runner/foundation.json", "sha256": "<report SHA-256>" },
      "execution": {
        "repository": "symbiogenesis/SwiftBonsai2Runner",
        "revision": "<exact selected Runner main revision>",
        "gitRef": "refs/heads/main",
        "runId": "123456789",
        "runAttempt": 1,
        "architecture": "ARM64",
        "deviceModel": "Snapdragon X Elite",
        "selfHosted": true
      },
      "reviewerId": "<actual independent reviewer>",
      "humanReviewed": false
    }
  ]
}
```

Each completed (`passed` or `rejected`) gate requires a nonempty report, bounded to 16
MiB, with an exact lowercase SHA-256 and a relative path without traversal. Reports must
remain outside Git, namespace paths and reparse points. Runtime gates bind the selected
Prism source revision; application gates bind the supplied settled Runner revision. Each
named CI execution must use `refs/heads/main`. Passed gates require an identified
reviewer; human quality and sustained comfort also require `humanReviewed: true`.
Pending records need only `id`, `status` and a bounded nonempty `note`.

Unknown or duplicate gates, malformed identities, non-main provenance, wrong devices,
missing reports and digest mismatches fail the command. Any known rejected gate makes
the aggregate rejected even when other work remains pending. Only every required gate
passing produces `promotionReady: true`.

Evidence collection/review follows [validation](validation.md), the [runtime
contract](native-rust-engine.md) and [PLAN.md](../PLAN.md).
