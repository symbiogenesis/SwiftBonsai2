//! Windows credentials created with owner-and-LocalSystem ACLs before any secret
//! bytes are written. Artifacts remain in the selected external storage after
//! shutdown. Same-user processes and administrators can read the credentials;
//! owner-controlled parents remain a requirement, not a process-isolation claim.
#![cfg(windows)]

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};

#[allow(
    unsafe_code,
    reason = "Keep audited Win32 calls in this native boundary."
)]
#[expect(
    clippy::redundant_pub_crate,
    reason = "Explicit parent-only visibility keeps the audited boundary private."
)]
mod native;

#[allow(
    unsafe_code,
    reason = "Audited user-scoped Windows data-protection boundary with owned, scrubbed output buffers."
)]
mod sealed;

pub use sealed::{protect_private_data, unprotect_private_data};

#[allow(
    unsafe_code,
    reason = "Audited CNG SHA-256 boundary owns its hash handle and borrows input only during each call."
)]
mod hash;

pub use hash::Sha256;

/// Queries the actual Windows process token, without trusting an environment flag.
///
/// # Errors
/// Returns an unavailable/malformed token or a failed native query.
pub fn is_app_container() -> io::Result<bool> {
    native::is_app_container()
}

/// Validates an existing regular file for a read-only consumer.
///
/// Inside an actual Windows `AppContainer`, the OS-validated local volume root is
/// not opened or searched for `.git`; PSEC can deny that metadata while granting
/// the exact file. Every non-root ancestor still receives the usual checks.
/// The trusted owner must validate and retain ancestry before granting the file;
/// this helper never grants filesystem access or relaxes creation-path checks.
///
/// # Errors
/// Rejects unsafe paths, unavailable token/drive facts, nonregular files,
/// reparse points, Git ancestry and failed metadata queries.
pub fn validate_read_path(path: &Path) -> io::Result<()> {
    validate_path_inner(path, !is_app_container()?)?;
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.file_attributes() & native::REPARSE_ATTRIBUTE != 0 {
        return Err(invalid("Read access requires an existing regular file."));
    }
    Ok(())
}

/// Fresh owner-and-LocalSystem directory with a retained noninheritable reader.
///
/// The protected inheritable DACL is installed during creation and verified
/// before publication. Storage is retained after drop for evidence inspection.
/// The caller must control the existing parent; this is not same-user isolation.
pub struct PrivateDirectory {
    path: PathBuf,
    _reader: File,
}

impl PrivateDirectory {
    /// Creates new private storage without creating or exposing credentials.
    ///
    /// # Errors
    /// Rejects unsafe/nonexisting parents, creation collisions, unavailable
    /// randomness, unexpected ownership/DACLs and failed native/file operations.
    pub fn create(parent: &Path) -> io::Result<Self> {
        validate_external_path(parent)?;
        if !fs::metadata(parent)?.is_dir() {
            return Err(invalid(
                "Private storage parent must be an existing directory.",
            ));
        }
        let mut nonce = Secret::<16>::new();
        native::random(&mut *nonce.0)?;
        let path = parent.join(format!("SwiftBonsai-Private-{}", hex(&*nonce.0)));
        let security = native::Security::new(true)?;
        native::create_directory(&path, &security)?;
        validate_external_path(&path)?;
        let reader = native::open_reader(&path, true)?;
        security.verify(&reader)?;
        Ok(Self {
            path,
            _reader: reader,
        })
    }

    /// Returns the retained directory's expanded absolute path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// An existing external token with a retained read handle and scrubbed secret memory.
///
/// The handle denies write/delete opens until this value drops. Path checks do
/// not attest the file's ACL or freeze mutable ancestors; the caller must use an
/// owner-controlled parent. This type never creates or modifies the token file.
pub struct RetainedCredential {
    key: Secret<64>,
    _reader: File,
}

impl RetainedCredential {
    /// Read exactly 64 lowercase ASCII hex bytes from a regular external file.
    ///
    /// # Errors
    /// Rejects unsafe paths, reparse points, unavailable files, invalid lengths
    /// or token syntax, and read failures. No error includes the secret bytes.
    pub fn open(path: &Path) -> io::Result<Self> {
        use std::os::windows::fs::OpenOptionsExt as _;

        validate_read_path(path)?;
        let mut reader = fs::OpenOptions::new()
            .read(true)
            .share_mode(1) // FILE_SHARE_READ; deny write and delete sharing.
            .custom_flags(0x0020_0000) // FILE_FLAG_OPEN_REPARSE_POINT.
            .open(path)?;
        let metadata = reader.metadata()?;
        if !metadata.is_file()
            || metadata.len() != 64
            || metadata.file_attributes() & native::REPARSE_ATTRIBUTE != 0
        {
            return Err(invalid("Use a regular 64-byte credential file."));
        }
        let mut key = Secret::<64>::new();
        reader.read_exact(&mut *key.0)?;
        let mut surplus = Secret::<1>::new();
        if reader.read(&mut *surplus.0)? != 0
            || !key
                .0
                .iter()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
        {
            return Err(invalid("Use a lowercase hexadecimal credential."));
        }
        Ok(Self {
            key,
            _reader: reader,
        })
    }

