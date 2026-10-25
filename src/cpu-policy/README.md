# Shared CPU policy

`bonsai-cpu-policy` owns the reviewed reference and Swift memory budgets, source
accounting, CPU completion and server arguments, and explicit source/PTQ1 artifact selection.
It forbids unsafe code and uses the workspace's existing Serde dependencies.

The `hardware` module defines the initial H0 runtime discovery parser and
immutable profile/evidence keys. It does not select or qualify profiles; see the
[wire and evidence contracts](../../docs/hardware-capabilities.md).

The maintenance CLI combines its pure plan with a physical-memory observation.
The broker reuses profile memory limits, server arguments and environment-name
policy. The PowerShell server preflight delegates strict reference profile checks
and its launch-disabled plan to the same crate. Qualification samplers remain a
separate contract.

Planning uses compiled immutable configurations and constructs paths without
reading artifacts, querying the operating system, writing files, or starting
processes. Callers validate external storage and retain verified artifacts through
native execution. PTQ1 and adapter choices require explicit Swift-only options.

The committed fixture was frozen from PowerShell before migration. It covers all
five approved profile/adapter/representation combinations, whole policies, artifact
identities, and exact argument vectors. Focused evidence and schema regressions
exercise rejected selections. Full suites and all benchmarks remain CI-only.
