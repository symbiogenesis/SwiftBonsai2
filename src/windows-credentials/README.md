# Windows credential provisioning

`PrivateCredentials::create(parent)` creates a fresh random child directory in an
existing external local-drive parent. `directory`, `frontend_path`, and
`backend_path` expose paths; `frontend_key` and `backend_key` borrow the two secret
strings. There is deliberately no `Debug` implementation or secret-bearing error.

The native boundary in `src/native.rs` is the only module permitted to use unsafe
Rust. It uses Windows security attributes when calling `CreateDirectoryW` and
`CreateFileW(CREATE_NEW)`, so protected ACLs exist before secrets are generated or
written. Both directory and files explicitly grant full control only to the
process token's user and LocalSystem. The requested owner, protected DACL bit, and
every ACE are compared with the actual object security before secret writes.
Directories grant object/container inheritance; the files have their own explicit
protected ACLs. Creation never opens or overwrites an existing object.

Each credential contains 32 independent bytes from the system-preferred
`BCryptGenRandom` CSPRNG, encoded as exactly 64 lowercase ASCII hexadecimal bytes
without a newline. Files are flushed, then reopened read-only with share-read.
The reopened handles are checked again for owner/DACL, reparse status, and exact
content. Retained noninheritable read handles permit ordinary readers and deny
write/delete opens while the owner lives. The private directory is also retained
without delete sharing. Owning secret buffers use volatile zero stores and a
compiler fence on drop, including random input and readback buffers; callers are
responsible for copies made from borrowed secrets.

`validate_external_path` rejects relative/drive-root paths, UNC/device namespaces,
alternate data streams, NUL/percent spellings, dot/parent segments, reserved DOS
names, trailing dots/spaces, reparse ancestors, missing intermediate directories,
and existing ancestors containing `.git`. Only recognized local disks are
accepted. A new final component under an existing validated directory is allowed.

An owner-controlled parent is required. Validation does not lock every ancestor.
ACLs do not isolate a credential from other processes running as the same user or
from administrators. The writer must close before a share-read reader can open;
the reopen verification detects changed state but does not make that transition
a security boundary against a hostile same-user process. Credentials and private
partial artifacts are deliberately retained on success, failure, and shutdown;
there is no recursive cleanup or deletion of a supplied directory.

Focused Windows tests cover live ACLs and handle sharing plus unsafe path aliases.
Tests use `BONSAI_CREDENTIAL_TEST_ROOT` when supplied, otherwise the external
Windows temporary directory, and retain their small artifact directories.
