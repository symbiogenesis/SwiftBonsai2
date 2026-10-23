# Broker dependency comparison

This opt-in tool compares bounded request preparation and framed loopback I/O on
Windows 11 ARM64. All candidates use the same pinned nightly compiler with
`-Znext-solver=globally -Zpolonius=next -C target-cpu=native`. The production
toolchain is a separate decision; the compiler flags are held constant here.

Run the main-only **Dependency trial** workflow. Benchmarks run only in CI.
Local builds, Release analyzers and focused correctness tests are allowed.
The executable's `--check` mode performs differential correctness without timing;
its JSON report explicitly records ineligible candidates.

JSON candidates are the existing Serde JSON path, Sonic 0.5.10,
and a bespoke streaming Serde visitor.
All use the production schema and canonical backend options. The bespoke path
serializes each validated message immediately, avoiding the intermediate message
vector while retaining the established JSON lexer and string handling. It does
not introduce an independent general-purpose JSON parser or reusable global
scratch. [Rejected options and evidence](../../docs/dependency-choice.md) are
recorded before their implementations and dependencies are removed.

The timed corpus includes small plain/escaped input, 8/32/128-message histories,
and a near-body-limit request. Each trial starts with immutable input and ends
with owned canonical output, including allocation, copying and destruction.
After a discarded warmup, nine rounds rotate engine order. Selection uses the
equal-weight geometric mean of per-case median time ratios. A challenger must
reduce aggregate time by at least 5% and regress no case by more than 5%; otherwise
the existing implementation remains selected. This is a quick screening result,
not inference throughput, memory allocation profiling or battery qualification.

Runtime cases compare Tokio, smol, Compio and bounded standard-library threads
using one/nine connections and 256/4096-byte verified frames. Connections, runtime
initialization and warmup are excluded. Bespoke clients use an OS thread per
connection, while async clients use one thread; resource use is not equivalent.
These reused-connection echo timings cannot alone authorize replacing the
broker's one-request connections, admission, cancellation or deadline behavior.

Artifacts retain all observations, exclusion reasons, medians, selection scores,
compiler flags/identity, CPU/OS/image identity, source digests and tested revision.
The workflow never changes production automatically. The winner is integrated
separately after reviewing correctness and the complete measured tradeoff.
