# Rust compiler trial

This dependency-free crate compares the stable baseline with the nightly pinned
in [configuration](../../config/rust-nightly-trial.json). It explores borrowed
logits independently of production decoder, numerical and checkpoint contracts.
Stable remains the production toolchain.

## Stable storage reuse

`LogitsSource` ties its generic associated output to an exclusive source borrow.
`LogitsCache` returns `&[f32]` from storage allocated by its constructor. Cache
hits reuse values; changed tokens or `invalidate()` refill that same buffer.
`maximum_logit` demonstrates a consumer that releases its view before reevaluation.

The synthetic regressions compare an owned numerical reference, cache hits,
invalidation, empty vocabulary and unchanged storage address/capacity. They
establish storage ownership/reuse, not allocation profiles, model parity or speed.
The token-only cache key is not a production decoder's sequence state. This
interface works on stable; nightly is needed only for the fixtures below.

## Compiler capability fixtures

`fixtures/` files compile directly with `rustc --edition=2024 --crate-type=lib
--emit=metadata`, outside Cargo's module/test discovery. They forbid unsafe code
and deny warnings/missing documentation.

| Fixture | Assertion | Baseline | Candidate component |
| --- | --- | --- | --- |
| `stable_control.rs` | Refill before lending; generic consumer | Accept | Accept |
| `invalid_alias.rs` | Two overlapping exclusive references | Reject `E0499` | Reject `E0499` |
| `polonius.rs` | Cached-borrow return or owner refill on alternate branch | Reject `E0499` | Accept with Polonius Alpha |
| `next_solver.rs` | Lifetime-generic view in higher-ranked callback bound | Reject `E0277` | Accept with next trait solver |

The examples follow Rust's [Polonius Alpha](https://blog.rust-lang.org/2026/08/04/enabling-polonius-alpha-on-nightly/)
and [next solver](https://blog.rust-lang.org/2026/08/21/enabling-next-solver-on-nightly/)
announcements. Controls distinguish compiler acceptance from broken invocation or
invalid aliasing. The hosted matrix isolates all four component combinations on
one nightly binary, with stable as another baseline. These are compiler
assertions, not evidence that production code requires nightly or that an
accelerator interface exists.

## Validation

For one focused local comparison, give the orchestrator a new absolute evidence
path outside Git:

```powershell
./scripts/Test-RustNightly.ps1 -Mode Probe -ProbeCase Polonius -EvidenceDirectory C:/absolute/temp/rust-trial-evidence
```

Local formatting, Release builds/analyzers, standalone metadata checks and focused
regressions are allowed. Full suites and the complete compiler matrix require
hosted CI with `-FullTests`; never approximate them by batching local selections.
This trial includes no benchmarks. Exact prototype regression names are:

- `tests::borrowed_outputs_match_reference_and_invalidate`
- `tests::repeated_outputs_reuse_storage`

The manual main-only [workflow](../../.github/workflows/rust-nightly.yml) checks
stable/nightly across Runtime/Broker groups in four parallel lanes. Runtime holds
ternary, GGUF, Prism, the CPU reference and this trial; Broker holds Windows
boundaries, broker and maintenance tools. Compiler/group targets remain separate.
Independent probes and every lane must pass the final `compatibility` check,
including evidence retention. Receipts bind compiler identity, exact arguments,
source hashes and verdicts. Native Prism linking remains in Foundation.

`-Mode Compatibility -PackageGroup Runtime` or `Broker` selects formatting,
Release Clippy/documentation locally; the default checks all packages. Full
correctness still requires hosted `-FullTests`. See [validation](../../docs/validation.md).
