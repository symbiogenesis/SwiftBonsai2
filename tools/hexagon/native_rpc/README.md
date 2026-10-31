# Native Rust FastRPC capability candidate

This std-only Rust boundary queries the installed Qualcomm Windows driver
directly. It has no ggml, GenieX, QNN or C/C++ inference-library dependency. Its
only vendor call is `remote_handle_control(DSPRPC_GET_DSP_INFO, ...)` for domain
3 and eight fixed read-only capability attributes. It cannot submit a kernel,
allocate RPC memory, open a DSP module, enable unsigned processes or alter power.

`lib.rs` provides a safe API around the isolated audited `windows.rs` boundary.
It resolves the fixed `qcnspmcdm` SCM service with query-only rights, derives
`libcdsprpc.dll` inside the Windows DriverStore and rejects path traversal and
reparse ancestors. The Windows directory comes from `GetWindowsDirectoryW`,
rather than an inherited environment variable. A retained file handle denies
writes/deletion through cached, no-UI Authenticode verification, restricted DLL
loading, every completed query and unload. Dependency search is limited to the
driver directory and System32. The session is deliberately neither Send nor
Sync, and no raw export pointer escapes the safe API.

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
