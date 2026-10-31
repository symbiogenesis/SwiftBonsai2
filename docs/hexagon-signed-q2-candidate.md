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

All fourteen reviewed source files have exact repository, revision, path, byte
extent, SHA-256 and URL entries in the candidate lock. They include the release
pipeline, package installation rules, host solver/repack/queue implementation,
DSP operation types, matmul arithmetic and IDL. The reviewed host/DSP source is
MIT-licensed; Qualcomm binary/SDK distribution terms remain separate review inputs.

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
layout checks; N=2 avoids its HMX minimum-row predicate but still needs effective
HVX placement evidence.

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
