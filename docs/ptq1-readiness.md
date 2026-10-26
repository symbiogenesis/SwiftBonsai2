# Current-weight PTQ1 readiness

PQ2 remains the application's default. The candidate is the verified
`Swift-Bonsai-2-current-repacked-PTQ1_0.gguf`, preserving the current Swift
weights, scales and other tensors exactly. It is not the publisher's older PTQ1
checkpoint. The [export receipt](../config/swift.ptq1-export.json) establishes
1,259,520,000 fewer serialized bytes; it does not establish a total RAM saving,
identical calculations, better answers or faster generation.

## Explicit development use

`scripts/Start-Chat.ps1 -UseRepackedPtq1` forwards the fixed selection through the
Rust app and broker to the pinned publisher Prism CPU server. Direct app/broker
launches accept `--use-repacked-ptq1`. Verification retains the exact pinned
artifact handle throughout the owned process lifetime. No arbitrary model path,
checkpoint substitution or missing-file fallback is accepted. The normal
16 GiB available-memory admission, 12 GiB Job ceiling, 4 GiB running physical
reserve and existing context/retry/recovery policies remain unchanged.

The pinned adapter can be selected on either packing. Its app setting is
experimentally 1.3; this is not evidence of mathematical validity or quality.
The optional native Rust inference service still rejects PTQ1 selection; codec
or kernel support alone is not an end-to-end service qualification.

## Correctness before timing

1. Preserve the original current Swift source and exact repacking verification.
2. Compare bounded PTQ1 full-model logits and live attention/recurrent state
   against the pinned native PTQ1 reference, including scalar and NEON paths.
   Use the existing declared numerical tolerance; do not relax it to pass.
3. Verify loaded-at-zero adapter logits identity and its nonzero computation.
   The original adapter's SSM input ordering and transfer to changed Swift
   weights remain independent requirements. Changed text or matching tensor
   dimensions cannot prove them. A base path using quantized activations and
   an adapter using unquantized inputs need separate error accounting.

## Small decision screen

The manual main-only `swift-ptq1-screen.yml` workflow uses the real native ARM64
Snapdragon runner and already provisioned artifacts. It captures one existing
creative prompt with both packings, adapter off/on at 1.3, three interleaved
repetitions: **12 timed 64-token prefixes plus two zero-scale controls**.
This is the first timing decision, not a broad benchmark campaign.

Inspect the plan without loading a model:

```powershell
./scripts/Invoke-Ptq1Screen.ps1 -DataDirectory "$env:LOCALAPPDATA/SwiftBonsai2" -PlanOnly
```

Inspect a capture offline:

```powershell
./scripts/Get-Ptq1Screen.ps1 -EvidencePath C:/external/screen-evidence.json
```

The screen keeps both models/runtime/adapter verified, uses the unchanged
resource limits, records individual failures and compares per-arm median decode
throughput. A rejection stops the candidate for investigation. Continue means
only that this small screen did not reject it: prefixes cannot establish useful
completed answers, long-context behavior or statistical equivalence. Job commit
excludes mapped weight residency and is not total physical RAM. The screen
never authorizes a default change or standard-profile promotion.

## Remaining acceptance

After correctness and the screen justify continuing, review completed responses
for continuity, instruction following, coherence, repetition and unwanted
refusals using the actual intended settings. Reuse compatible evidence rather
than repeating work. The [standard LoRA qualification](lora-qualification.md)
retains its independent throughput, human-review, zero-scale logits and native
basis requirements; the existing scale-one/512-token contract is not silently
relabelled as app-scale-1.3 qualification. Broader context/recovery and daily-use
requirements also remain separate. Promote only after the required evidence
passes, and retain PQ2 as a reproducible rollback.

The owner authorizes PTQ1 becoming the default once it is proven suitable, with
a corresponding reduction in memory requirements and checks. The verified
packing difference is **1.173019409 GiB** (1,259,520,000 bytes), not 1.7 GiB.
Use that exact verified amount for a candidate admission reduction; do not
assume a larger saving. Mapped weight residency and private committed memory
must be accounted separately before lowering the Job ceiling. Preserve the
host reserve and demonstrate the lower thresholds on the same intended profile
before activating them. Until then, defaults and enforcement remain unchanged.
