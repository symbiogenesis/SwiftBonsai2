# Broker dependency choices

Keep **Serde 1.0.229, serde_json 1.0.154 and Tokio 1.54.1**, as pinned in the
[workspace](../Cargo.toml). No evaluated challenger satisfied the production
replacement criteria. Strict request behavior precedes speed, and all benchmarks
run only in CI. Outstanding optional dependency experiments are tracked only in
[PLAN.md](../PLAN.md#deferred-dependency-experiments).

## JSON selection

The declared gate required at least **5% lower equal-weight geometric mean
time**, with no case more than **5% slower**. Six cases covered plain/escaped
small inputs, 8/32/128-message histories and a near-1 MiB request. Timing included
immutable-input preparation, schema/history validation, canonical serialization,
allocation/copying and destruction. One warmup round was discarded; nine measured
rounds rotated order. Unicode, duplicate/unknown-field, shape, bound and canonical
byte correctness are prerequisites for interpreting timing.

| Candidate | Time versus serde_json | Worst case | Decision |
| --- | ---: | ---: | --- |
| serde_json | 1.0000× | 1.0000× | Retain |
| Sonic 0.5.10 | 1.0026× | 1.1160× near body limit | No qualifying gain |
| Fused Serde visitor | 1.0408× | 1.2442× small plain input | No qualifying gain |
| SIMD-JSON 0.18.1 | Not timed | Strict Unicode mismatch | Reject |
| SIMD-JSON fixed source `ff8f5c8` | 1.3807× | 1.9802× near body limit | No qualifying gain |

SIMD-JSON's Serde adapter accepted `{"messages":[{"role":"user","content":"\ud800"}]}`
and canonicalized the lone surrogate to `\u0000`; production serde_json rejects
it as value-free `ChatSchema`. That contract violation excluded the tested
release regardless of speed.

Upstream revision
[`ff8f5c8`](https://github.com/simd-lite/simd-json/commit/ff8f5c8bb75dfc56803321c2f5a92656c7de842f)
rejects lone high surrogates, malformed surrogate escapes and second escapes
outside the low-surrogate range. It is distinct from the evaluated `v0.18.1`
tag despite carrying the same package version. The fixed-source screen below
does not qualify a production replacement; the exact source revision matters.

The complete fixed source is pinned as the `third_party/simd-json` Git submodule
until an upstream release includes the required fixes. Initialize it with
`git submodule update --init third_party/simd-json` before Cargo commands. It is
excluded from the first-party workspace. The broker's optional `json-candidate`
feature exposes the comparison parser while ordinary requests retain serde_json.
Both parsers share schema/history validation and canonical serialization; the
candidate includes the immutable-input copy in its measured work. The main-only
`json-candidate.yml` workflow runs correctness prerequisites before the same
six-case, rotated-round timing gate. Requests with an explicit adapter scale and
SIMD parsing failures use production parsing to retain its exact decimal and
rejection semantics. Supplemental scale-setting workloads include this fallback
cost; any supplemental case over 5% slower prevents overall qualification even
when the original six-case screen passes. A submodule checkout or successful
build does not promote the parser.

The [fixed-source CI comparison](https://github.com/symbiogenesis/SwiftBonsai2Runner/actions/runs/40405967479)
tested Runner `9448284a4057fb70fb039520debef44250851a76` on Windows ARM64
Cobalt 100 with the pinned nightly. All six original cases were slower:
1.3807× equal-weight geometric-mean time and 1.9802× worst-case time near the
body limit. Supplemental explicit-scale cases took 1.7289–1.7979× production
time, including parsing fallback. Both the original and overall screens failed;
retain serde_json. The `json-candidate-40405967479-1` artifact contains raw rounds,
decisions and source/compiler/CPU/OS identities. This is broker preparation
evidence on that hosted CPU, not Snapdragon inference throughput.

[Follow-up #1](https://github.com/symbiogenesis/SwiftBonsai2Runner/issues/1)
uses the existing `fill_tape_padded` API followed by `Tape::deserialize`, avoiding
the Serde entry point's internal input copy. The immutable-body copy, padding,
allocation, validation, serialization and destruction remain timed. The receipt
identifies this path as `padded-tape-serde`; the result above describes the older
copying path. Production and the acceptance thresholds remain unchanged until
the follow-up CI comparison passes. The decimal fallback remains necessary:
SIMD-JSON's correctly rounded conversion can differ from serde_json's default
conversion, even when neither parser has a bug.

Sonic's 3.5–4.7% history-case gains and the fused visitor's 6.9% gain at 128
messages did not offset other regressions. These results reject the tested
prototypes for this workload; they are not universal library rankings.

## Runtime screening

Reused loopback echo connections used 256/4096-byte frames and one/nine concurrent
connections, excluding setup/warmup. Ratios are geometric means of median time
per exchange, not full broker or model-inference qualification.

| Candidate | Time versus Tokio | Decision |
| --- | ---: | --- |
| Tokio current-thread | 1.0000× | Retain |
| smol 2.0.2 | 1.0264× | No demonstrated advantage |
| Compio 0.19.2 | 1.0214× | No demonstrated advantage |
| Blocking threads | 0.7357× | Does not qualify as a broker replacement |

The blocking design's aggregate gain came from nine active client threads;
async candidates used one client thread. One-connection results were roughly
flat (0.9702×/1.0234×), and the broker permits one active model request. The
screen omitted thread creation/stacks, HTTP policy, queued admission,
disconnect cancellation and absolute deadlines. It supplies no demonstrated
advantage for replacing the bounded Tokio runner.

## Evidence limits

The [retained comparison artifact](https://github.com/symbiogenesis/SwiftBonsai2Runner/actions/runs/40039211957/artifacts/12538059153)
binds these observations to their compiler/CPU/OS identities, hashes, selection
calculations and exclusions. Its four-core Cobalt 100 Windows ARM64 host and
common nightly compiler define the scope of the ratios above.

This is Windows ARM64 evidence, not Snapdragon inference evidence or a
stable-versus-nightly comparison. The [pinned nightly](../rust-toolchain.toml)
enables branch-sensitive lending in the broker; this dependency comparison
does not establish a compiler speedup.
