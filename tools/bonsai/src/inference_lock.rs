//! Offline review of an independently selected Prism release and its exact source.

use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::identity::{hex, is_lower_hex};
use bonsai_artifacts::{hash_reader, open_locked_file, validate_external_path};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use zip::ZipArchive;

const REPOSITORY: &str = "https://github.com/symbiogenesis/prism-inference";
const TARGET: &str = "aarch64-pc-windows-msvc";
const MIB: u64 = 1024 * 1024;
const FILES: [&str; 9] = [
    "prism-inference.exe",
    "bonsai-token-worker.exe",
    "bonsai-swift-worker.exe",
    "prism-inference-server.exe",
    "bonsai-gguf.exe",
    "LICENSE",
    "NOTICE",
    "PRISM-LICENSE.txt",
    "CAMELID-LICENSE.txt",
];

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    receipt_path: PathBuf,
    archive_path: PathBuf,
    source_archive_path: PathBuf,
    source_revision: String,
    version: String,
    run_id: String,
    run_attempt: u32,
    output_path: PathBuf,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Receipt {
    schema_version: u32,
    repository: String,
    source_revision: String,
    version: String,
    target: String,
    toolchain: String,
    run_id: Value,
    run_attempt: Value,
    files: Vec<Identity>,
    archive: Identity,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Identity {
    name: String,
    bytes: u64,
    sha256: String,
}

fn numeric_identity(value: &Value, expected: &str) -> bool {
    value.as_str().is_some_and(|actual| actual == expected)
        || value
            .as_u64()
            .is_some_and(|actual| actual.to_string() == expected)
}

fn open_input(path: &Path, maximum: u64) -> crate::Result<File> {
    let file = open_locked_file(path).map_err(|error| error.to_string())?;
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    if metadata.len() == 0 || metadata.len() > maximum {
        return Err("Release review input is not a bounded regular file.".into());
    }
    Ok(file)
}

fn bounded_bytes(reader: &mut impl Read, maximum: u64) -> crate::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader
        .take(maximum + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if u64::try_from(bytes.len()).map_err(|error| error.to_string())? > maximum {
        return Err("Release review input exceeded its byte bound.".into());
    }
    Ok(bytes)
}

fn digest(reader: &mut impl Read, expected: u64) -> crate::Result<String> {
    let mut buffer = vec![0; 65536].into_boxed_slice();
    hash_reader(reader, expected, &mut buffer, &mut || Ok(())).map_err(|error| error.to_string())
}

fn validate_receipt(request: &Request, receipt: &Receipt) -> crate::Result<String> {
    let filename = format!("prism-inference-{}-{TARGET}.zip", request.version);
    let version: Vec<_> = request.version.split('.').collect();
    if !is_lower_hex(&request.source_revision, 40)
        || version.len() != 3
        || version
            .iter()
            .any(|part| part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()))
        || request.run_id.starts_with('0')
        || request.run_id.is_empty()
        || !request.run_id.bytes().all(|byte| byte.is_ascii_digit())
        || !(1..=2_147_483_647).contains(&request.run_attempt)
        || receipt.schema_version != 1
        || receipt.repository != REPOSITORY
        || receipt.source_revision != request.source_revision
        || receipt.version != request.version
        || receipt.target != TARGET
        || !numeric_identity(&receipt.run_id, &request.run_id)
        || !numeric_identity(&receipt.run_attempt, &request.run_attempt.to_string())
        || receipt.archive.name != filename
        || receipt.archive.bytes == 0
        || receipt.archive.bytes > 1024 * MIB
        || !is_lower_hex(&receipt.archive.sha256, 64)
        || receipt.files.len() != FILES.len()
    {
        return Err(
            "The release receipt differs from the independently selected build identity.".into(),
        );
    }
    let mut names = BTreeSet::new();
    let mut total = 0_u64;
    for file in &receipt.files {
        if !FILES.contains(&file.name.as_str())
            || !names.insert(&file.name)
            || file.bytes == 0
            || file.bytes > 512 * MIB
            || !is_lower_hex(&file.sha256, 64)
        {
            return Err(
                "The release receipt needs every executable and license identity exactly once."
                    .into(),
            );
        }
        total += file.bytes;
    }
    if total > 1024 * MIB {
        return Err("Release inventory exceeded its expansion bound.".into());
    }
    Ok(filename)
}

