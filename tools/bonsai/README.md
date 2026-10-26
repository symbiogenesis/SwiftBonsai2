# Bonsai maintenance tools

`bonsai-tools` provides protected credential creation, fixed CPU and server invocation plans,
bounded completion-process supervision and LoRA qualification scoring. It forbids unsafe code and reuses the
audited Windows boundary crates.

## Packaged use

Select Foundation's native Windows ARM64 executable from the evaluated main
revision and extract it outside Git:

```powershell
$env:BONSAI_TOOLS_PATH = 'C:\BonsaiTools\bonsai-tools.exe'
./scripts/New-DevelopmentKeys.ps1 -ParentDirectory C:\BonsaiData
./scripts/Invoke-CpuCheck.ps1 -DataDirectory C:\BonsaiData -CheckOnly
./scripts/Get-LoraQualification.ps1 -EvidencePath C:\Evidence\capture.json
```

Wrappers also accept `-ToolPath` where applicable. Version/protocol checks establish
compatibility, not source-to-binary attestation; use a trusted build. Without a
selected binary, the resolver builds pinned sources with `--locked --release` in
an external incremental system-temp lane. This requires Rust and locked
dependencies. GGUF wrappers share the resolver through `-GgufToolPath`.

Key creation with `-WhatIf` creates nothing and resolves no executable. Planning
may resolve the maintenance tool but never starts inference.

These wrappers require `bonsai-tools` version 0.1.0, protocol 5. The version
handshake advertises supported commands, so a selected binary must support the
requested operation before its request is written. Runtime guards remain held
through version/help probes and complete completion batches. Older protocols are
rejected before an operation can start.

## Command boundary

```text
bonsai-tools --version
bonsai-tools credentials --request ABSOLUTE_JSON_PATH
bonsai-tools memory --request ABSOLUTE_JSON_PATH
bonsai-tools cpu-check-plan --request ABSOLUTE_JSON_PATH
bonsai-tools cpu-check-report --request ABSOLUTE_JSON_PATH
bonsai-tools server-policy --request ABSOLUTE_JSON_PATH
bonsai-tools runtime-stage --request ABSOLUTE_JSON_PATH
bonsai-tools runtime-verify --request ABSOLUTE_JSON_PATH
bonsai-tools completion --request ABSOLUTE_JSON_PATH
bonsai-tools qualification --request ABSOLUTE_JSON_PATH
bonsai-tools speech-models --request ABSOLUTE_JSON_PATH
bonsai-tools inference-lock --request ABSOLUTE_JSON_PATH
bonsai-tools inference-capabilities --request ABSOLUTE_JSON_PATH
bonsai-tools speech-build --request ABSOLUTE_JSON_PATH
bonsai-tools source-build --request ABSOLUTE_JSON_PATH
bonsai-tools chat-launch --request ABSOLUTE_JSON_PATH
```

Requests are JSON objects read through a retained read-only handle, capped at
16 MiB. Qualification envelopes allow 160 MiB for escaped responses; original
evidence/review files remain limited to 16 MiB each and require standard JSON.
Success writes one JSON object to stdout; failure writes stderr diagnostics and
returns nonzero. Credential receipts contain paths only. The bridge uses external
native working directories, retains failed requests and preserves inherited
Windows/profile environment variables.

`speech-models` validates the compiled native Canary and Qwen GGUF locks and
returns the selected download plan and aggregate paths/byte total without
networking or loading models. `speech-build` also owns native compiler/source
preparation, selected daemon builds and explicit model provisioning in Rust.
Provisioning uses Windows curl 8.4+ for HTTPS-only resumable downloads, verifies
exact bytes/SHA-256, then publishes exclusively. Ordinary startup size-checks
cached speech files; the native services verify their hashes before first use.

`source-build` owns content fingerprints, executable digest receipts and the
pinned Cargo invocation for source maintenance/chat builds. Cache hits skip
Cargo; rebuilds use content freshness without cache cleanup. `chat-launch` owns
the source launcher's option validation, provisioning selection, credentials,
arguments and native app lifetime. PowerShell remains a bootstrap/parameter bridge.
Build variants use independent, flushed-and-atomically-published receipts.
Non-build documentation/helper changes preserve reuse; referenced embedded
content and configuration changes invalidate it. Damaged outputs and incomplete
receipts never count as ready. Source-cache entries publish only after full
verified writes, and bad completed download partials get one fresh retry while
retaining diagnostic bytes.

Startup commands additionally advertise `StartupProtocolVersion: 1`. Their
optional `parentProcessId` and `parentCreationTimeFileTime` fields bind long work
to the caller's retained Windows creation identity. PowerShell supplies these
fields automatically; selected older tools receive current-build guidance.
Compiler/tool descendants belong to an atomic Windows Job with an inherited
Unicode environment and child-only overlays. Cancellation waits for the tree to
exit before build-lock release; owner death closes the Job. Model memory and
process policies remain separate and unchanged.

`inference-lock` reviews independently selected Prism release and source ZIPs.
It retains read-only input handles, checks bounded inventories, file hashes,
ARM64 headers, source package versions and the compiler pin, then writes a new
candidate lock and review evidence. It does not extract or execute the release
bundle. `Get-Inference.ps1` keeps its separate provisioning path without a
maintenance-tool or Cargo dependency.

