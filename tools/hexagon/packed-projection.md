# Packed first-projection operation control

[`packed_projection.rs`](packed_projection.rs) is a std-only safe Rust CPU
control for a future signed Hexagon projection. It opens no driver, loads no
model and dispatches no DSP work. It is an operation oracle and scheduling
candidate; inference integration belongs in Prism. The
[source/capture manifest](packed-sources.json) binds its arithmetic to Runner's
pinned Prism revision and independent native observations.

The control keeps original 34-byte PQ2 blocks, including all four symbols and
raw finite half-scale bits. It stages complete packed tiles without a Q2 copy,
prepares Q8_K once and carries each row's f32 reduction across column tiles.
Each original 128-weight block applies its own scale in original order.
Cancellation before a tile or final publication discards every provisional
output. Input validation and numerical errors also leave the caller's output
empty. A future asynchronous implementation must additionally retain in-use
device buffers until verified completion/teardown; this synchronous CPU control
does not prove that lifetime gate.

The selected mode is **pinned Swift binary arithmetic**. At Prism `7aca225`,
`Arithmetic::PinnedSwift` selects `with_pinned_backend` for the exact verified
Swift artifact identity. It uses fused magic-add Q8_K rounding and ordered
per-PQ2-block FMA, with scalar or eligible NEON integer dots. Generic/source
arithmetic uses rounded-product ties-to-even preparation and nonfused adds;
the two are distinct contracts. The retained 584-byte native Q8_K capture
distinguishes codes 71/72 and -71/-72 at half boundaries. A separate retained
native PQ2 observation distinguishes output bits `c1f90b01` from source-order
`c1f90b02`. Tests run those original inputs through preparation and the packed
tile loop; they never launch the old DLL or regenerate golden bytes.

The optional CPU transform takes exact explicit sign bytes, applies signs
before each 1024-element normalized Sylvester Walsh-Hadamard block and keeps
the input-last-dimension basis. Sign values/extent are checked; **actual model
sign-array provenance is still pending**. Synthetic sign tests establish
axis/order only. PTQ1/Q8_0, odd-128-group PQ2, grouped recurrent permutations,
LoRA/rank-one corrections and batched HMX execution are outside this control.
All adapters and committed graph state remain CPU-owned; no-LoRA and approved
adapter forward paths still require separate qualification.

For `blk.0.ffn_up.weight`, K=5120/M=17408/N=1, 32-row tiles and four Q8_K groups
per column tile, the derived ledger is:

| Storage or traffic | Bytes |
| --- | ---: |
| Borrowed original PQ2 weights | 23,674,880 |
| Prepared serialized Q8_K activations, once | 5,840 |
| Private complete f32 output | 69,632 |
| One packed CPU weight staging tile | 8,704 |
| Proposed device double weight stages + activation tile + row sums, 64-aligned | 18,752 |
| Proposed complete weight transfer, without repeated reads | 23,674,880 |
| Proposed activation transfer, once per 32-row tile | 3,176,960 |
| Proposed complete output return | 69,632 |

The CPU implementation uses one staging tile, reads original weights once,
writes staging once and reads staging once. The proposed device reservation
has two weight tiles for later double buffering; no overlap or DMA has been
implemented. Proposed activation traffic deliberately charges every row tile,
rather than assuming unproved cross-tile residency. A future resident-activation
schedule may reduce this traffic only with matching lifetime/admission evidence.
The ledger does not count DMA descriptors, session/queue storage, mappings,
allocator overhead or actual CPU/device synchronization. Those are additional
charges, including resources outside the Job; they must be added before timing
or admission. An observed VTCM page size is not an effective allocation.

The host ledger's private-output extent is not a complete peak-memory total.
`output.clear()` keeps any caller-owned output capacity until final publication,
so charge that live allocation in addition to the private pending result.
The borrowed F32 input and weights are existing CPU allocations; optional
`transform_input` adds another F32 vector (20,480 bytes for K5120), and caller-owned
sign arrays, Vec metadata/capacity rounding and allocator retention add separate
charges. The proposed VTCM figure is neither a bound on those CPU allocations nor
a complete admitted service-memory bound.

Hosted ARM64 CI owns full controls:

```powershell
python -B tools/hexagon/verify_packed_fixture.py
rustc --edition=2024 --test -D warnings -O tools/hexagon/packed_projection.rs -o ABSOLUTE_EXTERNAL_BUILD/packed-projection-tests.exe
& ABSOLUTE_EXTERNAL_BUILD/packed-projection-tests.exe
```

Cases cover complete native capture records, all packed bytes and full-range
i8 integer bounds, signed zero/subnormal/extreme scales, nonfinite rejection,
opposite-sign maxima ties, unaligned storage, partial row/column tiles at K5120,
preserved reduction carry, cancellation at every tile/commit boundary, late
invalid scales and bounded reservations/refusal. The first-projection ledger
does not execute an actual model matrix. No timing claim follows from any case.
The expanded numerical reference independently checks packed traversal and tile
reduction order; it shares Q8_K preparation, half expansion and FMA arithmetic.
It does not independently qualify those shared routines. The retained native
captures independently bind preparation/FMA only for their original captured
cases. The Walsh impulse control at coordinate 37 uses the closed-form parity
of `popcount(i & 37)` to bind interior butterfly ordering across separate blocks.
Actual packed/sign arrays, independent transform/activation/logit/full-state
parity, signed custom kernels, DMA64, mapping, ownership/failure recovery,
resources and measured whole-request benefit remain open.