// The release contract needs these literal fields, rather than general TOML evaluation.
fn quoted_field<'a>(text: &'a str, section: &str, field: &str) -> crate::Result<&'a str> {
    let header = format!("[{section}]");
    let mut active = false;
    let mut seen_section = false;
    let mut value = None;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            active = line == header;
            if active && seen_section {
                return Err("Repeated source manifest section.".into());
            }
            seen_section |= active;
        } else if active
            && let Some((name, declaration)) = line.split_once('=')
            && name.trim() == field
        {
            let declaration = declaration.trim();
            let literal = declaration
                .strip_prefix('"')
                .and_then(|text| text.strip_suffix('"'))
                .filter(|text| !text.contains(['"', '\\']));
            if value.is_some() || literal.is_none() {
                return Err("Unsupported source manifest identity field.".into());
            }
            value = literal;
        }
    }
    value.ok_or_else(|| "Missing source manifest identity field.".into())
}

fn source_identity(file: &mut File, request: &Request, toolchain: &str) -> crate::Result<Value> {
    let bytes = file.metadata().map_err(|error| error.to_string())?.len();
    let hash = digest(file, bytes)?;
    file.seek(SeekFrom::Start(0))
        .map_err(|error| error.to_string())?;
    let mut archive = ZipArchive::new(file).map_err(|error| error.to_string())?;
    if archive.is_empty() || archive.len() > 65536 {
        return Err("Invalid source archive inventory.".into());
    }
    let prefix = format!("prism-inference-{}/", request.source_revision);
    let mut names = BTreeSet::new();
    let mut total = 0_u64;
    for index in 0..archive.len() {
        let entry = archive.by_index(index).map_err(|error| error.to_string())?;
        let name = entry.name().map_err(|error| error.to_string())?;
        if !name.starts_with(&prefix)
            || name.contains(['\\', ':'])
            || name.split('/').any(|part| matches!(part, "." | ".."))
            || !names.insert(name.to_lowercase())
            || entry
                .unix_mode()
                .is_some_and(|mode| mode & 0o170_000 == 0o120_000)
            || entry.size() > 512 * MIB
        {
            return Err("Unexpected, duplicate or linked source archive entry.".into());
        }
        total += entry.size();
        if total > 1024 * MIB {
            return Err("Source archive contents exceeded their byte bound.".into());
        }
    }
    for (relative, package) in [
        ("Cargo.toml", Some("prism-inference")),
        ("server/Cargo.toml", Some("prism-inference-server")),
        ("rust-toolchain.toml", None),
    ] {
        let mut entry = archive
            .by_name(&(prefix.clone() + relative))
            .map_err(|error| error.to_string())?;
        if entry.size() == 0 || entry.size() > MIB {
            return Err("Missing or oversized source manifest.".into());
        }
        let encoded = bounded_bytes(&mut entry, MIB)?;
        let text = std::str::from_utf8(&encoded)
            .map_err(|error| error.to_string())?
            .trim_start_matches('\u{feff}');
        if let Some(package) = package {
            if quoted_field(text, "package", "name")? != package
                || quoted_field(text, "package", "version")? != request.version
            {
                return Err("The source package identity differs from the release receipt.".into());
            }
        } else {
            let channel = quoted_field(text, "toolchain", "channel")?;
            let date = channel
                .strip_prefix("nightly-")
                .ok_or("The source compiler pin is not an exact nightly.")?;
            if date.len() != 10
                || !date.bytes().enumerate().all(|(index, byte)| {
                    if index == 4 || index == 7 {
                        byte == b'-'
                    } else {
                        byte.is_ascii_digit()
                    }
                })
                || toolchain != format!("{channel}-{TARGET}")
            {
                return Err("The source compiler pin differs from the release receipt.".into());
            }
        }
    }
    Ok(
        json!({ "Repository": REPOSITORY, "Commit": request.source_revision,
        "Url": format!("{REPOSITORY}/archive/{}.zip", request.source_revision), "Bytes": bytes, "Sha256": hash }),
    )
}

fn copy_overlap(
    target: &mut [u8],
    target_start: u64,
    buffer: &[u8],
    buffer_start: u64,
) -> crate::Result<()> {
    let start = target_start.max(buffer_start);
    let end = (target_start + u64::try_from(target.len()).map_err(|error| error.to_string())?)
        .min(buffer_start + u64::try_from(buffer.len()).map_err(|error| error.to_string())?);
    if start < end {
        let source = usize::try_from(start - buffer_start).map_err(|error| error.to_string())?;
        let offset = usize::try_from(start - target_start).map_err(|error| error.to_string())?;
        let length = usize::try_from(end - start).map_err(|error| error.to_string())?;
        target[offset..offset + length].copy_from_slice(&buffer[source..source + length]);
    }
    Ok(())
}

