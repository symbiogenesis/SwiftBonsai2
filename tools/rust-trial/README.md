# Rust compiler trial

This dependency-free, unpublished crate keeps the Rust 1.99.1 / edition 2024
baseline and the repository's strict safety and lint settings. It explores
borrowed logits without changing the production `Decoder` contract, numerical
kernels, checkpoint ownership, or toolchain selection.

## What works on stable

`LogitsSource` uses a generic associated output type tied to an exclusive borrow
of its source. `LogitsCache` implements it with `&[f32]`, allocated once by its
constructor. A cache hit returns the existing values; changing the token or
calling `invalidate()` refills the same buffer. `maximum_logit` demonstrates a
generic consumer that finishes using the view before the next evaluation.

The two regression cases check an owned numerical reference, cache hits,
invalidation, an empty vocabulary, and unchanged storage address/capacity across
refills. The refill loop writes existing elements and does not allocate a new
output vector. These checks demonstrate a storage ownership technique; they are
not allocation profiling, a benchmark, model parity, or a claim about inference
speed. The data is synthetic and the cache key does not represent a production
decoder's full sequence state.

This reusable buffer and lending interface already work on stable. Nightly is
only needed for the two additional source shapes below.

## Compiler capability fixtures

Files in `fixtures/` are compiled directly with `rustc --edition=2024
--crate-type=lib --emit=metadata`; Cargo does not treat them as library modules,
examples, or tests. Every fixture forbids unsafe code and denies warnings and
missing documentation.

| Fixture | Purpose | Stable / old component | New component |
| --- | --- | --- | --- |
| `stable_control.rs` | Refill before returning a borrow; ordinary generic view consumer | Accept | Accept |
| `invalid_alias.rs` | Two live overlapping exclusive references | Reject `E0499` | Reject `E0499` |
| `polonius.rs` | Return a cached borrow on one branch; refill through the original owner on the other | Reject `E0499` | Accept with Polonius Alpha |
| `next_solver.rs` | Normalize a lifetime-generic backend view in a higher-ranked callback bound | Reject `E0277` | Accept with the next trait solver |

The conditional cache example follows the Rust project's
[Polonius Alpha announcement](https://blog.rust-lang.org/2026/08/04/enabling-polonius-alpha-on-nightly/).
The callback example adapts the higher-ranked associated type example in the
[next trait solver announcement](https://blog.rust-lang.org/2026/08/21/enabling-next-solver-on-nightly/).
The controls check that a compiler invocation actually accepts valid Rust while
still rejecting invalid aliasing. The complete hosted matrix isolates each
component on the same pinned nightly, including a mode with both disabled.

The table describes the trial's assertions, not proof that production code is
blocked by the old compiler. The backend callback fixture is a small type-system
example, not an implemented accelerator interface. These patterns may simplify
future interfaces; adopting a nightly compiler alone changes neither output
ownership nor inference behavior.

## Validation

The orchestrator is [`scripts/Test-RustNightly.ps1`](../../scripts/Test-RustNightly.ps1).
For one local compiler comparison, supply a new absolute evidence directory
outside every checkout:

```powershell
./scripts/Test-RustNightly.ps1 -Mode Probe -ProbeCase Polonius -EvidenceDirectory C:/absolute/temp/rust-trial-evidence
```

Local formatting, Release builds, Clippy checks, standalone metadata checks, and
an individual focused regression are permitted. The complete test suite and full
compiler matrix run only in hosted CI with `-Mode Probe -FullTests`; do not cover
the full suite by batching filtered local runs. No benchmark is part of this
experiment.

The exact prototype regression names are:

- `tests::borrowed_outputs_match_reference_and_invalidate`
- `tests::repeated_outputs_reuse_storage`