`Get-Inference.ps1 -IncludeCapabilities` additionally invokes
`inference-capabilities` to verify the compiled release archive and every
installed file, retaining their read handles through an owned ten-second,
512 MiB model-free `--capabilities` probe. The exact v1 report is parsed with
the shared CPU-policy contract and must match the pinned version and native
Windows ARM64 target. Each output stream is limited to 16 KiB; stdout remains
separate from stderr. The result adds `Capabilities` and `CapabilityVerification`,
including the retained evidence directory. No model is loaded or profile selected.
This option requires a trusted current maintenance tool (or the normal source
build fallback) and rejects custom locks. Ordinary provisioning is unchanged.

`speech-build` validates the compiled native compiler/source pins, checks compiler
archive names in bulk and stages source ZIPs. ZIP identity, complete path inventory,
Windows aliases, entry types, expansion limits and existing cache bytes are checked
in Rust. ZIP link entries become ordinary files containing their archived bytes.
Its explicit `environment`, `build` and `models` operations additionally own
downloads, compiler TAR extraction, Visual Studio environment setup and native
process execution. Planning and source staging remain nonexecuting operations.

Its `stage-sources` operation verifies and stages the compiled Qwen/GGML archive
pair together, then checks the complete cache inventory before returning.
Existing source-tree redirection and extra-file rejection run inside that operation.

`cpu-check-plan` combines the shared `bonsai-cpu-policy` plan with a current
available-memory observation. Its request contains `operation: "plan"`,
`dataDirectory`, `profile`, `useAbliterateLora` and `useRepackedPtq1`. It returns
the policy, compiled pins, selected paths and complete CPU-check argv without
reading model bytes or starting inference. `cpu-check-report` adds the existing
ordered CPU report and admission status using that same single observation;
`Invoke-CpuCheck.ps1 -CheckOnly` stamps its UTC time and preserves its PowerShell
value types. The original plan command retains its response and avoids report work.

`server-policy` owns reference preflight profile validation, environment-name
rejection and fixed server arguments through `bonsai-cpu-policy`. The development
broker shares the environment and argument policy. PowerShell validates local
storage and matches advertised help switches. Requests carry environment names
only; values are never serialized. Planning remains launch-disabled and may use
the tool resolver's external build/request storage without creating model or key
storage.

Upstream partitioning and baseline policy are owned by Prism's
[conformance policy tool](https://github.com/symbiogenesis/prism-inference/blob/main/tools/conformance-policy/README.md).
They are not maintenance commands in this application repository.

Qualification `invocations` requests take `modelPath` and validated `controls`.
They return all 24 paired and four zero-scale argument vectors with the same
`ControlsSha256` used for scoring. Qualification `plan` optionally accepts
`weightRepresentation` to include the shared capture memory policy. The sampler
and reasoning settings of ordinary CPU checks remain distinct from qualification.

Qualification `capture` retains the CI/device and admission requirements, derives
the fixed batch in Rust and uses the existing completion supervisor once. It saves
initial evidence, reconciles only completed sample receipts after ordinary batch
failures, and retains pending human/logits/basis checks. Abrupt termination leaves
the durable per-sample records and initial summary for inspection.

Completion supervision retains verified artifact handles and owns the complete
child Job lifetime. It checks parent PID/creation time/liveness, physical/process
memory, deadlines and caller-specific output limits. LoRA batches share
verification across samples. Restricted tokens, filesystem/egress isolation and
daily-service security remain unqualified.

`runtime-verify` verifies the whole pinned archive, the complete installation and
ARM64 EXE/DLL headers through `bonsai-artifacts`, then runs bounded version and
optional help probes while every verified file and ancestor remains guarded.
It returns the runtime descriptor and file identities. These receipts do not
retain handles across the JSON boundary: completion receives the descriptor,
verifies it again in Rust and keeps the resulting guard until all children exit.
The selected completion executable must belong to that verified installation.

`runtime-stage` verifies the locked archive and uses the same Rust archive policy
to extract and verify a fresh external staging directory. It rejects unsafe
directory entries, Windows filename aliases, collisions, incorrect ARM64 PE
headers, and archives exceeding 4096 entries or 512 MiB expanded. It runs no probes
and publishes no installation. Failed staging is retained for inspection.
PowerShell downloads the archive, publishes staging with its existing
no-overwrite directory move, then calls `runtime-verify`. Staging receipts do not
retain handles across that boundary; the published runtime is verified again
before probes or execution.

Completed CI samples publish a single `capture.json` containing exact response
text, its UTF-8 SHA-256 and diagnostics. Rust decodes stdout strictly and retains
leading U+FEFF and trailing newlines. `diagnostics.json` remains available for
inspection; the completed `supervision.json` is published after the capture
records. PowerShell joins these records without rereading native stdout. Failed
and incomplete samples never contribute completed response records to scoring.

Qualification scoring reads bounded original evidence/reviews directly.
[Committed fixtures](../../tests/fixtures/lora-qualification-v1.json) bind control
serialization, Unicode response hashes and accepted/pending/rejected reports.
Missing human reviews, native logits or basis evidence remain pending; known
regressions still reject. Scoring never launches inference.

Frozen pre-port PowerShell fixtures cover complete CPU policies, artifact identities
and argument vectors, including Unicode prompts and adapter scale syntax. Pure
policy tests run in Rust; focused script checks cover the packaged JSON bridge.

## Development checks

Use explicit workspace packages/features and external working/target directories.
Prism native linking and compiler trials need separate build lanes. Local builds,
Release analyzers and focused regressions are allowed; complete suites and all
benchmarks remain CI-only. See [validation](../../docs/validation.md).