fn verify_entry(reader: &mut impl Read, identity: &Identity) -> crate::Result<()> {
    let executable = Path::new(&identity.name)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"));
    let mut dos = [0; 64];
    let mut pe = [0; 6];
    let mut offset = None;
    let mut hash = Sha256::new();
    let mut total = 0_u64;
    let mut buffer = vec![0; 65536].into_boxed_slice();
    loop {
        let count = reader
            .read(&mut buffer)
            .map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        let end = total + u64::try_from(count).map_err(|error| error.to_string())?;
        if end > identity.bytes {
            return Err("Release entry exceeded its locked byte length.".into());
        }
        if executable {
            copy_overlap(&mut dos, 0, &buffer[..count], total)?;
            if offset.is_none() && end >= 64 {
                let position = u64::from(u32::from_le_bytes([dos[60], dos[61], dos[62], dos[63]]));
                if dos[..2] != *b"MZ"
                    || position < 64
                    || identity.bytes < 24
                    || position > identity.bytes - 24
                {
                    return Err("Invalid ARM64 PE header in release bundle.".into());
                }
                offset = Some(position);
            }
            if let Some(position) = offset {
                copy_overlap(&mut pe, position, &buffer[..count], total)?;
            }
        }
        hash.update(&buffer[..count]);
        total = end;
    }
    if total != identity.bytes
        || hex(&hash.finalize()) != identity.sha256
        || executable && (offset.is_none() || pe != [b'P', b'E', 0, 0, 0x64, 0xaa])
    {
        return Err("Release entry hash, length or ARM64 machine differs from its lock.".into());
    }
    Ok(())
}

fn verify_bundle(file: &mut File, receipt: &Receipt) -> crate::Result<()> {
    if file.metadata().map_err(|error| error.to_string())?.len() != receipt.archive.bytes
        || digest(file, receipt.archive.bytes)? != receipt.archive.sha256
    {
        return Err("The bundle archive differs from the release receipt.".into());
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|error| error.to_string())?;
    let mut archive = ZipArchive::new(file).map_err(|error| error.to_string())?;
    if archive.len() != receipt.files.len() {
        return Err("Release archive inventory differs from its receipt.".into());
    }
    let mut seen = BTreeSet::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|error| error.to_string())?;
        let name = entry
            .name()
            .map_err(|error| error.to_string())?
            .into_owned();
        let identity = receipt
            .files
            .iter()
            .find(|file| file.name == name)
            .ok_or("Unexpected release archive entry.")?;
        if !seen.insert(name)
            || entry.is_dir()
            || entry.size() != identity.bytes
            || entry
                .unix_mode()
                .is_some_and(|mode| mode & 0o170_000 == 0o120_000)
        {
            return Err("Unexpected, duplicate or linked release archive entry.".into());
        }
        verify_entry(&mut entry, identity)?;
    }
    Ok(())
}

fn write_new(path: &Path, value: &Value) -> crate::Result<()> {
    validate_external_path(path).map_err(|error| error.to_string())?;
    fs::create_dir_all(
        path.parent()
            .ok_or("Candidate output needs a parent directory.")?,
    )
    .map_err(|error| error.to_string())?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt as _;
        options.share_mode(0).custom_flags(0x0020_0000);
    }
    let mut output = options.open(path).map_err(|error| error.to_string())?;
    serde_json::to_writer_pretty(&mut output, value).map_err(|error| error.to_string())?;
    output
        .write_all(b"\n")
        .and_then(|()| output.sync_all())
        .map_err(|error| error.to_string())
}

