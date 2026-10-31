# Native Rust FastRPC capability candidate

This std-only Rust boundary queries the installed Qualcomm Windows driver
directly. It has no ggml, GenieX, QNN or C/C++ inference-library dependency. Its
ordinary vendor call is `remote_handle_control(DSPRPC_GET_DSP_INFO, ...)` for
domain 3 and eight fixed read-only attributes. A separate terminal probe uses
the fixed allocator/registration APIs described below. Neither capability/STATIC path submits a
kernel, opens a custom DSP module, enables unsigned processes or alters power.

`lib.rs` provides a safe API around the isolated audited `windows.rs` boundary.
It resolves the fixed `qcnspmcdm` SCM service with query-only rights, derives
`libcdsprpc.dll` inside the Windows DriverStore and rejects path traversal and
reparse ancestors. The Windows directory comes from `GetWindowsDirectoryW`,
rather than an inherited environment variable. A retained file handle denies
writes/deletion through cached, no-UI Authenticode verification, restricted DLL
loading, every completed query and unload. Dependency search is limited to the
driver directory and System32. The session is deliberately neither Send nor
Sync, and no raw export pointer escapes the safe API.
A process-wide nonblocking mutex lease also permits only one native owner across
all threads and independently created driver objects. Contention or poisoning
rejects immediately. That lease covers unload/file close and remains pinned with
the terminal mapping owner, so an old query cannot race a new mapping session.

Cached Authenticode acceptance is not a publisher pin, fresh revocation check,
installed-kernel acceptance or device qualification. The trusted supervisor must
also bind the resolved path, SHA-256, publisher and source/driver identities to
its reviewed receipt. The Rust code validates trust again on the same retained
file before loading; it supplies no override for a failing signature or export.
No native query has an in-process cancellation guarantee: execute the CLI under
an aggregate process deadline with verified parent-death ownership and teardown.

The separate `query.rs` child emits one bounded JSON record and accepts exactly
one of the following arguments:

- `--self-test`: checks the 12-byte C layout and architecture decoding without
  opening services, files or libraries.
- `--inspect`: returns the service-derived absolute `driverDll` path without
  loading it, for independent signature/hash checks.
- `--query`: verifies/loads that fixed vendor driver and records each query's
  native success/value or failure. Missing values remain null and unknown.
- `--session-prepare`: queries the exact architecture, default-CDSP effective
  domain and fixed bundled module URI. It opens no remote module or session,
  allocates/maps no buffer and requests no unsigned process. Both native return
  codes and every completed stage remain distinct; an unsupported family,
  changed input structure, malformed or mismatched URI never becomes a fallback
  domain or synthesized URI. This metadata gate still needs a supervised device
  observation and does not qualify signed deployment.
- `--session-mapping-probe ABSOLUTE_PACKAGE_DIRECTORY`: consumes one native owner
  for the fixed signed bundled v73/v81 module in default CDSP session zero,
  then a 4096-byte **FD=2** mapping, matching unmap/free and explicit remote handle
  close. It uses the exact validated driver-returned URI and never requests an
  unsigned process. The trusted supervisor must first verify all pinned package
  digests/current catalog membership and independently guard the driver. Its child
  `ADSP_LIBRARY_PATH` must equal that one directory exactly; all other inherited
  vendor overrides remain rejected. The library retains nonreparse module,
  catalog and INF files through the entire attempt. No DSP method or compute is
  invoked. Selected package/URI and successful open do not independently prove
  the exact DSP file actually loaded; that identity gate remains pending.
- `--mapping-probe`: consumes one driver/process admission for a fixed 4096-byte
  system-heap allocation, descriptor lookup, CPU byte check, static domain-3
  registration, matching unmap and free. It never submits or reads DSP work.

Before driver loading, the query/mapping CLI rejects inherited names beginning
with `ADSP_`, `CDSP_`, `FASTRPC_`, `RPCMEM_`, `GGML_`, `GENIEX_`, `QNN_`,
`QAIRT_` or `HEXAGON_`, regardless of case. Only the new session diagnostic permits its
exact explicitly selected `ADSP_LIBRARY_PATH`. It logs no names/values and mutates
no environment. The trusted library caller and supervisor own the corresponding
child-environment policy; these restrictions do not change the parent process.

