# Windows integration and worker sandboxing

Runner's native UI requires Windows process containment for its owned inference,
ASR and TTS workers. `bonsai-windows-process` composes the pinned **MXC 1.0.1**
Process Security Environment (PSEC) with suspended process creation,
verified Job ownership before resume, exact worker PID, resource limits and
restricted handle inheritance. Missing capabilities or rejected policies fail launch without an
uncontained retry. Actual-model and stronger broker-only isolation acceptance
remain unqualified. This document also owns the Runner-native handle, storage,
thread and audio contracts; separately published Prism/speech engines need their
own reviews. The [UI architecture](ui-architecture.md) remains binding.

## Windows consent and capabilities

Microphone permission and worker containment are separate Windows mechanisms.
The trusted native UI requests `AppCapability` microphone access after explicit
user action. Model workers receive no microphone, playback or presentation role.
PSEC applies the worker's filesystem, network and desktop restrictions in Windows;
these are kernel-enforced grants, not a generic permission prompt for each file.
A Windows Firewall prompt or rule is another independent control. A denied
Firewall request must be distinguished from a rejected PSEC policy during diagnosis.

## Technology selection

| Technology | Role |
| --- | --- |
| [Microsoft Execution Containers](https://github.com/microsoft/mxc/tree/v1.0.1) | Native Windows ARM64 process containment used by owned UI workers. |
| [NVX](https://github.com/microsoft/nvx) | Possible future separate-kernel boundary for hostile Linux tools. It runs a Linux guest and cannot substitute for the pinned Windows PE workers. |
| [LiteBox](https://github.com/microsoft/litebox) | Research only. Its [Windows userland platform](https://github.com/microsoft/litebox/blob/main/litebox_platform_windows_userland/src/lib.rs) targets x86_64 and explicitly shares writable host memory with the guest; it is not this application's security boundary. |
| [Windows Sandbox through MXC](https://github.com/microsoft/mxc/blob/v1.0.1/docs/backends/windows-sandbox/windows-sandbox.md) | Experimental future Windows VM lane. Upstream teardown is best-effort and a hard-killed launcher can leave an orphan VM. Model sharing, transport, resource accounting and cleanup need separate qualification. |

No performance benefit is assumed from a technology name. All performance
evaluation remains CI-only. The UI and transport contracts remain
[pure Rust/native](ui-architecture.md).

## Worker authority

Treat native model, tokenizer, adapter and audio parsing as potential compromise
entry points even when the bytes match a reviewed pin. Provisioning and compiler
tools remain trusted maintenance operations with their existing authority; they
are not launched under the inference policy. Independently managed external
speech companions also retain their separately managed permissions.

The owned-worker policy grants:

- Read-only access to exact selected executable/runtime files, model, tokenizer
  or codec, adapter and only that worker's backend credential. Public policy
  inputs accept files, not broad data/profile directories.
- Internal read-only Windows runtime directories. No drive-root grant is added:
  a native negative test showed that a generated read-only volume-root grant
  exposed an unrelated credential. The trusted owner validates and retains full
  ancestry. Read validation inside an actual AppContainer skips only the denied
  volume-root metadata check; local-drive and every non-root check remain in force.
- Writable application scratch below a fresh protected owner/LocalSystem
  directory. Keys and parent-opened stdout/stderr remain outside that writable
  grant; only their specific standard handles are inherited.
- Windows' own private PSEC temporary storage. Windows can replace the requested
  child TEMP/TMP with `Packages/sandbox.{id}/AC/Temp`; supplied scratch is not the
  sole writable location, and effective TEMP must not be claimed to equal cwd.
- Explicit bidirectional host-loopback access required by the current native
  HTTP servers, with external ingress and egress denied by policy and no
  Internet/private-LAN capability.

Host-loopback permission includes services reached through the host's own
non-loopback interface addresses. Native tests observed this even with explicit
all-address egress-deny rules and without the older loopback capability/sentinel.
It **does not enforce broker-only communication** or deny all other local services.
The [versioned networking contract](https://github.com/microsoft/mxc/blob/v1.0.1/docs/backends/process-container/networking.md)
explains the coarse host-loopback grant. Retain numeric-loopback binding, bearer
authentication, exact Host/routes and listener PID checks. A stricter boundary
requires a separately proven brokered transport or accepted peer identity;
changing a JSON toggle is insufficient.

OS/profile environment entries are passed through; the launcher does not clear
the environment or relocate Windows/profile folders into a checkout. Child
TEMP/TMP select scratch before Windows applies PSEC's own temporary-storage policy.
Known credential-shaped variables, including common tokens, API keys, passwords
and connection strings, reject sandbox launch with a value-free error. This is a
conservative guard, not proof that every arbitrarily named inherited value is
non-secret. Run from an appropriately scoped parent environment.

Existing artifact verification and retained handles remain in force. Jobs still
enforce process count and committed memory; the physical-RAM watchdog remains
necessary for mapped weights. Scratch and log storage do not acquire a disk quota
merely by being private. Same-user processes outside the sandbox can still read
owner-protected credentials or interfere with endpoints; administrator or trusted
owner compromise remains outside this worker boundary.

## Native integration contract

Runtime probes require PSEC 1.1 and container-hosted loopback support, then
create and dispose a real minimal security environment.
Version numbers or export presence alone are insufficient. The actual complete
policy is also admitted before a worker runs. Audit/learning modes, SDK backend
dispatch, DACL augmentation, global loopback exemptions and weaker fallback tiers
are not used. Host Firewall restrictions may still apply independently.

The supported [Rust V1 spawn API](https://github.com/microsoft/mxc/blob/v1.0.1/docs/api-reference/rust/v1/api.md)
cannot accept Runner's existing Job and suspended-launch contract. The audited
native boundary therefore uses the exact release's doc-hidden PSEC compatibility
exports and generated FlatBuffer types to add the security-environment attribute
alongside `JOB_LIST`, mitigations and the standard-handle list. Windows' PSEC
broker can omit the requested Job assignment, so the launcher verifies membership,
assigns the still-suspended root when needed, verifies again and requires exactly
one suspension before resume. The one-process Job limit is tested by an actual
denied child creation. A redundant child-process creation attribute prevented
ordinary native workers from initializing on the tested host; child denial
therefore uses the verified Job limit, including for fixed-command inspection.
The [native terminate-on-close flag](https://learn.microsoft.com/en-us/windows/win32/api/processmodel/ne-processmodel-process_security_environment_flags)
also bounds the environment's lifetime. No executor wrapper is mistaken for the
model PID.

Microsoft marks those compatibility exports as unsupported SDK interfaces in the
[SDK boundary](https://github.com/microsoft/mxc/blob/v1.0.1/src/mxc-sdk/src/lib.rs).
This is explicit compatibility debt: review the encoder, ABI, dependencies and
actual enforcement before changing either exact pin. Current-main documentation
must not substitute for the shipped release's behavior. Prefer a supported
composition hook when one becomes available.

Ordinary headless `ProcessSpec::start` requests creation-time Win32k denial,
legacy extension-point and remote/low-integrity image denial, and heap termination.
The PSEC broker does not preserve every ordinary creation attribute; contained
launches require verified Win32k denial, Job ownership and native lifetime control,
and must not claim the other ordinary mitigations as proven. The ordinary policy uses
[`PROC_THREAD_ATTRIBUTE_MITIGATION_POLICY`](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-updateprocthreadattribute),
not Job UI limits that prevent [nested Jobs](https://learn.microsoft.com/en-us/windows/win32/procthread/nested-jobs).
Future GPU drivers or GUI helpers require separate compatibility review.

## Launch and qualification

The native UI selects `--require-native-sandbox` for its owned broker and always
uses containment for owned speech. A direct development broker can instead
explicitly select `--allow-unisolated-backend`; the two options are exclusive.
That opt-out retains the earlier caller-authority development boundary and is
never an automatic recovery path for failed containment.

Focused local checks and main-only Foundation suites follow the
[validation policy](validation.md). Record the exact SDK, policy, OS/architecture,
runtime and artifact identities. Distinguish portable parser/policy checks from
actual native enforcement and actual-model acceptance. Model-free native fixtures
cover file and hard-link denial, retained artifact/key guards, separate private
temp storage, child-process denial, exact listener identity, external TCP denial
and worker teardown after abrupt owner death. Required remaining evidence
includes complete model/adapter/speech loading, lifecycle and cancellation under load,
and a separately implemented/proven broker-only transport. Native admission
errors and pending qualification must not be reported as passed.

The public API still rejects tools, arbitrary executables, dynamic runtime
selection and unsupported media. Model output remains inert text. Future plugins
or generated-code execution need separate per-task capabilities and, where the
threat model requires it, a separately qualified VM boundary. They must not
inherit the UI's or inference worker's authority.

## Resource ownership and process lifetime

Use Rust [owned/borrowed handle types](https://doc.rust-lang.org/std/os/windows/io/),
`File`, socket and process owners; transfer fresh Win32 handles into owners
immediately and borrow raw handles only for a call. Pseudo handles stay borrowed.
Match allocator-specific destructors: credentials use `LocalFree`, WinRT uses its
bindings, and audio retains waveOut headers/buffers until driver release.

`src/windows-process` creates hidden suspended processes with explicit executable,
working directory and quoted arguments. Creation attributes assign an unnamed
Job and inherit only the three standard streams. [Jobs](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects) kill
descendants on owner loss, reject breakaway and enforce process/commit bounds; they do not restrict
security identity. Parent monitoring retains query/synchronize handles and checks
creation time against PID reuse. Native PSEC composition and its distinct
effective mitigations are specified above.

Build tools preserve the Unicode Windows/profile environment with reviewed child
overlays. Never clear it, relocate system/profile folders or run native diagnostics
from a repository working directory; use [external validation lanes](validation.md).

## Files, locks and publication

| Purpose | Mechanism |
| --- | --- |
| One startup/build/download writer | Persistent sentinel, exclusive sharing and `File::try_lock`; handle release covers exit/crash. |
| One recovery writer | Persistent sentinel plus `File::try_lock`, read/write sharing without delete sharing, preventing replacement while locked. |
| Pinned artifacts/credentials | Retained read handles deny write/delete sharing; artifact ancestors remain held. |
| New evidence/private keys | Exclusive creation; protected owner/LocalSystem DACL precedes secret writes. |
| Artifact publication | Bounded staging, exact identity/hash verification and writer-guarded publication. |
| Recovery publication | Bounded user-scoped DPAPI, flushed temporary file and same-directory replacement under the recovery guard. |

Rust's [`File::try_lock`](https://doc.rust-lang.org/std/fs/struct.File.html#method.try_lock)
uses Windows byte-range locking (`LockFileEx`). Keep sentinels after releasing
their handles: unlink/recreate can give processes different locked file objects.
No record-oriented shared database needs independently locked ranges.
[Byte-range locks](https://learn.microsoft.com/en-us/windows/win32/fileio/locking-and-unlocking-byte-ranges-in-files)
do not constrain mapped I/O or replace deny-write/delete sharing for models.
Flush and atomic namespace replacement have distinct purposes; neither promises
survival of every device/power failure.

Validate expanded absolute paths, device names (including superscript COM/LPT
digits), alternate streams, control characters and reparse points. Audited storage
boundaries use `FILE_FLAG_OPEN_REPARSE_POINT` and metadata from opened handles,
not only earlier pathname checks. [Credential storage](../src/windows-credentials/README.md)
requires owner-controlled ancestors; leaf checks cannot freeze all ancestry or
protect against hostile same-user processes.

## Threads, responsiveness and power

Named `std::thread::Builder` workers are OS threads. Network, disk/DPAPI, speech
orchestration and playback stay off the event loop, with bounded queues,
cancellation generations and joined shutdown. The broker uses async I/O,
bounded admission and absolute deadlines; a separate speech watchdog can kill
an owned companion when native inference cannot cancel cooperatively. Microphone
operations stay on their WinRT apartment thread with balanced initialization
and bounded asynchronous operations.

Retain normal Windows scheduling. Do not give inference realtime priority,
guess affinity from SKU or raise system timer resolution for polling.
[MMCSS](https://learn.microsoft.com/en-us/windows/win32/procthread/multimedia-class-scheduler-service)
requires a demonstrated audio deadline problem on that audio thread and balanced
registration; it does not justify boosting inference.
[EcoQoS](https://learn.microsoft.com/en-us/windows/win32/procthread/quality-of-service)
can suit explicit background work, but response generation/playback needs device
latency/power evidence. Current configurable thread budgets reserve CPU capacity;
they establish neither optimal topology nor performance.

WinMM waveOut playback retains stable headers/buffers, bounded buffering,
stop/reset and unprepare-before-free ordering. Microsoft
[recommends newer audio APIs](https://learn.microsoft.com/en-us/windows/win32/multimedia/waveform-functions);
WASAPI/AudioGraph migration still needs device-change, suspend/resume,
cancellation and latency qualification.

## Permissions and capability isolation

Credentials use system CSPRNG bytes, protected ACLs, noninheritable retained
handles and scrubbed buffers; recovery uses user-scoped DPAPI. The UI requests
`AppCapability` microphone access only after user action and stops capture on
observed revocation. Deadlines/revocation are cooperative, not an OS-enforced
recording quota while UI/processes are suspended. Model-capability reports are
not Windows permission grants; microphone permission does not sandbox inference.

Owned UI inference/speech requires PSEC; explicitly unisolated development retains
caller authority. The daily profile remains disabled pending the
[broker boundary](broker-policy.md) and acceptance in [PLAN.md](../PLAN.md).
Focused synthetic regressions establish repaired path/handle/lock behavior;
Foundation owns full suites. Real models, audio hardware, restricted-identity
acceptance, performance and comfort require independent evidence.
