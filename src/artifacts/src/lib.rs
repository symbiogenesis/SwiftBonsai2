//! Offline, bounded artifact verification and archive staging for maintenance tools.
//!
//! Retain verification guards until the child has exited. On Windows, their read
//! handles deny file writes and deletion while the verified artifacts are in use.
//! Verification never downloads, writes, or executes code. Staging writes
//! only validated archive entries and never executes code.
#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
#[cfg(any(test, not(windows)))]
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom},
    path::{Component, Path, PathBuf},
};
use zip::ZipArchive;

const MAX_EXPANDED_BYTES: u64 = 512 * 1024 * 1024;
const MAX_ENTRIES: usize = 4096;
const HASH_BUFFER_BYTES: usize = 64 * 1024;

mod install;
pub use install::stage_runtime;

/// Value-free artifact rejection; no filesystem contents enter diagnostics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArtifactError {
    /// The platform or compiled policy cannot support this verification.
    Configuration,
    /// A path redirects, enters Git storage, or cannot be opened safely.
    Path,
    /// An artifact differs from its exact locked length or SHA-256.
    Identity,
    /// ZIP metadata, names, expansion, or entries are unsupported.
    Archive,
    /// The installation has missing, unexpected or changed entries.
    Installation,
    /// A native executable or DLL is not an ARM64 PE image.
    Architecture,
    /// The caller cancelled verification at a checkpoint.
    Cancelled,
}
impl fmt::Display for ArtifactError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Configuration => {
                "Artifact verification requires the reviewed Windows configuration."
            }
            Self::Path => "Artifact storage could not be opened safely.",
            Self::Identity => "Artifact length or SHA-256 differs from the compiled lock.",
            Self::Archive => "Unsupported or unsafe runtime ZIP archive.",
            Self::Installation => "Installed runtime differs from the verified archive.",
            Self::Architecture => "Expected a native ARM64 PE image.",
            Self::Cancelled => "Artifact verification was cancelled.",
        })
    }
}
impl std::error::Error for ArtifactError {}

/// Runtime identity and installation selected by a caller's reviewed policy.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeSpec {
    /// Whole runtime ZIP path held stable for the guard's lifetime.
    pub archive_path: PathBuf,
    /// Exact directory whose contents must match the verified ZIP.
    pub runtime_directory: PathBuf,
    /// Locked whole ZIP length in bytes.
    pub archive_bytes: u64,
    /// Locked lowercase whole ZIP SHA-256.
    pub archive_sha256: String,
}

/// Identity of one installed file established by verification or installation.
/// A runtime verification guard retains file ownership; installation receipts do not.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedRuntimeFile {
    path: PathBuf,
    bytes: u64,
    sha256: String,
}
impl VerifiedRuntimeFile {
    /// Exact installed file path recorded by verification or installation.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
    /// Verified installed file length.
    #[must_use]
    pub const fn bytes(&self) -> u64 {
        self.bytes
    }
    /// SHA-256 of the matching ZIP entry and installed file.
    #[must_use]
    pub fn sha256(&self) -> &str {
        &self.sha256
    }
}

/// Verified installed runtime and read handles that keep its files stable.
/// Retain this guard until every child using the runtime has exited.
pub struct VerifiedRuntime {
    executable: PathBuf,
    runtime_directory: PathBuf,
    archive_path: PathBuf,
    files: Vec<VerifiedRuntimeFile>,
    _handles: Vec<File>,
}
impl VerifiedRuntime {
    /// Verify the whole ZIP and installed files, including native ARM64 headers.
    ///
    /// # Errors
    /// Rejects unsupported platforms, unsafe paths, identity drift, unsafe ZIPs,
    /// and installations that differ from their verified archive.
    pub fn verify(spec: &RuntimeSpec) -> Result<Self, ArtifactError> {
        Self::verify_with(spec, || Ok(()))
    }