/// Verify a reviewed source/archive pair and publish a new candidate lock only.
///
/// Reads are bounded and retain read-only file handles. No archive is extracted,
/// no executable or network request is started, and existing output is never replaced.
///
/// # Errors
/// Rejects malformed requests, unsafe paths, mismatched release/source identities,
/// changed hashes or architecture, unsupported archives, and failed publication.
pub fn run(value: Value) -> crate::Result<Value> {
    if !value.is_object() {
        return Err("Release review request must be a JSON object.".into());
    }
    let request: Request = serde_json::from_value(value).map_err(|error| error.to_string())?;
    validate_external_path(&request.output_path).map_err(|error| error.to_string())?;
    if request
        .output_path
        .try_exists()
        .map_err(|error| error.to_string())?
    {
        return Err("Use a new candidate lock output file.".into());
    }
    let mut receipt_file = open_input(&request.receipt_path, MIB)?;
    let encoded = bounded_bytes(&mut receipt_file, MIB)?;
    let encoded = encoded
        .strip_prefix(&[0xef, 0xbb, 0xbf])
        .unwrap_or(&encoded);
    let receipt: Value = serde_json::from_slice(encoded).map_err(|error| error.to_string())?;
    if !receipt.is_object()
        || !receipt["Archive"].is_object()
        || receipt["Files"]
            .as_array()
            .is_none_or(|files| files.iter().any(|file| !file.is_object()))
    {
        return Err("Release receipt, archive and file identities must be JSON objects.".into());
    }
    let receipt: Receipt = serde_json::from_value(receipt).map_err(|error| error.to_string())?;
    let filename = validate_receipt(&request, &receipt)?;
    let mut source_file = open_input(&request.source_archive_path, 256 * MIB)?;
    let source = source_identity(&mut source_file, &request, &receipt.toolchain)?;
    let mut bundle_file = open_input(&request.archive_path, 1024 * MIB)?;
    verify_bundle(&mut bundle_file, &receipt)?;
    let candidate = json!({
        "schemaVersion": 1, "package": "prism-inference", "version": request.version,
        "repository": REPOSITORY, "commit": request.source_revision, "target": TARGET,
        "release": format!("v{}", request.version),
        "asset": { "filename": filename, "url": format!("{REPOSITORY}/releases/download/v{}/{filename}", request.version),
            "bytes": receipt.archive.bytes, "sha256": receipt.archive.sha256 },
        "files": receipt.files.iter().map(|file| json!({"name": file.name, "bytes": file.bytes, "sha256": file.sha256})).collect::<Vec<_>>(),
        "Source": source,
    });
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    let lane = std::env::temp_dir().join(format!(
        "SwiftBonsai-LockReview-{}-{stamp}",
        std::process::id()
    ));
    validate_external_path(&lane).map_err(|error| error.to_string())?;
    fs::create_dir(&lane).map_err(|error| error.to_string())?;
    write_new(&lane.join("candidate.lock.json"), &candidate)?;
    let result = json!({ "Path": request.output_path, "Version": request.version, "Commit": request.source_revision,
        "Target": TARGET, "RunId": request.run_id, "RunAttempt": request.run_attempt,
        "Verified": true, "VerificationDirectory": lane });
    write_new(&lane.join("verification.json"), &result)?;
    write_new(&request.output_path, &candidate)?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use std::io::{self, Read};

    use sha2::{Digest as _, Sha256};

    use super::{Identity, quoted_field, verify_entry};
    use crate::identity::hex;

    struct CappedReader<'a> {
        bytes: &'a [u8],
        maximum: usize,
    }

    impl Read for CappedReader<'_> {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            let count = buffer.len().min(self.maximum);
            self.bytes.read(&mut buffer[..count])
        }
    }

    #[test]
    fn arm64_entry_validation_retains_headers_across_short_and_buffer_boundary_reads()
    -> crate::Result<()> {
        for offset in [64_u32, 65534] {
            let position = usize::try_from(offset).map_err(|error| error.to_string())?;
            let mut image = vec![0; position + 24];
            image[..2].copy_from_slice(b"MZ");
            image[60..64].copy_from_slice(&offset.to_le_bytes());
            image[position..position + 6].copy_from_slice(&[b'P', b'E', 0, 0, 0x64, 0xaa]);
            let mut hash = Sha256::new();
            hash.update(&image);
            let mut identity = Identity {
                name: "prism-inference.exe".into(),
                bytes: u64::try_from(image.len()).map_err(|error| error.to_string())?,
                sha256: hex(&hash.finalize()),
            };
            for maximum in [3, 65536] {
                verify_entry(
                    &mut CappedReader {
                        bytes: &image,
                        maximum,
                    },
                    &identity,
                )?;
            }
            image[position + 5] = 0x86;
            let mut hash = Sha256::new();
            hash.update(&image);
            identity.sha256 = hex(&hash.finalize());
            assert!(
                verify_entry(
                    &mut CappedReader {
                        bytes: &image,
                        maximum: 3
                    },
                    &identity
                )
                .is_err()
            );
        }
        Ok(())
    }

    #[test]
    fn source_manifest_identity_rejects_duplicate_and_nonliteral_fields() -> crate::Result<()> {
        let valid = "[package]\nname = \"prism-inference\"\nversion = \"0.1.0\"\n[dependencies]\nname = \"other\"\n";
        assert_eq!(quoted_field(valid, "package", "name")?, "prism-inference");
        for invalid in [
            "[package]\nname = \"wrong\"\nname = \"prism-inference\"\n",
            "[package]\nname = \"prism-inference\"\n[package]\nname = \"prism-inference\"\n",
            "[package]\nname = [\"prism-inference\"]\n",
            "[package]\nname = \"prism-inference\\\"suffix\"\n",
        ] {
            assert!(quoted_field(invalid, "package", "name").is_err());
        }
        Ok(())
    }
}
