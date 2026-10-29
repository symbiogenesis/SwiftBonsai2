# Implementation decisions and evidence

This reference explains current design choices and their evidence limits.
Use the [development guide](development-guide.md) for commands,
[validation](validation.md) for checks and [PLAN.md](../PLAN.md) for remaining
delivery work.

## Native UI and interoperability

The first-party UI is `bonsai-chat` in `src/chat-ui/`: Rust application code
using an egui/eframe native window and a Rust HTTP/SSE client. Keep the UI pure Rust
through future changes; browser frontends, JavaScript/TypeScript, HTML/CSS,
WebViews, Electron and Tauri HTML frontends are excluded. Native operating-system
bindings are permitted. See the permanent [UI architecture](ui-architecture.md).

Compatibility with local LLM tools belongs at the protocol boundary: the
OpenAI-style chat endpoint, JSON text messages, bearer authentication and SSE.
The broker's strict schema remains authoritative, so generic clients need
adaptation when they send unsupported fields such as `tools`. A native UI
changes neither the pinned engine nor its independent qualification gates.

## Model and runtime evidence

| Area | Current evidence | Limit |
| --- | --- | --- |
| Model direction | Swift is the current memory priority; the [Qwen literature decision](model-selection.md) remains the default reference | Published base-model results do not establish exact-pack creative quality or laptop comfort |
| Artifact identity | Immutable locks, full length/SHA-256 checks and portable GGUF metadata in `config/` | Hashes and native PE checks do not prove parser isolation or source-to-binary provenance |
| Swift PQ2 | [Actual census](../config/swift.pq2-census.json): 32.76154% zeros, no fourth symbols, finite nonzero scales | This artifact is ternary; the generic native PQ2 loader must still preserve valid fourth symbols |
| Current-weight PTQ1 | [Export receipt](../config/swift.ptq1-export.json): 5,946,648,928 bytes, 1.173 GiB smaller than PQ2, exact symbol/scale reconstruction | PTQ1 uses different activation quantization; weight parity does not establish equal logits or quality |
| Native Swift decode | Fixed 16-token PQ2/PTQ1 checks complete with and without the pinned adapter | No sustained TPS, creative-quality or daily-service acceptance |
| Qwen reference | Verified mixed-type artifact and native CPU route; 18 GiB allowance plus 4 GiB reserve | Artifact checks and route availability do not establish actual-model acceptance |
| LoRA storage | [Immutable B sharing](prism-storage-overlay.md) saves 2.5 MiB in the native allocation/math regression | Adapter basis, full-model performance and standard-profile quality remain gated |
| Rust CPU reference | Verified loader, scalar/NEON hybrid graph, transactional generation, exact Swift chat/sampler, pinned `0.1.5` consumption and [bounded source parity/owned HTTP recovery](native-rust-engine.md) | These bounds do not establish broader-model/full-context, adapter or device qualification |
| Accelerators | [Adreno](adreno-gpu-plan.md) and [Hexagon](hexagon-npu-plan.md) have explicit implementation gates | Current launch profiles are CPU-only |
| Hardware selection goal | [X1/X2 profile plan](hardware-profiles.md) defines capability discovery, separate phase/objective choices and qualified CPU fallback | No automatic selector, cross-generation performance verdict or accepted accelerator default exists yet |

Swift's completion check omits the system-only preamble that its template
rejects. This is a Swift-specific compatibility repair; it does not replace the
embedded template or qualify a thinking-disabled chat profile. CPU checks and
development chat now request the verified templates' thinking-on/`xhigh`
defaults explicitly. See [inference defaults](inference-defaults.md) for the
sampler, prompt/harness decisions and benchmark limits. The
[Swift assessment](swift-memory.md) owns the reproducible commands and budgets.

## Provisioning and file integrity

Provisioning streams bounded downloads, resumes only when the server honors the
range, and verifies size/hash before publishing a final name. A header timeout
does not bound streamed body reads; cancellation must reach the reads too.
Archive installation rejects unsafe paths, duplicates and excessive expansion,
then checks every installed file, PE architecture and reported runtime identity.

Verification retains write/delete-denying handles. Downloads retain the
exclusive partial-file handle through verification and same-volume hard-link
publication, which cannot overwrite an existing destination. A hard-link-capable
filesystem such as NTFS is required; failures retain partials for inspection.
The Rust GGUF CLI similarly holds the verified source through bounded inspection
or export and validates the export before publication.