    /// Verify a runtime while checking caller cancellation between bounded reads.
    /// The checkpoint also runs between archive and installation entries.
    ///
    /// # Errors
    /// Returns artifact rejection or the checkpoint's cancellation error.
    pub fn verify_with(
        spec: &RuntimeSpec,
        mut checkpoint: impl FnMut() -> Result<(), ArtifactError>,
    ) -> Result<Self, ArtifactError> {
        checkpoint()?;
        if !cfg!(windows) {
            return Err(ArtifactError::Configuration);
        }
        let mut handles = Vec::new();
        hold_ancestors(&spec.archive_path, &mut handles, &mut checkpoint)?;
        hold_ancestors(
            &spec.runtime_directory.join("llama-server.exe"),
            &mut handles,
            &mut checkpoint,
        )?;
        let mut archive = open_locked_file(&spec.archive_path)?;
        let mut buffer = vec![0; HASH_BUFFER_BYTES];
        check_locked(
            &mut archive,
            spec.archive_bytes,
            &spec.archive_sha256,
            &mut buffer,
            &mut checkpoint,
        )?;
        let mut zip = ZipArchive::new(archive.try_clone().map_err(|_| ArtifactError::Archive)?)
            .map_err(|_| ArtifactError::Archive)?;
        let (executable, files) = verify_installation(
            &mut zip,
            &spec.runtime_directory,
            &mut handles,
            &mut buffer,
            &mut checkpoint,
        )?;
        handles.push(archive);
        Ok(Self {
            executable,
            runtime_directory: spec.runtime_directory.clone(),
            archive_path: spec.archive_path.clone(),
            files,
            _handles: handles,
        })
    }

    /// Server path established by the verified archive.
    #[must_use]
    pub fn executable(&self) -> &Path {
        &self.executable
    }
    /// Complete installed runtime directory.
    #[must_use]
    pub fn runtime_directory(&self) -> &Path {
        &self.runtime_directory
    }
    /// Locked runtime ZIP path.
    #[must_use]
    pub fn archive_path(&self) -> &Path {
        &self.archive_path
    }
    /// Whether this exact installed file path is held by the guard.
    /// Archive and directory paths are not installed files.
    #[must_use]
    pub fn contains_file(&self, path: &Path) -> bool {
        self.files.iter().any(|file| file.path == path)
    }
    /// Verified installed file identities, ordered by archive path ignoring case.
    #[must_use]
    pub fn files(&self) -> &[VerifiedRuntimeFile] {
        &self.files
    }
}

/// A whole locked file and read handles that keep it and its ancestors stable.
/// Retain this guard until every child reading the file has exited.
pub struct VerifiedFile {
    path: PathBuf,
    _handles: Vec<File>,
}
impl VerifiedFile {
    /// Verify a file's exact byte length and lowercase SHA-256.
    ///
    /// # Errors
    /// Rejects unsupported platforms, unsafe paths, and identity drift.
    pub fn verify(path: &Path, bytes: u64, hash: &str) -> Result<Self, ArtifactError> {
        Self::verify_with(path, bytes, hash, || Ok(()))
    }

    /// Verify a file while checking caller cancellation between bounded reads.
    ///
    /// # Errors
    /// Returns artifact rejection or the checkpoint's cancellation error.
    pub fn verify_with(
        path: &Path,
        bytes: u64,
        hash: &str,
        mut checkpoint: impl FnMut() -> Result<(), ArtifactError>,
    ) -> Result<Self, ArtifactError> {
        checkpoint()?;
        if !cfg!(windows) {
            return Err(ArtifactError::Configuration);
        }
        let mut handles = Vec::new();
        hold_ancestors(path, &mut handles, &mut checkpoint)?;
        let mut file = open_locked_file(path)?;
        let mut buffer = vec![0; HASH_BUFFER_BYTES];
        check_locked(&mut file, bytes, hash, &mut buffer, &mut checkpoint)?;
        handles.push(file);
        Ok(Self {
            path: path.to_owned(),
            _handles: handles,
        })
    }

    /// Exact file path held stable by this guard.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

fn classify_hash_error(error: ArtifactError, fallback: ArtifactError) -> ArtifactError {
    if error == ArtifactError::Cancelled {
        error
    } else {
        fallback
    }
}
/// Open an ordinary external file for reading without following a leaf redirect.
///
/// On Windows the returned handle permits only other readers, preventing writes
/// and deletion of the opened file. This does not retain ancestor directory
/// handles; use [`VerifiedFile`] when the complete path must remain stable.
///
/// # Errors
/// Rejects unsafe external paths, redirects, non-files and failed opens.
pub fn open_locked_file(path: &Path) -> Result<File, ArtifactError> {
    validate_read_path(path)?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_SHARE_READ only; OPEN_REPARSE_POINT prevents following a leaf redirect.
        options.share_mode(1).custom_flags(0x0020_0000);
    }
    let file = options.open(path).map_err(|_| ArtifactError::Path)?;
    let metadata = file.metadata().map_err(|_| ArtifactError::Path)?;
    if !metadata.is_file() || is_redirect(&metadata) {
        return Err(ArtifactError::Path);
    }
    Ok(file)
}

