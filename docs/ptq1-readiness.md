# Current-weight PTQ1 contract

PQ2 remains the application's default. The explicit development alternative is
`Swift-Bonsai-2-current-repacked-PTQ1_0.gguf`, preserving current Swift weights,
scales and other tensors exactly. It is distinct from the publisher's older
PTQ1 checkpoint. The [export receipt](../config/swift.ptq1-export.json) establishes
1,259,520,000 fewer serialized bytes (**1.173019409 GiB**), not an equivalent
physical-memory saving, identical activation arithmetic, quality or throughput.
All outstanding PTQ1 and memory-policy work is tracked in
[PLAN.md](../PLAN.md#creative-quality-adapters-and-memory).

## Supported selection and resource policy

`scripts/Start-Chat.ps1 -UseRepackedPtq1` selects this exact verified artifact
through the Rust app and broker on the pinned publisher Prism CPU server.
Direct app/broker launches accept `--use-repacked-ptq1`. Verification retains
the pinned artifact handle throughout the owned process lifetime. Arbitrary
paths, checkpoint substitution and missing-file fallback are unsupported.

Both packings use **15.5 GiB available-memory admission**, an **11.5 GiB Job
commit ceiling** and **4 GiB running physical reserve**, with the same context,
retry and recovery policies. This owner-selected development budget reduces
the original 16/12 GiB limits by 512 MiB; it does not promote PTQ1 or establish
measured model-memory savings. The experimental native Rust inference service
rejects PTQ1 selection. Codec/operator support and bounded native decode do not
establish full-model Rust service parity.

The approved adapter can be selected on either packing. Its app default of
1.3 is experimental; structural compatibility, changed text and matching greedy
tokens do not establish its mathematical basis, transfer to Swift, or quality.
The [LoRA contract](swift-lora.md#required-standard-profile-qualification)
applies independently.

## Numerical and admission boundaries

PTQ1 uses Q8_0 activations and PQ2 uses Q8_K. Their equal decoded weights do not
imply equal logits. Full-model comparisons bind the exact artifact, native
reference, graph arithmetic, scalar/NEON backend and complete live attention,
convolution and recurrent state. The fixed acceptance tolerance is
`1e-3 + 1e-3 * abs(reference)`; matching selected token IDs cannot override a
failure in logits or state.

The retained [full-model comparison](ptq1-arithmetic-evidence.json) records the
numerical acceptance failure. Native Q8 preparation and PTQ1
four-lane fused accumulation fixes do not establish full-model parity. The
corrected one-token capture still exceeded the bound for 430,526 of 39,504,384
logits/state values, with maximum absolute error 0.06584501266479492. Stored
state first differs at zero-based block 14 and first exceeds tolerance at
block 15. PTQ1 full-model numerical parity therefore remains unqualified.

A subsequent native-backed repair covers pinned F32/F16 projection rows divisible
by sixteen; generic arithmetic, BF16 and unqualified tails retain their prior
behavior. Twelve native operator controls do not establish its effect on the
full-model mismatch. The follow-up one-token capture was rejected before launch:
11,230,294,016 physical bytes were available against the unchanged 15.5 GiB
requirement. That attempt produced no new numerical verdict.

A subsequent bounded recheck of runtime `0.1.5` completed the same supplied token
`[248045]` using a separate diagnostic envelope: 12 GiB available admission,
8 GiB Job commit and the same 4 GiB running physical reserve. The direct Rust
supervisor retained parent lifetime, artifact verification, output bounds and
deadline without the PowerShell native-collector bootstrap. Peak Job commit was
6,275,588,096 bytes (5.845 GiB); minimum available physical memory was
7,574,892,544 bytes (7.055 GiB). All 129 output-file hashes matched the previous
capture, with the same 430,526 out-of-tolerance values. Resource checks passed;
numerical parity did not. This one-token envelope is not a full-context service
profile, and Job commit is not total physical RAM.

Subsequent localization found the first difference in block 13's FFN
down-projection: identical input/transform, but one prepared Q8_0 code differed.
The pinned native binary multiplies the maximum by rounded binary32 `1/127` for
scale and divides `127/maximum` directly for inverse scale. A source repair now
matches every prepared byte and all 5,120 isolated projection outputs, with
compact native-backed synthetic controls and separate activation-sharing
eligibility. Generic/PQ2 preparation is preserved. This repair is prepared for
runtime 0.1.6 CI; full-model rechecking and release/pin promotion remain pending.

The exact packing difference would imply a candidate admission threshold of
**15,383,478,272 bytes** from the current development budget if suitability and resource evidence justified that
change. It is not the enforced threshold. Mapped weight residency and private
commit are separate accounting domains; serialized savings alone do not justify
lowering admission or the Job ceiling. The host reserve remains mandatory.

## Bounded decision-screen interface

The manual main-only `swift-ptq1-screen.yml` workflow uses already provisioned
artifacts on the native ARM64 Snapdragon runner. Its fixed workload is one
creative prompt, both packings, adapter off/on at 1.3 and three interleaved
repetitions: **12 timed 64-token prefixes plus two zero-scale controls**.
All benchmark execution is CI-only.

Inspect its plan without loading a model:

```powershell
./scripts/Invoke-Ptq1Screen.ps1 -DataDirectory "$env:LOCALAPPDATA/SwiftBonsai2" -PlanOnly
```

Inspect a capture offline:

```powershell
./scripts/Get-Ptq1Screen.ps1 -EvidencePath C:/external/screen-evidence.json
```

The screen preserves artifact verification and resource enforcement, retains
individual failures, and compares per-arm median decode throughput. A rejection
blocks the candidate. A continue result establishes only that the small screen
did not reject it; prefixes cannot establish completed-answer quality,
long-context behavior or statistical equivalence. Job commit excludes mapped
weights and is not total physical RAM. The screen cannot authorize a default
change or relabel the independent scale-one/512-token standard LoRA contract as
app-scale-1.3 qualification.
