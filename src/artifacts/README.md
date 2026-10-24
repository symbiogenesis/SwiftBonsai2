# Shared artifact verification

`bonsai-artifacts` shares offline file, ZIP, installation and ARM64 PE verification
between the broker and maintenance tools. It forbids unsafe code and uses existing
workspace dependencies. Policy owners supply immutable identities; the verifier
does not select profiles, download artifacts, install files, or execute code.

Keep `VerifiedFile` and `VerifiedRuntime` alive until every child using their files
has exited. On Windows their handles deny writes, deletion and ancestor replacement.
Runtime verification retains the whole archive and every installed file, rejects
unexpected files/directories, bounds archives at 4096 entries and 512 MiB expanded
data, and requires native ARM64 EXE/DLL headers. Cancellation checkpoints cover
bounded reads and entry iteration.

Serialized descriptors and receipts contain identities, not live handles. A new
process must re-verify the descriptor and retain its own guard. Completion checks
that its executable belongs to the guarded runtime before starting a child, and
retains the same guard across the complete batch.

Synthetic fixtures exercise archive/PE bounds, sharing modes, installation drift,
cancellation cleanup, and guard lifetime through a supervised child. Full suites
and benchmarks run only in CI.