fn hold_ancestors(
    path: &Path,
    handles: &mut Vec<File>,
    checkpoint: &mut impl FnMut() -> Result<(), ArtifactError>,
) -> Result<(), ArtifactError> {
    checkpoint()?;
    validate_read_path(path)?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        let contained =
            bonsai_windows_credentials::is_app_container().map_err(|_| ArtifactError::Path)?;
        for ancestor in path.ancestors().skip(1) {
            checkpoint()?;
            // The host owner retains its root guard. A contained reader lacks
            // root metadata rights; granting the volume root would broaden its
            // file authority. Retain every nonroot ancestor without changing
            // the ordinary host path/installation policy.
            if contained && ancestor.parent().is_none() {
                continue;
            }
            let directory = OpenOptions::new()
                .read(true)
                .access_mode(0x80)
                .share_mode(1)
                .custom_flags(0x0220_0000)
                .open(ancestor)
                .map_err(|_| ArtifactError::Path)?;
            let metadata = directory.metadata().map_err(|_| ArtifactError::Path)?;
            if !metadata.is_dir() || is_redirect(&metadata) {
                return Err(ArtifactError::Path);
            }
            handles.push(directory);
        }
    }
    #[cfg(not(windows))]
    {
        let _ = handles;
    }
    Ok(())
}

fn validate_read_path(path: &Path) -> Result<(), ArtifactError> {
    #[cfg(windows)]
    if bonsai_windows_credentials::is_app_container().map_err(|_| ArtifactError::Path)? {
        return bonsai_windows_credentials::validate_read_path(path)
            .map_err(|_| ArtifactError::Path);
    }
    validate_external_path(path).map_err(|_| ArtifactError::Path)
}

fn check_locked(
    file: &mut File,
    bytes: u64,
    hash: &str,
    buffer: &mut [u8],
    checkpoint: &mut impl FnMut() -> Result<(), ArtifactError>,
) -> Result<(), ArtifactError> {
    checkpoint()?;
    if file.metadata().map_err(|_| ArtifactError::Identity)?.len() != bytes {
        return Err(ArtifactError::Identity);
    }
    let digest = hash_reader(file, bytes, buffer, checkpoint)?;
    if digest != hash {
        return Err(ArtifactError::Identity);
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|_| ArtifactError::Identity)?;
    Ok(())
}

fn digest_hex(digest: [u8; 32]) -> String {
    digest
        .into_iter()
        .flat_map(|byte| [byte >> 4, byte & 0x0f])
        .map(|nibble| char::from(b"0123456789abcdef"[usize::from(nibble)]))
        .collect()
}
/// Hash exactly `expected` bytes from the reader's current position through EOF.
///
/// The caller supplies a reusable, nonempty read buffer and a cancellation
/// checkpoint invoked before every read. The lowercase SHA-256 covers the exact
/// bytes read, without text decoding or newline normalization. This function
/// neither rewinds nor locks the reader.
///
/// # Errors
/// Rejects an empty buffer, unavailable native hashing, read or hash failures,
/// short or long streams, byte-count overflow, and checkpoint errors.
pub fn hash_reader(
    reader: &mut impl Read,
    expected: u64,
    buffer: &mut [u8],
    checkpoint: &mut impl FnMut() -> Result<(), ArtifactError>,
) -> Result<String, ArtifactError> {
    if buffer.is_empty() {
        return Err(ArtifactError::Configuration);
    }
    // RustCrypto's ARM64 runtime feature detection is unavailable on Windows;
    // CNG dispatches to the OS-supported implementation without requiring a
    // global CPU target flag or skipping any artifact bytes.
    #[cfg(windows)]
    let mut hash =
        bonsai_windows_credentials::Sha256::new().map_err(|_| ArtifactError::Configuration)?;
    #[cfg(not(windows))]
    let mut hash = Sha256::new();
    let mut total = 0_u64;
    loop {
        checkpoint()?;
        let count = reader.read(buffer).map_err(|_| ArtifactError::Identity)?;
        if count == 0 {
            break;
        }
        total = total
            .checked_add(u64::try_from(count).map_err(|_| ArtifactError::Identity)?)
            .ok_or(ArtifactError::Identity)?;
        if total > expected {
            return Err(ArtifactError::Identity);
        }
        #[cfg(windows)]
        hash.update(&buffer[..count])
            .map_err(|_| ArtifactError::Identity)?;
        #[cfg(not(windows))]
        hash.update(&buffer[..count]);
    }
    if total != expected {
        return Err(ArtifactError::Identity);
    }
    #[cfg(windows)]
    let digest = hash.finish().map_err(|_| ArtifactError::Identity)?;
    #[cfg(not(windows))]
    let digest = hash.finalize().into();
    Ok(digest_hex(digest))
}

