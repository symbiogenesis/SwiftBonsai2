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

Runtime **0.1.6** now passes the bounded no-adapter comparison for supplied tokens
`[248045, 872, 198]` on the automatic ARM64 backend: all **483 files and
118,611,456 logits/live-state values match the native reference byte for byte**,
with zero maximum absolute error. Greedy IDs also match `[198, 198, 220]`.
The [repair receipt](ptq1-arithmetic-evidence.json) binds the clean source,
checker, native reference, comparison and supervision hashes. It preserves the
earlier failures, including the 430,526 out-of-tolerance one-token values.

The repaired Q8_0 preparation matches the pinned DLL's maximum multiplied by
rounded binary32 `1/127` for scale and direct `127/maximum` inverse scale. This
resolved a halfway-code difference first localized in block 13's FFN
down-projection. Generic/PQ2 preparation is preserved. The offline comparator
reconstructs every reference KV, convolution and recurrent value from the
original hash-bound serialized native state, checks the exact file inventory
and rejects nonfinite values. Both historical failure controls were reproduced
exactly before the new comparison.

The successful capture used a separate **12 GiB admission / 8 GiB Job ceiling**
diagnostic envelope with the unchanged **4 GiB physical reserve**. Peak Job commit
was 6,275,977,216 bytes (5.845 GiB), and minimum available physical memory was
8,027,590,656 bytes (7.476 GiB). The direct Rust supervisor retained caller
lifetime, exact artifact verification, output bounds and deadline. This is
three-token resource evidence, not full-context service admission; Job commit
does not include mapped weights or measure total physical RAM.

Scalar execution completed two supplied tokens before its 180-second diagnostic
deadline. Its incomplete capture is not accepted. A fresh attempt with a
360-second deadline was admission-rejected before launch, with 9,289,871,360
physical bytes available against the same 12 GiB requirement. Completed scalar
and explicit NEON controls remain pending. The retained default-feature build
and compiler configuration establish ARM64 NEON eligibility; the checker does
not emit observed per-operation backend counters.

The exact runtime main revision `02c8a24` passed
[Foundation](https://github.com/symbiogenesis/prism-inference/actions/runs/40449357453)
and [conformance](https://github.com/symbiogenesis/prism-inference/actions/runs/40449357329).
Both checked Windows bundles were published as
[0.1.6](https://github.com/symbiogenesis/prism-inference/releases/tag/v0.1.6),
fresh published asset bytes were verified, and Runner's release lock was updated.
The Rust service still selects PQ2. Longer trajectories/full-context behavior,
adapter execution, complete answers at actual app settings, context/recovery and
resource/comfort qualification remain outstanding. In particular, the scale-one
standard LoRA harness does not establish PTQ1-versus-PQ2 answer parity at app
scale 1.3.

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
