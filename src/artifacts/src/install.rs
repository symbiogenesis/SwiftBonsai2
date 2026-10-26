//! Runtime extraction shares the verifier's archive policy.

use super::{
    ArtifactError, HASH_BUFFER_BYTES, Installation, RuntimeSpec, VerifiedRuntimeFile, check_arm64,
    check_locked, hold_ancestors, inspect_archive, locked_file, validate_external_path,
    verify_installation,
};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, Write};
use std::path::Path;
use zip::ZipArchive;

/// Stage a locked ARM64 runtime ZIP in a fresh directory without executing it.
///
/// The caller supplies a fresh staging sibling, retained on failure. The archive
/// remains guarded through extraction and verification. Staging file guards are
/// released when this operation returns. The caller must publish without replacing
/// an existing destination, then verify the published runtime before executing it.
/// This operation neither publishes nor starts a native process.
///
/// # Errors
/// Rejects unsupported platforms, unsafe paths or archive entries, identity or
/// architecture drift, failed writes, and existing staging or destination paths.
pub fn stage_runtime(
    spec: &RuntimeSpec,
    staging: &Path,
) -> Result<Vec<VerifiedRuntimeFile>, ArtifactError> {
    if !cfg!(windows) {
        return Err(ArtifactError::Configuration);
    }
    validate_external_path(&spec.runtime_directory)?;
    validate_external_path(staging)?;
    if staging == spec.runtime_directory || staging.parent() != spec.runtime_directory.parent() {
        return Err(ArtifactError::Path);
    }
    absent(staging)?;
    absent(&spec.runtime_directory)?;

    let mut archive_handles = Vec::new();
    hold_ancestors(&spec.archive_path, &mut archive_handles, &mut || Ok(()))?;
    let mut archive = locked_file(&spec.archive_path)?;
    let mut buffer = vec![0; HASH_BUFFER_BYTES];
    check_locked(
        &mut archive,
        spec.archive_bytes,
        &spec.archive_sha256,
        &mut buffer,
        &mut || Ok(()),
    )?;
    let mut zip = ZipArchive::new(archive).map_err(|_| ArtifactError::Archive)?;
    // Complete policy inspection precedes creation of even the staging directory.
    let installation = inspect_archive(&mut zip, staging, &mut || Ok(()))?;
    let parent = staging.parent().ok_or(ArtifactError::Path)?;
    fs::create_dir_all(parent).map_err(|_| ArtifactError::Path)?;
    hold_ancestors(
        &spec.runtime_directory,
        &mut archive_handles,
        &mut || Ok(()),
    )?;
    fs::create_dir(staging).map_err(|_| ArtifactError::Path)?;
    extract(&mut zip, staging, &installation, &mut buffer)?;

    let mut staging_handles = Vec::new();
    let (_, files) = verify_installation(
        &mut zip,
        staging,
        &mut staging_handles,
        &mut buffer,
        &mut || Ok(()),
    )?;
    Ok(files)
}

fn absent(path: &Path) -> Result<(), ArtifactError> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        _ => Err(ArtifactError::Path),
    }
}

fn extract<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    staging: &Path,
    installation: &Installation,
    buffer: &mut [u8],
) -> Result<(), ArtifactError> {
    for directory in &installation.directories {
        let path = staging.join(directory);
        validate_external_path(&path)?;
        fs::create_dir_all(path).map_err(|_| ArtifactError::Installation)?;
    }
    for entry in installation.files.values() {
        let path = staging.join(&entry.name);
        validate_external_path(&path)?;
        let mut source = archive
            .by_index(entry.index)
            .map_err(|_| ArtifactError::Archive)?;
        let mut options = OpenOptions::new();
        options.read(true).write(true).create_new(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt as _;
            options.share_mode(0).custom_flags(0x0020_0000);
        }
        let mut output = options
            .open(&path)
            .map_err(|_| ArtifactError::Installation)?;
        copy_bounded(&mut source, &mut output, entry.bytes, buffer)?;
        output.sync_all().map_err(|_| ArtifactError::Installation)?;
        if path.extension().is_some_and(|extension| {
            extension.eq_ignore_ascii_case("exe") || extension.eq_ignore_ascii_case("dll")
        }) {
            check_arm64(&mut output)?;
        }
    }
    Ok(())
}

fn copy_bounded(
    source: &mut impl Read,
    output: &mut File,
    expected: u64,
    buffer: &mut [u8],
) -> Result<(), ArtifactError> {
    let mut total = 0_u64;
    loop {
        let count = source.read(buffer).map_err(|_| ArtifactError::Archive)?;
        if count == 0 {
            break;
        }
        total = total
            .checked_add(u64::try_from(count).map_err(|_| ArtifactError::Archive)?)
            .ok_or(ArtifactError::Archive)?;
        if total > expected {
            return Err(ArtifactError::Archive);
        }
        output
            .write_all(&buffer[..count])
            .map_err(|_| ArtifactError::Installation)?;
    }
    if total != expected {
        return Err(ArtifactError::Archive);
    }
    Ok(())
}
