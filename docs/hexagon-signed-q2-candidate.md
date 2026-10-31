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

All twenty-nine reviewed public source files have exact repository, revision, path, byte
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

Additional queue-source review: **2026-10-31**. The pinned
[host driver wrapper](https://github.com/ggml-org/llama.cpp/blob/19cce6919992fdffd92e0ac152caaf1e2b8f254e/ggml/src/ggml-hexagon/htp-drv.cpp)
and [its header](https://github.com/ggml-org/llama.cpp/blob/19cce6919992fdffd92e0ac152caaf1e2b8f254e/ggml/src/ggml-hexagon/htp-drv.h)
are 18,461 and 3,116 bytes respectively; their full digests are in the candidate
lock. They resolve queue create/close/export/write/read/read-noblock exports from
the service-derived `libcdsprpc.dll`. On Windows, an
[explicit read timeout of zero](https://github.com/ggml-org/llama.cpp/blob/19cce6919992fdffd92e0ac152caaf1e2b8f254e/ggml/src/ggml-hexagon/htp-drv.cpp#L174-L184)
routes to `dspqueue_read_noblock`. That differs from the reference header's
[Windows default](https://github.com/ggml-org/llama.cpp/blob/19cce6919992fdffd92e0ac152caaf1e2b8f254e/ggml/src/ggml-hexagon/htp-drv.h#L85-L88),
`DSPQUEUE_TIMEOUT_NONE`: the
[public queue header](https://github.com/qualcomm/fastrpc/blob/a56e9d4de3614a8d8f4b0e15ff561350c4b686e3/inc/dspqueue.h#L20-L21)
defines that as `0xffffffff`, an infinite wait. Neither branch supplies an
aggregate process deadline or proves installed Windows timeout behavior.

This reference is not an ABI to copy. Its
[close function-pointer typedef](https://github.com/ggml-org/llama.cpp/blob/19cce6919992fdffd92e0ac152caaf1e2b8f254e/ggml/src/ggml-hexagon/htp-drv.cpp#L72)
takes the 32-bit `remote_handle`, although the wrapper accepts `remote_handle64`.
The separately pinned
[authoritative declaration](https://github.com/qualcomm/fastrpc/blob/a56e9d4de3614a8d8f4b0e15ff561350c4b686e3/inc/remote.h#L871)
takes a 64-bit handle. Reusing that pointer typedef would narrow the argument;
the existing native Rust terminal control correctly keeps the full u64. This
source mismatch does not diagnose any device observation.

Public queue ownership requirements also remain explicit. A
[buffer reference](https://github.com/qualcomm/fastrpc/blob/a56e9d4de3614a8d8f4b0e15ff561350c4b686e3/inc/dspqueue.h#L95-L127)
requires a prior mapping with the same FD, with its subsection inside that mapping;
reference accounting covers the whole mapped buffer. Reference/dereference and
sender/recipient cache flags are separate. Export returns an ID for a later DSP
import, and [host close can fail while that import remains open](https://github.com/qualcomm/fastrpc/blob/a56e9d4de3614a8d8f4b0e15ff561350c4b686e3/inc/dspqueue.h#L243-L296).
[Blocking read](https://github.com/qualcomm/fastrpc/blob/a56e9d4de3614a8d8f4b0e15ff561350c4b686e3/inc/dspqueue.h#L421-L454)
requires no packet callback. These declarations guide the future ownership review;
they establish neither matching SDK layouts/callback lifetimes nor safe buffer
release, cache completion or vendor teardown. No callable queue binding is added.

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

The toolchain has a separate hosted data-only intake. Qualcomm's
[exact SDK setup action](https://github.com/qualcomm/GenieX/blob/071f609995470bdb0e0769ab964754adfaab7e47/.github/actions/setup-snapdragon-sdks/action.yml#L9-L37)
selects Hexagon SDK6.7.0.0 and tools21.0.04 and references
[the SDK release](https://github.com/snapdragon-toolchain/hexagon-sdk/releases/tag/v6.7.0.0).
Its ARM64-WoS archive metadata gives 900,461,020 bytes and SHA-256
`9eb256cc7b102f721365151a470a33980069a18b9209e1d56a6d2fa430369759`.
The [retained hosted inventory](evidence/hexagon-sdk-inventory-41168523349/provenance.json)
binds run 41168523349 at `9f87dde4c198fff404c024bb737c0861cb3002b2` to the checked
archive and eight selected file identities. It read the archive without extracting
or executing SDK content. The SDK archive was not downloaded on the owner's laptop;
only bounded JSON metadata was retained there. The artifact's reported GitHub digest
is distinct from a locally verified artifact ZIP digest. The retained inventory
hash matches the host receipt, and the three hosted script/pin hashes match the
exact source revision after its Windows LF-to-CRLF checkout conversion.

The SDK data establishes these different candidate inputs; the candidate lock retains
their full hashes and paths. Byte differences require content/ABI review and do not
by themselves prove incompatible semantics.

| SDK input | SDK bytes | Separately pinned public bytes |
| --- | ---: | ---: |
| `qaic.exe` | 35,301,888 | Independent Linux source-built control: 12,077,520; no SDK equivalence |
| `dspqueue.h` | 37,079 | 24,632 |
| `remote.h` | 67,004 | 65,625 |
| `remote.idl` | 1,387 | 1,498 |
| `AEEStdDef.idl` | 3,448 | 1,793 |
| `AEEStdErr.idl` | 7,253 | No candidate import pin |

The SDK `hexagon_fun.cmake` is 36,308 bytes, SHA-256
`805e17e0df4e3a476038c66e7f08787a017e62a9432c2d100212fd9a972cdd85`.
The [second retained review](evidence/hexagon-sdk-inventory-41168872514/provenance.json)
binds run 41168872514 to exact recipe/helper statement tokens without executing
CMake expansion or SDK code. Its `build_idl` function at lines 269–323 selects
`-mdll`, `-o` and the binary directory, followed by default SDK `incs/` and
`incs/stddef/` include paths and every target include directory in order. The
[pinned host target](https://github.com/ggml-org/llama.cpp/blob/19cce6919992fdffd92e0ac152caaf1e2b8f254e/ggml/src/ggml-hexagon/CMakeLists.txt)
adds SDK `incs`, `incs/stddef`, `utils/examples`, the source `htp` directory and
the output directory. Working directory is `ggml/src/ggml-hexagon`; input is
`htp/htp_iface.idl`. The retained command template preserves those paths and
duplicate includes. This function adds no `-rf` or external-preprocessor option.
The helper predicts the header, stub and skeleton filenames. Actual import
resolution, generated output and wire compatibility remain unqualified.

The [third retained data-only review](evidence/hexagon-sdk-functional-41171339745/provenance.json)
binds run 41171339745 at `5feae0d6ac2152692001f8ffcc8b3f13d2cdcf08`
to the same checked archive and four exact SDK/public input pairs. Its receipt
hash, all four hosted source/pin hashes and all seven reviewed input pins match.
Independent review also recomputed the 235 retained declaration-token hashes.
After comment-aware lexical normalization, SDK/public `remote.idl` have the same
25 tokens and `AEEStdDef.idl` the same 42 tokens, despite different raw hashes.
The lexer preserves preprocessor boundaries but does not evaluate conditionals,
resolve imports, check types or establish generator or ABI equivalence.

The queue and remote header token streams differ. Selected queue declarations
match as tokens: the queue handle, buffer structure, buffer flags, existing
callback type, and create/close/read/read-noblock prototypes. The SDK additionally
declares `dspqueue_mdq_callback_t` and a framework packet flag with a different
reserved-mask expression. A shared identifier can occur in several declarations;
different identifier-associated hash sets alone do not prove a changed existing
prototype. The SDK `remote.h` includes `remote_work.h`, which this inventory did
not select. The public header's Windows `remote_ext.h` also lacks an established
pin, so that public header alone cannot serve as a Windows comparison control.
Close the actual SDK transitive inputs and review the effective Windows conditionals,
layout, calling convention and installed driver binding before adding a callable
Rust boundary. Matching declaration tokens leave callback lifetime, FD/cache
ownership, import/close completion and safe teardown unqualified.

The SDK generator's PE machine is `8664` (Windows x64), so running this binary on
hosted Windows ARM64 would involve emulation. The broader legal-filename scan
found six files: a Hexagon-tools `NOTICE.txt` and five bundled Python-package
licenses. The notice is 118,826 bytes, SHA-256
`39af9dbb366f4bbbd28c6d07a411a400bd4b33ad4b7d745287820a9cdcac359a`;
the first UTF-8 decoder did not decode it. The third data review applies an
explicit, uncertain Windows-1252 fallback with no BOM and observes no non-text
control characters. That decoded view has no matches for the bounded QAIC-name,
IDL-compiler or SDK-phrase searches; it does contain license/agreement,
redistribution and patent words. Neither the fallback nor keyword presence or
absence establishes encoding certainty, applicability to QAIC or a grant. Header
confidential/proprietary markers, import redistribution words and binary BSD
strings likewise do not establish the applicable tool/output terms. The
[official product page](https://www.qualcomm.com/developer/software/hexagon-npu-sdk)
names a product license agreement, but the reviewed public response and
[catalog page](https://softwarecenter.qualcomm.com/catalog/item/Hexagon_SDK)
do not expose the exact SDK 6.7 agreement. SDK-generator execution and generated
source redistribution remain unqualified by this evidence; this is not a finding
that the SDK has no license. The matching IDL token streams and selected queue
declarations narrow the source review; effective Windows inputs and semantics
remain pending. All further preparation stays
on hosted CI, outside the owner-device trial allowance.

An independent public source control is also obtainable. Qualcomm's
[QAIC compiler](https://github.com/qualcomm/QAIC/tree/a66a0019fb488abae955bbc9b3a9de3dc15962d0)
is a Haskell FastRPC IDL generator with C output templates. Its reviewed
[Makefile](https://github.com/qualcomm/QAIC/blob/a66a0019fb488abae955bbc9b3a9de3dc15962d0/src/Makefile)
supports a hosted Linux build through `make -C src TARGET_ARCH=x86_64-linux`.
The source revision alone does not make that build reproducible: its
[Cabal definition](https://github.com/qualcomm/QAIC/blob/a66a0019fb488abae955bbc9b3a9de3dc15962d0/src/qaic.cabal)
leaves dependency versions unbounded. The [first retained source
control](evidence/hexagon-public-qaic-41169481882/provenance.json) binds successful
hosted run 41169481882 at `c8b0ad19a258c025c00168298af54f78a33dd8f1` to GHC
9.10.3, Cabal 3.12.1.0, the pinned setup action and the clean QAIC source. It built
the generator and ran the checked public IDL with public imports; version output
is `01.00.51`. No SDK executable, SDK import or device code ran.

Independent receipt review checked the exact Linux script/pin Git blobs, all
fourteen successful command logs, public input and output hashes, eight retained
project/freeze/plan files and the before/after freeze equality. The source
inventory retains 163 tracked file identities, totaling 953,938 bytes. All 38
artifact files are retained as bounded data with their original byte hashes;
captured C/header files have `.txt` suffixes and no build consumer. The reported
GitHub artifact digest is recorded separately from local file verification: the
artifact ZIP digest was not independently recomputed.

The only additional package was `syb 0.7.4`; the retained plan binds its source
tar SHA-256 to `ddcaca983f67a170c0cfdbd037a268779880e2f9b57f93135eaccb6ae4c5da3f`
and records the Cabal-metadata hash and GHC package identities. Both freezes are
unchanged through the build. Requested Hackage index time is
`2026-10-29T00:00:00Z`; the effective frozen index entry is
`2026-10-28T23:27:22Z`. Replaying a checked-in dependency resolution remains a
separate control. The GHC file identity is its 424-byte launcher, so it does not
independently identify the complete compiler/toolchain closure. Cabal and the
12,077,520-byte generator have hosted-reported executable identities; those
executable bytes were not uploaded in the artifact.

The [documented flags](https://github.com/qualcomm/QAIC/blob/a66a0019fb488abae955bbc9b3a9de3dc15962d0/README.md)
provide a command candidate using `--map-dll`, `--include-path` and `--output-path`,
with its default internal preprocessor. Compile only the checked `htp_iface.idl`,
with the separately pinned public FastRPC
[`AEEStdDef.idl`](https://github.com/qualcomm/fastrpc/blob/a56e9d4de3614a8d8f4b0e15ff561350c4b686e3/idl/AEEStdDef.idl)
and [`remote.idl`](https://github.com/qualcomm/fastrpc/blob/a56e9d4de3614a8d8f4b0e15ff561350c4b686e3/idl/remote.idl)
available as imports. `AEEStdDef.idl` explicitly warns against compiling it as a
standalone input. Retain every import, command and generated header/stub/skeleton
digest. The actual generated header is 7,889 bytes, host stub 16,931 bytes and
skeleton 20,188 bytes. Their exact identities and unchanged captured bytes are
retained. This closes public-source generation only; independently captured
marshalling bytes, Rust replay, equivalence to SDK6.7 imports/tool and the exact
signed-module build remain separate gates. A declaration or generated C function
signature does not authorize device invocation.

The pinned public compiler's
[BSD-3-Clause-Clear license](https://github.com/qualcomm/QAIC/blob/a66a0019fb488abae955bbc9b3a9de3dc15962d0/LICENSE.txt)
permits redistribution subject to its notice/conditions requirements and excludes
an express or implied patent grant. The reviewed C templates carry license and
copyright notices. The three generated files contain no copyright or SPDX marker;
the retained bundle therefore keeps the complete QAIC, FastRPC and llama.cpp
licenses, original public IDL notices, generator version notice and separate
template-notice data. Additional read-only primary-source checks matched the
embedded `Allocator`, `Env`, `Slim`, `Skel` and `Wstring` template hashes to the
source inventory and retained their original copyright/SPDX notices as data;
the retained `CStub`/`Stub` pair alone does not describe every embedded template. These public-source terms
do not establish the terms of the separate SDK archive. Both generation controls
stay outside the production Rust inference dependency graph.

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
