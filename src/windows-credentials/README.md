# Windows credential provisioning

`PrivateCredentials::create(parent)` creates a random child directory under an
existing external local-drive parent. Accessors expose directory/key paths and
borrow the two secret strings. The type has no `Debug` implementation or
secret-bearing errors.

`RetainedCredential::open(path)` reads an existing regular external token file
containing exactly 64 lowercase ASCII hex bytes. Speech clients and companions
share this reader. It retains a noninheritable read handle that denies write and
delete opens, and uses the same scrubbed secret storage as newly created keys.
It does not attest the existing file's ACL or freeze mutable ancestors.

Only the audited `src/native.rs` and `src/sealed.rs` boundaries may use unsafe Rust. `CreateDirectoryW` and
`CreateFileW(CREATE_NEW)` establish protected ACLs at creation, granting full
control only to the process user and LocalSystem. Directory ACLs allow
object/container inheritance; files have explicit protected ACLs. Actual owner,
protected DACL and every ACE are checked before secret writes. Existing objects
are never opened or overwritten for creation.

Each key uses 32 independent system-preferred `BCryptGenRandom` bytes, encoded as
exactly 64 lowercase ASCII hex bytes without a newline. Files are flushed then
reopened read-only/share-read; owner/DACL, reparse status and exact content are
checked again. Retained noninheritable handles deny write/delete opens; the
directory handle also denies delete sharing. Owning secret/random/readback
buffers use volatile zero stores and a compiler fence on drop. Callers must
protect copies made from borrowed secrets.

`validate_external_path` accepts expanded absolute local-drive paths outside Git.
It rejects drive roots, UNC/device namespaces, alternate data streams,
NUL/percent spellings, dot/parent segments, reserved DOS names, trailing dots/spaces,
reparse ancestors and missing intermediate directories. A new final component is
allowed under an existing validated directory.

The caller must control the parent. Validation does not lock every ancestor;
ACLs do not isolate same-user processes or administrators. Reopen verification
detects changes across the required writer-to-reader transition but does not make
it safe against hostile same-user interference. Credentials and partial artifacts
remain retained after success, failure or shutdown; no recursive cleanup occurs.

Focused Windows regressions cover live ACLs, handle sharing and unsafe aliases.
They use `BONSAI_CREDENTIAL_TEST_ROOT` when supplied, otherwise external Windows
temp, and retain their small evidence directories. See [validation](../../docs/validation.md)
for local/CI limits.

`protect_private_data` / `unprotect_private_data` support opt-in conversation
recovery through user-scoped Windows DPAPI with fixed application-purpose entropy,
no interactive prompts and an 8 MiB plaintext bound. Windows authenticates the
encrypted data; corrupted or inaccessible data fails closed. Native output
allocations are scrubbed before `LocalFree`. These APIs do not write files or
provide isolation against another process running as the same Windows user.
