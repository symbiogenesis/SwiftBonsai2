//! Offline, bounded verification of the compiled model and runtime identities.
//!
//! Retain [`VerifiedArtifacts`] until the child has exited: Windows read handles
//! deny file writes and deletion while the artifacts are in use.

use crate::launch_policy::{is_redirect, reviewed_pins, validate_external_path};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};
use zip::ZipArchive;

const MAX_EXPANDED_BYTES: u64 = 512 * 1024 * 1024;
const MAX_ENTRIES: usize = 4096;

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
        })
    }
}
impl std::error::Error for ArtifactError {}

/// Verified locked artifacts and the read handles that keep their files stable.
/// This is offline verification; it never downloads, writes, or executes code.
pub struct VerifiedArtifacts {
    executable: PathBuf,
    model: PathBuf,
    runtime_directory: PathBuf,
    archive_path: PathBuf,
    _handles: Vec<File>,
}
impl VerifiedArtifacts {
    #[cfg(test)]
    pub(crate) const fn synthetic_for_test() -> Self {
        Self {
            executable: PathBuf::new(),
            model: PathBuf::new(),
            runtime_directory: PathBuf::new(),
            archive_path: PathBuf::new(),
            _handles: Vec::new(),
        }
    }

    /// Verify whole locked artifacts and every installed file against the ZIP.
    /// Native executable and DLL entries must be ARM64 PE images.
    ///
    /// # Errors
    /// Rejects policy/path drift, changed artifacts, ZIP hazards and installation drift.
    pub fn verify(data: &Path) -> Result<Self, ArtifactError> {
        if !cfg!(windows) {
            return Err(ArtifactError::Configuration);
        }
        let pins = reviewed_pins().map_err(|_| ArtifactError::Configuration)?;
        let archive_path = data.join("downloads").join(&pins.runtime.asset.filename);
        let runtime_directory = data.join("runtime").join(&pins.runtime.commit);
        let model = data
            .join("models")
            .join(&pins.model.revision)
            .join(&pins.model.filename);
        let mut handles = Vec::new();
        hold_ancestors(data, &mut handles)?;
        hold_ancestors(&archive_path, &mut handles)?;
        hold_ancestors(&model, &mut handles)?;
        hold_ancestors(&runtime_directory.join("llama-server.exe"), &mut handles)?;
        let mut archive = locked_file(&archive_path)?;
        check_locked(
            &mut archive,
            pins.runtime.asset.bytes,
            &pins.runtime.asset.sha256,
        )?;
        let mut model_handle = locked_file(&model)?;
        check_locked(&mut model_handle, pins.model.bytes, &pins.model.sha256)?;
        handles.push(model_handle);
        let mut zip = ZipArchive::new(archive.try_clone().map_err(|_| ArtifactError::Archive)?)
            .map_err(|_| ArtifactError::Archive)?;
        let executable = verify_installation(&mut zip, &runtime_directory, &mut handles)?;
        handles.push(archive);
        Ok(Self {
            executable,
            model,
            runtime_directory,
            archive_path,
            _handles: handles,
        })
    }

