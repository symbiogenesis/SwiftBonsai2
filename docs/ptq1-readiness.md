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

Both packings retain **16 GiB available-memory admission**, a **12 GiB Job
commit ceiling** and **4 GiB running physical reserve**, with the same context,
retry and recovery policies. The experimental native Rust inference service
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

The exact packing difference would imply a candidate admission threshold of
**15,920,349,184 bytes** if suitability and resource evidence justified that
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