    /// Borrow the token bytes without copying or debug-formatting the secret.
    #[must_use]
    pub fn bytes(&self) -> &[u8; 64] {
        &self.key.0
    }

    /// Borrow the validated token text. Callers must protect any copies.
    #[must_use]
    pub fn as_hex(&self) -> &str {
        self.key.as_hex()
    }
}

/// Independent credentials with retained read handles and scrubbed secret memory.
///
/// Read handles deny write/delete opens for this object's lifetime. Secret bytes
/// are never debug formatted and their owning memory is scrubbed on drop.
///
/// The artifacts are intentionally retained after drop. Their paths must remain
/// outside repositories, under an owner-controlled local parent. ACLs do not
/// prevent access by the same user or an administrator, or replacement through
/// mutable ancestors. The writer-to-reader transition is checked again before
/// publication, but is not a security boundary against a hostile same-user peer.
pub struct PrivateCredentials {
    directory: PathBuf,
    frontend_path: PathBuf,
    backend_path: PathBuf,
    frontend_key: Secret<64>,
    backend_key: Secret<64>,
    _directory_handle: File,
    _frontend_handle: File,
    _backend_handle: File,
}

impl PrivateCredentials {
    /// Creates a fresh randomized private directory and two separate 256-bit
    /// credentials, each encoded as exactly 64 lowercase ASCII hex bytes.
    ///
    /// The directory and files receive protected explicit owner-and-LocalSystem
    /// ACLs atomically during native creation, then have their actual owner and
    /// DACL verified before secret generation/writes. Existing objects are never
    /// reused or overwritten. Failure may leave private partial artifacts.
    ///
    /// # Errors
    /// Returns an error for an unsafe/nonexisting parent, random-source failure,
    /// creation collision, unexpected ACLs, or file I/O failure.
    pub fn create(parent: &Path) -> io::Result<Self> {
        validate_external_path(parent)?;
        if !fs::metadata(parent)?.is_dir() {
            return Err(invalid("Credential parent must be an existing directory."));
        }
        let mut nonce = Secret::<16>::new();
        native::random(&mut *nonce.0)?;
        let directory = parent.join(format!("SwiftBonsai-Keys-{}", hex(&*nonce.0)));
        let security = native::Security::new(true)?;
        native::create_directory(&directory, &security)?;
        validate_external_path(&directory)?;
        let directory_handle = native::open_reader(&directory, true)?;
        security.verify(&directory_handle)?;
        let file_security = native::Security::new(false)?;
        let frontend_path = directory.join("frontend.key");
        let backend_path = directory.join("backend.key");
        let (frontend_key, frontend_handle) = create_key(&frontend_path, &file_security)?;
        let (backend_key, backend_handle) = create_key(&backend_path, &file_security)?;
        if frontend_key.0 == backend_key.0 {
            return Err(io::Error::other(
                "The random source returned duplicate credentials.",
            ));
        }
        Ok(Self {
            directory,
            frontend_path,
            backend_path,
            frontend_key,
            backend_key,
            _directory_handle: directory_handle,
            _frontend_handle: frontend_handle,
            _backend_handle: backend_handle,
        })
    }

    /// Returns the retained private artifact directory.
    #[must_use]
    pub fn directory(&self) -> &Path {
        &self.directory
    }

    /// Returns the frontend credential file path.
    #[must_use]
    pub fn frontend_path(&self) -> &Path {
        &self.frontend_path
    }

    /// Returns the backend credential file path.
    #[must_use]
    pub fn backend_path(&self) -> &Path {
        &self.backend_path
    }

    /// Borrows the frontend secret. Callers must not log or debug-format it.
    #[must_use]
    pub fn frontend_key(&self) -> &str {
        self.frontend_key.as_hex()
    }