struct Entry {
    index: usize,
    name: String,
    bytes: u64,
}
struct Installation {
    executable: PathBuf,
    files: BTreeMap<String, Entry>,
    directories: BTreeSet<String>,
}

fn inspect_archive<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    root: &Path,
    checkpoint: &mut impl FnMut() -> Result<(), ArtifactError>,
) -> Result<Installation, ArtifactError> {
    if archive.is_empty() || archive.len() > MAX_ENTRIES {
        return Err(ArtifactError::Archive);
    }
    let mut names = BTreeSet::new();
    let mut expected_files = BTreeMap::new();
    let mut expected_directories = BTreeSet::new();
    let mut total = 0_u64;
    let mut server = None;
    for index in 0..archive.len() {
        checkpoint()?;
        let entry = archive
            .by_index(index)
            .map_err(|_| ArtifactError::Archive)?;
        let name = archive_name(
            &entry.name().map_err(|_| ArtifactError::Archive)?,
            entry.is_dir(),
        )?;
        if !names.insert(name.to_ascii_lowercase()) {
            return Err(ArtifactError::Archive);
        }
        if let Some(mode) = entry.unix_mode() {
            let kind = mode & 0o170_000;
            if kind != 0 && kind != if entry.is_dir() { 0o040_000 } else { 0o100_000 } {
                return Err(ArtifactError::Archive);
            }
        }
        if entry.is_dir() {
            expected_directories.insert(name.to_ascii_lowercase());
        } else {
            total = total
                .checked_add(entry.size())
                .ok_or(ArtifactError::Archive)?;
            if total > MAX_EXPANDED_BYTES {
                return Err(ArtifactError::Archive);
            }
            if name
                .rsplit('/')
                .next()
                .is_some_and(|leaf| leaf.eq_ignore_ascii_case("llama-server.exe"))
            {
                if server.is_some() {
                    return Err(ArtifactError::Archive);
                }
                server = Some(root.join(&name));
            }
            expected_files.insert(
                name.to_ascii_lowercase(),
                Entry {
                    index,
                    name: name.clone(),
                    bytes: entry.size(),
                },
            );
        }
        let mut parent = Path::new(&name).parent();
        while let Some(path) = parent {
            if !path.as_os_str().is_empty() {
                expected_directories.insert(
                    path.to_string_lossy()
                        .replace('\\', "/")
                        .to_ascii_lowercase(),
                );
            }
            parent = path.parent();
        }
    }
    if expected_files
        .keys()
        .any(|file| expected_directories.contains(file))
    {
        return Err(ArtifactError::Archive);
    }
    let server = server.ok_or(ArtifactError::Archive)?;
    Ok(Installation {
        executable: server,
        files: expected_files,
        directories: expected_directories,
    })
}

fn verify_installation<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    root: &Path,
    handles: &mut Vec<File>,
    buffer: &mut [u8],
    checkpoint: &mut impl FnMut() -> Result<(), ArtifactError>,
) -> Result<(PathBuf, Vec<VerifiedRuntimeFile>), ArtifactError> {
    let installation = inspect_archive(archive, root, checkpoint)?;
    verify_installed_files(archive, root, installation, handles, buffer, checkpoint)
}

