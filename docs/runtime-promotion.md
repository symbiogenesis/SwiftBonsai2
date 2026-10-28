# Rust default-runtime promotion evidence

`Get-RuntimePromotion.ps1` reports the reviewed requirements for replacing the
current Swift PQ2 standard profile with Rust Prism while retaining its pinned
LoRA at scale 1.3. It reads the compiled release/model/adapter/template locks and
requires an explicit full Runner revision. PTQ1 remains a separate lane.

```powershell
$revision = git rev-parse main
$plan = ./scripts/Get-RuntimePromotion.ps1 -RunnerRevision $revision
$plan.policy.requiredGates
./scripts/Get-RuntimePromotion.ps1 -RunnerRevision $revision -EvidencePath D:\Evidence\promotion.json
```

This is an offline evidence-completeness and byte-integrity check. It never
starts inference, runs benchmarks, changes the default, authenticates a reviewer
or verifies GitHub metadata over the network. Before accepting a passed result,
independently inspect the named CI runs, source revisions, reports and reviewer
identities. A caller choosing to authorize promotion must require
`promotionReady: true`; successful command execution alone is insufficient.

The policy requires release provenance, full runtime Foundation and conformance,
Runner Foundation, full 8K state/logit parity, exact prompts/sampling/stops,
4K-output lifecycle/recovery, complete-model resources, adapter basis/logits,
independent standard-scale TPS and human quality, actual-model sandbox/transport,
speech/offline recovery, sustained device/creative acceptance and installation
with reproducible rollback. Every device gate requires the real self-hosted
Snapdragon X Elite ARM64 runner. Generic hosted ARM64 cannot satisfy those gates.
Full suites and every benchmark remain CI-only; focused local correctness checks
remain allowed.
Paired target-device promotion also requires the predeclared 10% end-to-end
latency benefit or a reviewed capacity/energy benefit within accepted limits;
the independent same-engine LoRA TPS gate cannot replace that comparison.

The 1 MiB UTF-8 JSON evidence manifest must live outside Git alongside its
retained reports. Copy `policySha256` and the entire `policy.identities` object
from the plan. `gates` may omit unfinished work; every omitted gate stays pending.
A complete pending skeleton is also returned as `evidenceTemplate` by the plan.
A supplied record has this shape:

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

Each completed (`passed` or `rejected`) gate requires a nonempty report, bounded
to 16 MiB, with an exact lowercase SHA-256 and a relative path without traversal.
Reports must remain outside Git, namespace paths and reparse points. Runtime
gates bind the selected Prism source revision; application gates bind the supplied
settled Runner revision. Each named CI execution must use `refs/heads/main`.
Passed gates require an identified reviewer; human quality and sustained comfort
also require `humanReviewed: true`. Pending records need only `id`, `status` and
a bounded nonempty `note`.

Unknown or duplicate gates, malformed identities, non-main provenance, wrong
devices, missing reports and digest mismatches fail the command. Any known
rejected gate makes the aggregate rejected even when other work remains pending.
Only every required gate passing produces `promotionReady: true`.

The numerical, security, quality and resource evidence still needs to be
collected and reviewed under [validation policy](validation.md), the
[runtime contract](native-rust-engine.md) and the [canonical work list](../PLAN.md).
