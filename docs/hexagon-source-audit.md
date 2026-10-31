# Hexagon source, placement and evidence

Source reviews: **2026-10-30–31**. [Hexagon contracts](hexagon.md) govern execution;
[PLAN.md](../PLAN.md#hexagon-npu) owns outstanding work. The
[experiment lock](../config/hexagon-experiment.lock.json) and separate
[Q2 candidate lock](../config/hexagon-signed-q2-candidate.lock.json) retain exact
source/package identities and keep admission disabled. Production CPU policy,
runtime pins and backend availability are unchanged; automatic selection is absent.

X2 has generic signed-package numeric access and a native terminal FD cycle.
The original Swift projection/signs are bound as data. Native Rust DSP computation,
selected DSP-byte identity, Swift numerics/state, resources and performance remain
unqualified; X1 needs independent evidence. The sections below distinguish source
eligibility, data verification, native access and hosted wire controls.

## Pinned packages and source decisions

| Input | Exact identity and scoped decision |
| --- | --- |
| [GenieX v0.8.0](https://github.com/qualcomm/GenieX/releases/tag/v0.8.0), selected access reference | GenieX `a5c8e7fb4a364ddcccefbf187afd1bac52d88de5`; bundled llama.cpp `94256114c229674ef96e76eb2dea596e65b43818`. ARM64 ZIP: 84,788,957 bytes, SHA-256 `5676e27cc5639717890ce92f3beec937a6aefc645c9cc57bcb537f972b4fecc7`. Generic access only; no PQ2/PTQ1/Q1/Q2 matmul. |
| [GenieX v0.10.0](https://github.com/qualcomm/GenieX/releases/tag/v0.10.0), separate Q2 candidate | Published 2026-10-28; GenieX `071f609995470bdb0e0769ab964754adfaab7e47`; llama.cpp `19cce6919992fdffd92e0ac152caaf1e2b8f254e`. ARM64 ZIP: 92,386,770 bytes, SHA-256 `410dc39e79a6d0b7e8991bd4267285f7c73de7c46511a1fe0549b83dc4a26e4e`. Archive, 17 runtime files and five catalog members verified as data; no package code loaded, executed or installed. |
| [ggml b11913](https://github.com/ggml-org/llama.cpp/commit/7aca0d2ea8d92b56911233b822db01f9fbb1ae50) | Q1/Q2, DMA64 and VTCM source lead; the v0.8.0 catalog does not cover this revision. |
| [Ternary HTP prototype](https://github.com/aakar-mutha/llama.cpp/commit/5183c091447a0ed31fe41809a54408f5a586e674) | MIT PQ2/PTQ1 repack/HVX lead. Rejects `GGML_HINT_SRC0_IS_HADAMARD` because FWHT is absent; matching type IDs 142/143 prove no Swift arithmetic or deployment. Its 32×32 tile is 320 bytes, including 32 duplicated FP16 scales, aligned to 384. |
| [Prism v0.1.9](https://github.com/symbiogenesis/prism-inference/tree/7aca2251f09acb345d4afb53d5ce411f81017745) | `7aca2251f09acb345d4afb53d5ce411f81017745` binds the audited Rust graph, packed CPU oracle and extractor. This historical control identity is distinct from the current production runtime lock. |

The v0.8.0 [support path](https://github.com/ggml-org/llama.cpp/blob/94256114c229674ef96e76eb2dea596e65b43818/ggml/src/ggml-hexagon/ggml-hexagon.cpp)
admits Q4_0/Q4_1/Q8_0/IQ4_NL/MXFP4/Q4_K/Q6_K and F16/F32 subject to shape,
contiguity, broadcasting, kernel and VTCM checks. Generic GATED_DELTA_NET/SSM_CONV
dispatch does not establish Swift's grouped transformed basis or state arithmetic.
The [plugin build](https://github.com/qualcomm/GenieX/blob/a5c8e7fb4a364ddcccefbf187afd1bac52d88de5/sdk/plugins/llama_cpp/CMakeLists.txt)
sets `LLAMA_BUILD_TESTS OFF`, installs v73–v81 SOs/catalog under `lib/llama_cpp`
and uses `-ffp-model=fast` in the reviewed Windows Clang configuration. It supplies
neither a promised `test-backend-ops` fixture nor a strict Swift float-order oracle.

The [Q2 lock](../config/hexagon-signed-q2-candidate.lock.json) pins all 29 reviewed
public files with repository/revision/path/extent/hash/URL. The 2026-10-30 inventory
also found v0.10.1-rc.1 (2026-10-29) using the same HTP source and v0.9.0/0.9.1/0.9.3
using `e467e5b5e5ba43071d6151b3d881deb9300d000e`. This is a dated inventory,
not automatic latest-tag selection. GenieX is BSD-3-Clause, llama.cpp/ggml MIT;
FastRPC and QAIC have separately pinned BSD terms. SDK/QAIRT, driver binaries,
third-party notices and Runner distribution acceptance remain separate.
The GGUF `llama_cpp` and prepared-QAIRT `.bin` plugins are distinct model routes;
switching plugins supplies neither a faithful PQ2 conversion nor native Rust inference.

### Custom-kernel signing boundary

The pinned [release pipeline](https://github.com/qualcomm/GenieX/blob/a5c8e7fb4a364ddcccefbf187afd1bac52d88de5/.github/workflows/release.yml)
overlays a signed bundle selected by bundled llama.cpp revision. Its catalog binds
only those DSP bytes/ABI. ELF Authenticode status alone cannot verify catalog
membership; verify archive, actual members, chain, exact SO/host DLL and driver
before loading. Changed kernels or ABI require a new binding.
The [operator instructions](https://github.com/qualcomm/GenieX/blob/a5c8e7fb4a364ddcccefbf187afd1bac52d88de5/notes/release.md#hexagon-htp-signing)
use Qualcomm Samba/Jenkins and separate signing storage; no public submission
entitlement or Runner access is established. A custom release needs an accepted
operator, built v73/v81 ELF digests, source/compiler/ABI manifest, distribution terms
and independent returned-catalog checks. The self-signed alternative changes
certificate/boot trust and requires a separate owner decision.

## Native Rust integration and placement

Integration belongs directly in **native Rust Prism**, retaining its loader,
packed kernels, transforms/adapters, graph, sampler and transaction/state ownership.
A small audited vendor driver/RPC/mapping boundary may use system bindings with
safe ownership; it must add no C/C++ inference-library dependency. DSP kernels
remain separately versioned/signed. GenieX is an access/deployment reference,
not the replacement engine or a required production dependency.

The [Prism source candidate, draft PR 6](https://github.com/symbiogenesis/prism-inference/pull/6)
on `codex/hexagon-session-native-next-1030` extends `17ca7b3` from the audited Prism
pin. `bonsai-hexagon-native` is isolated and `publish = false`; the engine neither
consumes nor re-exports it. Runner and Prism retain the same seven native sources.
The [boundary guide](../tools/hexagon/native_rpc/README.md) owns CLI/ABI details.
One nonblocking process lease, fixed capability/URI/family/layout controls, guarded
files and consuming terminal probes retain uncertain buffers/modules/driver owners.
Map/unmap failure skips free and close; explicit handle close or an empty Job does
not establish complete vendor session teardown.

The separately selected reference initializer requests **unsigned DSP process class**
with request2/enable1 before URI/open, while still requiring Windows signed-package/
catalog checks. It is not signed-PD-only access or authority for arbitrary modules
or OS trust changes. Public Linux behavior is process-local pre-open selection;
closed Windows behavior and effective process class remain unproved. Returned
mutable fields are recorded as raw data; only the original validated domain controls
later calls. There is no automatic retry from the default to reference mode.

### Swift placement and traffic ledger

The audited `src/model/swift.rs`, `src/graph.rs` and `src/kernels.rs` define tensor,
transform, projection, attention/checkpoint and activation contracts.
[Metadata](../config/swift.metadata.json): 64 layers (16 full-attention/48 recurrent),
402 PQ2, 96 BF16 and 353 F32 tensors. Row storage is `34*K/128` bytes for PQ2,
`28*K/128` for current-weight PTQ1. PQ2 uses Q8_K for K divisible by 256, otherwise
Q8_0; PTQ1's independently pinned Q8_0 rounding/accumulation cannot substitute.

| Region / layout | First-prototype placement and boundary |
| --- | --- |
| Embedding `[5120,248320]`, inverse transform/sign and embedding adapter | CPU selected row; preserve transform direction and adapter order. |
| RMS norms; explicit normalized 1024-element Sylvester Hadamard/sign transforms | CPU F32, input-last-dimension; share preparation only for identical transform/permutation/quantizer. |
| Attention Q `[5120,12288]`, K/V `[5120,1024]`, output `[6144,5120]` | CPU; Q includes gate. Preserve norm/RoPE, FP16 K/V and attention normalization/reduction; no Flash substitution. |
| Recurrent QKV `[5120,10240]`, gate `[5120,6144]`, alpha/beta BF16 `[5120,48]`, output `[6144,5120]` | CPU F32 convolution/GDN state; retain 16 key groups, 48 value heads, grouped V basis and head mapping. |
| FFN up/gate `[5120,17408]`, down `[17408,5120]`, all 64 layers | **One up projection is the NPU prototype**; CPU transform/exact Q8_K, completed result before adapter/SwiGLU/residual. |
| Output norm/logits `[5120,248320]`, sampler/tokenization | CPU; output optional during state-only ingestion. Preserve vocabulary/template/sampler. |
| LoRA and rank-one additions | CPU; separate adapter gate for original input/writer basis, scale and rounding/order. |
| Checkpoints, KV pages, convolution/recurrent matrices | CPU sole owner; no stateless device projection commits state. |

First tensor: `blk.0.ffn_up.weight`, K=5120/M=17408/N=1; original PQ2 payload
23,674,880 bytes, F32 input 20,480 and output 69,632, before alignment/RPC/
activation/mapping/VTCM/allocator charges. Its adjacent gate may later share verified
preparation. Preserve original scales/symbols, Q8_K blocks and ordered per-128 FMA
with bounded tiles/double buffers/DMA descriptors. Many small RPCs may cost more
than they save. HMX prefill needs batched weight reuse and independent scheduling;
sequential matvecs do not provide it.

The graph stages token deltas and reserves appends before commit. Failure/cancellation
must drain or tear down in-use buffers, discard provisional output and preserve
complete prior CPU state. Region migration includes all attention/recurrent/
convolution state; partial device continuation or secret replay is forbidden.
No whole-model F16/F32 expansion or production whole-GGUF Q2 bridge is proposed:
splitting original 128-weight PQ2 blocks into two 64-weight Q2 blocks adds
419,840,000 bytes (5.88235%) to the 7,137,280,000-byte payload without proving
activation/output parity. One session, the 11.5 GiB Swift service allowance and
4 GiB physical reserve include copies, mappings, driver/state allocations and
resources outside Job commit.

## Observed X2 access

All results below are scoped controls, not Swift inference or performance evidence.
The installed user driver is Microsoft-signed Qualcomm `libcdsprpc.dll`, SHA-256
`5cf6aa86670cceb84461050ad00b012662e4859d15be2a1cf147d98d5002f37d`.
Native probes use independently checked service-derived paths/digests with retained
deny-mutation guards and cached Authenticode validation before loading.

| Retained observation | Result and limit |
| --- | --- |
| [Inventory](evidence/hexagon-x2-inventory.json), [v81 catalog check](evidence/hexagon-v81-catalog-verification.txt) | X2E94100, native ARM64, driver `30.0.220.11010`; archive checked without loading code. `signtool verify /pa /v /c` verifies v81 membership/chain to Microsoft Root Certificate Authority 2010, exit 0/no warnings. Host ggml DLLs mostly `NotSigned`; release hashes identify their bytes. No X1/installed-driver-package trust qualification follows. |
| [Native capability/STATIC](evidence/hexagon-native-rust-access.json) | Eight queries passed; raw architecture 32897 decodes v81, eight 128-byte HVX contexts and one 8,388,608-byte VTCM page reported. HMX zeros prove no absence; page size proves no available allocation/throughput. 4096-byte allocation/FD/CPU-byte check passed; domain 3 STATIC=0 mapping returned 512 (`AEE_ERPC`, generic cause unknown). No unmap/free followed; owners pinned through supervised exit/empty Job. |
| [Session metadata](evidence/hexagon-x2-session-metadata.json) | Requests 14/15 returned 0, effective domain 3/v81/fixed URI matched; no remote open, map, process-class request or compute. |
| [Default session](evidence/hexagon-x2-native-session.json) | Source/package/catalog/driver bound; `remote_handle64_open` returned 14/no handle. Allocation/FD map not attempted. Cause unknown; this does not rule out FD mapping. |
| [Old reference initializer guard](evidence/hexagon-x2-reference-initialization.json) | Request 2/enable 1 returned 0 but changed writable input; old equality guard stopped before URI/open. Candidate validation refusal, not vendor initialization failure. |
| [Corrected reference FD cycle](evidence/hexagon-x2-native-fd-cycle.json) | Module open, 4096-byte allocation/FD/CPU-byte check, FD=2 map/unmap, free and explicit close all returned 0. Returned domain 0/enable 1 retained as data; later calls used original domain 3. Package/catalog/driver checks passed; Job empty. No DSP visibility, DMA, compute, effective process class or vendor teardown proof. |
| [Generic numeric dispatch](evidence/hexagon-x2-dispatch.json) | Explicit HTP0, no CPU scheduler/fallback; K=128/M=32 with N=1/32 matched all 32/1024 poisoned outputs against independent closed forms. All 16 package files/four catalog members checked; Job empty. Initial receipt did not independently bind loaded driver or selected DSP. |
| [Driver-bound dispatch](evidence/hexagon-x2-driver-bound-dispatch.json) | Both graphs/all 1056 comparisons passed again. Independent supervisor captured live user-driver module path, hash/Microsoft publisher and held its file through exit. Package/member checks and empty Job passed. Selected DSP bytes/architecture, mapping modes, effective VTCM/DMA64 and vendor teardown remain unqualified. |

STATIC, default-open and old mutable-input refusals remain independent observations;
the later FD success assigns no cause to them. No model, custom kernel, installation,
certificate or boot-policy change occurred. X1/v73 qualification is separate.

### Supervision, hosted controls and retention

[Build-HexagonDispatch](../scripts/Build-HexagonDispatch.ps1) prepares the Rust
fixture/query and model-free tests, retaining a two-file access bundle separately
from compiler evidence. Its `asInvoker` [manifest](../tools/hexagon/diagnostic.manifest)
is source/build-bound. [Dispatch](../scripts/Invoke-HexagonDispatch.ps1) and
[session](../scripts/Invoke-HexagonSessionProbe.ps1) supervisors verify source/build,
package/member and independent driver identities. Child-only environment cleanup
removes inherited accelerator/FastRPC overrides and Actions credentials; bounded
streams, checked-file lifetime and a kill-on-close Job cover the process tree.
Strict complete/partial JSON rejects duplicate names, coerced booleans and contradictory
results. Flushed fixed stage markers prove entry only, including before native hangs.

The [workflow](../.github/workflows/hexagon-qualification.yml) separates hosted
native controls from artifact-binder preparation; an unrelated binder failure
neither qualifies that tool nor erases a successful native job. Main-only manual
owner execution selects one inventory, dispatch or explicitly selected native
session mode. The clock starts before checkout/intake; total job limit is five
minutes, dispatch operation at most 120 seconds, internal cleanup reserve 75 seconds.
Missing time/resources leaves the gate pending without a weaker retry. Isolated
`codex/hexagon-*` pushes run hosted controls only; full suites stay hosted-only.

| Hosted evidence | Scope |
| --- | --- |
| [Runner 41162447956](https://github.com/symbiogenesis/SwiftBonsai2Runner/actions/runs/41162447956), [source/build selection](evidence/hexagon-advance-controls.json) | `d7eee0ab8c0dcf3fa4e6e0ab49c94f07f679a55f`: eight native/seven packed tests, immutable captures, layout, preflight/dispatch policy, ranking and policy Clippy. Earlier access accounting 111,967 ms includes a conservative 30-second focused-check allowance. |
| [Prism 41165607707](https://github.com/symbiogenesis/prism-inference/actions/runs/41165607707) | Mutable-payload native binding: formatting, contract tests and strict Release Clippy. Runner additionally checks full/partial JSON, explicit modes, ABI/requested/returned values, uncertain lifetimes, refusal/cancellation and signature/source provenance. |
| Settled Runner `53f717e887e80fb1c4042db4296c46b67af8097f` | [Native/packed and binder](https://github.com/symbiogenesis/SwiftBonsai2Runner/actions/runs/41172604323), [public wire](https://github.com/symbiogenesis/SwiftBonsai2Runner/actions/runs/41172604298), [SDK review](https://github.com/symbiogenesis/SwiftBonsai2Runner/actions/runs/41172604331), complete selected [Foundation groups](https://github.com/symbiogenesis/SwiftBonsai2Runner/actions/runs/41172604342) passed. |
| Prism `527d1432170ee78b258d489805b11f798121267f` | [Foundation](https://github.com/symbiogenesis/prism-inference/actions/runs/41169223029) and [runtime conformance](https://github.com/symbiogenesis/prism-inference/actions/runs/41169223028) passed. |

The [single owner investigation budget](evidence/hexagon-session-next-budget.json)
includes focused compile/result checks, hosted intake, three terminal variants,
driver-bound dispatch, full-file extraction, Q2 data verification and conservative
setup/administration. The [credential-free archive](evidence/hexagon-session-next-archive.json)
retains 45 exact reports, owned Rust bundles, binding metadata, callers and losing
results in owner-managed storage; copied digests were independently checked.
Weights, adapters and vendor binaries are excluded. Final charged total including
archive verification: **140,537 ms of 300,000 ms**; subsequent SDK/wire work was hosted.
This is accounting, not a benchmark. Off-device backup, retrieval without the
original runner and independent qualification review remain pending.

## Bound Swift inputs and CPU controls

[Actual extraction](evidence/hexagon-swift-projection-binding.json) verified all
7,206,168,928 Swift bytes on a retained handle and exported the original first
projection (23,674,880 bytes, SHA-256
`a0f21e1d4492de2496309b6383b958cba53c54f05c9de710348facdd31706354`)
and exact 5120 sign bytes (`5ab93190ef5cd4f8322d52c1006783b25a3e118453bda0d0f90b1bf301a67197`).
Both output digests were independently checked; retained header charge was
19,248,870 bytes against 128 MiB. The [binder build](../scripts/Build-HexagonPackedArtifact.ps1)
uses a temporary Cargo example in the clean exact GGUF reader, without changing
tracked dependency source. Cargo owns metadata/linking; the owned LLD tree must
empty. Its receipt binds source/example, compiler/linker, dependencies, lock,
manifest and executable. Extraction establishes weights/signs, not transform,
activation, logits/state, adapters or device computation.

The [packed control guide](../tools/hexagon/packed-projection.md) owns arithmetic,
commands and ledgers. Its CPU tile loop retains original PQ2, exact Q8_K and
ordered per-block FMA, independent native rounding captures and closed-form Walsh
lane ordering; partial tiles, cancellation and late invalid scales publish no
partial output. Proposed VTCM/traffic is derived, excluding actual DMA/session
costs. The [version 1 tile protocol](../tools/hexagon/packed-projection.md#proposed-signed-kernel-tile-protocol)
is an application-owned CPU receiver contract, not a Qualcomm RPC ABI or existing
signed operation. Consumption of the bound weights/signs, actual activations,
independent transform/logit/full-state parity and asynchronous lifetime remain
pending. A future custom kernel needs its own implementation and signing route.

## Newer signed Q2 candidate

The [data-only package audit](evidence/hexagon-signed-q2-package.json) verifies
v0.10.0 bytes/catalog/chain, including v81; it supplies no loaded-DSP identity,
operation-entry or numerical result. Source targets v73/v75/v79/v81/v85; generic
Q1_0/Q2_0 IDs are 41/42, with matmul kernels but no native PQ2/PTQ1 operation.

Ordinary [matmul eligibility](https://github.com/ggml-org/llama.cpp/blob/19cce6919992fdffd92e0ac152caaf1e2b8f254e/ggml/src/ggml-hexagon/htp-kparams.cpp#L971-L995)
requires contiguous nonview/nonpermuted Q2 weights, K divisible by 64, F32/F16
activations and F32 output, with solver/resource checks still required. N=2 fails
the HMX minimum-row predicate; effective HVX placement remains unobserved.
[Repacking](https://github.com/ggml-org/llama.cpp/blob/19cce6919992fdffd92e0ac152caaf1e2b8f254e/ggml/src/ggml-hexagon/htp-repack.cpp#L666-L707)
uses 32-row/64-weight tiles: 512 symbol + 64 FP16-scale bytes, 576 total, device-aligned
to 640. [Q8_1 activation tiles](https://github.com/ggml-org/llama.cpp/blob/19cce6919992fdffd92e0ac152caaf1e2b8f254e/ggml/src/ggml-hexagon/htp/matmul-ops.h#L184-L214)
consume 1280 bytes per 32 columns. Internal F32 quantization uses 32-element
normalization, FP16 scale/scaled sums, integer dot/correction and per-64-weight
scaling/reduction; half reciprocal/qf32 conversions need independent checks.
Ordinary full-K Q2/Q8_1 cannot replace Swift's Q8_K/per-128 FMA; PTQ1/Q8_0 is separate.
Prepared Q8_1 upload is not an established public input.

The [guarded integer primitive](../tools/hexagon/packed-projection.md#conditional-signed-q2-integer-primitive)
keeps CPU Q8_K and ordered CPU FMA while requesting bounded scale-one integer dots
from supported F32 inputs. Limbs/guards/padding must prove exact quantization,
FP16 scale/sum representation, correction, saturation and qf32 reduction against
the signed binary; CPU construction alone does not qualify vendor intrinsics.
Charge temporary packing, internal quantization, extra activations/results, queues,
transfers and CPU combination. Compare with a directly fused packed kernel: this
signed route may avoid custom signing for a narrow primitive yet lose on total cost.

## Native Rust computation requires another reviewed boundary

Both package generations process queues; module open/FD mapping submits no DSP
operation. A compute control must bind generated-IDL marshalling, create/export
one bounded queue, start, register checked buffers/descriptors, submit, validate
response extent/buffer count/sequence and poisoned numeric output, then stop/drain/
close or retain every uncertain owner through verified teardown. Synchronous host
waiting does not remove asynchronous device ownership or permit post-submit free.
The [new IDL](https://github.com/ggml-org/llama.cpp/blob/19cce6919992fdffd92e0ac152caaf1e2b8f254e/ggml/src/ggml-hexagon/htp/htp_iface.idl)
changes five-argument `start` to seven, adding VTCM-size and power-mode requests to
session/queue/HVX/HMX/mapping inputs. Neither the old wire layout nor catalog applies.

The pinned [public FastRPC queue header](https://github.com/qualcomm/fastrpc/blob/a56e9d4de3614a8d8f4b0e15ff561350c4b686e3/inc/dspqueue.h)
is 24,632 bytes (full hash in Q2 lock). Its four-u32/pointer-or-u64 buffer record
derives offsets 0/4/8/12/16, size 24/alignment 8 under ordinary 64-bit C, not an executed
Windows ARM64 ABI check. Queue export requires later DSP import; close can fail
while imported. Buffer references require prior same-FD mapping and an in-range
subsection, but reference accounting covers the entire mapping. Reference/dereference
and sender/recipient cache flags differ; blocking read requires no packet callback.
These declarations qualify neither installed layouts nor callback/cache/teardown safety.

The pinned [host wrapper](https://github.com/ggml-org/llama.cpp/blob/19cce6919992fdffd92e0ac152caaf1e2b8f254e/ggml/src/ggml-hexagon/htp-drv.cpp)
and header (18,461/3116 bytes, lock digests) resolve create/close/export/write/read/
read-noblock from service-derived `libcdsprpc.dll`. Windows explicit timeout 0 routes
to `dspqueue_read_noblock`; the symbolic header default `DSPQUEUE_TIMEOUT_NONE`
is public `0xffffffff` (infinite). Neither provides an aggregate deadline or observed
Windows timeout behavior. Its close-pointer typedef incorrectly takes 32-bit
`remote_handle` despite a 64-bit wrapper; authoritative public `remote_handle64_close`
takes u64, as does the native Rust terminal control. Do not copy that narrowing ABI
or use this mismatch to explain device errors.

The exact [build](https://github.com/ggml-org/llama.cpp/blob/19cce6919992fdffd92e0ac152caaf1e2b8f254e/ggml/src/ggml-hexagon/htp/CMakeLists.txt#L17-L56)
generates `htp_iface_stub.c`/skeleton through SDK `build_idl`; reviewed official
repositories did not expose its generated stub. Invocation needs exact generator,
flags/import closure, generated header/stub hashes and an independent wire capture
covering inherited method indices, scalar tags, extents and packing. Keep native
reference controls outside production Rust inference. A declaration/signature
does not establish a callable queue binding or qualify an unchecked terminal-probe invoke.

### SDK inputs, recipe and terms

The [pinned setup action](https://github.com/qualcomm/GenieX/blob/071f609995470bdb0e0769ab964754adfaab7e47/.github/actions/setup-snapdragon-sdks/action.yml#L9-L37)
selects SDK 6.7.0.0/tools 21.0.04. Its ARM64-WoS ZIP is 900,461,020 bytes, SHA-256
`9eb256cc7b102f721365151a470a33980069a18b9209e1d56a6d2fa430369759`.
[Hosted inventory 41168523349](evidence/hexagon-sdk-inventory-41168523349/provenance.json)
at `9f87dde4c198fff404c024bb737c0861cb3002b2` read eight selected archive entries
without extracting/executing SDK code. Only bounded JSON reached the laptop.
Receipt/input hashes match, including three source pins after Windows LF→CRLF;
reported GitHub artifact digest is not an independently verified ZIP digest.

| Input | SDK bytes | Pinned public control bytes |
| --- | ---: | ---: |
| `qaic.exe` | 35,301,888 (PE8664/x64; ARM64 execution would emulate) | 12,077,520 Linux source build; no equivalence |
| `dspqueue.h` / `remote.h` | 37,079 / 67,004 | 24,632 / 65,625 |
| `remote.idl` / `AEEStdDef.idl` | 1387 / 3448 | 1498 / 1793 |
| `AEEStdErr.idl` | 7253 | No candidate import pin |

[Recipe review 41168872514](evidence/hexagon-sdk-inventory-41168872514/provenance.json)
retains exact tokens/template, not executed CMake expansion. `hexagon_fun.cmake`
is 36,308 bytes (lock hash); `build_idl` lines 269–323 selects `-mdll -o BINARY_DIR`,
default SDK `incs/`, `incs/stddef/`, then target includes in order. The host adds
SDK `incs`, `incs/stddef`, `utils/examples`, source `htp` and output directories;
working directory is `ggml/src/ggml-hexagon`, input `htp/htp_iface.idl`. Retain duplicate
includes. No `-rf`/external preprocessor is added; predicted output names do not
prove actual import resolution, generated bytes or wire equivalence.

[Functional review 41171339745](evidence/hexagon-sdk-functional-41171339745/provenance.json)
at `5feae0d6ac2152692001f8ffcc8b3f13d2cdcf08` checked four SDK/public pairs,
receipt/four source/seven input pins and 235 declaration-token hashes. Comment-aware
normalization matches `remote.idl`'s 25 tokens and `AEEStdDef.idl`'s 42 despite raw
differences; it preserves preprocessor boundaries but evaluates no conditionals,
imports or types. Whole queue/remote streams differ. Queue handle/buffer/flags,
existing callback and create/close/read/read-noblock prototypes match selected tokens;
SDK adds `dspqueue_mdq_callback_t` and a framework packet flag/reserved-mask expression.
Identifier-associated hash differences alone do not prove an existing prototype changed.
SDK `remote_work.h` is unreviewed; public Windows `remote_ext.h` remains unpinned.
Effective Windows import closure/layout/calling convention/driver binding and
callback/FD/cache/import-close lifetimes must precede a callable Rust queue API.

The legal-filename scan found one 118,826-byte Hexagon-tools `NOTICE.txt` (lock hash)
and five Python-package licenses. UTF-8 failed; uncertain no-BOM Windows-1252 fallback
had no nontext controls, no bounded QAIC/IDL-compiler/SDK-phrase matches and some
license/redistribution/patent words. These observations, header proprietary/import
markers and binary BSD strings establish neither encoding/applicability nor a grant.
The [product page](https://www.qualcomm.com/developer/software/hexagon-npu-sdk) names
an agreement, but reviewed public/catalog responses did not expose exact SDK 6.7 terms.
SDK generator execution/output redistribution remain unqualified, not proven unlicensed.
Further SDK preparation stays hosted, outside the owner trial allowance.

### Public QAIC generation and wire control

Public [QAIC `a66a0019fb488abae955bbc9b3a9de3dc15962d0`](https://github.com/qualcomm/QAIC/tree/a66a0019fb488abae955bbc9b3a9de3dc15962d0)
is a Haskell generator with C templates. Its `make -C src TARGET_ARCH=x86_64-linux`
build has unbounded Cabal dependencies, so source pin alone is insufficient.
Controls use GHC 9.10.3/Cabal 3.12.1.0, pinned setup action/freezes/plan, version 01.00.51,
only added package `syb 0.7.4` (tar SHA-256
`ddcaca983f67a170c0cfdbd037a268779880e2f9b57f93135eaccb6ae4c5da3f`), Cabal-metadata
and GHC package identities. Requested Hackage index 2026-10-29T00:00:00Z resolves
to frozen 2026-10-28T23:27:22Z. Freeze equality is retained and replayed.

Generate only checked `htp_iface.idl`, with pinned public FastRPC `remote.idl`/
`AEEStdDef.idl` imports, documented `--map-dll --include-path --output-path` and
default internal preprocessor; `AEEStdDef.idl` warns against standalone compilation.
Retain import/command/output hashes. No SDK executable/import or device code runs.

| Retained hosted control | Result |
| --- | --- |
| [Generation 41169481882](evidence/hexagon-public-qaic-41169481882/provenance.json), `c8b0ad19a258c025c00168298af54f78a33dd8f1` | Fourteen successful logs, eight project/freeze/plan files and 163 tracked sources (953,938 bytes) checked; 38 artifact files retained. Header/stub/skeleton 7889/16,931/20,188 bytes. Generation only. |
| [Compile 41170405233](evidence/hexagon-public-qaic-41170405233/provenance.json) | Legacy `uint32`/`uint64` missing in modern public headers; stopped before C mock/Rust. No device diagnosis. |
| [Standard types 41171073546](evidence/hexagon-public-qaic-41171073546/provenance.json) | Documented `--standard-types`; outputs 7920/16,963/20,248 bytes. Whole-file comparison finds only `uint32_t`/`uint64_t` and `AEEStdDef.h`→`stdint.h` substitution; 48-byte golden frame unchanged. No mock selected. |
| [Compile 41171448060](evidence/hexagon-public-qaic-41171448060/provenance.json) | Missing `AEEResult`; stopped before C mock/Rust. Final control forces exact public `AEEStdDef.h` include, without custom typedef/generated edit/SDK input. |
| [Passing wire 41171859264](evidence/hexagon-public-qaic-41171859264/provenance.json), `8c69947b7b98a8d406d15c639d83f926a3b643cc` | Seven source/pin identities, successful logs, headers/inputs, unchanged generated files/freezes and independent vector verified; 66 files/545,496 bytes retained. GCC 13.3.0 C mock and standalone std-only [Rust encoder](../tools/hexagon/public_idl_control.rs) agree on two calls/zero returns/no extra calls. |

The fixed positive vector uses a synthetic 64-bit handle, start scalar `0x02010000`,
one 48-byte input with seven fields at 0/8/16/20/24/32/40 and zero padding 4–7/44–47;
stop scalar `0x03000000` has null arguments. This qualifies only that little-endian
Linux LP64 start/stop vector. Link map: system CRT/libgcc/libc/local objects, no SDK/
FastRPC library. SDK equivalence, Windows ARM64 ABI, other methods/vectors, queue/
cache/callback ownership, signed device computation and Swift/profile gates remain open.

Captured C/header/Rust files are immutable `.txt` data with raw hashes/notices and
no production build consumer. Artifact ZIP digests are GitHub-reported, distinct
from locally checked retained-file hashes. Initial GHC identity was a 424-byte
launcher; later receipts add the 2,123,392-byte ELF compiler/runtime candidates,
not complete compiler/OS closure. Generator/compiler/executable hashes, dated Rust
identities and flags are recorded, but binaries were not uploaded.
QAIC's [BSD-3-Clause-Clear](https://github.com/qualcomm/QAIC/blob/a66a0019fb488abae955bbc9b3a9de3dc15962d0/LICENSE.txt)
requires notices/conditions and excludes a patent grant. Generated files lack
copyright/SPDX markers; retain complete QAIC/FastRPC/llama licenses, IDL/version
notices and template provenance. Independent embedded Allocator/Env/Slim/Skel/Wstring
hash checks supplement CStub/Stub notices; that pair alone is incomplete. Public
terms do not establish SDK terms. Both generation controls stay outside production
Rust inference dependencies.

A native old-package F32 fixture can follow reviewed queue/lifetime implementation;
generic C++ access evidence is separate. New Q2 additionally needs source-bound
native intake/new ABI and signed quantizer/integer parity before consuming the bound
projection. Neither is implemented by an unchecked invoke. Qualification and any
future CPU/GPU/NPU choice remain subject to [PLAN](../PLAN.md#hexagon-npu) and the
[execution/resource/failure contracts](hexagon.md).
