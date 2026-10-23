//! Bounded GGUF v3 tooling for verified artifacts, with opt-in pinned Prism types.
//!
//! Inspection skips tensor data unless a PQ2 census is requested. PTQ1 export
//! preserves raw weight symbols and scales; inference equivalence and performance
//! remain unqualified. PowerShell retains provisioning and external-path policy.

#![forbid(unsafe_code)]

mod census;
mod export;
mod inspect;

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::Value;
use sha2::{Digest, Sha256};

/// The only supported private GGUF tensor dialect.
pub const PRISM_DIALECT: &str = "Prism-73bfe6cc485554822f80c473a3c0c4f25e80f41b";
/// Immutable Prism codec revision used by the private dialect.
pub const CODEC_COMMIT: &str = "73bfe6cc485554822f80c473a3c0c4f25e80f41b";
/// A failed I/O, artifact verification, or bounded format operation.
pub type Error = Box<dyn std::error::Error + Send + Sync>;
/// A checked tooling result.
pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn bad(message: impl Into<String>) -> Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, message.into()).into()
}

fn open_read(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(1); // FILE_SHARE_READ: deny write/delete during use.
    }
    let file = options.open(path)?;
    if !file.metadata()?.is_file() {
        return Err(bad("GGUF: a regular input file is required."));
    }
    Ok(file)
}

pub(crate) fn hash_range(file: &mut File, start: u64, mut length: u64) -> Result<String> {
    let restore = file.stream_position()?;
    let result = (|| {
        file.seek(SeekFrom::Start(start))?;
        let mut hash = Sha256::new();
        let mut buffer = vec![0_u8; 65_536];
        while length != 0 {
            let count = usize::try_from(length.min(65_536))?;
            file.read_exact(&mut buffer[..count])?;
            hash.update(&buffer[..count]);
            length -= u64::try_from(count)?;
        }
        Ok(format!("{:x}", hash.finalize()))
    })();
    file.seek(SeekFrom::Start(restore))?;
    result
}

fn verify(file: &mut File, expected_bytes: u64, expected_sha256: &str) -> Result<()> {
    if expected_bytes == 0
        || expected_sha256.len() != 64
        || !expected_sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(bad(
            "GGUF: a positive byte count and 64-digit SHA-256 are required.",
        ));
    }
    if file.metadata()?.len() != expected_bytes {
        return Err(bad("GGUF: artifact size mismatch."));
    }
    if !hash_range(file, 0, expected_bytes)?.eq_ignore_ascii_case(expected_sha256) {
        return Err(bad("GGUF: artifact SHA-256 mismatch."));
    }
    Ok(())
}

/// Verifies an artifact and inspects metadata, optionally streaming PQ2 payloads.
///
/// # Errors
/// Rejects identity mismatches, unsupported dialects, malformed or over-limit
/// metadata/descriptors, invalid extents, and I/O failures.
pub fn inspect(
    path: &Path,
    dialect: &str,
    census: bool,
    expected_bytes: u64,
    expected_sha256: &str,
) -> Result<Value> {
    if census && dialect != PRISM_DIALECT {
        return Err(bad(
            "GGUF: PQ2 census requires the explicitly pinned Prism dialect.",
        ));
    }
    let mut file = open_read(path)?;
    verify(&mut file, expected_bytes, expected_sha256)?;
    let mut inspection = inspect::inspect(&mut file, path, dialect)?;
    if census {
        inspection.report["Pq2Census"] = census::census(&mut file, &inspection)?;
        inspection.report["TensorPayloadRead"] = true.into();
    }
    inspection.report["ArtifactSha256"] = expected_sha256.to_ascii_lowercase().into();
    inspection.report["ArtifactVerification"] = "verified-size-and-sha256".into();
    Ok(inspection.report)
}

/// Verifies and exports an experimental PTQ1 GGUF to a new output name.
///
/// Writes a unique sibling partial, validates its identity and metadata, then
/// publishes using a hard link that cannot replace an existing destination.
/// Failed partials remain available for diagnosis. Publication requires a
/// filesystem supporting hard links, such as NTFS.
///
/// # Errors
/// Rejects artifact mismatches, invalid tensors/scales/symbols, deadlines outside
/// one second through one hour, existing output paths, and filesystem failures.
pub fn export_ptq1(
    source: &Path,
    output: &Path,
    expected_bytes: u64,
    expected_sha256: &str,
    timeout: Duration,
) -> Result<Value> {
    if timeout < Duration::from_secs(1) || timeout > Duration::from_secs(3600) {
        return Err(bad(
            "GGUF: export deadline must be between one second and one hour.",
        ));
    }
    if output.try_exists()? {
        return Err(bad("GGUF: output must name a new file."));
    }
    let mut file = open_read(source)?;
    verify(&mut file, expected_bytes, expected_sha256)?;
    let inspection = inspect::inspect(&mut file, source, PRISM_DIALECT)?;
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let mut partial_name = output.as_os_str().to_os_string();
    partial_name.push(format!(".partial-{}-{nonce}", std::process::id()));
    let partial = std::path::PathBuf::from(partial_name);
    let result = (|| {
        let mut report = export::export(&mut file, &partial, &inspection, timeout)?;
        let mut exported = open_read(&partial)?;
        let output_bytes = report["OutputBytes"]
            .as_u64()
            .ok_or_else(|| bad("missing output size"))?;
        let output_hash = report["OutputSha256"]
            .as_str()
            .ok_or_else(|| bad("missing output hash"))?;
        verify(&mut exported, output_bytes, output_hash)?;
        let checked = inspect::inspect(&mut exported, &partial, PRISM_DIALECT)?;
        if checked.report["TokenizerMetadataSha256"] != inspection.report["TokenizerMetadataSha256"]
            || checked.report["TensorCount"] != inspection.report["TensorCount"]
        {
            return Err(bad("GGUF: export metadata identity differs."));
        }
        report["SourceSha256"] = expected_sha256.to_ascii_lowercase().into();
        report["TokenizerMetadataSha256"] = checked.report["TokenizerMetadataSha256"].clone();
        report["OutputPath"] = output.to_string_lossy().into_owned().into();
        fs::hard_link(&partial, output)?;
        drop(exported);
        if let Err(error) = fs::remove_file(&partial) {
            // Publication already succeeded. Report a retained alias instead of
            // claiming that the validated final output failed to publish.
            report["RetainedPartialPath"] = partial.to_string_lossy().into_owned().into();
            report["PartialCleanupError"] = error.to_string().into();
        }
        Ok(report)
    })();
    result.map_err(|error: Error| {
        bad(format!(
            "{error} Partial path (retained if created): {}",
            partial.display()
        ))
    })
}