fn verify_installed_files<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    root: &Path,
    installation: Installation,
    handles: &mut Vec<File>,
    buffer: &mut [u8],
    checkpoint: &mut impl FnMut() -> Result<(), ArtifactError>,
) -> Result<(PathBuf, Vec<VerifiedRuntimeFile>), ArtifactError> {
    let Installation {
        executable: server,
        files: expected_files,
        directories: expected_directories,
    } = installation;
    let mut installed_files = BTreeSet::new();
    let mut installed_directories = BTreeSet::new();
    enumerate_installation(
        root,
        root,
        &mut installed_files,
        &mut installed_directories,
        checkpoint,
    )?;
    // Both collections iterate sorted, unique names; compare them directly
    // instead of cloning every expected path into a second tree.
    if !installed_files.iter().eq(expected_files.keys())
        || installed_directories != expected_directories
    {
        return Err(ArtifactError::Installation);
    }
    let mut files = Vec::with_capacity(expected_files.len());
    for entry in expected_files.values() {
        checkpoint()?;
        let path = root.join(&entry.name);
        hold_ancestors(&path, handles, checkpoint)?;
        let mut installed = open_locked_file(&path)?;
        let mut source = archive
            .by_index(entry.index)
            .map_err(|_| ArtifactError::Archive)?;
        let expected_hash = hash_reader(&mut source, entry.bytes, buffer, checkpoint)
            .map_err(|error| classify_hash_error(error, ArtifactError::Archive))?;
        check_locked(
            &mut installed,
            entry.bytes,
            &expected_hash,
            buffer,
            checkpoint,
        )
        .map_err(|error| classify_hash_error(error, ArtifactError::Installation))?;
        if path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("exe") || ext.eq_ignore_ascii_case("dll"))
        {
            check_arm64(&mut installed)?;
        }
        files.push(VerifiedRuntimeFile {
            path,
            bytes: entry.bytes,
            sha256: expected_hash,
        });
        handles.push(installed);
    }
    Ok((server, files))
}

fn archive_name(raw: &str, directory: bool) -> Result<String, ArtifactError> {
    let name = if directory {
        raw.strip_suffix('/').ok_or(ArtifactError::Archive)?
    } else {
        raw
    };
    if name.is_empty()
        || name.len() > 240
        || name.starts_with('/')
        || name.contains('\\')
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._ /-".contains(&b))
    {
        return Err(ArtifactError::Archive);
    }
    for part in name.split('/') {
        let stem = part
            .split('.')
            .next()
            .unwrap_or_default()
            .trim_end_matches(' ')
            .to_ascii_uppercase();
        if part.is_empty()
            || part == "."
            || part == ".."
            || part.ends_with('.')
            || part.ends_with(' ')
            || ["CON", "PRN", "AUX", "NUL"].contains(&stem.as_str())
            || ((stem.starts_with("COM") || stem.starts_with("LPT"))
                && stem.len() == 4
                && stem.as_bytes()[3].is_ascii_digit())
        {
            return Err(ArtifactError::Archive);
        }
    }
    Ok(name.to_owned())
}

fn enumerate_installation(
    root: &Path,
    directory: &Path,
    files: &mut BTreeSet<String>,
    directories: &mut BTreeSet<String>,
    checkpoint: &mut impl FnMut() -> Result<(), ArtifactError>,
) -> Result<(), ArtifactError> {
    checkpoint()?;
    validate_external_path(directory).map_err(|_| ArtifactError::Path)?;
    for entry in fs::read_dir(directory).map_err(|_| ArtifactError::Installation)? {
        checkpoint()?;
        let path = entry.map_err(|_| ArtifactError::Installation)?.path();
        let metadata = fs::symlink_metadata(&path).map_err(|_| ArtifactError::Installation)?;
        if is_redirect(&metadata) {
            return Err(ArtifactError::Path);
        }
        let name = path
            .strip_prefix(root)
            .map_err(|_| ArtifactError::Path)?
            .to_str()
            .ok_or(ArtifactError::Path)?
            .replace('\\', "/");
        if files.len() + directories.len() >= MAX_ENTRIES * 2 {
            return Err(ArtifactError::Installation);
        }
        if metadata.is_dir() {
            if !directories.insert(name.to_ascii_lowercase()) {
                return Err(ArtifactError::Installation);
            }
            enumerate_installation(root, &path, files, directories, checkpoint)?;
        } else if metadata.is_file() {
            if !files.insert(name.to_ascii_lowercase()) {
                return Err(ArtifactError::Installation);
            }
        } else {
            return Err(ArtifactError::Installation);
        }
    }
    Ok(())
}