    /// Server path established by the verified archive.
    #[must_use]
    pub fn executable(&self) -> &Path {
        &self.executable
    }
    /// Whole-model path held stable by verification handles.
    #[must_use]
    pub fn model(&self) -> &Path {
        &self.model
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
}

fn locked_file(path: &Path) -> Result<File, ArtifactError> {
    validate_external_path(path).map_err(|_| ArtifactError::Path)?;
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

fn hold_ancestors(path: &Path, handles: &mut Vec<File>) -> Result<(), ArtifactError> {
    validate_external_path(path).map_err(|_| ArtifactError::Path)?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        for ancestor in path.ancestors().skip(1) {
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

fn check_locked(file: &mut File, bytes: u64, hash: &str) -> Result<(), ArtifactError> {
    if file.metadata().map_err(|_| ArtifactError::Identity)?.len() != bytes {
        return Err(ArtifactError::Identity);
    }
    let digest = hash_reader(file, bytes)?;
    if digest != hash {
        return Err(ArtifactError::Identity);
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|_| ArtifactError::Identity)?;
    Ok(())
}

fn digest_hex(hash: Sha256) -> String {
    hash.finalize()
        .into_iter()
        .flat_map(|byte| [byte >> 4, byte & 0x0f])
        .map(|nibble| char::from(b"0123456789abcdef"[usize::from(nibble)]))
        .collect()
}
fn hash_reader(reader: &mut impl Read, expected: u64) -> Result<String, ArtifactError> {
    let mut hash = Sha256::new();
    let mut total = 0_u64;
    let mut buffer = vec![0; 64 * 1024].into_boxed_slice();
    loop {
        let count = reader
            .read(&mut buffer)
            .map_err(|_| ArtifactError::Identity)?;
        if count == 0 {
            break;
        }
        total = total
            .checked_add(u64::try_from(count).map_err(|_| ArtifactError::Identity)?)
            .ok_or(ArtifactError::Identity)?;
        if total > expected {
            return Err(ArtifactError::Identity);
        }
        hash.update(&buffer[..count]);
    }
    if total != expected {
        return Err(ArtifactError::Identity);
    }
    Ok(digest_hex(hash))
}

struct Entry {
    index: usize,
    name: String,
    bytes: u64,
}
fn verify_installation<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    root: &Path,
    handles: &mut Vec<File>,
) -> Result<PathBuf, ArtifactError> {
    if archive.is_empty() || archive.len() > MAX_ENTRIES {
        return Err(ArtifactError::Archive);
    }
    let mut names = BTreeSet::new();
    let mut expected_files = BTreeMap::new();
    let mut expected_directories = BTreeSet::new();
    let mut total = 0_u64;
    let mut server = None;
    for index in 0..archive.len() {
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
    let mut installed_files = BTreeSet::new();
    let mut installed_directories = BTreeSet::new();
    enumerate_installation(root, root, &mut installed_files, &mut installed_directories)?;
    // Both collections iterate sorted, unique names; compare them directly
    // instead of cloning every expected path into a second tree.
    if !installed_files.iter().eq(expected_files.keys())
        || installed_directories != expected_directories
    {
        return Err(ArtifactError::Installation);
    }
    for entry in expected_files.values() {
        let path = root.join(&entry.name);
        hold_ancestors(&path, handles)?;
        let mut installed = locked_file(&path)?;
        let mut source = archive
            .by_index(entry.index)
            .map_err(|_| ArtifactError::Archive)?;
        let expected_hash =
            hash_reader(&mut source, entry.bytes).map_err(|_| ArtifactError::Archive)?;
        check_locked(&mut installed, entry.bytes, &expected_hash)
            .map_err(|_| ArtifactError::Installation)?;
        if path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("exe") || ext.eq_ignore_ascii_case("dll"))
        {
            check_arm64(&mut installed)?;
        }
        handles.push(installed);
    }
    Ok(server)
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
) -> Result<(), ArtifactError> {
    validate_external_path(directory).map_err(|_| ArtifactError::Path)?;
    for entry in fs::read_dir(directory).map_err(|_| ArtifactError::Installation)? {
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
            enumerate_installation(root, &path, files, directories)?;
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
        assert!(hash_reader(&mut Cursor::new(b"abc"), 2).is_err());
        assert!(hash_reader(&mut Cursor::new(b"abc"), 4).is_err());
        assert_eq!(
            hash_reader(&mut Cursor::new(b"abc"), 3).ok().as_deref(),
            Some("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
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
        assert_eq!(
            verify_installation(&mut archive, &root, &mut handles)?,
            executable
        );
        drop(handles);
        image[127] = 1;
        fs::write(&executable, &image)?;
        let mut handles = Vec::new();
        assert_eq!(
            verify_installation(&mut archive, &root, &mut handles),
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
