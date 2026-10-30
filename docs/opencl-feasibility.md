# Native ARM64 OpenCL feasibility

This bounded information-query/source screen selects no inference profile and is independent of
Vulkan qualification. The pinned public backend has **no reusable Swift PQ2/PTQ1 packed
projection**: neither format appears in `GGML_OP_MUL_MAT` eligibility, and PQ2/PTQ1/Hadamard
implementations are absent. Generic Q1/Q4/Q8 or an installed compiler cannot supply them. This
limits direct reuse at the audited revision; public kernels can still be written.

The screen considers one installed implementation and one immutable source revision, with no
prompts/tokens/projection shapes or device compute. Its ten-second query runs in a 512 MiB
one-process Job with 1 MiB per-output-stream bounds. Feasibility budget: 30 minutes; initial
model/kernel compute: zero. No context, queue, program, kernel or device allocation is created.

## Reproducible native query

`bonsai-tools host-opencl --request <absolute-empty-object.json>` launches its own ARM64 child with
the ordinary headless worker mitigations. It loads only the absolute Windows system `OpenCL.dll`,
restricting initial dependency search to that directory and System32. `OCL_*`, `OPENCL_*` and `CL_*`
environment overrides reject. Loader/ICD calls still execute installed native code and remain an
operator trust boundary. The internal `host-opencl-query` command has no intrinsic driver timeout;
callers must use the owning wrapper.

The query bounds eight platforms, sixteen total GPU devices, 16 KiB per native string and 512
extension names per device. Malformed UTF-8, missing terminators, invalid native sizes, non-GPU
handles and excessive counts reject. CPU devices are not requested. A successful empty enumeration
is observed, while load, driver, framing or deadline failure returns unknown and retains bounded
stderr. There is no retry with a weaker process policy.

The separate `runner-native-opencl-observations` report records platform/device, driver and OpenCL C
versions, compiler/device availability, address width, extensions and capacity limits. The loader
size/SHA-256 is a bounded file observation after querying, not proof that exactly those bytes or
every dependency were executed. DLL signatures, ICD dependencies and provenance remain unverified.
Global/local memory and maximum-allocation capacities are not live free-memory budgets and cannot
change admission. `backend_ready` stays false.

`Doctor.ps1 -ToolPath <existing-bonsai-tools.exe>` adds this report as `NativeOpenClObservations`.
It neither builds tools nor changes the existing CPU compatibility digest or launch policy. The
native query follows the Khronos
[platform](https://registry.khronos.org/OpenCL/specs/unified/refpages/man/html/clGetPlatformIDs.html),
[GPU
enumeration](https://registry.khronos.org/OpenCL/specs/unified/refpages/man/html/clGetDeviceIDs.html)
and
[device-information](https://registry.khronos.org/OpenCL/specs/unified/refpages/man/html/clGetDeviceInfo.html)
contracts.

## Retained X2 information-query evidence

The [2026-10-29 access receipt](opencl-x2-access-evidence.json) binds the tested maintenance
executable hash and separates two model-free observations on this native Snapdragon X2 Elite Extreme
X2E94100:

- The production `host-opencl` wrapper's child exits with `3221225477`
  (`0xC0000005`, access violation). Its capability report stays unknown and its
  raw output files remain in external temporary evidence. No worker access or
  loader/device capability is inferred from that failure.
- One separately labelled ten-second ordinary-tool-policy diagnostic succeeds.
  It reports Adreno X2-90, OpenCL and OpenCL C 3.0, an available online compiler,
  and driver `QUALCOMM build: 863.0 Compiler DX.50.39.00`. The advertised
  extensions include FP16 and integer-dot features. It observes a 535,112-byte
  system loader with SHA-256
  `b5b0c8e990c06d4477ee8d9957460b466e52e7d8bf80e413d7e41aa7039dab7f`.

The diagnostic used existing ordinary tool ownership and a deadline, without the worker's 512 MiB
Job or its mitigation envelope. The comparison therefore identifies a combined policy/resource
incompatibility, not its exact cause, failing DLL or specific mitigation. The shipped command has no
such retry. Neither observation creates a context or executable GPU workload. The successful
diagnostic does not qualify worker GPU access, compiler output, provenance or Swift arithmetic. The
next packed-kernel pilot must resolve that access boundary without weakening worker policy before
device execution.

## Immutable public source audit

The audit uses Runner's existing C++ runtime revision `73bfe6cc485554822f80c473a3c0c4f25e80f41b` in
`PrismML-Eng/llama.cpp`. The retained [source identities](opencl-source-identities.json) bind all
three reviewed files to their byte counts and SHA-256. No source is executed or copied into the
application.

| Checked source | Result |
| --- | --- |
| [OpenCL dispatch](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/ggml/src/ggml-opencl/ggml-opencl.cpp#L7734) | Packed PQ2/PTQ1 projections are ineligible; the limited FP32 GDN and convolution branches do not establish complete Swift coverage. |
| [Build recipe](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/ggml/src/ggml-opencl/CMakeLists.txt) | Public OpenCL source kernels can be embedded; an OpenCL SDK and Python are build requirements. No accelerated Runner binary is built in this screen. |
| [Backend documentation](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/docs/backend/OPENCL.md) | Windows ARM64 and X2-90 are listed, but optional Qualcomm prebuilt kernels are a separate manually provisioned library for selected other formats. They are excluded from this public-source comparison. |

The [exact published
release](https://github.com/PrismML-Eng/llama.cpp/releases/tag/prism-b10787-73bfe6c) asset metadata
checked on 2026-10-29 contains no Windows ARM64 OpenCL bundle; its Windows ARM64 bundles are CPU and
CUDA. The retained [asset snapshot](opencl-release-assets.json) records their public names, sizes
and reported digests without downloading them. Release metadata is an observation, not an immutable
execution or publisher-trust receipt. The shipped runtime remains the CPU archive in
`config/swift-assessment.lock.json`. The [next Prism pilot](../PLAN.md#adreno-vulkan) is one real
Swift packed-projection shape against scalar CPU arithmetic, with unchanged-policy worker access
proven before execution. A Windows ARM64 source route is not a reviewed accelerated binary. New
kernels need build, arithmetic/device and partial-placement transfer/state/fallback evidence;
changed formats or whole-model expansion are outside this screen. Neither information-query outcome
resolves Vulkan access or establishes speed/quality. Model/kernel comparisons and full suites remain
[CI-only](validation.md).
