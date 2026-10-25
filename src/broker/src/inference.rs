//! Compiled release identity for the independently built inference server.
#![allow(
    clippy::redundant_pub_crate,
    reason = "Explicit parent scope keeps these implementation helpers private."
)]

use std::path::{Path, PathBuf};

use bonsai_artifacts::VerifiedFile;
use serde::Deserialize;

use crate::{Object, launcher::LaunchError};

const LOCK: &str = include_str!("../../../config/prism-inference.lock.json");
pub(super) const MODEL_SHA256: &str =
    "5912bb739217cf25b4283a098c7b893d9baff36bdf2e72d3dbf4cb999d5633d5";
pub(super) const TEMPLATE_SHA256: &str =
    "c3cf9e34abf4f9e36c2d72165aa9c132d3e2a725b6c2586aaa3a8af9d7a81041";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReleasePin {
    schema_version: u8,
    package: String,
    version: String,
    repository: String,
    commit: String,
    target: String,
    release: String,
    asset: Object<Asset>,
    files: Vec<Object<File>>,
    #[serde(rename = "Source")]
    source: Object<Source>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase", deny_unknown_fields)]
struct Source {
    repository: String,
    commit: String,
    url: String,
    bytes: u64,
    sha256: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Asset {
    filename: String,
    url: String,
    bytes: u64,
    sha256: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    name: String,
    bytes: u64,
    sha256: String,
}

fn hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn release(input: &str) -> Result<ReleasePin, LaunchError> {
    let Object(release): Object<ReleasePin> =
        serde_json::from_str(input).map_err(|_| LaunchError::Artifacts)?;
    let filename = format!("prism-inference-{}-{}.zip", release.version, release.target);
    if release.schema_version != 1
        || release.package != "prism-inference"
        || release.repository != "https://github.com/symbiogenesis/prism-inference"
        || release.target != "aarch64-pc-windows-msvc"
        || release.version.is_empty()
        || !release
            .version
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'.')
        || release.release != format!("v{}", release.version)
        || !hex(&release.commit, 40)
        || release.asset.filename != filename
        || release.asset.url
            != format!(
                "{}/releases/download/{}/{}",
                release.repository, release.release, filename
            )
        || release.asset.bytes == 0
        || !hex(&release.asset.sha256, 64)
        || release.source.repository != release.repository
        || release.source.commit != release.commit
        || release.source.url != format!("{}/archive/{}.zip", release.repository, release.commit)
        || release.source.bytes == 0
        || release.source.bytes > 256 * 1024 * 1024
        || !hex(&release.source.sha256, 64)
        || release.files.is_empty()
    {
        return Err(LaunchError::Artifacts);
    }
    let mut names = std::collections::HashSet::new();
    for Object(file) in &release.files {
        if file.bytes == 0
            || !hex(&file.sha256, 64)
            || !names.insert(file.name.as_str())
            || ![
                "prism-inference.exe",
                "bonsai-token-worker.exe",
                "bonsai-swift-worker.exe",
                "prism-inference-server.exe",
                "bonsai-gguf.exe",
                "LICENSE",
                "NOTICE",
                "PRISM-LICENSE.txt",
                "CAMELID-LICENSE.txt",
            ]
            .contains(&file.name.as_str())
        {
            return Err(LaunchError::Artifacts);
        }
    }
    if !names.contains("prism-inference-server.exe") {
        return Err(LaunchError::Artifacts);
    }
    Ok(release)
}

pub(super) fn verify(data: &Path) -> Result<(PathBuf, VerifiedFile), LaunchError> {
    let release = release(LOCK)?;
    let file = release
        .files
        .iter()
        .find(|Object(file)| file.name == "prism-inference-server.exe")
        .ok_or(LaunchError::Artifacts)?;
    let executable = data
        .join("inference")
        .join(&release.commit)
        .join(&release.target)
        .join(&file.name);
    let guard = VerifiedFile::verify(&executable, file.bytes, &file.sha256)
        .map_err(|_| LaunchError::Artifacts)?;
    Ok((executable, guard))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiled_server_release_is_pinned_and_rejects_path_or_identity_drift()
    -> Result<(), serde_json::Error> {
        assert!(release(LOCK).is_ok());
        let value: serde_json::Value = serde_json::from_str(LOCK)?;
        for (field, replacement) in [
            ("schemaVersion", serde_json::json!(2)),
            ("package", serde_json::json!("other")),
            ("target", serde_json::json!("../escape")),
            ("commit", serde_json::json!("../escape")),
            ("release", serde_json::json!("latest")),
            ("files", serde_json::json!([])),
        ] {
            let mut changed = value.clone();
            changed[field] = replacement;
            assert!(
                release(&serde_json::to_string(&changed)?).is_err(),
                "{field}"
            );
        }
        let mut changed = value.clone();
        changed["files"][0]["name"] = serde_json::json!("../prism-inference-server.exe");
        assert!(release(&serde_json::to_string(&changed)?).is_err());
        for (field, replacement) in [
            ("Repository", serde_json::json!("https://example.com/prism")),
            ("Commit", serde_json::json!("../escape")),
            ("Url", serde_json::json!("https://example.com/source.zip")),
            ("Bytes", serde_json::json!(0)),
            ("Bytes", serde_json::json!(268_435_457_u64)),
            ("Sha256", serde_json::json!("not-a-hash")),
        ] {
            let mut changed = value.clone();
            changed["Source"][field] = replacement;
            assert!(
                release(&serde_json::to_string(&changed)?).is_err(),
                "Source.{field}"
            );
        }
        let mut changed = value.clone();
        changed["Source"] = serde_json::json!([]);
        assert!(release(&serde_json::to_string(&changed)?).is_err());
        let mut changed = value.clone();
        changed["Source"]["unexpected"] = serde_json::json!(true);
        assert!(release(&serde_json::to_string(&changed)?).is_err());
        let mut changed = value;
        changed["asset"]["url"] = serde_json::json!("https://example.com/runtime.zip");
        assert!(release(&serde_json::to_string(&changed)?).is_err());
        Ok(())
    }
}
