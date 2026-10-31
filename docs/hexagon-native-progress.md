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

## Prism source candidate

The isolated Prism branch `codex/hexagon-native-capability-1030`, based on
Runner's pinned `7aca2251f09acb345d4afb53d5ce411f81017745`, contains the
`bonsai-hexagon-native` workspace crate. It separates unsafe Windows/vendor calls
from the safe root engine, has no inference-library dependency and remains
`publish = false`. A root optional dependency was omitted because an unpublished
optional crate would add a registry prerequisite to existing runtime publication.
Source review, registry/release policy and runtime integration remain pending;
Runner's release pin does not consume this branch.

Strict Clippy with all targets passed for the candidate. Focused ABI and partial
mapping receipt regressions passed. The library uses one nonblocking process-wide
lease, fixed requests, a consuming one-shot mapping probe and explicit uncertain
resource retention. Full regressions remain hosted-CI only.

## Signed-package numeric fixture

The separate [Rust access fixture](../tools/hexagon/dispatch_fixture.rs) calls only
the explicitly selected Hexagon backend from the pinned signed reference package.
Two fixed F32 graphs use K=128, M=32 and N=1/32, poison all outputs and compare
all 32/1024 results against an independent closed form. There is no CPU scheduler
or CPU fallback. The host self-check passed; actual device graphs remain pending
a trusted hosted build and current signed deployment access.

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

The [workflow](../.github/workflows/hexagon-qualification.yml) prepares on hosted
Windows ARM64. Its main-only manual owner job selects either inventory or numeric
dispatch, with the clock started before checkout/intake, a five-minute job limit,
at most 120 seconds for the complete dispatch operation and a 75-second internal
cleanup reserve. It installs nothing. Missing resources, deployment failures or
insufficient time retain a pending gate without a weaker retry. Hosted CI and
actual signed-package device dispatch have not yet run for this candidate.
