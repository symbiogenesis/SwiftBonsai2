# Implementation decisions and evidence

This page records design rationale. Commands live in the [development guide](development-guide.md),
checks in [validation](validation.md), sources in the [research register](research-register.md),
and outstanding work in [PLAN.md](../PLAN.md).

## Native UI and interoperability

The lasting [UI contract](ui-architecture.md) requires Rust code and native
presentation. Compatibility belongs at the [HTTP/JSON/SSE boundary](broker-policy.md),
whose strict subset can require adapters for generic clients. A native window
establishes neither runtime qualification nor broad OpenAI SDK compatibility.

## Model and runtime evidence

The [model/runtime index](README.md#models-memory-and-runtime) maps artifact,
arithmetic, adapter and promotion contracts. Keep their evidence separate:
lossless storage does not prove numerical parity, source checks do not qualify
a release, and base-model results do not qualify Swift. Launch profiles remain
[CPU-only](hardware-profiles.md).

Swift's bounded completion check omits a system-only preamble rejected by its
template; it does not replace that template or qualify another chat profile.
Completion/development checks explicitly request the verified templates' thinking-on/`xhigh`
defaults. [Inference defaults](inference-defaults.md) distinguishes those checks
from the app's thinking-off default and records sampler and harness limits.

## Provisioning and file integrity

Bounded downloads resume only when the server honors ranges and pass size/hash
checks before publication. Cancellation reaches streamed bodies; header timeouts
alone cannot bound reads. Extraction rejects unsafe/duplicate paths and excessive
expansion, then verifies installed files, ARM64 PE architecture and runtime identity.

Verified files retain write/delete-denying handles. Downloads keep the exclusive
partial-file handle through verification and same-volume hard-link publication,
which cannot overwrite a destination. This requires a hard-link-capable filesystem
such as NTFS; failure preserves the partial. GGUF inspection/export likewise
holds its verified source and validates output before publication.

`bonsai-artifacts` shares ZIP, SHA-256, installed-file and PE checks between broker
and maintenance. Guards retain the archive, verified files and ancestors through
version/help probes; completion re-verifies and holds a fresh guard for the batch.
Receipts cannot transfer live ownership; identity checks prove neither parser
isolation nor source-to-binary provenance.

Primary contracts: [streamed HTTP](https://learn.microsoft.com/en-us/dotnet/api/system.net.http.httpcompletionoption),
[PE format](https://learn.microsoft.com/en-us/windows/win32/debug/pe-format),
[file sharing](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilew),
[exclusive creation](https://doc.rust-lang.org/std/fs/struct.OpenOptions.html#method.create_new),
[hard links](https://learn.microsoft.com/en-us/windows/win32/fileio/hard-links-and-junctions).

## HTTP and admission choices

The [broker policy](broker-policy.md) owns bounds/routes. Its HTTP/1.1 transport
retains duplicate wire headers, checks raw targets before normalization, rejects
ambiguous framing and uses absolute deadlines. Strict JSON visitors reject
duplicate/unknown keys and invalid shapes before rebuilding one immutable request
for counting and generation. Permissive maps/enums can erase rejection evidence.

Socket EOF cannot establish response completeness. Only clean decoded framing
permits downstream terminal chunks; transport completion still does not prove
successful generation. Writers rebuild fixed headers around bounded JSON/SSE.
Frontend EOF, extra input or read failure cancels work, so clients keep their
sending side open. Unacknowledged cancellation closes admission before ownership
is released: new sockets/credentials do not prove the old model slot stopped.

These rules follow [RFC 9110](https://www.rfc-editor.org/rfc/rfc9110.html),
[RFC 9112](https://www.rfc-editor.org/rfc/rfc9112.html) and Tokio's
[absolute timeout contract](https://docs.rs/tokio/1.54.1/tokio/time/fn.timeout_at.html).
Expiry checks surround immediately ready operations too. Rejected parser/runtime
alternatives live in the [dependency audit](dependency-audit.md#json-selection).

## Windows process contracts

The [sandbox architecture](sandboxing.md) owns process, credential and authority
contracts. Atomic suspended-child Job assignment, explicit handle inheritance,
kill-on-close and empty-Job waits protect lifetime/resource ownership. Job commit
does not bound mapped residency/device memory; physical watchdogs remain necessary,
and reported peaks can include denied allocations. Jobs alone establish no
restricted identity, filesystem or egress policy.

The broker reserves `SO_EXCLUSIVEADDRUSE` sockets and checks listener PIDs.
Distinct random credentials receive owner/LocalSystem ACLs before writing and
remain under read-only write/delete-denying guards. These controls alone cannot
prevent same-user/admin access or backend bypass.

Primary contracts: [process attributes](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-updateprocthreadattribute),
[Job limits](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_basic_limit_information),
[commit accounting](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_extended_limit_information),
[exclusive sockets](https://learn.microsoft.com/en-us/windows/win32/winsock/using-so-reuseaddr-and-so-exclusiveaddruse).
Byte-range locks do not protect mapped views. Affinity, large pages, priority and
power hints remain workload-specific tuning candidates.

## Build and numerical boundaries

Rust owns systems code; safe policy/tooling forbids unsafe code, with Win32/FFI/native
loading confined to audited boundaries. `bonsai-cpu-policy` shares profile limits,
memory accounting, completion flags and source/PTQ1 selection. Qualification
derives argv from validated controls and reports their canonical identity.
[Language standards](validation.md#standards) distinguish external-engine exceptions.

Prism's ARM source route requires Clang and must not silently reuse the MSVC
builder. Archives, build configuration and overlays stay pinned. Compiler-object
and Cargo caches are separate; reuse cannot skip checks. Native codec validation
runs the production Rust codec against the pinned DLL in a supervised probe,
using independent base-3 bytes, frozen FP32 scale bits and captured C++ fixtures.
The embedded C# HTTP handler has a separate strict analyzer build.

PQ2/Q8_K and PTQ1/Q8_0 have distinct activation contracts. Integer SIMD parity
does not establish floating-reduction or full-model parity; use the [SIMD](simd-plan.md)
and [algorithm](inference-algorithms.md) contracts. The [pinned nightly](../rust-toolchain.toml)
enables Polonius branch-sensitive broker lending; caller-owned reusable logits
already work on stable. [Compiler probes](https://github.com/symbiogenesis/prism-inference/blob/main/tools/rust-trial/README.md)
retain stable as an isolated control. Release overflow checks stay enabled and
debug assertions disabled to preserve [subtle's release contract](https://github.com/dalek-cryptography/subtle/blob/2.6.1/src/lib.rs#L50-L53).

## 2026 literature: runtime overhead and work placement

Reviewed 2026-10-29. Sources and platform limits live in the
[systems catalog](research-register.md#2026-systems-measurement-and-output-budget-work);
none establishes a defect or new profile in the pinned runtime.

| Register source | Project implication |
| --- | --- |
| S1 ProfInfer | Correlate timelines/counters before optimizing; qualify Windows instrumentation overhead separately from eBPF. |
| S5 rvllm | Audit cache replacement, broadcasts and contiguous operations for repeated copies. Shared immutable storage and independent mutable snapshots need distinct ownership. |
| S7 SMG | Attribute tokenization, parsing and backpressure separately from accelerator service; its broader gateway API is unnecessary here. |
| S4 Elastic Gang | Dynamic participants need acknowledged barrier/epoch transitions; compare existing phase/thread controls before adding a scheduler. |
| S6 Event-triggered SGLang | Measure idle polling, wake latency and active decode separately; affinity only relocates polling. |
| S3 Beyond the Buzz | Charge state handoff, rate matching and coordination before selecting CPU/GPU/NPU phase placement. |

Measure work/traffic before changing representation, ownership, scheduling or fusion.
Preallocation can hide full-size copies; in-place writes can alias snapshots.
Record allocation lifetimes alongside independent state ownership.
[Streaming contracts](streaming-resources.md) cover Runner queues/text/PCM;
runtime experiments belong in Prism. Follow the
[measurement guide](performance-expectations.md#2026-literature-measurement-and-comparison)
for CI-only timing/tracing, actual-model comparisons and evidence limits.
