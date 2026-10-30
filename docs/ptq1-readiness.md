# Current-weight PTQ1 contract

PQ2 remains the app default. `Swift-Bonsai-2-current-repacked-PTQ1_0.gguf` preserves
current weights, raw scales and other tensors; it is distinct from the publisher's older
checkpoint. The [export receipt](../config/swift.ptq1-export.json) proves 1,259,520,000
fewer serialized bytes (**1.173019409 GiB**), not equal activation arithmetic, outputs,
speed or physical-memory savings.
[PLAN.md](../PLAN.md#creative-quality-adapters-and-memory) owns outstanding
qualification and memory policy.

## Supported selection and resource policy

`scripts/Start-Chat.ps1 -UseRepackedPtq1` selects only this verified artifact on the pinned
publisher Prism CPU server; direct app/broker launches use `--use-repacked-ptq1`. The
artifact handle stays verified/held through owned teardown. Arbitrary paths, checkpoint
substitutions and missing-file fallback are unsupported. The experimental Rust service
still rejects PTQ1; codec/operator support does not establish service parity.

Both packings retain **15.5 GiB available admission / 11.5 GiB Job ceiling / 4 GiB
running physical reserve**, context, retry and recovery. This owner-selected 512 MiB
reduction from 16/12 GiB is not measured PTQ1 savings or promotion. The approved adapter
can run on either packing, with experimental app scale 1.3; structure, changed text and
greedy agreement do not settle basis, Swift transfer or quality. [Independent LoRA
gates](lora-qualification.md) still apply.

## Numerical and admission boundaries

PTQ1 uses Q8_0 activations; PQ2 uses Q8_K. Full-model comparisons bind exact
artifact/reference, graph arithmetic, scalar/NEON backend and complete live
KV/convolution/recurrent state, with tolerance `1e-3 + 1e-3 * abs(reference)`. Matching
token IDs cannot override a logits/state failure.

| Retained evidence | Result and limit |
| --- | --- |
| Rust **0.1.6**, no adapter, automatic ARM64, supplied `[248045, 872, 198]` | All **483 files / 118,611,456 logits/live-state values** match native bytes; max absolute error zero. Greedy IDs `[198, 198, 220]` match. [Repair receipt](ptq1-arithmetic-evidence.json) binds source/checker/reference/comparison/supervision hashes and earlier failures, including 430,526 out-of-tolerance one-token values. |
| Q8_0 repair | Matches pinned DLL scale `maximum * rounded-binary32(1/127)` and inverse `127/maximum`; resolves halfway-code divergence first localized to block 13 FFN down. Generic/PQ2 preparation is unchanged. |
| Offline comparator | Reconstructs every KV/convolution/recurrent value from original hash-bound serialized native state; exact inventory and finite values required. Both historical failure controls were reproduced exactly first. |
| Diagnostic resource envelope | **12 GiB admission / 8 GiB Job / 4 GiB reserve**; peak commit 6,275,977,216 bytes (5.845 GiB), minimum available physical 8,027,590,656 (7.476 GiB). Direct Rust supervision retains caller lifetime, artifact verification, output cap and deadline. Three tokens do not qualify service admission; Job commit excludes mapped weights/total RAM. |
| Scalar control | Two supplied tokens before 180-second deadline: incomplete, not accepted. Fresh 360-second attempt rejected before launch at 9,289,871,360 available bytes versus 12 GiB required. Completed scalar/explicit NEON controls remain pending. Default-feature ARM64 build establishes NEON eligibility, without observed per-operation counters. |

Exact runtime main `02c8a24` passed
[Foundation](https://github.com/symbiogenesis/prism-inference/actions/runs/40449357453)
and
[conformance](https://github.com/symbiogenesis/prism-inference/actions/runs/40449357329).
Both Windows bundles were published as
[0.1.6](https://github.com/symbiogenesis/prism-inference/releases/tag/v0.1.6), fresh
asset bytes verified and Runner's lock updated. Longer/full-context trajectories,
adapters, completed app-setting answers, recovery and resource/comfort remain pending.
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

The screen enforces artifacts/resources, retains failures and compares per-arm median
decode throughput. Rejection blocks the candidate; continue means only that this small
prefix screen did not reject it. It cannot establish quality of completed answers,
long-context/statistical equivalence, total physical RAM, default promotion or
app-scale-1.3 acceptance under the scale-one/512-token LoRA contract.