The terminal probe uses heap ID 25, cached allocation flags 1, offset 0, length
4096 and `FASTRPC_MAP_STATIC=0`. The public vendor header documents that mapping
may implicitly open a domain session. There is no explicit remote-handle,
module-open or unsigned-process call. A successful host allocation proves no
registration; registration proves no DSP visibility, DMA64, DMA or compute.
The report keeps all returned codes and completed steps separate. Mapping and
unmapping failures pin the uncertain allocation, DLL and deny-mutation file
guard until the supervising process dies. No native free or DLL unload follows
uncertain use. Every later driver open/query/probe in that process rejects.
Normal completion unregisters and frees the scratch, but every mapping attempt
retains its DLL/file owner until supervised process death. Complete implicit-session
teardown and failure recovery remain unqualified.

This probe's **STATIC flag 0** is a deliberately narrow control. The pinned
ggml host uses `FASTRPC_MAP_FD` (2) for pinned buffers and `FASTRPC_MAP_FD_DELAYED`
(3), or a matching-SDK extended mode, for ordinary buffers inside its established
HTP session. A STATIC refusal does not rule out those paths, DMA64/VTCM access or
Hexagon execution. The probe never retries with another flag or opens a custom
module to force admission. The referenced public
[`AEEStdErr.h`](https://github.com/qualcomm/fastrpc/blob/a56e9d4de3614a8d8f4b0e15ff561350c4b686e3/inc/AEEStdErr.h#L92)
defines host `AEE_ERPC` as `0x200` (512), a generic FastRPC implementation error.
A matching return value supplies no underlying permission, signature, descriptor
or hardware diagnosis; retain the exact code and mode before investigating.

## Established-session mapping prerequisite

`session.rs` and `session_native.rs` now encode the next narrow native step:
`FASTRPC_GET_EFFECTIVE_DOMAIN_ID=14` and `FASTRPC_GET_URI=15` for `cdsp`, session
zero, using the exact 24-byte and 48-byte Windows ARM64 structures in the pinned
vendor header. Architecture comes from the driver, not the processor name. The
returned URI must match the exact v73/v81 module, `htp_iface_skel_handle_invoke`,
module version 1.0, domain and session. Inputs, pointer fields, extents and the
NUL-terminated output are checked after each call. No vendor-controlled string
is loaded or printed. All source identities remain those in `sources.json`.

This deliberately stops before `remote_handle64_open`. A module basename URI
does not bind the installed ELF bytes, catalog membership or trust to the
owner's checked package directory. Supplying an absolute URI has not been
established as a supported Windows deployment route. The exact installed module,
its catalog and the supported search/deployment route must be independently
verified and held against mutation through open and teardown before adding that
call. No environment search-path override, catalog installation or certificate
change is supplied by this candidate.

The reviewed ggml session initializer requests unsigned-process control before
opening its HTP interface. That request is omitted here; neither its presence in
source nor the bundled catalog proves that Runner may enable it. A future signed
session must establish its actual process/deployment policy independently.

After a verified session is established, the matching effective domain must own
every mapping and unmapping tuple. Public FD mode 2 and delayed-FD mode 3 are
recorded as constants, not executed by metadata preparation. The public pinned
header describes delayed mode as registration until DSP-side `HAP_mmap`; its
success alone does not prove DSP visibility or DMA. CPU/DSP cache maintenance
also remains a separate ownership obligation. The ggml extended delayed flag
and its SDK ABI are absent from this pinned public header and remain pending;
no guessed enum value is exposed. Actual asynchronous submission, completion,
cancellation and in-use buffer retention require the signed kernel/queue ABI
and cannot be established by a metadata or map/unmap control.

The architecture decoder follows the pinned ggml low-byte packed-decimal
interpretation, but rejects invalid nibbles and zero; raw values remain in the
receipt. HVX/HMX values and VTCM page/count queries do not measure throughput or
available allocation. Every report keeps `profileEligible: false`. DSP dispatch,
PQ2/PTQ1 arithmetic, DMA64, mapping, state, lifecycle and performance remain
pending, independently for X1 and X2.

Hosted CI can build without acquiring a vendor SDK:

```powershell
rustup run nightly-2026-10-22-aarch64-pc-windows-msvc rustc --edition=2024 --crate-type=lib --crate-name=hexagon_native_rpc -D warnings -O tools/hexagon/native_rpc/lib.rs --out-dir ABSOLUTE_EXTERNAL_BUILD
rustup run nightly-2026-10-22-aarch64-pc-windows-msvc rustc --edition=2024 -D warnings -O tools/hexagon/native_rpc/query.rs --extern hexagon_native_rpc=ABSOLUTE_EXTERNAL_BUILD/libhexagon_native_rpc.rlib -o ABSOLUTE_EXTERNAL_BUILD/hexagon-native-query.exe
```

Full regressions are hosted-CI only. Every owner
diagnostic's setup, loading, queries and cleanup share the existing single
five-minute aggregate budget. This is a transfer candidate for Prism's audited
device boundary, not a Runner production engine or a backend-availability claim.

The immutable source identities, hashes and Microsoft ABI references are in
[`sources.json`](sources.json). The code is an independently written narrow
binding; no vendor or ggml implementation is vendored.

## Terminal signed-session FD control

`session_mapping.rs` retains independent metadata, module open, handle, mapping,
close and lifetime fields. `session_mapping_native.rs` uses only the pinned
public vendor ABI. In particular `remote_handle64_close` takes a **u64** handle
as declared by the authoritative public `remote.h`; the reviewed reference host
wrapper contains an inconsistent 32-bit close-pointer typedef that is not reused.
The same process-wide terminal admission covers both STATIC and session probes.
After any remote-open attempt the driver, package guards and lease stay pinned
until supervised process death, including after a successful explicit handle
close. A refused open or missing handle cannot become a mapped session. Failed
map/unmap retains the scratch and module handle; no free or close follows uncertain
use. Process death is not a demonstrated vendor session teardown.

The CLI flushes fixed stderr `hexagon-session:stage=...` markers before native
open, allocation, descriptor, FD mapping/unmapping, free and close. A marker
records **entry**, not completion; only returned receipt codes establish completed
operations. Model-free mock controls check exact URI/map arguments, full success,
open/handle/allocation/descriptor/map/unmap/close refusal and retained uncertain
lifetimes. They do not load a driver. Actual signed-session access, selected DSP
byte binding, DMA64, effective VTCM, compute and Swift arithmetic remain separate
device gates. Build and complete controls stay hosted-CI only.
## Explicit reference process initialization

The separate `--reference-session-mapping-probe ABSOLUTE_PACKAGE_DIRECTORY` and
`Driver::reference_session_mapping_probe` select one fixed additional native step.
The original `--session-mapping-probe` still requests no unsigned process and its
retained observation remains independent. There is no automatic retry between them.
The reference mode requests an **unsigned DSP process class**, even though only
the exact Windows catalog-signed package is eligible for this diagnostic. It must
not be described as signed-PD-only access or authorization for arbitrary DSP code.

The [pinned public header](https://github.com/qualcomm/fastrpc/blob/a56e9d4de3614a8d8f4b0e15ff561350c4b686e3/inc/remote.h#L603-L622)
defines `DSPRPC_CONTROL_UNSIGNED_MODULE=2`; its two-int structure at lines389-392
has `domain` and `enable`. The fixed control uses enable1 and the checked effective
default-CDSP domain. It occurs after the effective-domain request, before GET_URI
and module open, matching the [bundled reference initializer](https://github.com/ggml-org/llama.cpp/blob/94256114c229674ef96e76eb2dea596e65b43818/ggml/src/ggml-hexagon/ggml-hexagon.cpp#L3875-L3912).
That source unconditionally selects the unsigned PD, including Windows builds.
This difference does not establish why an earlier module open was refused.

The public [Linux implementation](https://github.com/qualcomm/fastrpc/blob/a56e9d4de3614a8d8f4b0e15ff561350c4b686e3/src/fastrpc_apps_user.c#L2434-L2454)
changes a process-local per-domain attribute before opening a session; zero selects
signed and nonzero unsigned. Once open, changing its class rejects. The array is
zero-initialized at line4116. Lines2596-2616 dispatch this request; lines3238-3244
use its flag when constructing process attributes and lines3688/3735 use those
attributes for shell/session initialization. This public Linux behavior does not
prove the exact closed Windows driver implementation or complete remote teardown.
The source manifest records its exact digest and that limitation.

Windows package catalog signing is a separate requirement. The
[pinned GenieX run guide](https://github.com/qualcomm/GenieX/blob/a5c8e7fb4a364ddcccefbf187afd1bac52d88de5/notes/run.md#L303-L342)
requires no extra setup for its Microsoft-signed SDK, while its self-signed fallback
requires certificate trust and test-signing changes. The reference diagnostic makes
none of those OS, certificate or boot-policy changes and continues to require
independent exact package/catalog checks. It still explicitly records
`unsignedProcessRequested=true` only after its fixed runtime request is invoked.

Driver, package guards and process lease become terminally pinned **before** that
initialization attempt. Its exact return and unchanged domain/enable inputs must
pass before GET_URI/module open. Refusal, changed input or later incomplete URI
returns a separate stage with no weaker retry. The flushed
`hexagon-session:stage=reference-process-init-entered` marker establishes entry only.
The receipt records explicit mode selection, actual request/result and fixed request2,
size8/enable1 independently. Tests cover ordering, refusal, changed inputs and retained
ownership; hosted builds and independent device observations remain required.