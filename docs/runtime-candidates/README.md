# Prism row-parallel source candidate

The [patch](oxibonsai-row-parallel.patch) retains Prism commit
`5ff5f3c4719362da6efcbb58114b702db282e5b2` for upstream review; the
[manifest](oxibonsai-row-parallel.json) binds base archive and patch length/SHA-256. It
is source delivery, without release qualification or a Runner lock override.

The base is Prism main `ff3d54de662ddd951dc0ca51d9581129b55239bf`, newer than Runner's
pinned `02c8a24b31ac1c35f2ffd3530f6a753ce440bdc0`. The candidate retains that newer
base's arithmetic, including native PTQ alias handling. Applying it directly to the
older released source is unsupported. Review the intervening source changes
independently before any release/pin update.

## Implementation

The opt-in `parallel-rows` feature adds a private persistent Rayon 1.12.0 pool.
`--row-workers` selects an explicit budget of 1 through 16 for the Swift JSONL worker or
private HTTP child. One retains serial execution and creates no pool; requesting
multiple workers without the feature rejects before loading a model. Each worker
requests a 512 KiB stack. Pool creation, stack commitments and scheduling overhead
remain part of device resource/performance qualification.

The live prepared-projection path borrows original packed weight rows and shared Q8
activations, partitions pre-reserved output into disjoint contiguous chunks, and retains
at most one reusable error slot per worker. It preserves each row's scalar/NEON
arithmetic, selects the earliest failing row deterministically and clears failed output.
Unsupported floating projections use their existing serial path. Graph reset/checkpoint
recovery retain the chosen scheduling policy and existing cooperative cancellation
boundaries. There is no stronger preemption claim or measured throughput gain.

## Reproduce the source change

Verify patch length and SHA-256 against the manifest. In a clean Prism checkout at the
exact base, apply it with:

```powershell
git switch --detach ff3d54de662ddd951dc0ca51d9581129b55239bf
git am C:/absolute/path/to/oxibonsai-row-parallel.patch
```

Independent-worktree `git am` reproduced the tested commit's entire tree. The patch
includes independently authored Rust, focused checks, CI feature variants and build/CLI
docs; runtime/docs attribute OxiBonsai's scheduling idea and pin.

## Focused validation

Formatting, Release Clippy for enabled runtime/server library, binary and test targets,
scalar-only library Clippy, and Release rustdoc passed. Nine focused test executions
passed, covering seven distinct names (worker policy and server options ran with the
feature both enabled and disabled):

| Target | Focused case |
| --- | --- |
| `parallel_rows` | `enabled::parallel_rows_preserve_packed_arithmetic_and_prepared_siblings` |
| `parallel_rows` | `enabled::parallel_rows_clear_failed_output_and_recover_with_serial_float_fallback` |
| runtime library | `kernels::parallel_rows::tests::pinned_parallel_rows_preserve_both_activation_contracts` |
| runtime library | `kernels::parallel_rows::tests::row_tasks_borrow_original_storage_and_return_first_row_failure` |
| `graph` | `graph_parallel_rows_preserve_logits_checkpoints_and_recovery` |
| `parallel_rows` | `row_worker_policy_is_explicit_and_bounded` |
| server library | `native_child::tests::native_child_rejects_unknown_duplicate_relative_and_unbounded_options` |

Use the pinned toolchain from an external working/build directory, with an absolute
manifest and target directory. For example:

```powershell
cargo +nightly-2026-10-22 test --release --locked --offline `
  --manifest-path C:/absolute/prism/Cargo.toml --target-dir C:/external/build `
  --package prism-inference --features parallel-rows --test parallel_rows `
  enabled::parallel_rows_preserve_packed_arithmetic_and_prepared_siblings -- --exact
```

Checks cover actual Swift widths 5120/17408/6144, odd-group activation fallback, generic
PQ2's fourth symbol, the base's PTQ aliases, malformed/nonfinite payload recovery,
pinned accumulation, shared-storage identity and complete synthetic graph
checkpoint/reset behavior. Full suites, benchmarks, actual-model/device qualification
and release publication remain separate work under [PLAN.md](../../PLAN.md#cpu-kernels)
and the [runtime lock-update
procedure](../development-guide.md#prepare-a-runtime-lock-update).

## Legacy completion startup candidate

The [headless UCRT argv candidate](headless-ucrt-argv.md) retains a separately
reviewable MIT-licensed change against the exact legacy Prism source. Its CPU control
reproduces the X2 headless DLL-initialization failure; its candidate passes fixed
startup and Unicode argument checks with worker mitigations intact. It leaves Runner's
runtime lock unchanged pending upstream integration and CI.