fn check_arm64(file: &mut (impl Read + Seek)) -> Result<(), ArtifactError> {
    let length = file
        .seek(SeekFrom::End(0))
        .map_err(|_| ArtifactError::Architecture)?;
    file.seek(SeekFrom::Start(0))
        .map_err(|_| ArtifactError::Architecture)?;
    let mut dos = [0; 64];
    file.read_exact(&mut dos)
        .map_err(|_| ArtifactError::Architecture)?;
    let offset = u64::from(u32::from_le_bytes([dos[60], dos[61], dos[62], dos[63]]));
    if dos[..2] != *b"MZ" || length < 24 || offset > length - 24 {
        return Err(ArtifactError::Architecture);
    }
    file.seek(SeekFrom::Start(offset))
        .map_err(|_| ArtifactError::Architecture)?;
    let mut pe = [0; 6];
    file.read_exact(&mut pe)
        .map_err(|_| ArtifactError::Architecture)?;
    if pe != [b'P', b'E', 0, 0, 0x64, 0xaa] {
        return Err(ArtifactError::Architecture);
    }
    Ok(())
}

/// Validate path spelling and existing ancestors without following redirects.
/// Native launch code additionally verifies the volume is a recognized local disk.
///
/// # Errors
/// Rejects nonabsolute, namespace, ADS, unresolved-variable, Git and reparse paths.
pub fn validate_external_path(path: &Path) -> Result<(), ArtifactError> {
    let spelling = path.to_str().ok_or(ArtifactError::Path)?;
    if !path.is_absolute()
        || spelling.contains('%')
        || spelling.contains('\0')
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
    {
        return Err(ArtifactError::Path);
    }
    #[cfg(windows)]
    {
        use std::path::Prefix;
        let Some(Component::Prefix(prefix)) = path.components().next() else {
            return Err(ArtifactError::Path);
        };
        if !matches!(prefix.kind(), Prefix::Disk(_))
            || spelling.get(2..).is_none_or(|rest| rest.contains(':'))
            || path.parent().is_none()
        {
            return Err(ArtifactError::Path);
        }
    }
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) => {
                if is_redirect(&metadata) {
                    return Err(ArtifactError::Path);
                }
                if metadata.is_dir() {
                    match fs::symlink_metadata(ancestor.join(".git")) {
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                        _ => return Err(ArtifactError::Path),
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(ArtifactError::Path),
        }
    }
    Ok(())
}

