//! Windows credentials created with owner-and-LocalSystem ACLs before any secret
//! bytes are written. Artifacts remain in the selected external storage after
//! shutdown. Same-user processes and administrators can read the credentials;
//! owner-controlled parents remain a requirement, not a process-isolation claim.
#![cfg(windows)]

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};

#[allow(unsafe_code)]
mod native;

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
        native::random(&mut nonce.0)?;
        let directory = parent.join(format!("SwiftBonsai-Keys-{}", hex(&nonce.0)));
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
    let spelling = path
        .to_str()
        .ok_or_else(|| invalid("Path must be valid Unicode."))?;
    let normalized = spelling.replace('/', "\\");
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
            || part.contains(['<', '>', '"', '|', '?', '*'])
            || part.eq_ignore_ascii_case(".git")
            || reserved_component(part)
    }) {
        return Err(invalid("Path contains an unsupported component."));
    }
    let mut cursor = PathBuf::from(&normalized[..3]);
    native::validate_local_drive(&cursor)?;
    check_existing(&cursor)?;
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
    let base = component.split('.').next().unwrap_or_default();
    let bytes = base.as_bytes();
    matches!(
        base.to_ascii_uppercase().as_str(),
        "CON" | "PRN" | "AUX" | "NUL"
    ) || (base.len() == 4
        && (bytes[..3].eq_ignore_ascii_case(b"COM") || bytes[..3].eq_ignore_ascii_case(b"LPT"))
        && matches!(bytes[3], b'1'..=b'9'))
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
    native::random(&mut random.0)?;
    let mut secret = Secret::<64>::new();
    encode_hex(&random.0, &mut secret.0);
    writer.write_all(&secret.0)?;
    writer.sync_all()?;
    drop(writer);
    let mut reader = native::open_reader(path, false)?;
    security.verify(&reader)?;
    let mut actual = Secret::<64>::new();
    reader.read_exact(&mut actual.0)?;
    let mut extra = Secret::<1>::new();
    if actual.0 != secret.0 || reader.read(&mut extra.0)? != 0 {
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

struct Secret<const N: usize>([u8; N]);

impl<const N: usize> Secret<N> {
    const fn new() -> Self {
        Self([0; N])
    }

    fn as_hex(&self) -> &str {
        // Only encode_hex populates a published secret, so conversion cannot fail.
        std::str::from_utf8(&self.0).unwrap_or_default()
    }
}

impl<const N: usize> Drop for Secret<N> {
    fn drop(&mut self) {
        native::scrub(&mut self.0);
    }
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

#[cfg(test)]
mod tests;
