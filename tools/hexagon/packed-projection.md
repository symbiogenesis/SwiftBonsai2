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
the input-last-dimension basis. Sign values/extent are checked; [verified extraction](../../docs/evidence/hexagon-swift-projection-binding.json)
binds the original model's signs. Their consumption through this control remains
pending; synthetic sign tests establish axis/order only. PTQ1/Q8_0, odd-128-group PQ2, grouped recurrent permutations,
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
Consumption of the verified packed/sign arrays, independent transform/activation/logit/full-state
parity, signed custom kernels, DMA64, device mapping/visibility, ownership/failure recovery,
resources and measured whole-request benefit remain open.

## Proposed signed-kernel tile protocol

[`packed_tiles.rs`](packed_tiles.rs) defines an application-owned version 1
payload contract and a strict **CPU-only** receiver. This is not a Qualcomm
RPC ABI or a supported operation in the signed GenieX bundle. A future separately
reviewed and signed packed kernel must implement the protocol before device
submission can use it. No native submission or asynchronous buffer lifetime is
implemented by this control.

Each little-endian payload consists of a 64-byte header, complete original PQ2
row fragments and complete pinned prepared Q8_K records. The eight-byte magic is
`PQ2Q8K01`. Fourteen u32 fields follow: version (1), header bytes (64), projection
columns and rows, maximum tile rows and Q8_K groups, starting row and group,
active rows and groups, zero-based sequence, flags, packed weight bytes and
activation bytes. Flags are bit 0 for the first column tile of a row tile, bit 1
for its last column tile and bit 2 for the final projection tile. Other bits
reject. Packed weights remain row-major; each Q8_K group consumes two original
PQ2 blocks. Q8_K records follow weights with their original scale, 256 codes and
sixteen signed i16 sums. No native struct layout or padding crosses this boundary.

`TileSource` prepares activations once and lends each payload from one source-owned
staging vector. The slice prevents source reuse or destruction during synchronous
consumption; asynchronous device consumers still need independently verified
ownership through completion. Its input has already passed the graph's CPU transform.
The receiver derives the exact expected tile from the admitted geometry and
sequence, compares every header byte and checks exact payload length before
accessing records. Unknown versions, changed flags or geometry, duplicates,
reordering, truncation and trailing bytes reject. It verifies finite scales and
Q8_K sums, retains one CPU copy of prepared activations to reject changes across
row tiles, and carries per-row FMA reduction across column tiles. Any arithmetic,
protocol or cancellation error terminally discards provisional output. The
consuming final commit requires every tile and a final cancellation/deadline
check. No partial result is exposed.

For the first projection's 32-row/four-group schedule, there are 2720 payloads,
each at most **9936 bytes**, with **174,080 header bytes** and **27,025,920 total
payload bytes**. Those totals include repeated activation traffic and the
original 23,674,880 packed weight bytes. Vendor RPC, queue and DMA-descriptor
framing, padding, mapping and output-return costs are additional. These are
derived wire extents, not observed transfers. CPU replay additionally retains
5840 bytes of activation identity, its complete private output and caller-owned
staging capacity. It does not change the proposed VTCM reservation or prove
device-resident activation reuse.

The payload carries neither authenticated artifact identity nor trust authority.
The trusted session owner must bind model, sign arrays, prepared input, custom
kernel version/signature, geometry, mapping and resources before submission.
Tests retain byte/traffic parity, awkward final tiles, independent native FMA
bits, changed headers, malformed arithmetic, sequence faults and cancellation.
They replay on CPU; verified artifact extraction does not close consumption through
these controls, signed execution, DMA completion, buffer retention after submission,
full-state parity or benefit.

## Actual artifact extraction candidate

[`packed_artifact.rs`](packed_artifact.rs) is a narrow Rust export tool using
the existing `bonsai_gguf::VerifiedModel` API from exact Prism revision
`7aca2251f09acb345d4afb53d5ce411f81017745`. It adds no GGUF parser, inference
dependency or alternate model admission. Hosted preparation verifies that
source checkout and builds the exact extractor as a temporary Cargo example,
using its `bonsai_gguf` and `sha2` dependencies with default features disabled.
Tracked dependency source remains unchanged; Cargo owns metadata/link resolution. The tool's
model-free `--self-test` checks width selection, complete-array sign validation
and packed-scale rejection without opening a model or device. Building or
executing the candidate alone does not establish actual artifact evidence.