    /// Borrows the backend secret. Callers must not log or debug-format it.
    #[must_use]
    pub fn backend_key(&self) -> &str {
        self.backend_key.as_hex()
    }
}

/// Validates expanded absolute local-drive storage outside Git repositories.
///
/// Rejects UNC/device namespaces, ADS, NUL/percent spellings, dot/parent segments,
/// drive roots, reparse ancestors, and any existing ancestor containing `.git`.
/// A new leaf is allowed only under an existing validated directory. The caller
/// must control its parent; validation cannot freeze mutable ancestors.
///
/// # Errors
/// Returns an error for unsupported spelling, a nonlocal drive, unsafe ancestors,
/// missing intermediate directories, or an unexpected filesystem error.
pub fn validate_external_path(path: &Path) -> io::Result<()> {
    validate_path_inner(path, true)
}

fn validate_path_inner(path: &Path, check_root: bool) -> io::Result<()> {
    let spelling = path
        .to_str()
        .ok_or_else(|| invalid("Path must be valid Unicode."))?;
    let normalized = spelling.replace('/', "\\");
    let normalized = normalized.trim_end_matches('\\');
    let bytes = normalized.as_bytes();
    if bytes.len() <= 3
        || !bytes[0].is_ascii_alphabetic()
        || bytes[1] != b':'
        || bytes[2] != b'\\'
        || normalized.contains(['\0', '%'])
        || normalized[2..].contains(':')
    {
        return Err(invalid(
            "Use an expanded absolute local-drive path below the drive root.",
        ));
    }
    let components: Vec<_> = normalized[3..].split('\\').collect();
    if components.iter().any(|part| {
        part.is_empty()
            || *part == "."
            || *part == ".."
            || part.ends_with(['.', ' '])
            || part.chars().any(|character| character <= '\u{1f}')
            || part.contains(['<', '>', '"', '|', '?', '*'])
            || part.eq_ignore_ascii_case(".git")
            || reserved_component(part)
    }) {
        return Err(invalid("Path contains an unsupported component."));
    }
    let mut cursor = PathBuf::from(&normalized[..3]);
    native::validate_local_drive(&cursor)?;
    if check_root {
        check_existing(&cursor)?;
    }
    for (index, part) in components.iter().enumerate() {
        cursor.push(part);
        match check_existing(&cursor) {
            Ok(is_directory) if !is_directory && index + 1 != components.len() => {
                return Err(invalid("Path ancestor must be a directory."));
            }
            Ok(_) => {}
            Err(error)
                if error.kind() == io::ErrorKind::NotFound && index + 1 == components.len() => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn reserved_component(component: &str) -> bool {
    let base = component
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end_matches(' ')
        .to_ascii_uppercase();
    matches!(
        base.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || base
        .strip_prefix("COM")
        .or_else(|| base.strip_prefix("LPT"))
        .is_some_and(|suffix| {
            // Win32 also treats ISO-8859-1 superscript 1, 2 and 3 as
            // device digits, including when followed by an extension.
            (suffix.len() == 1 && matches!(suffix.as_bytes()[0], b'1'..=b'9'))
                || matches!(suffix, "\u{b9}" | "\u{b2}" | "\u{b3}")
        })
}

fn check_existing(path: &Path) -> io::Result<bool> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_attributes() & native::REPARSE_ATTRIBUTE != 0 {
        return Err(invalid("Reparse-point credential storage is unsupported."));
    }
    if metadata.is_dir() {
        match fs::symlink_metadata(path.join(".git")) {
            Ok(_) => {
                return Err(invalid(
                    "Credential storage must be outside Git repositories.",
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(metadata.is_dir())
}

fn create_key(path: &Path, security: &native::Security) -> io::Result<(Secret<64>, File)> {
    // CREATE_NEW and the protected ACL precede generation of this file's secret.
    let mut writer = native::create_file(path, security)?;
    security.verify(&writer)?;
    let mut random = Secret::<32>::new();
    native::random(&mut *random.0)?;
    let mut secret = Secret::<64>::new();
    encode_hex(&random.0, &mut secret.0);
    writer.write_all(&*secret.0)?;
    writer.sync_all()?;
    drop(writer);
    let mut reader = native::open_reader(path, false)?;
    security.verify(&reader)?;
    let mut actual = Secret::<64>::new();
    reader.read_exact(&mut *actual.0)?;
    let mut extra = Secret::<1>::new();
    if actual.0 != secret.0 || reader.read(&mut *extra.0)? != 0 {
        return Err(io::Error::other(
            "Credential content changed before publication.",
        ));
    }
    Ok((secret, reader))
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    bytes
        .iter()
        .flat_map(|byte| {
            [
                char::from(DIGITS[usize::from(byte >> 4)]),
                char::from(DIGITS[usize::from(byte & 15)]),
            ]
        })
        .collect()
}

fn encode_hex(bytes: &[u8; 32], encoded: &mut [u8; 64]) {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    for (byte, output) in bytes.iter().zip(encoded.as_chunks_mut::<2>().0) {
        output[0] = DIGITS[usize::from(byte >> 4)];
        output[1] = DIGITS[usize::from(byte & 15)];
    }
}

// Populate only after allocating: moving an owner then moves a pointer rather
// than leaving earlier copies of a populated secret in stack slots.
struct Secret<const N: usize>(Box<[u8; N]>);

impl<const N: usize> Secret<N> {
    fn new() -> Self {
        Self(Box::new([0; N]))
    }

    fn as_hex(&self) -> &str {
        // Published secrets are encoded or validated ASCII hex, so conversion cannot fail.
        std::str::from_utf8(&*self.0).unwrap_or_default()
    }
}

impl<const N: usize> Drop for Secret<N> {
    fn drop(&mut self) {
        native::scrub(&mut *self.0);
    }
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

#[cfg(test)]
mod tests;
