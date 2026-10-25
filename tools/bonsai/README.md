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
bonsai-tools server-policy --request ABSOLUTE_JSON_PATH
bonsai-tools prism-source --request ABSOLUTE_JSON_PATH
bonsai-tools runtime-stage --request ABSOLUTE_JSON_PATH
bonsai-tools runtime-verify --request ABSOLUTE_JSON_PATH
bonsai-tools completion --request ABSOLUTE_JSON_PATH
bonsai-tools qualification --request ABSOLUTE_JSON_PATH
```

Requests are JSON objects read through a retained read-only handle, capped at
16 MiB. Qualification envelopes allow 160 MiB for escaped responses; original
evidence/review files remain limited to 16 MiB each and require standard JSON.
Success writes one JSON object to stdout; failure writes stderr diagnostics and
returns nonzero. Credential receipts contain paths only. The bridge uses external
native working directories, retains failed requests and preserves inherited
Windows/profile environment variables.

`cpu-check-plan` combines the shared `bonsai-cpu-policy` plan with a current
available-memory observation. Its request contains `operation: "plan"`,
`dataDirectory`, `profile`, `useAbliterateLora` and `useRepackedPtq1`. It returns
the policy, compiled pins, selected paths and complete CPU-check argv without
reading model bytes or starting inference. `Invoke-CpuCheck.ps1 -CheckOnly`
uses this operation; the former per-function PowerShell policy helpers are removed.

`server-policy` owns reference preflight profile validation, environment-name
rejection and fixed server arguments through `bonsai-cpu-policy`. The development
broker shares the environment and argument policy. PowerShell validates local
storage and matches advertised help switches. Requests carry environment names
only; values are never serialized. Planning remains launch-disabled and may use
the tool resolver's external build/request storage without creating model or key
storage.

`prism-source` selects deterministic ordinary-source partitions and merges the
four revision-bound C++/Rust result pairs. Rust validates numeric provenance,
complete source membership and duplicates while preserving original results and
failure evidence. Reads retain input and ancestor guards on Windows; receipt
digests identify the same bounded bytes that were parsed. Source artifacts accept
UTF-8 and BOM-marked UTF-8, UTF-16 or UTF-32 JSON. The partition wrapper returns
the original PowerShell objects so caller properties and object identity survive.
This command preserves the existing 100-level source JSON limit.

Qualification `invocations` requests take `modelPath` and validated `controls`.
They return all 24 paired and four zero-scale argument vectors with the same
`ControlsSha256` used for scoring. Qualification `plan` optionally accepts
`weightRepresentation` to include the shared capture memory policy. The sampler
and reasoning settings of ordinary CPU checks remain distinct from qualification.

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
