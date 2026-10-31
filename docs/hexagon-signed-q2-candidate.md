# Newer signed generic Q2 candidate

Source review: **2026-10-30**. The separate
[candidate lock](../config/hexagon-signed-q2-candidate.lock.json) identifies a newer
signed-package lead without changing the selected v0.8.0 access package, runtime
pin, CPU fallback or admission. [PLAN.md](../PLAN.md#hexagon-npu) remains the backlog.
The later [bounded data-only audit](evidence/hexagon-signed-q2-package.json)
verified the archive, seventeen runtime files and all five Microsoft catalog
members, including v81. No candidate package code was loaded, executed or installed.

## Exact release and source identity

[GenieX v0.10.0](https://github.com/qualcomm/GenieX/releases/tag/v0.10.0), published
2026-10-28, is the latest stable release observed in the official release API.
Its tag resolves to `071f609995470bdb0e0769ab964754adfaab7e47`; the
[`third-party/llama.cpp` gitlink](https://api.github.com/repos/qualcomm/GenieX/contents/third-party/llama.cpp?ref=071f609995470bdb0e0769ab964754adfaab7e47)
resolves to `19cce6919992fdffd92e0ac152caaf1e2b8f254e`.
The official asset metadata identifies `geniex-sdk-windows-arm64-v0.10.0.zip`,
**92,386,770 bytes**, SHA-256
`410dc39e79a6d0b7e8991bd4267285f7c73de7c46511a1fe0549b83dc4a26e4e`.
The release reports a Microsoft-signed HTP catalog. The retained local audit
independently verifies its current catalog membership and publisher/chain;
loaded DSP identity, actual operation support and numerical parity remain pending.

The newer prerelease [v0.10.1-rc.1](https://github.com/qualcomm/GenieX/releases/tag/v0.10.1-rc.1),
published 2026-10-29, reports the same HTP source revision. The reviewed v0.9.0,
v0.9.1 and v0.9.3 releases use `e467e5b5e5ba43071d6151b3d881deb9300d000e`.
The stable candidate is sufficient for this source decision; no automatic latest-tag
selection follows from this inventory.

All twenty-six reviewed source files have exact repository, revision, path, byte
extent, SHA-256 and URL entries in the candidate lock. They include the release
pipeline, package installation rules, host solver/repack/queue implementation,
DSP operation types, matmul arithmetic, IDL and public compiler sources. The llama
host/DSP source is MIT-licensed; the FastRPC declarations and QAIC compiler have
separately identified BSD licenses. Binary/SDK terms remain separate review inputs.

## Supported low-bit operations and arithmetic limits

The [source build](https://github.com/ggml-org/llama.cpp/blob/19cce6919992fdffd92e0ac152caaf1e2b8f254e/ggml/src/ggml-hexagon/CMakeLists.txt#L93-L100)
includes v73/v75/v79/**v81**/v85 targets. The
[operation types](https://github.com/ggml-org/llama.cpp/blob/19cce6919992fdffd92e0ac152caaf1e2b8f254e/ggml/src/ggml-hexagon/htp/htp-ops.h#L19-L38)
assign generic Q1_0/Q2_0 IDs 41/42. Its
[matmul dispatch](https://github.com/ggml-org/llama.cpp/blob/19cce6919992fdffd92e0ac152caaf1e2b8f254e/ggml/src/ggml-hexagon/htp/matmul-ops.c#L1687-L1706)
contains those kernels. The reviewed type/host/repack/kernel paths expose no native
PQ2_0/PTQ1_0 operation. Newer signed generic low-bit execution is therefore a real
candidate; custom-kernel signing is not the only route worth investigating.

For ordinary matmul, the
[type check](https://github.com/ggml-org/llama.cpp/blob/19cce6919992fdffd92e0ac152caaf1e2b8f254e/ggml/src/ggml-hexagon/htp-kparams.cpp#L971-L995)
requires F32 output, F32/F16 activation and contiguous, nonview, nonpermuted Q2
weights with K divisible by the format block size. Static eligibility is not a
successful VTCM solution or device observation. The solver retains resource and
layout checks; N=2 is excluded by its HMX minimum-row predicate and still needs
effective HVX placement evidence.

The [Q2 repack](https://github.com/ggml-org/llama.cpp/blob/19cce6919992fdffd92e0ac152caaf1e2b8f254e/ggml/src/ggml-hexagon/htp-repack.cpp#L666-L707)
uses 64-weight blocks and 32-row tiles: 512 code bytes plus 64 FP16-scale bytes,
576 bytes total. Device staging aligns these tiles to 640 bytes. A lossless
weight-layout conversion preserves weights only; it cannot qualify arithmetic,
transforms, adapters, outputs or state, and must not become a whole-model copy.

The [activation contract](https://github.com/ggml-org/llama.cpp/blob/19cce6919992fdffd92e0ac152caaf1e2b8f254e/ggml/src/ggml-hexagon/htp/matmul-ops.h#L184-L214)
selects Q8_1 for Q2_0. Each 32-column activation tile consumes 1,280 bytes.
Its [quantizer and Q2 dot](https://github.com/ggml-org/llama.cpp/blob/19cce6919992fdffd92e0ac152caaf1e2b8f254e/ggml/src/ggml-hexagon/htp/hvx-mm-kernels-tiled.h)
use half-precision scale and scaled block sum, separate 32-element normalization,
integer dot/correction, then weight scaling and reduction per 64 weights. The
half reciprocal path and qf32 conversions also need independent qualification.
This is not the pinned Swift Q8_K preparation and ordered per-128-block FMA;
PTQ1/Q8_0 is another independent scope. Generic full-K Q2 output cannot replace
the Swift control merely because packed symbols can be preserved.

A narrower experiment can retain CPU Q8_K preparation and CPU ordered FMA while
asking the signed Q2 kernel for bounded integer-dot components. Scale-one weight
blocks and controlled integer F32 inputs need a proof of exact quantization,
FP16 scale/sum representability, correction, saturation and qf32 reduction. A
prepared Q8_1 memory image is not an established public operation input: ordinary
matmul prepares its activations internally. Guarded input construction, padding,
multiple components and extra outputs must therefore fit the supported F32 path
and pay all preparation, transfer, queue and CPU-combination costs. This is a
conditional source hypothesis, not a device-capable adapter or speedup claim.

## Native Rust computation requires another reviewed boundary

Successful native module opening and terminal FD mapping through the old package
do not submit a DSP operation. Both package generations use a queue-processing
interface. A Rust compute control must bind exact generated-IDL marshalling,
create/export one bounded DSP queue, start processing, register checked operation
buffers and tensor/kernel descriptors, write a request, validate its correlated
response and poisoned numeric output, then stop/drain/close or retain every
uncertain owner until verified teardown. Synchronous host waiting does not remove
asynchronous device ownership or make post-submit cancellation a normal free.

The [new IDL](https://github.com/ggml-org/llama.cpp/blob/19cce6919992fdffd92e0ac152caaf1e2b8f254e/ggml/src/ggml-hexagon/htp/htp_iface.idl)
changes `start` from the selected old revision's five arguments to seven, adding
an explicit VTCM-size request and power mode to the session/queue/HVX/HMX/mapping inputs.
Its [host implementation](https://github.com/ggml-org/llama.cpp/blob/19cce6919992fdffd92e0ac152caaf1e2b8f254e/ggml/src/ggml-hexagon/ggml-hexagon.cpp)
creates/exports the queue, initializes batching/fence storage, starts processing,
checks response extent/buffer count/sequence and stops the interface before
releasing resources. Those controls need a separately reviewed Rust implementation;
copying a function-pointer signature or old five-argument wire layout is insufficient.
The old catalog cannot identify or authorize the newer DSP bytes or changed ABI.

The pinned [public queue declaration](https://github.com/qualcomm/fastrpc/blob/a56e9d4de3614a8d8f4b0e15ff561350c4b686e3/inc/dspqueue.h#L104-L117)
is 24,632 bytes, SHA-256
`571c74ce3651042873b0e93eb53baa7d9ec0963665355933d4a8518272e1e2a3`.
Its buffer record places four u32 fields before a pointer/u64 union. Ordinary
64-bit C layout derives offsets 0/4/8/12/16, extent24 and alignment8. These are
source-derived expectations, not an independently executed Windows ARM64 ABI
probe or proof that the installed driver/SDK use the same declaration. The header
also distinguishes queue export/import and buffer references: successful host
queue creation does not establish remote import or safe buffer release. Bind
matching Windows declarations, native layouts, flags/cache semantics and driver
identity before adding a callable Rust queue boundary.

Generated host marshalling is a concrete prerequisite. The exact build compiles
`htp_iface_stub.c` and invokes the SDK's `build_idl`; the
[DSP build recipe](https://github.com/ggml-org/llama.cpp/blob/19cce6919992fdffd92e0ac152caaf1e2b8f254e/ggml/src/ggml-hexagon/htp/CMakeLists.txt#L17-L56)
also generates its skeleton. The reviewed official repositories did not expose
that generated host stub as source. Before device invocation, retain the exact
QAIC executable identity, command/flags, imported `remote.idl`/`AEEStdDef.idl`,
generated host stub/header digests and an independent model-free wire capture.
That capture must bind inherited method indices, scalar tags, buffer extents and
packing to this seven-argument IDL; declarations alone do not prove that wire ABI.
Keep any temporary native reference outside the production Rust inference path.

The toolchain is obtainable as a separate hosted intake lead. Qualcomm's
[exact SDK setup action](https://github.com/qualcomm/GenieX/blob/071f609995470bdb0e0769ab964754adfaab7e47/.github/actions/setup-snapdragon-sdks/action.yml#L9-L37)
selects Hexagon SDK6.7.0.0 and tools21.0.04 and references
[the SDK release](https://github.com/snapdragon-toolchain/hexagon-sdk/releases/tag/v6.7.0.0).
Its ARM64-WoS archive metadata gives 900,461,020 bytes and SHA-256
`9eb256cc7b102f721365151a470a33980069a18b9209e1d56a6d2fa430369759`.
This audit did not download or execute that archive. The metadata does not identify
its actual QAIC bytes or imported IDLs, establish their license terms or reproduce
the generated stub. Resolve those through checked hosted intake and source-bound
capture; this large SDK preparation is outside the owner-device trial allowance.

An independent public source control is also obtainable. Qualcomm's
[QAIC compiler](https://github.com/qualcomm/QAIC/tree/a66a0019fb488abae955bbc9b3a9de3dc15962d0)
is a Haskell FastRPC IDL generator with C output templates. Its reviewed
[Makefile](https://github.com/qualcomm/QAIC/blob/a66a0019fb488abae955bbc9b3a9de3dc15962d0/src/Makefile)
supports a hosted Linux build through `make -C src TARGET_ARCH=x86_64-linux`.
The source revision alone does not make that build reproducible: its
[Cabal definition](https://github.com/qualcomm/QAIC/blob/a66a0019fb488abae955bbc9b3a9de3dc15962d0/src/qaic.cabal)
leaves dependency versions unbounded. Retain GHC/Cabal identities, package-index
state, a resolved dependency freeze, template/source digests and the resulting
executable identity before treating generated bytes as a repeatable control.
This audit neither built nor executed the generator.

The [documented flags](https://github.com/qualcomm/QAIC/blob/a66a0019fb488abae955bbc9b3a9de3dc15962d0/README.md)
provide a command candidate using `--map-dll`, `--include-path` and `--output-path`,
with its default internal preprocessor. Compile only the checked `htp_iface.idl`,
with the separately pinned public FastRPC
[`AEEStdDef.idl`](https://github.com/qualcomm/fastrpc/blob/a56e9d4de3614a8d8f4b0e15ff561350c4b686e3/idl/AEEStdDef.idl)
and [`remote.idl`](https://github.com/qualcomm/fastrpc/blob/a56e9d4de3614a8d8f4b0e15ff561350c4b686e3/idl/remote.idl)
available as imports. `AEEStdDef.idl` explicitly warns against compiling it as a
standalone input. Retain every import, command and generated header/stub/skeleton
digest. These public imports and generator offer an independent model-free wire
control; equivalence to the SDK6.7 tool/imports and exact signed-module build is
unproven. They do not supply the missing SDK `build_idl` invocation by inference.

The public compiler's
[BSD-3-Clause-Clear license](https://github.com/qualcomm/QAIC/blob/a66a0019fb488abae955bbc9b3a9de3dc15962d0/LICENSE.txt)
permits redistribution subject to its notice/conditions requirements and excludes
an express or implied patent grant. The reviewed C templates carry license and
copyright notices. Preserve those notices and inspect actual generated output
before publishing artifacts; this source license does not establish the terms of
the separate SDK archive. Both generation controls stay outside the production
Rust inference dependency graph.

A tiny old-package F32 compute fixture is a feasible development target after
that queue/lifetime boundary is built; its passing generic C++ fixture remains
separate evidence. The newer Q2 candidate additionally needs source-bound native intake of its
verified package/member set and the new ABI. Neither can be completed by extending the terminal mapping
probe with an unchecked invoke. Production inference remains native Rust Prism,
with no C/C++ inference-library dependency and one session until independently
qualified. No unsafe queue sketch or device dispatch is introduced by this audit.

## Qualification decision

Keep the candidate separate and admission disabled. Before any newer device trial,
prepare its source-bound Rust controls on hosted ARM64 CI; independently verify
package/catalog/v81/driver identities, effective VTCM, queue ownership and exact
numeric outputs. Only then evaluate the integer-dot composition against the bound
first PQ2 tensor, actual signs and CPU control. Preserve complete-state ownership,
full service/physical reserves, Stop and failed-submission cleanup, independent
X1/X2 evidence and the shared five-minute owner-device ceiling. A source or signing
lead alone supplies no profile eligibility, actual-model parity or performance verdict.
