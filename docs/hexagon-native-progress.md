# Native Rust Hexagon access candidate

The [direct driver boundary](../tools/hexagon/native_rpc/README.md) now has local
X2 evidence and a source candidate in Prism. It is not an inference backend.
The production runtime lock, CPU policy and backend availability remain unchanged.
[PLAN.md](../PLAN.md#hexagon-npu) owns the remaining work.

## Observed access

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

The [default-process terminal observation](evidence/hexagon-x2-native-session.json)
adds an exact hosted source/build and independent package/catalog/driver binding.
Architecture/domain/URI preparation passed, but `remote_handle64_open` returned
**14** with no handle. Allocation and FD mapping were not attempted; all owners
remained pinned and the supervising Job became empty. The error's cause is not
established, and this does not establish that FD mapping is unsupported.

The [explicit reference-process observation](evidence/hexagon-x2-reference-initialization.json)
then returned zero for the fixed request2/enable1 initialization, but the driver
changed its mutable input structure. That old source's equality guard stopped
before URI/open. This was a candidate validation refusal, not a failed vendor
initialization. The revised binding retains both raw returned i32 fields and
uses only its original validated domain for subsequent calls; returned fields
never become configuration or an effective-process-class claim.

The [corrected native reference FD observation](evidence/hexagon-x2-native-fd-cycle.json)
then completed module open, 4096-byte allocation, descriptor/CPU-byte check, FD=2
registration/unregistration, normal free and explicit module close with zero native
returns. The returned process-control fields were domain0/enable1; only the original
validated domain3 was used afterward. All package/catalog/driver checks passed and
the owning Job was confirmed empty. This proves the bounded native API cycle, not
DSP-side visibility, DMA, compute, effective process class or vendor teardown.
The first STATIC/default-open refusals and old equality-guard result remain separate.

## Prism source candidate

[Prism draft PR 6](https://github.com/symbiogenesis/prism-inference/pull/6), on
`codex/hexagon-session-native-next-1030`, extends the earlier `17ca7b3` candidate
based on Runner's pinned `7aca2251f09acb345d4afb53d5ce411f81017745`.
The `bonsai-hexagon-native` crate remains isolated, `publish = false`, without
an inference-library dependency. The main engine does not consume or re-export
it, preserving the existing runtime publication boundary and release pin.

The safe API has fixed capability and metadata controls plus consuming terminal
STATIC, default-session FD and explicitly selected reference-initialization FD
probes. Unsafe vendor/Windows operations stay in the audited boundary. One
nonblocking process lease, exact URI/family/layout checks, guarded files and
terminal ownership prevent a failed or uncertain operation from releasing a
live buffer, module or driver. Map/unmap failures skip free and close; explicit
handle close does not qualify complete vendor teardown.

The separately selected reference initializer matches the bundled host source's
request2/enable1 before URI/open. It requests an unsigned DSP process class while
still requiring the pinned Windows package/catalog signatures. This is distinct
from signed-PD-only access and grants no arbitrary-module or OS trust authority.
The public Linux implementation documents process-local pre-open selection;
closed Windows behavior and effective process class remain separately observed.
No certificate, test-signing, boot-policy or installation changes occur.

[Hosted Windows ARM64 native controls](https://github.com/symbiogenesis/prism-inference/actions/runs/41165607707)
passed formatting, all contract tests and strict release Clippy for the mutable
payload binding. Runner and Prism retain the same seven native library sources.
Runner separately validates complete/partial JSON, explicit mode selection,
unknown in-flight results, fixed request ABI, requested-versus-returned values,
prerequisite refusal, cancellation/ownership and signature/source provenance.
Full suites remain hosted-only. Delayed/extended mapping, asynchronous device
submission, exact DSP-byte identity, kernel execution, resources and complete
vendor teardown remain independent qualification gates in [PLAN.md](../PLAN.md).

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
original generic backend observation did not independently capture its selected
DSP binary or loaded driver. The later driver-bound observation below closes only
the loaded-user-driver portion; successful access does not qualify individual
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
Windows ARM64. Its main-only manual owner job selects one inventory, numeric dispatch or native
FD session probe, with reference initialization explicitly selected, with the clock started before checkout/intake, a five-minute job limit,
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

## Independently bound numeric access

The [later numeric observation](evidence/hexagon-x2-driver-bound-dispatch.json)
passed both fixed graphs and all **1056** output comparisons on HTP0 with no CPU
fallback. A separate supervising process observed the live fixture's actual
`libcdsprpc.dll` module path, checked its SHA-256 and Microsoft publisher, and
held its file against mutation through child exit. The driver digest matches the
native observations above. All sixteen package files and four catalog members
passed; the owning Job was confirmed empty. Selected DSP bytes/architecture,
individual mapping modes, available VTCM, vendor teardown and Swift inference
remain unqualified. These are access observations, not throughput measurements.

The workflow prepares native access and actual-artifact binding in separate
hosted jobs. An unrelated binder failure does not qualify that tool or erase a
successful native-control job. The [artifact binder](../scripts/Build-HexagonPackedArtifact.ps1)
compiles the exact narrow extractor as a temporary Cargo example in the clean
pinned GGUF reader; its tracked dependency source remains unchanged. Cargo owns
split metadata and linking, and the owned LLD compiler tree must become empty.
The build receipt binds the source, added example, compiler/linker, dependency
artifacts, lock, manifest and executable. Actual extraction and full numerical/
state/performance qualification are separate gates.

## Bound Swift inputs and newer signed candidate

The [actual artifact binding](evidence/hexagon-swift-projection-binding.json)
verified all 7,206,168,928 bytes of the existing Swift file on a retained handle.
It extracted the original 23,674,880-byte first projection (SHA-256
`a0f21e1d4492de2496309b6383b958cba53c54f05c9de710348facdd31706354`)
and exact 5120-byte signs (`5ab93190ef5cd4f8322d52c1006783b25a3e118453bda0d0f90b1bf301a67197`).
Both output digests were independently checked. The retained header charge was
19,248,870 bytes against a 128 MiB cap. No transform, model inference or device
computation ran; activation/logit/state and adapter parity remain pending.

The [v0.10.0 candidate audit](hexagon-signed-q2-candidate.md) identifies newer
signed generic Q1/Q2 kernels. [Data-only verification](evidence/hexagon-signed-q2-package.json)
checked its exact archive, seventeen runtime files and all five Microsoft catalog
members, including v81. No new package code was loaded or installed. Its seven-argument
start ABI, Q8_1/64-weight reductions and operation/shape support need independent
native and numerical qualification; the guarded integer-dot control is conditional.
The v0.8.0 access reference and all production pins remain unchanged.

The [shared owner budget](evidence/hexagon-session-next-budget.json) charges one
investigation, including focused compile/result checks, exact hosted intake, three
terminal native variants, the driver-bound generic control, full-file extraction,
new-package data verification and conservative preparation/administration allowances.
Complete Rust, PowerShell/C# and artifact-binder suites ran only on hosted ARM64.
No latency, energy or profile promotion follows from these access observations.

The [selected credential-free archive](evidence/hexagon-session-next-archive.json)
retains 45 exact reports, own Rust diagnostic bundles, binding metadata, callers
and losing observations under the owner-managed qualification data directory.
All copied file digests were independently rechecked. Model weights, adapters
and vendor runtime binaries are excluded. Off-device backup, retrieval without
the original runner and independent qualification review remain pending.
The final charged owner total, including archive verification, is **140,537 ms**
within the single 300,000 ms allowance; this is accounting, not a performance result.

## Independently checked public RPC bytes

The [hosted public-source control](https://github.com/symbiogenesis/SwiftBonsai2Runner/actions/runs/41171859264)
built pinned Qualcomm QAIC with frozen dependency identities, generated the
seven-argument interface in documented standard-width mode, and compiled its C
stub against pinned public headers and host RPC mocks. Two synthetic calls
matched the independent source-derived fixture: start uses scalars `0x02010000`
and a complete 48-byte input, including zero padding; stop uses `0x03000000`
and a null argument array. The separately compiled
[Rust encoder](../tools/hexagon/public_idl_control.rs) and its contract test
matched every shared field and payload byte. Public result-type declarations
are explicit compiler inputs; losing legacy-type and missing-alias builds are
retained as separate observations.

The [candidate audit](hexagon-signed-q2-candidate.md) binds the distinct SDK
headers, matching normalized IDL inputs, additional queue declarations and
remaining Windows extensions. This control ran with synthetic handles on hosted
Linux LP64. Matching Windows ABI, provider exports, asynchronous queue ownership,
signed device execution, Swift state/adapter parity and measured benefit remain
[PLAN gates](../PLAN.md#hexagon-npu). Production admission and automatic selection
remain disabled. Subsequent SDK and source controls ran in hosted CI; the owner
investigation remains charged at **140,537 ms**.