The only extraction invocation is:

```powershell
& ABSOLUTE_HOSTED_BUILD/hexagon-packed-artifact.exe --model ABSOLUTE_SWIFT_GGUF --output-directory NEW_EXTERNAL_DIRECTORY
```

An owned supervisor must enforce the remaining aggregate owner-device deadline
around the entire invocation. The command hashes all **7,206,168,928 bytes** of
the fixed Swift artifact against SHA-256
`5912bb739217cf25b4283a098c7b893d9baff36bdf2e72d3dbf4cb999d5633d5`.
There is no cached verification receipt or caller-supplied identity override.
The verified handle denies writes/deletion on Windows and remains alive through
descriptor, metadata and payload access. Parsing uses the existing bounded
reader with a 128 MiB retained-header allowance. Payload retention adds the
23,674,880-byte projection and a 5120-byte extracted sign vector; parser/hash
buffers and allocator overhead remain separate charges.

The command requires original PQ2 type 142 and `[5120,17408]` geometry for
`blk.0.ffn_up.weight`, checks all raw half scales and retains original packed
bytes. It requires the exact version-1 forward input transform declaration,
block size 1024, explicit signs and widths `[5120,6144,17408]`. All 28,672 sign
values must be integer +1/-1 and their canonical little-endian i32 SHA-256 must
match Prism's existing `3d1ea9136a6044cd680cc1e92cde65e9dd7238e1288fe2effabec5d201c48bed`
control. The first 5120 entries are therefore the exact width-5120 sign vector,
exported losslessly as two's-complement i8 bytes. It does not generate signs,
execute the transform or infer a basis from dimensions alone.

The new external directory receives `weights.pq2`, `signs.i8` and an at-most
8192-byte `binding.json` recording the full artifact, library revision, extractor
source digest, separate output digests, geometry and transform/sign definition.
Existing directories, linked/reparse storage and Git ancestors reject. Files
use exclusive creation and remain held against mutation through final manifest
creation. Failures can retain bounded partial files; only a successful owned
process receipt plus independently verified output digests permits intake.
The tool reads no other tensor and writes no complete-model conversion.

The [bounded actual extraction](../../docs/evidence/hexagon-swift-projection-binding.json)
and independent output-digest checks passed. Consumption through the operation
and device controls remains pending. The
binding establishes original weights and signs only; activations, logits,
complete state, adapters, signed DSP computation and performance need their
separate evidence. Any insufficient shared time allowance leaves extraction
pending rather than splitting its full-file verification across runs.

## Conditional signed Q2 integer primitive

