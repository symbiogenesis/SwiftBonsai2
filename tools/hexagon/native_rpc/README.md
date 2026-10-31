# Native Rust FastRPC capability candidate

This std-only Rust boundary queries the installed Qualcomm Windows driver
directly. It has no ggml, GenieX, QNN or C/C++ inference-library dependency. Its
ordinary vendor call is `remote_handle_control(DSPRPC_GET_DSP_INFO, ...)` for
domain 3 and eight fixed read-only attributes. A separate terminal probe uses
the fixed allocator/registration APIs described below. Neither path submits a
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
- `--mapping-probe`: consumes one driver/process admission for a fixed 4096-byte
  system-heap allocation, descriptor lookup, CPU byte check, static domain-3
  registration, matching unmap and free. It never submits or reads DSP work.

Before driver loading, the query/mapping CLI rejects inherited names beginning
with `ADSP_`, `CDSP_`, `FASTRPC_`, `RPCMEM_`, `GGML_`, `GENIEX_`, `QNN_`,
`QAIRT_` or `HEXAGON_`, regardless of case. It logs no names/values and mutates
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
