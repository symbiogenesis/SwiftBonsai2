# Bonsai maintenance tools

`bonsai-tools` provides private credential creation, bounded completion-process
supervision, and the fixed LoRA qualification policy/scorer. It reuses the
existing Windows boundary crates and forbids unsafe code. Inference executables,
arguments, compiler concurrency, and model qualification requirements are retained.

## Packaged use

Foundation CI publishes a native Windows ARM64 executable. Select the artifact
from the main revision being evaluated and extract it outside Git:

```powershell
$env:BONSAI_TOOLS_PATH = 'C:\BonsaiTools\bonsai-tools.exe'
./scripts/New-DevelopmentKeys.ps1 -ParentDirectory C:\BonsaiData
./scripts/Invoke-CpuCheck.ps1 -DataDirectory C:\BonsaiData -CheckOnly
./scripts/Get-LoraQualification.ps1 -EvidencePath C:\Evidence\capture.json
```

Wrappers also accept `-ToolPath` where applicable. The shared bridge checks the
tool's version and protocol. This is compatibility checking, not source-to-binary
attestation; choose an executable from a trusted build. Key creation's `-WhatIf`
creates no credentials and resolves no executable.

Without a selected binary, the shared resolver builds the reviewed sources with
pinned stable Rust and `--locked --release`, retaining incremental outputs under
system temp. This developer fallback requires a compiler and locked dependencies.
Packaged use avoids compilation during normal commands. GGUF wrappers use the
same resolver and retain their existing `-GgufToolPath` interface.

## Command boundary

```text
bonsai-tools --version
bonsai-tools credentials --request ABSOLUTE_JSON_PATH
bonsai-tools memory --request ABSOLUTE_JSON_PATH
bonsai-tools completion --request ABSOLUTE_JSON_PATH
bonsai-tools qualification --request ABSOLUTE_JSON_PATH
```

Requests are bounded JSON objects read through one retained read-only handle.
Success writes one JSON object to stdout; failures write diagnostics to stderr
with a nonzero exit code. The bridge keeps native working directories and failed
requests under system temp and preserves inherited Windows/profile environment
variables. Credential receipts contain paths only.

Completion supervision owns the child and its Job for the complete operation.
It retains verified artifact handles and checks parent identity/liveness, current
physical memory, process memory, deadlines, and caller-specific output limits.
LoRA captures share verification and supervision across all scheduled samples.
Parent identity includes creation time to avoid PID-reuse confusion. This remains
trusted-input development tooling, without restricted-token, filesystem, or
egress isolation.

Qualification scoring reads original bounded evidence/review files directly
through `Get-LoraQualification.ps1`. Committed legacy fixtures bind exact control
serialization, Unicode response hashes, and accepted/pending/rejected reports.
Missing external reviews and numerical/basis evidence remain pending. Planning
may resolve the maintenance executable; it never starts inference.

## Development checks

Use explicit workspace package and feature selections. Native Prism linking and
compiler trials retain separate target/cache lanes. Run Cargo from an external
temporary working directory with an absolute external `--target-dir`. Full suites
and all benchmarks remain CI-only; local builds, Release analyzers, and focused
correctness checks are supported. No inference speedup is claimed by this tooling
migration.
