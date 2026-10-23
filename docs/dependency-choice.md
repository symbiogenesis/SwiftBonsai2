# Broker dependency evidence

Comparisons target native Windows 11 ARM64 with the pinned nightly, new trait
solver and Polonius. Correctness and unchanged strict request policy precede
timing. Compiler acceptance is not evidence of faster inference.

## Rejected: SIMD-JSON 0.18.1

The focused `candidates_reject_the_same_malformed_schema_and_encodings` regression
found that its Serde adapter accepted the synthetic request
`{"messages":[{"role":"user","content":"\ud800"}]}` and canonicalized its content
to `\u0000`. The production Serde JSON parser rejects the lone high surrogate
with the value-free `ChatSchema` category. This violates the current input
contract and excludes the candidate regardless of throughput. The rejected
adapter, dependency and candidate-only regression were removed after recording
this result. The original experiment is preserved in commit `31d8b2c`; the
focused failure evidence is retained in this session's external diagnostic lane.

## October 23, 2026 measured decision

Keep **Serde 1.0.229, serde_json 1.0.154 and Tokio 1.54.1**. No challenger
qualified for production replacement in this experiment. The experimental
implementations, optional features, workflow, benchmark crate, extra dependency
graph and trial-only lint configuration were removed after recording the result.
Their source remains in commit `c63a7c0762f7cc64df0d67b34001a31fda6a0b5f`.

The [successful CI run](https://github.com/symbiogenesis/SwiftBonsai2/actions/runs/40039211957)
tested that exact revision. Its
[artifact](https://github.com/symbiogenesis/SwiftBonsai2/actions/runs/40039211957/artifacts/12538059153)
contains raw observations, correctness logs, compiler/CPU/OS identities, source
hashes, selection calculations and exclusion reasons. The machine was a
four-core Microsoft Cobalt 100 running native Windows 11 Enterprise build 26200.
Rust was `nightly-2026-10-22-aarch64-pc-windows-msvc`, identifying as
`1.101.0-nightly (1a2c4797e 2026-10-21)`, with
`-Znext-solver=globally -Zpolonius=next -C target-cpu=native` for every candidate.
This is Windows ARM64 evidence, not a Snapdragon-specific or model inference
measurement, and does not compare stable versus nightly performance.

JSON preparation started from immutable input and included schema/history
validation, canonical serialization, allocation/copying and output destruction.
The six cases were small plain/escaped text, 8/32/128-message histories and a
near-1 MiB request. A full warmup round was discarded, then nine measured rounds
rotated candidate order. The predeclared selection required at least 5% lower
equal-weight geometric mean time and no case more than 5% slower. The existing
schema's Unicode, duplicate/unknown-field, shape, bound and canonical-byte
contracts were checked before timing; complete broker correctness tests also
passed in CI with the experimental feature enabled.

| JSON candidate | Aggregate time versus serde_json | Worst case | Decision |
| --- | ---: | ---: | --- |
| serde_json | 1.0000x | 1.0000x | Retain |
| Sonic 0.5.10 | 1.0026x | 1.1160x near body limit | No qualifying gain; remove |
| Bespoke fused Serde visitor | 1.0408x | 1.2442x small plain input | No qualifying gain; remove |
| SIMD-JSON 0.18.1, including input copy | Not timed | Strict Unicode mismatch | Remove |

Sonic improved the measured history cases by about 3.5–4.7%, while the bespoke
visitor improved the 128-message case by about 6.9%. Neither compensated for its
regressions across the declared workload. These results reject these prototypes
for this selection; they do not establish universal rankings for the libraries.

Runtime screening used verified echo frames on reused loopback connections,
one or nine concurrent connections, and 256/4096-byte frames. Setup and warmup
were excluded. Ratios below are geometric means of median time per exchange;
these short observations are screening results, not full broker qualification.

| Runtime candidate | Time versus Tokio | Decision |
| --- | ---: | --- |
| Tokio current-thread | 1.0000x | Retain |
| smol 2.0.2 | 1.0264x | No demonstrated advantage; remove trial |
| Compio 0.19.2 | 1.0214x | No demonstrated advantage; remove trial |
| Bespoke blocking threads | 0.7357x | Does not qualify as a broker replacement; remove trial |

The blocking design was approximately flat with one connection (0.9702x and
1.0234x). Its aggregate advantage came from nine concurrently active client
threads (0.5598x and 0.5269x), whereas the async candidates used one client thread.
The broker permits only one active model request. The screen omitted thread
creation/stack costs and did not implement HTTP policy, queued admission,
disconnect cancellation or absolute request deadlines. It therefore provides
no demonstrated advantage for replacing our existing bounded Tokio runner.
The blocking design's general merit remains unresolved; its trial implementation
was removed rather than retained as an unqualified production option.

Polonius and the new solver remain useful for future concrete lending APIs.
This experiment supplies no reason to replace established JSON or I/O machinery
or to change the production compiler baseline merely to enable those analyses.
