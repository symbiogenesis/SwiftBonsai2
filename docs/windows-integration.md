# Windows integration and reliability

Runner targets native Windows ARM64. This review covers the Runner-owned native
boundaries, launcher, storage, broker, UI and speech workers. The separately
published Prism runtime and native speech engines need their own reviews. The
[UI architecture contract](ui-architecture.md) continues to apply.

## Resource ownership and process lifetime

Rust's [`OwnedHandle`, `OwnedSocket` and borrowed handle types](https://doc.rust-lang.org/std/os/windows/io/)
provide the lifetime discipline that .NET `SafeHandle` provides for native
resources. Use `File`, socket and process owners where available; transfer fresh
Win32 handles into an owner immediately. Borrow raw handles only for the native
call. A pseudo handle is borrowed and must not be closed. Allocator-specific
resources still need their matching destructor: credentials own `LocalFree`
allocations, WinRT interfaces use their Rust bindings, and audio owns waveOut
headers and buffers until the driver releases them.

`src/windows-process` creates a hidden suspended process with an explicit
executable, working directory and argument quoting. Its creation attributes
assign an unnamed Job before child code runs and allow inheritance of only the
three standard streams. Jobs kill descendants on owner loss, reject breakaway
and apply the selected process/commit bounds. Parent monitoring retains a
query/synchronize handle and checks creation time to reject reused PIDs.
These are lifecycle and resource controls. A
[Job object](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects)
does not provide a restricted security identity.

Build tools retain the inherited Unicode Windows/profile environment and apply
only reviewed child overlays. Do not clear the environment, relocate Windows
system/profile folders, or run native diagnostics with a repository working
directory. The [validation policy](validation.md) describes external evidence
and native working directories.

## Files, locks and publication

Use the mechanism matching the protected object:

| Purpose | Current mechanism and reason |
| --- | --- |
| One startup/build/download writer | Persistent sentinel, exclusive sharing and `File::try_lock`; handle lifetime releases ownership after exit or crash. |
| One recovery writer | Persistent sentinel, `File::try_lock`, and read/write sharing without delete sharing; a second process cannot replace the sentinel while its lock is held. |
| Verified artifacts and credentials | Retained read handles deny write/delete sharing; consumers also retain required artifact ancestors. |
| New evidence and private keys | Exclusive creation; credential files additionally receive a protected owner/LocalSystem DACL before secret bytes are written. |
| Artifact publication | Bounded staging, exact identity/hash verification and publication under the writer guard. |
| Recovery publication | Bounded user-scoped DPAPI data, flushed temporary file and same-directory replacement under the recovery guard. |

Rust's Windows file locking uses
[`LockFileEx`](https://doc.rust-lang.org/std/fs/struct.File.html#method.try_lock).
Byte-range locking already underlies the single-writer file locks; there is no
record-oriented shared database requiring additional independently locked
ranges. Keep sentinels after releasing their handles. Unlinking and recreating
a sentinel can give different processes different locked file objects.

Windows [byte-range locks](https://learn.microsoft.com/en-us/windows/win32/fileio/locking-and-unlocking-byte-ranges-in-files)
do not constrain memory-mapped I/O. They cannot replace deny-write/delete
sharing for pinned mapped models. Flushes and atomic namespace publication also
serve different purposes; these mechanisms do not promise survival of every
storage-device or power failure.

Validate expanded absolute paths, Win32 device names (including superscript
COM/LPT digits), alternate streams, control characters and reparse points.
Use `FILE_FLAG_OPEN_REPARSE_POINT` and check metadata on opened handles at audited
storage boundaries, rather than trusting an earlier pathname check alone.
The [credential contract](../src/windows-credentials/README.md) still requires
owner-controlled ancestors: leaf checks cannot freeze every ancestor or protect
against a hostile process with the same user identity.

## Threads, responsiveness and power

Named `std::thread::Builder` workers are Windows OS threads. Network, disk/DPAPI,
speech orchestration and playback work run outside the native event loop, with
bounded text/audio queues, cancellation generations and joined shutdown. The
broker uses asynchronous I/O, bounded admission and absolute deadlines. A
separate speech watchdog can terminate its owned companion when native inference
cannot cooperatively cancel. UI microphone operations remain on their WinRT
apartment thread with balanced initialization/uninitialization and bounded
asynchronous operations.

Use Windows' normal scheduling policy unless measured work warrants a change.
Do not give model inference realtime priority, apply CPU affinity by SKU guess,
or increase the system timer resolution for polling. The
[Multimedia Class Scheduler Service](https://learn.microsoft.com/en-us/windows/win32/procthread/multimedia-class-scheduler-service)
is appropriate for a demonstrated audio deadline problem on the relevant audio
thread, with balanced registration cleanup. It is not a reason to boost the
whole inference process.
[EcoQoS](https://learn.microsoft.com/en-us/windows/win32/procthread/quality-of-service)
can suit explicitly background work, but applying it to response generation or
audible playback needs target-device latency and power evidence. Current thread
budgets reserve CPU capacity and stay user-configurable; they do not claim an
optimal topology or performance result.

Playback currently uses WinMM waveOut with stable buffer/header ownership,
bounded buffering, stop/reset and unprepare-before-free ordering. Microsoft
[recommends newer audio APIs where possible](https://learn.microsoft.com/en-us/windows/win32/multimedia/waveform-functions).
A WASAPI/AudioGraph migration needs device-change, suspend/resume, cancellation
and latency qualification; replacing this boundary solely to use a newer API
would not establish those properties.

## Permissions and capability isolation

Credentials use system CSPRNG bytes, protected ACLs, noninheritable retained
handles and scrubbed owning buffers. Optional recovery uses user-scoped DPAPI.
Microphone capture requires an explicit user action and Windows `AppCapability`
access checks; the UI polls for revocation and stops capture when observed.
Recording deadlines and revocation handling are cooperative, rather than an
OS-enforced recording quota during UI/process suspension. These controls have
distinct purposes.
An application's model-capability report is not a Windows permission grant, and
a microphone privacy permission does not sandbox the inference process.

**The development inference process still runs with the caller's identity.**
Jobs, numeric-loopback binding, listener PID checks and bearer credentials do
not deny unrelated filesystem access, network egress or same-user backend
bypass. The daily profile remains disabled until the
[daily-service boundary](broker-policy.md)
is qualified. Outstanding implementation and acceptance work stays in
[PLAN.md](../PLAN.md).

The [worker sandbox architecture](sandboxing.md) selects native Microsoft
Execution Containers (MXC) as the integration target and records the NVX,
LiteBox and Windows Sandbox tradeoffs. It owns the version-specific policy,
Job-composition, credential separation and qualification requirements. MXC is
not yet integrated; the supported launch API must first preserve our atomic
Job limits and exact worker identity. AppContainer/LPAC, packaging and broad
loopback grants alone do not establish the required boundary.

Current headless workers receive creation-time Win32k, extension-point and
image-loading mitigations. These reduce attack surface while preserving nested
Jobs; they do not restrict ordinary file or network access. Build tools retain
their inherited policy. See the sandbox architecture for the exact mitigations,
compatibility limits and focused regression evidence.

Focused synthetic regressions establish the repaired path, handle and lock
contracts. Complete suites run in Foundation CI; device performance, real-model
operation, audio hardware behavior and restricted-identity acceptance remain
separate evidence requirements.