/// Whether metadata identifies a symbolic link or Windows reparse point.
#[must_use]
pub fn is_redirect(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};

    #[test]
    fn names_reject_windows_aliases_and_redirects() {
        for name in [
            "../llama-server.exe",
            "C:/llama-server.exe",
            "/llama-server.exe",
            "a\\b",
            "a//b",
            "NUL.dll",
            "NUL .txt",
            "con .dll",
            "COM1 .dll",
            "con",
            "COM1.exe",
            "x./file",
            "x /file",
            "file:stream",
            "./file",
            "a/../file",
        ] {
            assert_eq!(archive_name(name, false), Err(ArtifactError::Archive));
        }
        assert_eq!(archive_name("a/b.dll", false), Ok("a/b.dll".to_owned()));
    }
    #[test]
    fn bounded_hash_rejects_short_and_long_streams() {
        let mut buffer = vec![0; HASH_BUFFER_BYTES];
        assert!(hash_reader(&mut Cursor::new(b"abc"), 2, &mut buffer, &mut || Ok(())).is_err());
        assert!(hash_reader(&mut Cursor::new(b"abc"), 4, &mut buffer, &mut || Ok(())).is_err());
        assert_eq!(
            hash_reader(&mut Cursor::new(b"abc"), 3, &mut buffer, &mut || Ok(()))
                .ok()
                .as_deref(),
            Some("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
        );
    }
    #[test]
    fn bounded_hash_validates_buffer_and_cancellation_before_reading() {
        let mut reader = Cursor::new(b"abc");
        assert_eq!(
            hash_reader(&mut reader, 3, &mut [], &mut || Ok(())),
            Err(ArtifactError::Configuration)
        );
        assert_eq!(reader.position(), 0);

        let mut buffer = [0; 1];
        let mut reads = 0;
        assert_eq!(
            hash_reader(&mut reader, 3, &mut buffer, &mut || {
                reads += 1;
                if reads == 2 {
                    Err(ArtifactError::Cancelled)
                } else {
                    Ok(())
                }
            }),
            Err(ArtifactError::Cancelled)
        );
        assert_eq!(reader.position(), 1);
        assert_eq!(
            hash_reader(&mut Cursor::new(b""), 0, &mut buffer, &mut || Ok(())),
            Ok("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".into())
        );
    }
    #[cfg(windows)]
    #[test]
    fn cng_hash_matches_portable_sha256_across_binary_read_boundaries() -> Result<(), ArtifactError>
    {
        let payload: Vec<_> = (0..=255_u8)
            .cycle()
            .take(2 * HASH_BUFFER_BYTES + 137)
            .collect();
        let expected = digest_hex(Sha256::digest(&payload).into());
        for length in [63, 64, 65, HASH_BUFFER_BYTES] {
            let mut buffer = vec![0; length];
            assert_eq!(
                hash_reader(
                    &mut Cursor::new(&payload),
                    u64::try_from(payload.len()).map_err(|_| ArtifactError::Identity)?,
                    &mut buffer,
                    &mut || Ok(()),
                )?,
                expected
            );
        }
        Ok(())
    }
    #[test]
    fn bounded_hash_never_returns_a_digest_after_a_read_failure() {
        struct BrokenReader(bool);
        impl Read for BrokenReader {
            fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
                if self.0 {
                    return Err(std::io::Error::other("synthetic read failure"));
                }
                self.0 = true;
                buffer[0] = b'a';
                Ok(1)
            }
        }
        assert_eq!(
            hash_reader(&mut BrokenReader(false), 1, &mut [0; 64], &mut || Ok(())),
            Err(ArtifactError::Identity)
        );
    }
    #[test]
    fn zipped_installation_verifies_deflated_payload_and_rejects_drift()
    -> Result<(), Box<dyn std::error::Error>> {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "SwiftBonsai-zip-verification-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&root)?;
        let executable = root.join("llama-server.exe");
        let mut image = vec![0; 128];
        image[..2].copy_from_slice(b"MZ");
        image[60..64].copy_from_slice(&64_u32.to_le_bytes());
        image[64..70].copy_from_slice(&[b'P', b'E', 0, 0, 0x64, 0xaa]);
        fs::write(&executable, &image)?;
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        writer.start_file(
            "llama-server.exe",
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated),
        )?;
        writer.write_all(&image)?;
        let mut archive = ZipArchive::new(writer.finish()?)?;
        let mut handles = Vec::new();
        let mut buffer = vec![0; HASH_BUFFER_BYTES];
        let mut checkpoint = || Ok(());
        assert_eq!(
            verify_installation(
                &mut archive,
                &root,
                &mut handles,
                &mut buffer,
                &mut checkpoint,
            )?
            .0,
            executable
        );
        drop(handles);
        image[127] = 1;
        fs::write(&executable, &image)?;
        let mut handles = Vec::new();
        assert_eq!(
            verify_installation(
                &mut archive,
                &root,
                &mut handles,
                &mut buffer,
                &mut checkpoint,
            ),
            Err(ArtifactError::Installation)
        );
        drop(handles);
        fs::remove_dir_all(root)?;
        Ok(())
    }
    #[test]
    fn native_pe_requires_bounded_arm64_header() {
        let mut image = vec![0; 128];
        image[..2].copy_from_slice(b"MZ");
        image[60..64].copy_from_slice(&64_u32.to_le_bytes());
        image[64..70].copy_from_slice(&[b'P', b'E', 0, 0, 0x64, 0xaa]);
        assert!(check_arm64(&mut Cursor::new(&image)).is_ok());
        image[68] = 0x4c;
        assert_eq!(
            check_arm64(&mut Cursor::new(&image)),
            Err(ArtifactError::Architecture)
        );
        image[60..64].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(check_arm64(&mut Cursor::new(&image)).is_err());
    }
}
