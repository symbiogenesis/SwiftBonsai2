# Worker sandbox architecture

Reviewed against Microsoft's published sources on 2026-10-26. The current
development launcher is **not a filesystem or network sandbox**. Its workers
retain the caller's identity. Windows Jobs, artifact verification, credentials
and process mitigations reduce particular risks; none substitutes for a
restricted execution boundary. The daily profile remains unqualified.

## Choose containment for the workload

| Technology | Decision for Runner | Reason |
| --- | --- | --- |
| [Microsoft Execution Containers (MXC)](https://github.com/microsoft/mxc/tree/v1.0.1) | Preferred integration target for the native Windows ARM64 inference and speech workers; not yet integrated. | Native process containment supports filesystem, network and desktop policy without replacing the Rust UI or Windows runtime artifacts. |
| [NVX](https://github.com/microsoft/nvx) | Evaluate for future hostile Linux tools needing a separate kernel. | OpenVMM runs a Linux guest with hardware isolation. Existing Windows PE workers, artifacts, transport and resource accounting need a separate guest implementation and qualification. |
| [LiteBox](https://github.com/microsoft/litebox) | Research option, not Runner's security boundary. | Its current Windows userland implementation targets x86_64 and explicitly allows the guest to read or overwrite host memory in their shared address space. It cannot isolate these Windows ARM64 workers. |
| [Windows Sandbox](https://github.com/microsoft/mxc/blob/v1.0.1/docs/backends/windows-sandbox/windows-sandbox.md) | Experimental MXC backend; optional future Windows VM qualification lane. | Separate-kernel isolation, but upstream teardown is best-effort and a hard-killed launcher can leave an orphan VM. Boot, model sharing, transport and cleanup need qualification. |

LiteBox's limitation is stated in the
[Windows platform source](https://github.com/microsoft/litebox/blob/main/litebox_platform_windows_userland/src/lib.rs).
Its library-OS interface can reduce exposed services, but that does not by
itself create an enforced trust boundary. Do not combine technologies merely
because they are available: each additional runtime, image, bridge and update
path adds code and operational cost. No speed or memory benefit is assumed;
all performance evaluation remains CI-only.

## Threat model and authority

Treat native model, tokenizer, adapter and audio parsing as potential entry
points for a compromised worker, even when the bytes match a reviewed pin.
The trusted Rust UI owns microphone access and playback. The broker owns public
request validation, admission and frontend credentials. Provisioning and build
tools run separately from inference; an inference policy must never acquire
their download, compiler, profile or repository permissions.

Each model, ASR and TTS process needs an independent restricted identity and
policy. Only its exact executable/runtime, model/tokenizer/adapter files and its
own backend credential should be readable, together with the required OS
runtime. Only a fresh private worker scratch/cache directory should be writable.
Retain existing artifact handles and verification. Keep Windows/profile
environment variables intact: policy restricts access, not fabricated paths.
Child-only TEMP/TMP can point to that worker's private scratch directory.
Do not clear the environment or redirect SystemRoot, ProgramData or profile
folders into the checkout.

The existing inference working directory is the protected evidence directory,
which also contains **both frontend and backend keys**. A future sandbox must
separate worker scratch from owner evidence and grant only the worker's key
read access. Granting the entire evidence directory would expose the frontend
credential and weaken the broker boundary. Open stdout/stderr on the owner side
and inherit only their handles, rather than granting all log paths writable.
Do not grant the entire data directory, user profile, repository or host TEMP.

Default-deny external ingress and egress. Current native workers serve only
numeric loopback, so an HTTP integration needs explicit container-hosted
loopback support; preserve bounded HTTP/authentication, exact Host/routes and
listener PID checks. Loopback permission is not broker-only authentication.
It can permit access to unrelated local services and other same-user processes
can interfere with credentials and endpoints. Qualifying the stronger
same-user-bypass requirement needs an OS-enforced brokered communication design
and its own adversarial tests, rather than a blanket loopback exemption.
An administrator or a compromised trusted owner remains outside the proposed
worker boundary.

Model output remains inert text. The public API rejects tools, arbitrary
executables and dynamic model/runtime options. If code execution or plugins are
added later, give them separate capabilities, per-task storage, resource limits
and a separately qualified VM boundary where a hostile shared-kernel workload
requires one. They must not inherit inference or UI authority.

## MXC adoption requirements

Use the Rust SDK and retain the pure Rust/native presentation contract. Review
and pin an exact published version and its transitive dependencies. As reviewed,
[`mxc-sdk` v1.0.1](https://github.com/microsoft/mxc/blob/v1.0.1/src/mxc-sdk/Cargo.toml)
still contains AppContainer/BFS/DACL fallback paths. The moving `main` branch has
different enforcement behavior. Never infer the pinned release's protections
from current-main documentation.

Before creating credentials or starting a model, probe the **actual requested
policy**, require native Process Security Environment containment and reject
errors, unsupported restrictions, DACL augmentation and weaker fallback tiers.
Capability availability is authoritative; a sufficiently new Windows version
alone does not prove the request can run. Container-hosted HTTP additionally
requires `base_container_supports_ingress_host_loopback_allow`.
See the versioned [Rust capability types](https://github.com/microsoft/mxc/blob/v1.0.1/docs/api-reference/rust/v1/types.md)
and [networking contract](https://github.com/microsoft/mxc/blob/v1.0.1/docs/backends/process-container/networking.md).
Forbid audit/learning modes in ordinary execution, silent retries outside the
sandbox, global loopback exemptions and automatic host ACL/firewall changes.

The supported [Rust V1 launch API](https://github.com/microsoft/mxc/blob/v1.0.1/docs/api-reference/rust/v1/api.md)
does not expose our existing Job, suspended launch or process/memory limits.
A direct replacement with `v1::spawn`, or wrapping `wxc-exec.exe` and treating
the executor PID as the model PID, would lose existing guarantees. Integration
must first provide a supported way to compose containment with:

- Atomic Job assignment before any worker code runs, one-process policy,
  job-wide commitment limits and no breakaway.
- Exact model PID identity for listener checks and resource monitoring.
- Whole-tree teardown on startup failure, cancellation, owner drop and abrupt
  owner death, including bounded confirmation of process exit.
- Inherited standard streams without extra handles, absolute external working
  directories, preserved OS/profile environment and the physical-RAM watchdog.

The v1.0.1 crate exposes doc-hidden PSEC APIs and generated FlatBuffer types that
could attach a security-environment attribute to our existing `STARTUPINFOEXW`.
Microsoft explicitly marks these compatibility exports as unsupported for SDK
consumers in its [SDK boundary](https://github.com/microsoft/mxc/blob/v1.0.1/src/mxc-sdk/src/lib.rs).
Do not hand-copy its policy encoder or depend on those unsupported interfaces as an
unreviewed shortcut. Prefer a supported composition hook, or independently
review and qualify an explicit native boundary before introducing that debt.
No such hook or SDK integration is claimed by this change.

## Current defense in depth

`bonsai-windows-process` applies process-creation mitigations to `ProcessSpec`
workers before their first instruction: deny Win32k system calls, legacy
extension points and remote/low-integrity executable images, and terminate on
heap corruption. It keeps the atomic Job assignment and standard-handle list.
These policies suit the current headless CPU workers; GPU drivers, GUI helpers
or other future backends require separate compatibility review. Tool processes
keep their ordinary inherited policy. Failure to apply a required creation
attribute fails launch; there is no unmitigated retry.

The policy uses
[`PROC_THREAD_ATTRIBUTE_MITIGATION_POLICY`](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-updateprocthreadattribute).
It avoids Job UI limits because Windows
[does not allow those limits when forming nested Jobs](https://learn.microsoft.com/en-us/windows/win32/procthread/nested-jobs).
These mitigations reduce native/UI attack surface, but ordinary filesystem
reads/writes, sockets and the caller identity remain unrestricted.

## Qualification before enabling a sandboxed daily profile

Use focused synthetic checks locally and complete suites in main-only Foundation
CI under the [validation policy](validation.md). Preserve evidence for the exact
SDK/source, policy, OS/architecture, runtime and artifact revisions. A read-only
probe or successful command launch is insufficient. Required evidence includes:

- Positive access: the pinned Windows ARM64 model, adapter, tokenizer and native
  speech engines load and serve through the authenticated broker; approved
  scratch writes and streamed output work.
- Negative access: unrelated files, frontend/other-worker keys, model/runtime
  writes, profile/repository traversal, reparse/alternate-path escapes, Internet,
  LAN, unrelated loopback services and desktop operations are rejected.
- Boundary behavior: missing native support, unsupported ingress, stale policy,
  unavailable artifacts and audit/fallback attempts fail closed before serving.
- Lifecycle: memory/process limits, listener identity, stalled consumers,
  cancellation, native faults and abrupt owner death leave no runnable worker,
  warm VM, bridge or unowned writable mapping.
- Separate acceptance for adversarial same-user backend bypass. A successful
  model request, protected same-user DACL or native containment probe does not
  establish that property.

The focused process regression queries the policies from inside a newly started
worker, checks a second owner in a nested Job, and compares tool policy to an
ordinary inherited-policy control. It establishes mitigation application and
composition, not MXC containment or actual-model compatibility.