These choices follow the primary contracts for
[streaming HTTP](https://learn.microsoft.com/en-us/dotnet/api/system.net.http.httpcompletionoption),
[PE files](https://learn.microsoft.com/en-us/windows/win32/debug/pe-format),
[file sharing](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilew),
[exclusive creation](https://doc.rust-lang.org/std/fs/struct.OpenOptions.html#method.create_new)
and [hard links](https://learn.microsoft.com/en-us/windows/win32/fileio/hard-links-and-junctions).

The broker and maintenance supervisor share `bonsai-artifacts` for bounded ZIP,
installed-file, SHA-256 and ARM64 PE checks. Its guards retain the archive, verified
files and ancestors. Runtime version/help probes run under those guards; completion
re-verifies the runtime and retains a fresh guard through the whole batch. A JSON
receipt is evidence, not a live ownership transfer. PowerShell retains download and
installation orchestration, including path checks before extraction.

## HTTP and admission choices

The [broker contract](broker-policy.md) defines the enforced bounds and APIs.
Its narrow HTTP/1.1 transport preserves duplicate wire headers, checks the raw
request target before URI normalization, rejects ambiguous framing and uses
absolute deadlines rather than restarting a timeout per read.

Strict JSON visitors require object requests/messages and string roles, reject
duplicate/unknown keys and rebuild one canonical request. Exact counting and
generation receive identical immutable bytes. A parsed map or permissive enum
alone can erase distinctions the policy needs to reject.

Backend framing must report truncation as an error. Raw socket EOF cannot prove
a complete response. Only clean decoded completion permits downstream terminal
chunk framing; transport completion still does not prove generation success.
The writer rebuilds fixed headers and preserves bounded JSON/SSE payloads.

The supervisor owns endpoints, bounds queue/active work and checks expiry around
immediately ready operations. Frontend EOF, extra input or read failure cancels
work; clients must keep their sending side open. Unacknowledged generation
cancellation closes admission before releasing ownership. A new socket or
credential is not proof that the old model slot stopped.

The framing rules follow [RFC 9110](https://www.rfc-editor.org/rfc/rfc9110.html)
and [RFC 9112](https://www.rfc-editor.org/rfc/rfc9112.html). Tokio's
[absolute timeout contract](https://docs.rs/tokio/1.54.1/tokio/time/fn.timeout_at.html)
explains the explicit deadline checks. The parser selection and rejected
alternatives are recorded in [dependency choices](dependency-choice.md).

## Windows process contracts

`bonsai-windows-process` owns a noninheritable Windows Job, assigned through
`PROC_THREAD_ATTRIBUTE_JOB_LIST` while creating a suspended child. Only explicit
standard handles are inherited. Kill-on-close, aggregate commit and process
limits apply before child code executes. Stop terminates the Job and waits for
it to become empty; dropping ownership also covers abrupt parent death.

The maintenance tool reuses this boundary for CPU/LoRA checks; the broker uses
it for its development backend. Codec validation exercises the production Rust codec against the pinned
released native DLL in a separate supervised probe. Independent base-3 expected
bytes, frozen FP32 scale bits and the captured C++ fixture are its oracles.
The small embedded C# HTTP test handler
remains separately checked by the strict .NET analyzer build.

The Job commit cap does not bound all resident file-backed mappings or device
memory. Physical-memory watchdogs remain necessary, and a reported Job peak can
include denied allocation requests. The Job establishes no restricted identity,
filesystem or egress policy.

The broker requests `SO_EXCLUSIVEADDRUSE` before binding frontend/reservation
sockets and checks backend listener PID ownership. Credential creation protects
owner/LocalSystem ACLs before writing distinct random keys; retained read-only
handles deny modification/deletion. These controls do not prevent same-user or
administrator credential access and backend bypass.

Primary contracts: [process attributes](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-updateprocthreadattribute),
[Job limits](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_basic_limit_information),
[committed memory](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_extended_limit_information)
and [exclusive socket use](https://learn.microsoft.com/en-us/windows/win32/winsock/using-so-reuseaddr-and-so-exclusiveaddruse).
Byte-range locks do not protect mapped views; hard affinity, large pages,
priority and power hints remain workload-dependent tuning candidates.

## Build and numerical boundaries

Rust is the default for repository-owned systems code. Safe policy/tooling and
the CPU reference forbid unsafe code; Win32 and Prism FFI/native-loader calls
remain small audited boundaries. PowerShell owns installation/build orchestration
and delegates maintenance, credentials, CPU completion policy and qualification
planning/scoring to Rust. The shared `bonsai-cpu-policy` crate owns profile limits,
memory accounting, common completion flags and explicit source/PTQ1 selection;
the broker reuses its profile limits and batch sizes. Qualification produces argv
from its validated controls and returns their canonical identity with the plan.
The [language standards](validation.md) distinguish these first-party rules from
the pinned external engine's warning policy.

Prism's ARM source route requires Clang; it must not silently reuse the upstream
MSVC builder. Source archives, build configurations and overlay identities stay
pinned. Compiler-object caches and Cargo caches are separate, and cache reuse
never skips required checks.

PQ2 and PTQ1 have distinct Q8_K/Q8_0 activation contracts. Exact integer SIMD
parity does not automatically establish floating-reduction or full-model parity.
The [SIMD plan](simd-plan.md) defines dispatch and numerical gates; the
[algorithm map](inference-algorithms.md) covers lookup kernels, block prefill,
Gated DeltaNet and hybrid checkpoints. The
[pinned nightly](../rust-toolchain.toml) enables Polonius branch-sensitive
lending in the broker. The [compiler probes](https://github.com/symbiogenesis/prism-inference/blob/main/tools/rust-trial/README.md)
retain stable as an isolated control; caller-owned reusable logits themselves
already work on stable Rust.

Release overflow checks remain enabled, while debug assertions remain disabled
to preserve the pinned constant-time dependency's release contract. See
[`subtle`'s contract](https://github.com/dalek-cryptography/subtle/blob/2.6.1/src/lib.rs#L50-L53).
Local focused regressions establish specific correctness/ownership properties.
Full suites and all benchmarks remain CI-only; synthetic storage
reuse and hosted build timings are not laptop inference-performance evidence.

## 2026 literature: runtime overhead and work placement

Reviewed 2026-10-29. These sources extend the
[research register](research-register.md), without establishing a new Runner
profile or a defect in the pinned Prism runtime.

| Source | Mechanism worth inspecting | Boundary for this project |
| --- | --- | --- |
| [ProfInfer, MLSys 2026](https://proceedings.mlsys.org/paper_files/paper/2026/hash/03dbc11a22e79cd38bea53cf518c2371-Abstract-Conference.html) | Correlate operator/graph timelines with hardware counters before choosing a kernel, layout or scheduling experiment. | Its eBPF probes target a different runtime/OS environment. Transfer the attribution method; Windows tracing and instrumentation need their own overhead check. |
| [Rust rvllm optimization report, 2026-09-24](https://ermolushka.github.io/posts/rvllm-memcpy/) | Hidden cache replacement and broadcast/contiguous operations can copy large tensors on every token. Distinguish a tensor view, shared immutable allocation and independent mutable cache. | Author report on M5/SmolLM2-360M with weights expanded to F32. Frozen output checks support that experiment; its throughput gains are not predictions for packed Swift or evidence Prism repeats those copies. |
| [SMG, 2026-04-30, updated June 2](https://pytorch.org/blog/lightseek-smg/) | Separate tokenization, streaming parsing and other CPU work from accelerator execution; measure pipeline backpressure at each boundary. | Rust gateway/GIL evidence concerns distributed Python serving. Runner already uses Rust and a bounded supported protocol; adopting its broad gRPC/API/tool surface is unnecessary. |
| [Elastic Gang, 2026-07-06](https://arxiv.org/abs/2607.04668v1) | A generation-tagged acknowledgement protocol changes a barriered inference participant set between tokens while other OS work continues. | Bare-metal x86 Rust OS preprint, not a Windows scheduler library. Dynamic membership requires an explicit barrier/epoch correctness contract; existing safe phase/thread controls are the comparison. |
| [Event-triggered SGLang report, 2026-08-17](https://bobodell.com/blog/event-triggered-local-llm-inference-sglang-idle-cpu) | Measure idle polling separately from active inference. Parking idle workers can remove CPU work, while affinity alone only changes where a poll loop runs. | First-person Linux/dual-GPU report. Wake latency and active decoding can favor different wait policies; it does not establish a universally optimal sleep flag for Prism. |
| [Beyond the Buzz, MLSys 2026](https://proceedings.mlsys.org/paper_files/paper/2026/hash/d49cee5f3a79d97d719df255689d83d7-Abstract-Conference.html) | Prefill/decode separation pays off only in suitable workloads after transfer, rate matching and coordination costs. | Multi-node serving study. Local CPU/Adreno/Hexagon phase placement still needs complete hybrid-state handoff and measured break-even; disaggregation is not automatically beneficial. |

The review order is actual work/traffic first, representation and ownership
second, then finer scheduling/fusion. Preallocated buffers can still hide
repeated full-size copies; an in-place write can accidentally alias a snapshot.
Record allocation/copy lifetimes and independent state ownership together.
The [streaming resource contracts](streaming-resources.md) already bound Runner's
queues and text/PCM storage; runtime experiments belong upstream in Prism.

All timing, tracing and actual-model comparisons remain CI-only under
[validation](validation.md). Use [the performance guide](performance-expectations.md#2026-literature-measurement-and-comparison)
for the measurement implications of this survey.
