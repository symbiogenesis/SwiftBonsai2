# Pinned Hexagon source audit

Review date: **2026-10-30**. This is a source audit and prototype placement decision.
It contains no device execution, signing verification, Swift parity or performance
result. [PLAN.md](../PLAN.md#hexagon-npu) remains the outstanding-work source;
[Hexagon contracts](hexagon.md) govern execution. The
[experiment lock](../config/hexagon-experiment.lock.json) pins research inputs and
keeps admission disabled; it is not an installed-runtime or promotion lock.

## Signed access route and its limits

The [GenieX v0.8.0 release](https://github.com/qualcomm/GenieX/releases/tag/v0.8.0)
reports a Microsoft-signed HTP catalog requiring no certificate import. Its
[source commit](https://github.com/qualcomm/GenieX/commit/a5c8e7fb4a364ddcccefbf187afd1bac52d88de5)
is `a5c8e7fb4a364ddcccefbf187afd1bac52d88de5`; the `third-party/llama.cpp`
gitlink identifies `94256114c229674ef96e76eb2dea596e65b43818`.
The public release API identifies `geniex-sdk-windows-arm64-v0.8.0.zip`,
**84,788,957 bytes**, SHA-256
`5676e27cc5639717890ce92f3beec937a6aefc645c9cc57bcb537f972b4fecc7`.
The source audit did not download or run this archive.

The pinned [release workflow](https://github.com/qualcomm/GenieX/blob/a5c8e7fb4a364ddcccefbf187afd1bac52d88de5/.github/workflows/release.yml)
overlays a signed HTP bundle selected by its bundled llama.cpp revision. A signed
catalog for these kernels cannot authorize modified PQ2 kernels or a different
host/DSP RPC ABI. Verify archive digest, actual catalog membership and trusted
signature chain, exact v73/v81 SO, host DLL and driver identities before loading.
Authenticode status of an ELF SO alone does not verify Windows catalog membership.
The release's signing statement supplies a candidate, not local verification.

The [SDK plugin build](https://github.com/qualcomm/GenieX/blob/a5c8e7fb4a364ddcccefbf187afd1bac52d88de5/sdk/plugins/llama_cpp/CMakeLists.txt)
sets `LLAMA_BUILD_TESTS OFF`: `test-backend-ops` is not a promised SDK fixture.
Its install rules put runtime libraries, `libggml-htp-v73.so` through
`libggml-htp-v81.so` and `libggml-htp.cat` under `lib/llama_cpp`. Prepare any small
supported-type dispatch fixture on hosted CI against this exact ABI. It must
verify an expected numeric result and effective Hexagon execution; device
enumeration or successful DLL loading does not prove mapping or dispatch.
The host libraries also use `-ffp-model=fast` in the reviewed Clang/Windows
configuration, so this package is an access control rather than a strict Swift
floating-order oracle.

At the bundled [Hexagon source](https://github.com/ggml-org/llama.cpp/blob/94256114c229674ef96e76eb2dea596e65b43818/ggml/src/ggml-hexagon/ggml-hexagon.cpp),
`ggml_hexagon_supported_mul_mat` accepts packed Q4_0, Q4_1, Q8_0, IQ4_NL,
MXFP4, Q4_K and Q6_K, and floating F16/F32 weights, subject to type, shape,
contiguity, broadcasting, kernel and VTCM checks. It does **not** admit
PQ2_0, PTQ1_0, Q1_0 or Q2_0. The bundled source does dispatch generic
GATED_DELTA_NET and SSM_CONV operations; those names do not establish Swift's
grouped transformed basis or its exact state arithmetic. Thus the scoped
decision is: retain this package for signed **device access discovery**, while
direct same-artifact Swift packed execution through this revision remains a no-go.

GenieX's pinned [license](https://github.com/qualcomm/GenieX/blob/a5c8e7fb4a364ddcccefbf187afd1bac52d88de5/LICENSE)
is BSD-3-Clause and ggml is MIT. Third-party notices, Qualcomm SDK/QAIRT and driver
binary distribution terms remain separate review inputs. Source licensing and
an upstream signature do not complete Runner distribution acceptance. The
[SDK's two plugins](https://github.com/qualcomm/GenieX/blob/a5c8e7fb4a364ddcccefbf187afd1bac52d88de5/sdk/README.md)
consume GGUF (`llama_cpp`) and prepared QAIRT `.bin` (`qairt`) respectively;
changing plugins is not a faithful PQ2 conversion or a native Rust implementation.

## Other exact source leads

Subsequent bounded local verification on **2026-10-30** downloaded and checked
the exact SDK archive above, then inventoried its runtime directory without
loading code. The [retained observation selection](evidence/hexagon-x2-inventory.json)
records X2E94100, native ARM64 and the signed Qualcomm NPU driver
`30.0.220.11010`. Windows reported a valid HTP catalog signature; a separate
read-only `signtool verify /pa /v /c` check verified the bundled **v81** SO's
membership and chain to Microsoft Root Certificate Authority 2010, with exit
code zero and no warnings. The [sanitized verification output](evidence/hexagon-v81-catalog-verification.txt)
and per-file hashes retain that narrow result. It does not verify X1/v73,
installed driver-package trust, mapping, dispatch, DMA64 or VTCM access. Most
host ggml DLLs report `NotSigned`; their release digest identifies their bytes
without establishing inference qualification. No driver, certificate or boot
policy changed and no model or DSP code executed. Durable raw archival and
main-only matching-device CI remain pending.

| Source | What the audit can use | What remains unproved |
| --- | --- | --- |
| [ggml b11913, `7aca0d2ea8d92b56911233b822db01f9fbb1ae50`](https://github.com/ggml-org/llama.cpp/commit/7aca0d2ea8d92b56911233b822db01f9fbb1ae50) | Q1/Q2 Hexagon source, DMA64 and VTCM request/limit review | PQ2 Q8_K arithmetic, Swift transforms and a matching signed Windows package; the GenieX catalog does not cover this revision |
| [Ternary HTP prototype, `5183c091447a0ed31fe41809a54408f5a586e674`](https://github.com/aakar-mutha/llama.cpp/commit/5183c091447a0ed31fe41809a54408f5a586e674) | MIT-licensed PQ2/PTQ1 repacking and HVX source lead | Exact activation/scale order, full model/state, adapters and signed deployment |
| [Native Rust Prism v0.1.9, `7aca2251f09acb345d4afb53d5ce411f81017745`](https://github.com/symbiogenesis/prism-inference/tree/7aca2251f09acb345d4afb53d5ce411f81017745) | Runner-pinned Rust graph, packed CPU arithmetic and complete-state ownership oracle | Native Rust NPU boundary and signed device implementation |

The ternary prototype's operation-support path explicitly rejects
`GGML_HINT_SRC0_IS_HADAMARD` because its Hexagon backend lacks the FWHT. Its
PQ2/PTQ1 tile stores 32 by 32 weights in **320 bytes**, including 32 duplicated
FP16 scales, with a **384-byte aligned tile**. Its symbol repack is therefore
a layout lead, not proof of the CPU PQ2 Q8_K quantizer/reduction contract. Do not
select it merely because its type IDs match 142/143, or reuse the unrelated
GenieX signature for a rebuild. Source arithmetic inspection and independent
operation fixtures must precede any timing.

The intended runtime integration belongs directly in **native Rust Prism**.
Retain its GGUF loader, PQ2/PTQ1 kernels, transform and adapter identities, graph,
sampler and transaction/state ownership. Add a small audited host boundary for
the supported Qualcomm driver/RPC/mapping ABI, with safe Rust ownership around
submissions, cancellation and retained asynchronous buffers. That boundary may
use vendor system bindings; it must not add a C/C++ inference-library dependency.
The DSP kernels remain separately built, versioned and signed deployment
artifacts. Rust host code does not remove that signing requirement or supply
an exact PQ2 device kernel. An existing signed operation ABI may be used only
for the operations it implements; a custom packed kernel needs a supported
signing/distribution route. This architecture also avoids a whole-GGUF Q2 bridge
in the production path. GenieX is a reference for device access and deployment,
not a proposed replacement engine or required dependency.

## Swift placement and traffic ledger

The audit reads the exact pinned Rust source's `src/model/swift.rs`
(`expected_tensors`), `src/graph.rs` (`evaluate_token_into`, `Transforms`,
`Evaluation::project_group_into`, attention and checkpoint paths), and
`src/kernels.rs` (`ActivationContract`, preparation and packed dot paths).
[Swift metadata](../config/swift.metadata.json) identifies 64 layers: 16 full
attention and 48 recurrent, 402 PQ2 tensors, 96 BF16 tensors and 353 F32 tensors.
For a row of width `K`, PQ2 storage is `34 * K / 128` bytes and current-weight
PTQ1 is `28 * K / 128`; these are separate activation contracts. PQ2 uses Q8_K
when `K` is divisible by 256, otherwise Q8_0. Current PTQ1 uses its independently
pinned Q8_0 rounding/accumulation contract. No CPU/NPU bridge may substitute one.

| Region / layout | Placement for the first prototype | State and traffic boundary |
| --- | --- | --- |
| Packed embedding `[5120,248320]`, inverse transform/sign and embedding adapter | CPU | One selected row; retain exact transform direction and adapter order |
| RMS norms and explicit 1024-element normalized Sylvester Hadamard/sign transforms | CPU | F32 input-last-dimension; share preparation only for matching transform, permutation and quantizer |
| Full-attention Q `[5120,12288]`, K/V `[5120,1024]`, output `[6144,5120]` | CPU | Q includes gate; norm/RoPE, FP16 K/V, attention normalization/reduction remain pinned; no Flash substitution |
| Recurrent QKV `[5120,10240]`, gate `[5120,6144]`, alpha/beta BF16 `[5120,48]`, output `[6144,5120]` | CPU | F32 convolution and GDN state; 16 key groups / 48 value heads; preserve grouped V basis and head mapping |
| FFN up/gate `[5120,17408]`, down `[17408,5120]` in all 64 layers | **One up projection is the selected NPU prototype** | CPU transform and exact Q8_K input; NPU output must complete before adapter correction, SwiGLU or residual consumption |
| Output norm and packed logits `[5120,248320]`; sampler/tokenization | CPU | Output is optional during state-only ingestion; retain exact output vocabulary, template and sampler |
| LoRA and rank-one additions | CPU for prototype; separate adapter gate | Preserve original input basis, scale, writer basis and rounding/order; no adapter inference from a no-LoRA pass |
| Checkpoints, KV pages, convolution and recurrent matrices | CPU sole owner | No device state is committed by this stateless projection; full-state and restore gates remain open |

Select `blk.0.ffn_up.weight` with `K=5120`, `M=17408`, decode `N=1` first.
Its original PQ2 payload is **23,674,880 bytes**, its F32 input **20,480 bytes**
and F32 output **69,632 bytes**, excluding alignment, RPC, activation blocks,
mapping, VTCM and allocator overhead. The adjacent gate has identical dimensions
and can later share a validated transform/preparation. A valid candidate must
consume the exact packed scales/symbols and exact Q8_K blocks, tile weights in
bounded VTCM, bound double buffers and DMA descriptors, and preserve scale and
reduction order. This chooses a narrow measurement region without claiming that
many small RPCs will repay their cost.

The graph stages token deltas and reserves appends before committing state.
A cancelled or failed device projection must drain/tear down its in-use buffers,
discard uncommitted outputs and preserve the prior complete CPU state. A later
whole-region migration must include complete attention/recurrent/convolution
state; it cannot resume from a partial device result or replay secretly. Batching
this projection for HMX is a later prefill experiment: repeated sequential
matvecs alone do not establish HMX weight reuse or chunkwise GDN scheduling.

Do not expand all packed weights to F16/F32. The original **7,137,280,000-byte**
PQ2 payload alone gains **419,840,000 bytes** under a 128-to-two-64 Q2 block
bridge (two additional scale bytes per original block, approximately 5.88%).
The weight-layout bridge preserves neither activation quantization nor model
outputs by itself. Charge any such representation, padding, shared mappings,
driver allocations, state and simultaneous residency against the existing
11.5 GiB Swift service allowance and 4 GiB physical reserve, including allocations
outside Job commit. One session is the initial control.

## Dynamic eligibility and remaining execution gates

X1 remains eligible for its own bounded investigation; this audit does not infer
an X1 no-go from processor family or an X2 win from TOPS. Dynamic selection needs
matching device/driver/kernel/artifact/adapter and workload evidence, measured
relative to independently qualified CPU and GPU choices. Prefill and decode may
have different winners. Charge load, repack, mappings, RPC, synchronization,
migration, sustained power and memory, with AC/battery and responsiveness evidence.
Unknown capabilities, changed identities and missing measurements keep that
candidate pending. A selector must preserve the qualified CPU policy while
evidence is missing; hardware presence and theoretical throughput cannot open it.

The concrete next execution gate is a hosted-built supported-type numeric fixture
for the exact signed package, run under one aggregate owner-device allowance of
at most five minutes including setup, mapping, dispatch and cleanup. Custom
packed execution then needs a supported signing route, independent exact
PQ2/PTQ1 operation checks, asynchronous lifetime/failure checks and native Rust
device integration in Prism. Whole-model logits/state, LoRA, broker isolation,
same-artifact repeated device benefit and sustained-use gates remain pending.
No CPU default or broker admission changes follow from this source audit.