[`q2_integer.rs`](q2_integer.rs) adds a **CPU-only** operation control for using
the Q2_0/Q8_1 arithmetic in the newer signed-package lead as an integer-dot
primitive. Its source identity is llama.cpp
[`19cce6919992fdffd92e0ac152caaf1e2b8f254e`](https://github.com/ggml-org/llama.cpp/tree/19cce6919992fdffd92e0ac152caaf1e2b8f254e),
bundled by GenieX v0.10.0. That source is separate from the v0.8.0 access pin.
This control neither changes the access package nor proves catalog membership,
actual operation entry or numerical execution of the newer binary.

The source's [Q2 dot](https://github.com/ggml-org/llama.cpp/blob/19cce6919992fdffd92e0ac152caaf1e2b8f254e/ggml/src/ggml-hexagon/htp/hvx-mm-kernels-tiled.h#L1543-L1567)
multiplies unsigned two-bit symbols and subtracts two Q8_1 32-code block sums
per 64-weight block. Both activation scales and sums are stored as FP16. Its
[ordinary activation path](https://github.com/ggml-org/llama.cpp/blob/19cce6919992fdffd92e0ac152caaf1e2b8f254e/ggml/src/ggml-hexagon/htp/matmul-ops.c#L681-L704)
quantizes F32 inputs internally. No supported prepared-Q8_1 upload is assumed.
Simply converting original full-range Q8_K codes to scale-one Q8_1 fails to prove
parity: some 32-code sums exceed 2048 and lose odd integer bits in FP16.

The conditional adapter keeps the original CPU Q8_K quantizer and its scale.
For each original PQ2 128-weight block it performs the following bounded work:

1. Split each already prepared i8 code `q` into integer `q / 2` (truncating
   toward zero) and `q - q / 2`. Both limbs lie within -64 through +64, with no
   quantization, discarded bits or alternative activation scale.
2. Preserve every original two-bit symbol. Place at most 31 real lanes in each
   32-lane activation group and add one guard with zero weight (symbol 1).
   Choose the guard as -127 for a nonnegative real-code sum, otherwise +127.
   Pad to K=256 with zero-weight lanes and a guard in every group. Each complete
   group has maximum absolute value exactly 127 and sum magnitude at most 1857.
3. Use logical Q2 scales of one for all four 64-weight blocks and two F32 rows
   for the limbs. The intended Q8_1 scale/inverse is one. All input integers and
   complete block sums are exactly representable in FP16. Each zero-weight guard
   cancels through the unsigned-dot-minus-sum expression; all integer-dot and
   F32 addition intermediates remain well inside the exact 24-bit integer range.
4. Return the two integer dots for that original 128-weight block, add them
   exactly on CPU, and apply `(original_half_scale * original_Q8_K_scale).mul_add`
   into the original CPU accumulator in original block order. No original scale
   or complete projection reduction is delegated to a different floating order.

The actual [quantizer](https://github.com/ggml-org/llama.cpp/blob/19cce6919992fdffd92e0ac152caaf1e2b8f254e/ggml/src/ggml-hexagon/htp/hvx-mm-kernels-tiled.h#L1-L138)
converts inputs/maxima to FP16, forms its half scale using reciprocal-127 bits
`0x2008`, calls a polynomial FP16 inverse and uses HVX rounding/conversion
intrinsics. For maximum 127, the mathematical half-scale product is
0.99993896484375, which rounds to half one; the separate F32 scale used for sums
also rounds to one. **The CPU control does not emulate or qualify the vendor
inverse polynomial and conversion intrinsics.** Exact guarded input codes,
half scales, half offsets and final integer outputs must be independently checked
against the signed binary before this becomes an exact device adapter.

Source [shape checks](https://github.com/ggml-org/llama.cpp/blob/19cce6919992fdffd92e0ac152caaf1e2b8f254e/ggml/src/ggml-hexagon/htp-kparams.cpp#L971-L995)
admit contiguous ordinary Q2 weights with K divisible by 64, F32/F16 activation
and F32 output, subject to later solver/resource checks. K=256 also respects the
activation quantizer's 128-element padding. N=2 is a proposed HVX case: the source
HMX condition excludes N at or below four. This does not prove that a selected
signed deployment admits the constructed operation or that native Rust can
submit its supported entry correctly.

The control exposes only ordinary logical Q2 records and F32 rows. It does not
guess the DSP's repack or RPC ABI. A block needs 72 logical Q2 bytes and 2048 F32
activation bytes, with eight returned bytes per original row. The source's
640-byte Q2 tile for 32 rows and K=64 implies four tiles, or 2560 bytes for this
K=256 primitive, before other framing/storage. For the full first projection:

| Proposed bounded schedule cost | Bytes or work units |
| --- | ---: |
| Original 128-column slices, before row tiling | 40 |
| 32-row tile work units across all slices | 21,760 |
| Repacked weight transfer, once per tile work unit | 55,705,600 bytes |
| F32 activation transfer charged once per tile work unit | 44,564,480 bytes |
| Two F32 limb results per original row/block | 5,570,560 bytes |
| Final complete CPU projection | 69,632 bytes |

These are conservative derived extents, not observed transfers, native request
counts or admitted memory. Actual entry/queue batching is pending; 40 column
slices do not establish 40 submissions. A whole-M logical operation may require
larger host staging, while a 32-row submission may incur many more RPCs. Reuse
or residency cannot reduce the charged traffic without matching ownership and
device evidence. Charge internal Q8_1 quantization, padded activation tiles,
output padding, original scales, DMA, queues, mappings, synchronization and
resources outside the Job separately. The construction uses bounded temporary
packing and never justifies a persistent expanded whole-model copy.

Model-free controls cover every packed byte, the complete i8 range, mixed codes,
31-lane boundaries and guards, scale-one records, retained native per-block FMA
bits and rejection of fractional, nonfinite or oversized returned dots. The
retained FMA capture also rejects nonfused complete-projection reduction. Device
quantizer/integer parity, original artifact consumption, signed native entry,
lifetime/rollback, complete state and performance remain pending. This lead may
avoid a custom-kernel signing requirement for a narrow exact primitive if those
gates pass; its extra work and traffic may also make it uncompetitive.
