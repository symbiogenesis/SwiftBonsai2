//! Bounded, fixed-layout validation of installed native chat packages.

use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::Read as _,
    path::{Path, PathBuf},
};

use bonsai_artifacts::{is_redirect, validate_external_path};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

const MAX_METADATA_BYTES: u64 = 32_768;
const FILES: &[&str] = &[
    "bin/bonsai-chat.exe",
    "bin/bonsai-broker.exe",
    "bin/bonsai-asr.exe",
    "bin/bonsai-tts.exe",
    "bin/bonsai-tools.exe",
    "LICENSE",
    "NOTICE",
    "THIRD-PARTY-NOTICES.txt",
    "scripts/Start-Chat.ps1",
    "scripts/Start-InstalledChat.ps1",
    "scripts/Install-ChatPackage.ps1",
    "scripts/Bonsai.ChatPackage.psm1",
    "scripts/Bonsai.Common.psm1",
    "scripts/Bonsai.Native.psm1",
    "scripts/Bonsai.Tools.psm1",
    "scripts/Bonsai.NativeSpeech.psm1",
    "scripts/Get-NativeSpeechModels.ps1",
    "scripts/New-DevelopmentKeys.ps1",
];

#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "kebab-case", deny_unknown_fields)]
enum Request {
    #[serde(rename_all = "camelCase")]
    Installed { installation_directory: PathBuf },
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Selection {
    schema_version: u64,
    #[serde(deserialize_with = "nullable")]
    active: Option<Selected>,
    #[serde(deserialize_with = "nullable")]
    previous: Option<Selected>,
}

fn nullable<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Selected {
    id: String,
    manifest_sha256: String,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Manifest {
    schema_version: u64,
    kind: String,
    version: String,
    source_revision: String,
    target: String,
    files: Vec<PackageFile>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PackageFile {
    name: String,
    bytes: u64,
    sha256: String,
}

fn open(path: &Path) -> crate::Result<File> {
    validate_external_path(path).map_err(|error| error.to_string())?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt as _;
        options.share_mode(1).custom_flags(0x0820_0000);
    }
    let file = options.open(path).map_err(|error| error.to_string())?;
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    if !metadata.is_file() || is_redirect(&metadata) {
        return Err("Package inputs must be ordinary files.".into());
    }
    Ok(file)
}

fn metadata(path: &Path) -> crate::Result<Vec<u8>> {
    let file = open(path)?;
    if file.metadata().map_err(|error| error.to_string())?.len() > MAX_METADATA_BYTES {
        return Err("Package metadata exceeds its byte limit.".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_METADATA_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if u64::try_from(bytes.len()).map_err(|error| error.to_string())? > MAX_METADATA_BYTES {
        return Err("Package metadata exceeds its byte limit.".into());
    }
    Ok(bytes)
}

fn json_bytes(bytes: &[u8]) -> &[u8] {
    // PowerShell's existing metadata reader accepts a UTF-8 BOM.
    bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes)
}

fn version(text: &str) -> bool {
    let parts: Vec<_> = text.split('.').collect();
    text.len() <= 32
        && parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
}

fn selected(item: &Selected) -> crate::Result<()> {
    let mut parts = item.id.split('-');
    let valid = item.id.len() <= 138
        && parts.next().is_some_and(version)
        && parts
            .next()
            .is_some_and(|part| crate::identity::is_lower_hex(part, 40))
        && parts.next() == Some(item.manifest_sha256.as_str())
        && parts.next().is_none()
        && crate::identity::is_lower_hex(&item.manifest_sha256, 64);
    if !valid {
        return Err("Invalid installed package selection.".into());
    }
    Ok(())
}

fn verify_file(directory: &Path, entry: &PackageFile) -> crate::Result<()> {
    let limit = if entry.name.starts_with("bin/") {
        512 * 1024 * 1024
    } else {
        16 * 1024 * 1024
    };
    if entry.bytes == 0 || entry.bytes > limit || !crate::identity::is_lower_hex(&entry.sha256, 64)
    {
        return Err("Invalid package file identity or byte bounds.".into());
    }
    let path = directory.join(&entry.name);
    let mut file = open(&path)?;
    if file.metadata().map_err(|error| error.to_string())?.len() != entry.bytes {
        return Err(format!("Package byte length mismatch: {}", entry.name));
    }
    let mut digest = Sha256::new();
    let mut buffer = [0; 65_536];
    loop {
        let count = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    if crate::identity::hex(&digest.finalize()) != entry.sha256 {
        return Err(format!("Package SHA-256 mismatch: {}", entry.name));
    }
    if entry.name.starts_with("bin/") {
        // Keep the deny-write handle alive while the ARM64 header is checked.
        crate::startup_cache::assert_arm64(&path)?;
    }
    Ok(())
}

fn inventory(directory: &Path) -> crate::Result<()> {
    let mut count = 0;
    for container in [
        directory.to_path_buf(),
        directory.join("bin"),
        directory.join("scripts"),
    ] {
        validate_external_path(&container).map_err(|error| error.to_string())?;
        for entry in fs::read_dir(container).map_err(|error| error.to_string())? {
            let path = entry.map_err(|error| error.to_string())?.path();
            validate_external_path(&path).map_err(|error| error.to_string())?;
            let relative = path
                .strip_prefix(directory)
                .map_err(|error| error.to_string())?
                .to_string_lossy()
                .replace('\\', "/");
            let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
            if is_redirect(&metadata) {
                return Err("Package links are unsupported.".into());
            }
            if metadata.is_dir() {
                if !matches!(relative.as_str(), "bin" | "scripts") {
                    return Err("Unexpected directory in chat package.".into());
                }
            } else if metadata.is_file()
                && (relative == "package.json" || FILES.contains(&relative.as_str()))
            {
                count += 1;
            } else {
                return Err("Unexpected files in chat package.".into());
            }
        }
    }
    if count != FILES.len() + 1 {
        return Err("Unexpected files in chat package.".into());
    }
    Ok(())
}

fn installed(root: &Path) -> crate::Result<Value> {
    validate_external_path(root).map_err(|error| error.to_string())?;
    let selection: Selection =
        serde_json::from_slice(json_bytes(&metadata(&root.join("active.json"))?))
            .map_err(|error| error.to_string())?;
    if selection.schema_version != 1 {
        return Err("Unsupported package selection.".into());
    }
    let active = selection.active.ok_or("An active package is required.")?;
    selected(&active)?;
    if let Some(previous) = &selection.previous {
        selected(previous)?;
    }
    let directory = root.join("versions").join(&active.id);
    let path = directory.join("package.json");
    // Hash and parse the same bounded bytes, including a BOM in the digest.
    let bytes = metadata(&path)?;
    if crate::identity::sha256(&bytes) != active.manifest_sha256 {
        return Err("Package manifest SHA-256 mismatch.".into());
    }
    let manifest: Manifest =
        serde_json::from_slice(json_bytes(&bytes)).map_err(|error| error.to_string())?;
    if manifest.schema_version != 1
        || manifest.kind != "bonsai-chat-package"
        || manifest.target != "aarch64-pc-windows-msvc"
        || !version(&manifest.version)
        || !crate::identity::is_lower_hex(&manifest.source_revision, 40)
        || manifest.files.len() != FILES.len()
    {
        return Err("Unsupported chat package identity or inventory.".into());
    }
    let id = format!(
        "{}-{}-{}",
        manifest.version, manifest.source_revision, active.manifest_sha256
    );
    if id != active.id {
        return Err("Selected package identity disagrees with its manifest.".into());
    }
    let mut seen = BTreeSet::new();
    for entry in &manifest.files {
        if !FILES.contains(&entry.name.as_str()) || !seen.insert(&entry.name) {
            return Err("Invalid or repeated package filename.".into());
        }
        verify_file(&directory, entry)?;
    }
    inventory(&directory)?;
    Ok(json!({
        "Directory": directory, "ManifestSha256": active.manifest_sha256,
        "Manifest": manifest, "Id": id,
    }))
}

/// Verify the active package without invoking a compiler or loading a model.
///
/// # Errors
/// Rejects unsafe paths, malformed or oversized metadata, changed package bytes,
/// non-ARM64 executables, and any inventory outside the fixed package layout.
pub fn run(value: Value) -> crate::Result<Value> {
    let request: Request = serde_json::from_value(value).map_err(|error| error.to_string())?;
    match request {
        Request::Installed {
            installation_directory,
        } => installed(&installation_directory),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn fixture() -> crate::Result<PathBuf> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("Bonsai-package-{}-{nonce}", std::process::id()));
        let stage = root.join("stage");
        let mut files = Vec::new();
        for name in FILES {
            let path = stage.join(name);
            fs::create_dir_all(path.parent().ok_or("Missing package parent.")?)
                .map_err(|error| error.to_string())?;
            let bytes = if name.starts_with("bin/") {
                let mut pe = vec![0; 512];
                pe[..2].copy_from_slice(b"MZ");
                pe[60..64].copy_from_slice(&128_u32.to_le_bytes());
                pe[128..134].copy_from_slice(&[b'P', b'E', 0, 0, 0x64, 0xaa]);
                pe
            } else {
                b"fixture".to_vec()
            };
            fs::write(path, &bytes).map_err(|error| error.to_string())?;
            files.push(PackageFile {
                name: (*name).into(),
                bytes: u64::try_from(bytes.len()).map_err(|error| error.to_string())?,
                sha256: crate::identity::sha256(&bytes),
            });
        }
        let manifest = Manifest {
            schema_version: 1,
            kind: "bonsai-chat-package".into(),
            version: "0.1.0".into(),
            source_revision: "a".repeat(40),
            target: "aarch64-pc-windows-msvc".into(),
            files,
        };
        let bytes = serde_json::to_vec(&manifest).map_err(|error| error.to_string())?;
        let digest = crate::identity::sha256(&bytes);
        fs::write(stage.join("package.json"), bytes).map_err(|error| error.to_string())?;
        let id = format!("0.1.0-{}-{digest}", "a".repeat(40));
        fs::create_dir(root.join("versions")).map_err(|error| error.to_string())?;
        fs::rename(stage, root.join("versions").join(&id)).map_err(|error| error.to_string())?;
        fs::write(root.join("active.json"), json!({
            "schemaVersion": 1, "active": {"id": id, "manifestSha256": digest}, "previous": null,
        }).to_string()).map_err(|error| error.to_string())?;
        Ok(root)
    }

    #[test]
    fn installed_package_rejects_tampering_extras_and_pointer_traversal() -> crate::Result<()> {
        let root = fixture()?;
        let result = installed(&root)?;
        let directory = PathBuf::from(result["Directory"].as_str().ok_or("Missing directory.")?);
        let notice = directory.join("NOTICE");
        fs::write(&notice, b"changed").map_err(|error| error.to_string())?;
        assert!(installed(&root).is_err());
        fs::write(&notice, b"fixture").map_err(|error| error.to_string())?;
        fs::create_dir(directory.join("extra")).map_err(|error| error.to_string())?;
        assert!(installed(&root).is_err());
        fs::remove_dir(directory.join("extra")).map_err(|error| error.to_string())?;
        assert!(installed(&root).is_ok());
        fs::write(
            root.join("active.json"),
            json!({
                "schemaVersion": 1, "active": {"id": "../escape", "manifestSha256": "a".repeat(64)},
                "previous": null,
            })
            .to_string(),
        )
        .map_err(|error| error.to_string())?;
        assert!(installed(&root).is_err());
        fs::remove_dir_all(root).map_err(|error| error.to_string())?;
        Ok(())
    }

    #[test]
    fn package_metadata_rejects_duplicate_unknown_missing_and_oversized_fields() -> crate::Result<()>
    {
        for text in [
            r#"{"schemaVersion":1,"schemaVersion":1,"active":null,"previous":null}"#,
            r#"{"schemaVersion":1,"active":null}"#,
            r#"{"schemaVersion":1,"active":null,"previous":null,"extra":0}"#,
        ] {
            assert!(serde_json::from_str::<Selection>(text).is_err());
        }
        let root = fixture()?;
        fs::write(root.join("active.json"), vec![b' '; 32_769])
            .map_err(|error| error.to_string())?;
        assert!(installed(&root).is_err());
        fs::remove_dir_all(root).map_err(|error| error.to_string())?;
        Ok(())
    }
}
