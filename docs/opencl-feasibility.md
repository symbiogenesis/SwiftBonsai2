# Native ARM64 OpenCL feasibility

This is a bounded access and source-coverage screen, separate from Vulkan
qualification. No OpenCL inference profile is selected. The public pinned
backend has **no directly reusable Swift PQ2/PTQ1 packed-projection path**: its
`GGML_OP_MUL_MAT` eligibility dispatch has neither format, and the source
contains no PQ2, PTQ1 or Hadamard implementation. Generic Q1/Q4/Q8 support and
an installed OpenCL compiler do not supply Swift's packed arithmetic. This is
a direct-reuse limitation of that source revision, not a rejection of writing
the missing public OpenCL kernels.

The pilot considers one installed system implementation and one immutable
public source revision. Its sample has no prompts, tokens or projection shapes:
it executes information queries only. The query has a ten-second deadline,
512 MiB one-process Job and 1 MiB per-output-stream bound; the feasibility
review budget is 30 minutes and the initial model/kernel compute budget is zero.
No context, command queue, program, kernel or device allocation is created.

## Reproducible native query

`bonsai-tools host-opencl --request <absolute-empty-object.json>` launches its
own ARM64 child with the ordinary headless worker mitigations. It loads only
the absolute Windows system `OpenCL.dll`, restricting initial dependency search
to that directory and System32. `OCL_*`, `OPENCL_*` and `CL_*` environment
overrides reject. Loader/ICD calls still execute installed native code and remain
an operator trust boundary. The internal `host-opencl-query` command has no
intrinsic driver timeout; callers must use the owning wrapper.

The query bounds eight platforms, sixteen total GPU devices, 16 KiB per native
string and 512 extension names per device. Malformed UTF-8, missing terminators,
invalid native sizes, non-GPU handles and excessive counts reject. CPU devices
are not requested. A successful empty enumeration is observed, while load,
driver, framing or deadline failure returns unknown and retains bounded stderr.
There is no retry with a weaker process policy.

The separate `runner-native-opencl-observations` report records platform/device,
driver and OpenCL C versions, compiler/device availability, address width,
extensions and capacity limits. The loader size/SHA-256 is a bounded file
observation after querying, not proof that exactly those bytes or every
dependency were executed. DLL signatures, ICD dependencies and provenance remain
unverified. Global/local memory and maximum-allocation capacities are not live
free-memory budgets and cannot change admission. `backend_ready` stays false.

`Doctor.ps1 -ToolPath <existing-bonsai-tools.exe>` adds this report as
`NativeOpenClObservations`. It neither builds tools nor changes the existing CPU
compatibility digest or launch policy. The native query follows the Khronos
[platform](https://registry.khronos.org/OpenCL/specs/unified/refpages/man/html/clGetPlatformIDs.html),
[GPU enumeration](https://registry.khronos.org/OpenCL/specs/unified/refpages/man/html/clGetDeviceIDs.html)
and [device-information](https://registry.khronos.org/OpenCL/specs/unified/refpages/man/html/clGetDeviceInfo.html)
contracts.

## Retained X2 information-query evidence

The [2026-10-29 access receipt](opencl-x2-access-evidence.json) binds the tested
maintenance executable hash and separates two model-free observations on this
native Snapdragon X2 Elite Extreme X2E94100:

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

The diagnostic used existing ordinary tool ownership and a deadline, without
the worker's 512 MiB Job or its mitigation envelope. The comparison therefore
identifies a combined policy/resource incompatibility, not its exact cause,
failing DLL or specific mitigation. The shipped command has no such retry.
Neither observation creates a context or executable GPU workload. The successful
diagnostic does not qualify worker GPU access, compiler output, provenance or
Swift arithmetic. The next packed-kernel pilot must resolve that access boundary
without weakening worker policy before device execution.

## Immutable public source audit

The audit uses Runner's existing C++ runtime revision
`73bfe6cc485554822f80c473a3c0c4f25e80f41b` in `PrismML-Eng/llama.cpp`.
The retained [source identities](opencl-source-identities.json) bind all three
reviewed files to their byte counts and SHA-256. No source is executed or copied
into the application.

| Checked source | Result |
| --- | --- |
| [OpenCL dispatch](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/ggml/src/ggml-opencl/ggml-opencl.cpp#L7734) | Packed PQ2/PTQ1 projections are ineligible; the limited FP32 GDN and convolution branches do not establish complete Swift coverage. |
| [Build recipe](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/ggml/src/ggml-opencl/CMakeLists.txt) | Public OpenCL source kernels can be embedded; an OpenCL SDK and Python are build requirements. No accelerated Runner binary is built in this screen. |
| [Backend documentation](https://github.com/PrismML-Eng/llama.cpp/blob/73bfe6cc485554822f80c473a3c0c4f25e80f41b/docs/backend/OPENCL.md) | Windows ARM64 and X2-90 are listed, but optional Qualcomm prebuilt kernels are a separate manually provisioned library for selected other formats. They are excluded from this public-source comparison. |

The [exact published release](https://github.com/PrismML-Eng/llama.cpp/releases/tag/prism-b10787-73bfe6c)
asset metadata checked on 2026-10-29 contains no Windows ARM64 OpenCL bundle;
its Windows ARM64 bundles are CPU and CUDA. The retained
[asset snapshot](opencl-release-assets.json) records their public names,
sizes and reported digests without downloading them. Release metadata is an
observation, not an immutable execution or publisher-trust receipt.
The shipped verified runtime remains the CPU-only archive in
`config/swift-assessment.lock.json`. A public Windows ARM64 backend build route
does not establish a reviewed Swift-compatible accelerated binary. The next
implementation opportunity is a bounded public packed-projection pilot in
Prism, tracked in [PLAN.md](../PLAN.md#adreno-vulkan): one real Swift shape,
scalar CPU arithmetic control and unchanged worker-policy access first. Such a
kernel can be written; it needs its own build, arithmetic and device evidence.
Partial placement additionally needs transfer/state costs and exact fallback
evidence. Changing weight formats or expanding a complete model is outside this
screen.

The existing Vulkan headless instance-creation no-go remains independent. An
OpenCL information-query result cannot resolve it, qualify GPU execution, prove
speed/quality or promote an accelerator profile. Model/kernel comparisons and
full suites remain CI-only under [validation](validation.md).
