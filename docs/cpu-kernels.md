# CPU kernels and X2 thread evaluation

Baseline NEON supports PQ2, odd-group PQ2 and PTQ1 with scalar reference/fallback. This guide owns
arithmetic, dispatch, qualification and the [X2 thread screen](#x2-cpu-thread-screen).
Synthetic parity does not qualify whole-model parity or device speedup; pinned Prism remains the
control engine. [PLAN.md](../PLAN.md#cpu-kernels) owns outstanding work.

## Execution and dispatch

The default `experimental-neon` feature uses [`fearless_simd` 1.1.0][fearless]'s safe core API under
`forbid(unsafe_code)`. `MatvecBackend::Auto` selects once per matrix operation; Qwen35 and the token
driver inherit it. `Qwen35::with_backend` and `--backend auto|scalar|neon` select explicit policies.

`backend_requested` reports the requested policy. `backend_statistics` retains cumulative matrix
attempts/successes, scalar/NEON dispatches, preparation failures and fallback reasons, including
provisional work. `MatvecScratch::with_backend` forces a kernel policy; `selected_backend()` reports
the last operation and `statistics()` exposes aggregate counts without payloads. Counters saturate
at `u64::MAX`; reset and checkpoint restoration preserve diagnostics. Floating projections and
ineligible packed operations use scalar.

Baseline eligibility requires native `aarch64-pc-windows-msvc`, the compiled feature and baseline
NEON. Complete PQ2/PTQ1 rows use packed kernels; unsupported formats/targets and disabled features
use scalar. Scalar runs natively on ARM64. There is no optional-ISA detector, global target override
or persistent expanded weight copy. Kernel `Auto` does not select Snapdragon generations or
CPU/GPU/NPU profiles; that boundary belongs to the [hardware contract](hardware-capabilities.md#automatic-profile-selection).

Runtime, loader, graph, codecs and broker retain `forbid(unsafe_code)`. Safe dependency APIs are the
first boundary. Intrinsics requiring local unsafe belong in a reviewed isolated crate with safe
slice APIs and explicit bounds, alignment, aliasing, initialization, lifetime and CPU/OS feature
invariants. Public vector types and a relaxed runtime prohibition are outside that contract; Rust's
[target-feature rules][target-feature] apply.

The checked facade validates kinds, dimensions, byte extents, input/output lengths and finite
activations. Immutable payloads are checked at load or per checked block; public arbitrary `Tensor`
calls still reject malformed payloads. Fallible scratch growth stays outside hot loops, errors leave
empty output, and immutable weights/LoRA B remain shareable. Unaligned loads, scalar tails or local
copies are bounded; overreads are invalid.

Optional dotprod and i8mm require verified Windows CPU/OS detection, independently qualified
layouts/shapes/arithmetic and useful device benefit. Compile-time enabled features can bypass
runtime probing; model names, Linux reports, compilation and fault recovery do not establish
eligibility. The relevant interfaces are [`is_aarch64_feature_detected!`][detection],
[`IsProcessorFeaturePresent`][windows-features] and the installed SDK. Distributable flags remain at
the reviewed baseline; global `target-cpu=native`, `+dotprod` or `+i8mm` are outside the contract.
Detection/policy selection stays outside row/block loops. Overrides can disable features or request
supported paths.

## Packed arithmetic

The [kernel module][kernels], [graph][graph], [ternary codec][ternary], [Swift
metadata](../config/swift.metadata.json) and [symbol census](../config/swift.pq2-census.json) define
format identities. Q4_0/Q8_0/F16 helpers do not establish loader support for every artifact
inventory.

| Operation | Required arithmetic |
| --- | --- |
| PQ2 rows divisible by 256 | Q8_K activation groups; each 128-weight block decodes two-bit codes to -1, 0, 1, 2; preserve integer dots and ordered floating scales/adds |
| Odd-group PQ2 | Four independent Q8_0 scales per 128 weights, then ordered sums |
| PTQ1 | Direct canonical base-three decode, four Q8_0 groups per 128 weights, then block scale; no intermediate PQ2 block |
| Hadamard/elementwise work | Preserve stage boundaries, signs, axes/basis, normalization and multiply/add order |
| Activation quantizers | Preserve maxima/ties, clipping, scales, rounding and rejection |
| Floating reductions/RMS/attention/recurrent work | Rounded f32 products and specified f64 accumulation; reassociation has a separate numerical contract |
| Prefill tiles | Bounded causal/state scheduling; faster matvec alone does not batch graph execution |

PQ2 code 3 means **+2**, despite the exporter's narrower ternary-only contract. PTQ1 digits are
interleaved: bytes 0–15 hold five digits at stride 16, bytes 16–23 hold five at stride 8, and bytes
24–25 hold four at stride 2. Q4_0 stores 16 low nibbles before 16 high nibbles. Canonicality and
byte order remain exact.

The [integer-bound derivation](research-register.md#t3-exact-low-bit-dots-need-bounds-at-every-intermediate)
covers arbitrary i8 activations, including -128: a 128-element PQ2 dot fits i16
(`[-32768, 32512]`), but its uncorrected unsigned-code dot does not (`[-49152, 48768]`).
At 32 elements the ternary/PQ2 absolute bounds are 4,096/8,192. Bound every intermediate and
correction before narrowing; Rust overflow flags do not check wrapping SIMD. Apply each block's
scale before accumulating later blocks. Malformed scales/noncanonical PTQ1 bytes reject even at
zero scale.

Q8_K chooses the first signed value with strictly greater absolute magnitude, uses `-127 / maximum`,
ties-to-even rounding and an f32 scale. Q8_0 uses an absolute maximum, ties-away rounding and an
FP16-rounded stored scale, while quantizing with the original inverse scale. Opposite-sign ties,
signed zero, finite extremes and nonfinite rejection are part of the contract. Equal decoded
PQ2/PTQ1 weights can produce different matvec outputs; see the [packing
assessment](swift-memory.md).

## Signed constant floor division

[Ben Joffe's algorithm](https://www.benjoffe.com/fast-floordiv), published
2026-10-27 and reviewed here on **2026-10-30**, computes signed integer division
rounded toward negative infinity using a multiply and arithmetic shift. It is
a candidate for a measured signed constant-divisor hotspot in Prism. Ordinary
Rust signed `/` truncates toward zero; replacing it requires an existing floor
contract or proof that the caller's admitted inputs give the same result.

The [reference implementation](https://github.com/benjoffe/fastdiv/blob/main/algorithms/benjoffe.hpp)
searches multiplier/shift pairs for both rounding directions. When both work,
it can select the multiplier using the dividend's sign. Some divisors require
a bounded input adjustment instead; the sign-selected formula is not universal.
For a valid sign-selected pair `(M, S)`, the quotient is
`(i128(x) * (M + (x >> 63))) >> S`, with an arithmetic right shift.
Positive powers of two already reduce to an arithmetic shift. A Rust port must
use compile-time parameters and widened integer products, preserve full-range
behavior and explicitly reject zero and the unrepresentable `i64::MIN / -1`.
Negative-divisor floor division differs from `div_euclid`; do not use the latter
as its general reference. The reference's `main` link is a discovery pointer;
pin a reviewed revision and retain its license notices before copying source.

The **2026-10-30 source audit found no eligible production caller in Runner**:

| Existing arithmetic | Applicability |
| --- | --- |
| Speech graph `(n_kv_pad + 255) / 256` in `src/speech-tts/build_overlay.rs` | Nonnegative ceiling division by a power of two |
| Audio elapsed time `/ 48` in `src/chat-ui/src/speech/pipeline.rs` | Unsigned byte-count conversion |
| Buffer, sample and packed-code indexing | Unsigned extents or power-of-two divisors |
| Quantization scales and timing ratios | Floating-point division |

The audit also covered the clean pinned Prism revision
`fce300b94234c2dd70756dd4bbc33ea703848cb6`. Its
[convolution extent calculation](https://github.com/symbiogenesis/prism-inference/blob/fce300b94234c2dd70756dd4bbc33ea703848cb6/src/backend/call.rs#L73)
uses an `i128` numerator and a runtime stride. Its
[backward-image divisions](https://github.com/symbiogenesis/prism-inference/blob/fce300b94234c2dd70756dd4bbc33ea703848cb6/src/backend/backward_image.rs#L90)
use runtime strides after exact-divisibility guards. Neither is a fixed-`i64`
replacement site. Runtime-invariant specialization would require its own setup
and reuse-cost evidence. No runtime code or dependency pin changes follow from
this audit, and no throughput, latency or complete-request gain has been measured.

For a future eligible caller, compare exact results with an independent widened
quotient/remainder reference: both signs, zero dividend, signed extrema, exact
multiples and their neighbors, large divisors, forced round-up/down cases and
power-of-two cases. Check optimized ARM64 code generation under the pinned Rust
compiler; the article's C++ results do not establish Rust performance. Measure
throughput, dependent latency and complete-request cost in owning CI under the
[five-minute device budget](validation.md), retaining losing results. Implement
only a justified consumer upstream; [PLAN.md](../PLAN.md#cpu-kernels) owns that gate.

## Numerical and resource acceptance

Optimized-Rust/scalar and Rust/Prism comparisons have separate purposes; an existing
scalar/reference tolerance does not authorize additional SIMD drift.

| Scope | Acceptance |
| --- | --- |
| Symbols, scales, activations, integer sums | Exact bytes/bits/integers, including ties and signs |
| Order-preserving packed dots, elementwise work, butterflies | Bitwise finite f32 parity, signed zero and matching failures; preserve grouping and avoid unintended FMA |
| Reassociated reductions/FMA | Predeclared absolute/relative bounds, error distribution and logits/state/token checks |
| Full graph | Fixed IDs/positions/context/cache/adapter/decoding; layer/logit, KV/GDN, greedy tie, long-prefix, restoration and cancellation checks |

Full attention retains FP16 Q/K/V and normalized-probability conversions. GDN retains
decay/delta/state-update/output order. Fusion cannot move reads across state mutation. Scalar
transcendental functions are the control; vector versions have explicit NaN/infinity/subnormal/mask
and accuracy contracts. Negative-infinity masking and all-masked-row rejection remain required.
[Algebraic float operators][algebraic] and fast-math have distinct numerical contracts.

Exact fixtures include packed codes/offsets, lane extrema, immutable C++ bytes, unaligned blocks,
scale/quantization edges, awkward rows, malformed extents, format changes and error/rollback
recovery. Incomplete packed blocks are invalid. Negative optional-capability cases are simulated;
real optional ISA execution requires eligible hardware. See [kernel fixtures][kernel-tests] and
[SIMD fixtures][simd-tests].

A performance receipt binds artifacts, workload, context, threads, power/cache state, adapter,
tolerances, primary metric, useful-benefit threshold and allowed regressions. It distinguishes
row/tile dispatch, hot-cache/streaming weights, quantization/unpack/dot/transforms/scheduling, cold
load/repack, prefill/first-token/ decode/task cost, scratch/residency/allocations and overlapping
source/destination storage. Compiler/LLVM/options, actual backend/fallback and native CPU/OS
identity are part of its scope. A microkernel result does not qualify a whole-model replacement or
another X1/X2 device. Standard LoRA TPS/quality remains independent.

Local builds, disassembly and focused correctness checks are permitted. Full suites and every
benchmark run only in CI under the [validation policy](validation.md). Hosted ARM64 supplies
correctness infrastructure; target-device CI supplies performance/thermal/energy evidence. Four
threads and total core count are control settings, not universal optima; phase placement and
mixed-device contention are part of a profile's declared scope.

## CPU kernel research reviewed in October 2026

The [research catalog](research-register.md#packed-projections-cpu-kernels-and-compression) owns
sources, versions and transfer limits through **2026-10-29**. Its CPU applicability rules are:

| Candidate | Constraint |
| --- | --- |
| Layout/LUT reuse (L1–L4, L10) | Derive NEON consumption order from lanes, quantizers and scale groups. Include unpack/preparation/dot/scale work, repacking and retained copies. Larger tables/accumulators can spill registers or evict useful cache; choose bounded tiles using complete-workload CI. |
| Entropy coding (L12) | Charge indexes, decode dependencies, scratch and consumed bytes. Independent streams may feed kernels, but 8-bit/communication results do not establish packed-matvec benefit. |
| Representations (L11, L13–L17) | Nonuniform codes, compensation, fitted rotations, lattices and trellises need new artifacts and separate qualification. |
| Hardware (L18–L19) | Simulated ISA/datapath changes are outside existing Snapdragon software work. |

Retain packed storage without persistent expansion, distinguish decode from genuine multi-token
prefill and apply the intermediate/correction bounds above to every borrowed layout.

## X2 CPU thread screen

The **2026-10-29 model-free inventory** identifies the development target, not a qualified speedup
or GPU/NPU profile. Native ISA semantics live in [capability contracts](hardware-capabilities.md).

| Observation | Retained value |
| --- | --- |
| CIM/independent hardware-registry name | `Snapdragon(R) X2 Elite Extreme - X2E94100 - Qualcomm Oryon(TM) CPU` |
| Native toolchain/target/kernel | Pinned Rust; `aarch64-pc-windows-msvc`; Windows `10.0.26300` (`10.0.26300.0` probe display) |
| CPU topology | 18 physical/logical cores, group 0, efficiency/scheduling classes `[0, 1]`; class numbers do not name core types |
| Native Windows ISA | Positive NEON, dot-product and **NEON I8MM** flags; dispatched kernels still need codegen/numerical/device qualification |
| GPU inventory | Adreno X2-90, driver `32.0.149.0` |
| Power policy | Effective `balanced`; scheme `381b4222-f694-41f0-9685-ff5bb260df2e` |
| Planned screen | `d6-p6`, `d4-p12`, `d8-p18`; 24 samples; `ModelProcessStarted: false` |

Identity checks accept unchanged observations and reject injected AC/mode/kernel changes and legacy
records lacking host identity. The speech-host query agrees on CPU/kernel. Reports, plans and
comparison receipts are retained externally in `Bonsai-PerfNext3-20261029/x2-evidence`; they contain
no model timing.

The manual [X2 CPU workflow](../.github/workflows/x2-cpu-evaluation.yml) runs a bounded first thread
screen on the owner-managed native ARM64 runner and settled `main`, sharing the model-capture
concurrency group. Performance execution remains [CI-only](validation.md).

### Controls and cost

The Rust maintenance planner selects at most three candidates from the reviewed decode/prefill
thread budgets `6/6`, `8/8`, `4/12`, `8/12`, `8/18`, pruning budgets larger than the observed
logical processor count. If more than three remain, it selects the first, middle and last. On this
18-core machine those are `6/6`, `4/12` and `8/18`. These are thread counts without affinity
pinning; Windows chooses their placement.

Each candidate pairs with the unchanged `4/4` control on two fixed synthetic creative-writing
prompts of different lengths, reversing pair order in the second repetition. The cap is 24 fresh
processes, each with 128 tokens and a 180-second deadline: at most 72 minutes of model execution,
plus verification/setup within a two-hour job. OS file cache is not flushed.

The screen fixes the pinned original PQ2 model, runtime, adapter at scale 1.3, publisher sampler,
seed 42, 8,192-token context, FP16 KV, 128/64 prompt batches, CPU-only placement, no repacking and
zero polling. The legacy completion executable uses its own template defaults; these differ from the
chat server's explicit thinking policy. This is a CPU thread screen, not chat qualification.

Artifacts must already exist; the batch supervisor verifies and holds exact model, adapter and
runtime files. Every sample retains 15.5 GiB admission, an 11.5 GiB Job commit ceiling, 4 GiB
physical reserve, one-process/caller ownership and bounded logs. The screen downloads no models
and promotes no runtime/default.

Before verifying model bytes or starting thread trials, execution runs the exact verified completion
executable with only `--version` under the ordinary headless worker policy. This model-free
preflight retains a separate `worker-preflight/` receipt and bounded logs, with a 512 MiB Job and
ten-second deadline. Failure stops the screen and leaves `ModelTrialsRequested: false` in its
report. The shared supervisor's `ModelProcessStarted` field means an executable was started; the
preflight's explicit `ModelLoaded: false` distinguishes it from model trials. PlanOnly remains
nonexecuting. The relaxed inspection launch cannot satisfy this worker check.

### Inspecting the plan

From native ARM64 PowerShell 7.6+, use an existing checked maintenance executable and an absolute
data directory outside Git:

```powershell
./scripts/Invoke-X2CpuEvaluation.ps1 -DataDirectory $data -ToolPath $tool -PlanOnly
./scripts/Doctor.ps1 -ToolPath $tool -PassThru
```

Plan inspection probes hardware and reads compiled policy without loading a model. It requires an
observed X2 Elite Extreme name and agreement between CIM and the native CPU-set count, plus
agreement between the CIM processor name and the bounded native registry name. Missing native
processor/kernel/feature identity rejects planning. The plan retains `HostIdentity`, separate from
live power or CPU availability. The planner never treats a name as kernel eligibility. Unknown
topology or a foreign device rejects the screen.

Doctor exposes the same `host-cpu` output as optional `NativeCpuObservations`; without `-ToolPath`
it retains build-independent inventory. [Native CPU observations](hardware-capabilities.md#native-cpu-observations)
defines group-relative topology, transient availability and the separate compatibility-digest scope.
CPU-set IDs/classes are not persistent affinity settings.

### Reading evidence

The workflow retains plans, CIM/native processor/kernel/feature/topology observations, ISA,
effective arguments, response hashes, raw synthetic captures and per-sample supervision.
Prompt/decode timings and process wall time remain separate. `WallMilliseconds` includes load,
but excludes prior artifact verification and CPU/power observations before creation/after owned
teardown. Fresh processes retain OS file cache; they are not cold-storage trials.

| Memory observation | Meaning and limitation |
| --- | --- |
| `ProcessMemory` | Existing nominal one-second supervisor queries plus final exit when available; successful count retained, including failed/over-ceiling samples; missing data is null with zero samples |
| `SampledPeakPrivateBytes`, `SampledPeakWorkingSetBytes` | Maxima of current process counters; brief allocations may escape sampling |
| `LifetimePeakWorkingSetBytes` | Windows lifetime resident peak, including mapped pages |
| Job commit / system available RAM | Job commit omits mapped residency; one-second physical observations can miss brief pressure |

Private commit, working set, Job commit and available RAM overlap: never add them. These
aggregates identify neither phase allocations/lifetimes, complete model storage nor energy.

For a separate main-only CI attribution capture, select the X2 workflow's
`observe_output_progress` dispatch input, or pass `-ObserveOutputProgress` to
`Invoke-X2CpuEvaluation.ps1` in its owning CI lane. The default remains off.
Supervision's optional `observeOutputProgress: true` requires a finite stdout cap and reuses the existing
file-length checks; it reads no payload and creates no extra polling loop.
`ProcessMemory.OutputProgress` partitions current-counter samples before the first
observed nonempty stdout, after that observation while the root has not been observed
exited, and after root exit. It retains the last observed empty-file time, first
nonempty-file time and whether that first observation happened after exit. Missing
boundaries and unsampled stage peaks remain null, with zero stage samples.

These are file/exit observations: prompt echoes, progress, buffering, final markers and
an exit racing a running observation prevent startup/prefill/decode or first-token
claims. The final exit observation belongs only to the exit bucket. The OS lifetime
working-set peak remains process-wide; stage maxima cannot be summed. Instrumented
captures set `InstrumentedAttribution: true` and suppress every timing ratio even when
conditions and token counts match. Actual-model coverage, phase instrumentation inside
the runtime and complete allocation/lifetime attribution remain pending.

Timing ratios require **both** gates below; invalid conditions preserve raw samples and
independent token-count comparability:

| Gate | Required comparison |
| --- | --- |
| Batch `ExecutionConditions` | Known unchanged native topology, AC, battery saver, active scheme/effective mode and CPU/kernel/full-feature identity at initial/final boundaries |
| `PairExecutionConditions` | Both samples' `CpuConditions.Before`/`After`/`Comparison` plus the transition between them, captured by optional `observeCpuConditions` outside process wall time without model polling |

The pair gate catches mode changes that reverse before batch end. Missing/malformed observations
or legacy receipts without `host_identity` remain raw-only. CIM/registry agreement proves neither
physical SKU nor optional-kernel parity. Power notifications are sequential with a one-second
initial wait. Charge percentage/transient parking/allocation do not gate comparison; equal
boundaries cannot exclude within-sample changes or same-scheme edits. Conditions measure no energy.

All repository device workflows, including TTS/sandbox trials, share one non-cancelling physical-
runner concurrency group; unrelated applications may still contend. Emit candidate/control
ratios only when both reach 128 tokens with matching prompt/decode counts. Natural early
completion stays visible without a fixed-length ratio; response equality is separate from quality.
Two pairs per workload give an initial screen, not significance/sustained qualification.
Retain losses/failures; promising phases need complete-task, quality, responsiveness and comfort
follow-up. The screen supplies no energy verdict, accelerator enablement or new default.

#### CI availability and headless startup

The retained **2026-10-29** GitHub Actions inventory contains zero self-hosted runners and no
X2 screen runs. Device evidence requires native ARM64 `swift-bonsai-snapdragon` on settled `main`;
local model execution cannot substitute for CI.

The pinned legacy `llama-completion.exe --version` fails with `3221225794` (`0xC0000142`, DLL
initialization failure) under ordinary supervision: ten seconds, 512 MiB Job, one process, all 63
verified files held through exit and no selected/loaded model. Identity: source
`73bfe6cc485554822f80c473a3c0c4f25e80f41b`, archive SHA-256
`cc808b33d35c72a3bd5c69a0d609d422426feb9dce23868e77992b6c33b8ddb3`. The PE chain
`llama-completion.exe` → `llama-completion-impl.dll` → `llama-common.dll` → `SHELL32.dll` suggests a
shell dependency without identifying the failing DLL. External
`Bonsai-X2-HeadlessProbe-dcbb58f9ed3440ff809d03673989ec31` retains `runtime-verification.json` and
`verified-headless/supervision.json`. Model trials require a passing bounded worker preflight;
inspection-only help/version cannot qualify it or authorize weaker Win32k denial.

The [UCRT argv source candidate](runtime-candidates/README.md#headless-ucrt-argv-candidate) removes the shell
import in an exact-source CPU build. Its paired control reproduces `0xC0000142`; candidate version
and seven Unicode/quoting/ownership cases pass unchanged worker policy on X2. Source/compiler/
imports/hashes/supervision are retained there. This model-free source result leaves provisioned
runtime/thread defaults unchanged pending upstream release review and device CI.

### Device evidence separation

Native speech builds use a separate checked host identity for build-cache reuse. CPU registry
identity, Windows kernel version, native target and OS feature observations partition the entire
Cargo/native output directory and its receipt. Changing from X1 to X2 repeats native compilation and
CMake's ISA probes. Speech CI retains the same host identity in every compiled-cache restore prefix
and verifies it in the static-library receipt. This prevents an older machine's native output from
silently bypassing X2 compilation; it does not select new thread counts or qualify a speech
performance gain.

LoRA and PTQ1 captures retain the observed processor name and distinguish `Snapdragon X Elite`,
`Snapdragon X2 Elite` and `Snapdragon X2 Elite Extreme`. X2 receipts require a matching processor
observation; relabeling historical X1 evidence rejects. Device-dependent runtime promotion gates
cannot combine different families. Existing frozen X1 contract hashes and legacy receipt shapes
remain readable; identity observations alone do not qualify any X2 profile.

[fearless]: https://docs.rs/fearless_simd/1.1.0/fearless_simd/
[target-feature]: https://doc.rust-lang.org/reference/attributes/codegen.html#the-target_feature-attribute
[detection]: https://doc.rust-lang.org/std/arch/macro.is_aarch64_feature_detected.html
[windows-features]: https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-isprocessorfeaturepresent
[algebraic]: https://doc.rust-lang.org/std/primitive.f32.html#algebraic-operators
[kernels]: https://github.com/symbiogenesis/prism-inference/blob/main/src/kernels.rs
[graph]: https://github.com/symbiogenesis/prism-inference/blob/main/src/graph.rs
[ternary]: https://github.com/symbiogenesis/prism-inference/blob/main/tools/ternary/src/lib.rs
[kernel-tests]: https://github.com/symbiogenesis/prism-inference/blob/main/tests/kernels.rs
[simd-tests]: https://github.com/symbiogenesis/prism-inference/blob/main/tests/simd.rs
