# Native Rust Hexagon access candidate

The [direct driver boundary](../tools/hexagon/native_rpc/README.md) now has local
X2 evidence and a source candidate in Prism. It is not an inference backend.
The production runtime lock, CPU policy and backend availability remain unchanged.
[PLAN.md](../PLAN.md#hexagon-npu) owns the remaining work.

## Observed access and current blocker

The [credential-free observation selection](evidence/hexagon-native-rust-access.json)
records the 2026-10-30 owner-context checks against the installed Microsoft-signed
Qualcomm `libcdsprpc.dll`, SHA-256
`5cf6aa86670cceb84461050ad00b012662e4859d15be2a1cf147d98d5002f37d`.
Its service-derived path and file digest were checked and held against mutation;
the Rust boundary also verified cached Authenticode trust before loading.

All eight fixed capability requests returned success. The raw architecture value
32897 decodes to v81. The driver reported eight 128-byte HVX contexts and one
8,388,608-byte VTCM page. These observations establish neither available VTCM nor
throughput. The HMX fields returned zero; this is not evidence that HMX is absent.

One terminal 4096-byte probe allocated RPC memory, obtained its descriptor and
verified CPU writes/reads. `fastrpc_mmap` with domain 3 and
`FASTRPC_MAP_STATIC=0` returned **512**. The pinned Qualcomm host header names
this generic return `AEE_ERPC`; its cause remains unknown. No unmap or normal
free followed that uncertain result. The allocation, DLL, file guard and exclusive
process lease stayed pinned until the supervised process exited; its Job became
empty. Vendor session/device teardown has not been independently qualified.

This is a scoped STATIC-mode failure. The signed reference source uses FD,
delayed-FD or extended mapping within an established session; those paths were
not tried. Neither this result nor successful capability queries choose an NPU
profile or establish DMA64, DSP compute, Swift packed arithmetic, worker isolation,
state parity or performance. No model, custom DSP module, driver installation,
certificate or boot-policy change was involved.

The later [native session-metadata observation](evidence/hexagon-x2-session-metadata.json)
used the exact successful hosted executable and the same independently guarded
driver identity. Requests 14 and 15 both returned zero, effective domain was 3,
architecture was v81 and the fixed default-session URI matched exactly. Its Job
was confirmed empty. No remote module, mapping, unsigned-process request or
compute was performed by this native query; established-session FD mapping
remains pending.

## Prism source candidate

The isolated Prism branch `codex/hexagon-advance-native-1030` at `17ca7b3`, based on
Runner's pinned `7aca2251f09acb345d4afb53d5ce411f81017745`, contains the
`bonsai-hexagon-native` workspace crate. It separates unsafe Windows/vendor calls
from the safe root engine, has no inference-library dependency and remains
`publish = false`. A root optional dependency was omitted because an unpublished
optional crate would add a registry prerequisite to existing runtime publication.
Source review, registry/release policy and runtime integration remain pending;
Runner's release pin does not consume this branch.

Strict offline/locked release Clippy with all targets passed for the native-only
candidate. Runner and Prism's five native library source files are byte-identical.
The library adds fixed default-session effective-domain/URI preparation and
validates architecture, pointers, input bytes, lengths and the exact terminated
module URI. It opens no remote module and issues no unsigned-process control;
successful metadata does not grant mapping or compute authority. The signed
installed-module/search route must be bound before adding session-correct FD
mapping. The extended mapping flag is absent from the pinned public header, so
its SDK ABI remains pending rather than guessed.

Focused ABI and partial mapping receipt regressions passed. The library uses one nonblocking process-wide
lease, fixed requests, a consuming one-shot mapping probe and explicit uncertain
resource retention. Full regressions remain hosted-CI only.

## Packed operation control

The [safe Rust tiled projection](../tools/hexagon/packed-projection.md) is a
CPU-only operation control for the first K=5120/M=17408/N=1 projection. It keeps
original PQ2 bytes, Q8_K preparation and ordered per-block FMA, with independent
retained native rounding observations and a closed-form Walsh lane-order check.
It carries reductions across partial tiles and publishes no partial result after
cancellation or a late malformed scale. Its proposed VTCM/traffic ledger charges
bounded tiles rather than a whole Q2 model copy; actual DMA/session costs,
artifact/sign-array binding, adapters, logits and complete state remain pending.

## Signed-package numeric fixture

The separate [Rust access fixture](../tools/hexagon/dispatch_fixture.rs) calls only
the explicitly selected Hexagon backend from the pinned signed reference package.
Two fixed F32 graphs use K=128, M=32 and N=1/32, poison all outputs and compare
all 32/1024 results against an independent closed form. There is no CPU scheduler
or CPU fallback. The host self-check and both actual X2 device graphs passed in
the [retained dispatch receipt](evidence/hexagon-x2-dispatch.json): 32 and 1024
output values matched, the explicit backend was HTP0, CPU fallback was unavailable
and the supervising Job became empty. All sixteen package files and all four
catalog members passed checks. No installation or OS trust change occurred. The
generic backend's selected DSP binary and loaded driver tuple were not independently
captured; successful access does not qualify that identity gate, individual
mapping modes, DMA64, available VTCM or vendor-session teardown. The prior STATIC
refusal remains valid for its different native control.

The executable embeds [asInvoker](../tools/hexagon/diagnostic.manifest), preventing
Windows installer heuristics from requesting elevation for the name `dispatch`.
The manifest is bound to the same source/build receipt as the fixture.

[Hosted preparation](../scripts/Build-HexagonDispatch.ps1) builds the fixture,
direct Rust query and model-free contract tests, retaining a two-file access bundle
separately from compiler evidence. The [supervisor](../scripts/Invoke-HexagonDispatch.ps1)
checks same-revision source/manifest/build identities, all sixteen pinned package
files and current catalog membership for all four bundled DSP libraries. It
removes inherited accelerator/FastRPC overrides and Actions credentials only in
the worker, caps both fixture streams while running and retains checked files
through confirmed process exit. A shared kill-on-close Job owns the entire tree.
Strict complete/partial receipt parsing rejects duplicate JSON names, coerced
booleans and contradictory nested results. It preserves fixed entered-stage
markers, including graph-context allocation, before a later abort/hang. A marker
does not prove the entered native operation completed.

The [workflow](../.github/workflows/hexagon-qualification.yml) prepares on hosted
Windows ARM64. Its main-only manual owner job selects either inventory or numeric
dispatch, with the clock started before checkout/intake, a five-minute job limit,
at most 120 seconds for the complete dispatch operation and a 75-second internal
cleanup reserve. It installs nothing. Missing resources, deployment failures or
insufficient time retain a pending gate without a weaker retry. Isolated
`codex/hexagon-*` pushes run hosted controls only; owner jobs stay main-only.

The [successful hosted ARM64 run](https://github.com/symbiogenesis/SwiftBonsai2Runner/actions/runs/41162447956)
at `d7eee0ab8c0dcf3fa4e6e0ab49c94f07f679a55f` passed eight native contract
tests, seven packed-operation tests, immutable capture verification, layout,
preflight/dispatch policy, comparative-ranking tests and policy Clippy. The
[source/build selection](evidence/hexagon-advance-controls.json) binds exact
artifacts and receipt digests. Artifact intake, the single two-graph dispatch,
native metadata query and confirmed process cleanup charged **111,967 ms** in
total, including a conservative 30-second allowance for prior focused checks.
These are access observations, not a latency benchmark. No model was loaded and
Swift/profile admission remains disabled. Independent review, complete driver/
DSP identity, native signed-session mapping, custom packed kernels, actual-model
parity, resources and durable owner-managed archival remain open.
