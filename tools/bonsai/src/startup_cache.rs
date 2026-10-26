//! Content identities and checked executable receipts for native startup.
#![expect(
    clippy::redundant_pub_crate,
    reason = "Private startup modules expose only a crate-local implementation API."
)]

use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{Read as _, Seek as _, SeekFrom, Write as _},
    path::{Component, Path, PathBuf},
};

use bonsai_artifacts::{is_redirect, validate_external_path};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

pub(crate) fn hex(hash: &[u8]) -> String {
    hash.iter()
        .flat_map(|byte| {
            [
                char::from(b"0123456789abcdef"[usize::from(byte >> 4)]),
                char::from(b"0123456789abcdef"[usize::from(byte & 15)]),
            ]
        })
        .collect()
}

pub(crate) fn lock_file(path: &Path) -> crate::Result<File> {
    validate_external_path(path).map_err(|error| error.to_string())?;
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt as _;
        options.share_mode(0);
    }
    let guard = options
        .open(path)
        .map_err(|_| "Another process is preparing this startup cache.")?;
    guard
        .try_lock()
        .map_err(|_| "Another process is preparing this startup cache.")?;
    Ok(guard)
}

fn source_file(path: &Path) -> crate::Result<File> {
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if !metadata.is_file() || is_redirect(&metadata) {
        return Err("Build inputs must be ordinary files.".into());
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt as _;
        options.share_mode(1).custom_flags(0x0820_0000);
    }
    options.open(path).map_err(|error| error.to_string())
}

pub(crate) fn digest_file(path: &Path) -> crate::Result<String> {
    let mut file = source_file(path)?;
    let mut hash = Sha256::new();
    let mut buffer = vec![0; 65536];
    loop {
        let count = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(hex(&hash.finalize()))
}

pub(crate) fn assert_arm64(path: &Path) -> crate::Result<()> {
    let mut file = source_file(path)?;
    let length = file.metadata().map_err(|error| error.to_string())?.len();
    let mut dos = [0; 64];
    file.read_exact(&mut dos)
        .map_err(|_| "Invalid executable DOS header.")?;
    if dos[..2] != *b"MZ" {
        return Err("Invalid executable DOS header.".into());
    }
    let offset = u64::from(u32::from_le_bytes(
        dos[60..64].try_into().map_err(|_| "Invalid PE offset.")?,
    ));
    if offset < 64 || offset.checked_add(24).is_none_or(|end| end > length) {
        return Err("Invalid executable PE offset.".into());
    }
    file.seek(SeekFrom::Start(offset))
        .map_err(|error| error.to_string())?;
    let mut pe = [0; 6];
    file.read_exact(&mut pe)
        .map_err(|error| error.to_string())?;
    if pe != [b'P', b'E', 0, 0, 0x64, 0xaa] {
        return Err("Expected a native ARM64 PE image.".into());
    }
    Ok(())
}

fn part(hash: &mut Sha256, bytes: &[u8]) {
    hash.update(bytes.len().to_le_bytes());
    hash.update(bytes);
}

fn enumerate(root: &Path, directory: &Path, files: &mut BTreeSet<PathBuf>) -> crate::Result<()> {
    for entry in fs::read_dir(directory).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if matches!(
            name.as_ref(),
            ".git" | ".agents" | ".codex" | "target" | "node_modules"
        ) || (directory == root && matches!(name.as_ref(), "bin" | "obj" | "artifacts"))
            || (directory == root.join("tools/validation")
                && matches!(name.as_ref(), "bin" | "obj"))
        {
            continue;
        }
        let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
        if is_redirect(&metadata) {
            return Err("Reparse-point build inputs are unsupported.".into());
        }
        if metadata.is_dir() {
            enumerate(root, &path, files)?;
        } else if metadata.is_file() {
            files.insert(path);
        } else {
            return Err("Build inputs must be ordinary files or directories.".into());
        }
    }
    Ok(())
}

fn build_environment(name: &str) -> bool {
    let name = name.to_ascii_uppercase();
    [
        "CARGO",
        "RUST",
        "BONSAI",
        "CMAKE",
        "CFLAGS",
        "CXXFLAGS",
        "VSCMD",
        "VCTOOLS",
        "WINDOWSSDK",
        "UNIVERSALCRT",
        "TRANSCRIBE",
        "BINDGEN",
        "PKG_CONFIG",
        "PROGRAMFILES",
    ]
    .iter()
    .any(|prefix| name.starts_with(prefix))
        || matches!(
            name.as_str(),
            "CC" | "CXX"
                | "CL"
                | "_CL_"
                | "AR"
                | "RC"
                | "INCLUDE"
                | "LIB"
                | "LIBPATH"
                | "PATH"
                | "PATHEXT"
                | "SYSTEMROOT"
                | "SYSTEMDRIVE"
                | "WINDIR"
                | "PROGRAMDATA"
                | "USERPROFILE"
                | "LOCALAPPDATA"
                | "APPDATA"
                | "VSINSTALLDIR"
                | "VCINSTALLDIR"
                | "UCRTVERSION"
                | "HOST"
                | "TARGET"
                | "NUM_JOBS"
                | "OPT_LEVEL"
                | "PROFILE"
                | "DEBUG"
                | "SDKROOT"
        )
        || ["CC_", "CXX_", "AR_", "HOST_", "TARGET_"]
            .iter()
            .any(|prefix| name.starts_with(prefix))
}

fn assignments<'a>(content: &'a str, key: &str) -> Vec<&'a str> {
    let mut values = Vec::new();
    for (index, _) in content.match_indices(key) {
        if index > 0 && content.as_bytes()[index - 1].is_ascii_alphanumeric() {
            continue;
        }
        let suffix = &content[index + key.len()..];
        let Some(suffix) = suffix.trim_start().strip_prefix('=') else {
            continue;
        };
        let suffix = suffix.trim_start();
        let Some(quote @ ('\'' | '"')) = suffix.chars().next() else {
            values.push("");
            continue;
        };
        let suffix = &suffix[1..];
        if let Some(end) = suffix.find(quote) {
            values.push(&suffix[..end]);
        }
    }
    values
}

fn normalized(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                result.pop();
            }
            _ => result.push(component.as_os_str()),
        }
    }
    result
}

fn external_dependencies(content: &str, manifest: &Path, root: &Path) -> bool {
    assignments(content, "path").into_iter().any(|value| {
        let dependency = Path::new(value);
        let dependency = normalized(&if dependency.is_absolute() {
            dependency.to_owned()
        } else {
            manifest.parent().unwrap_or(root).join(dependency)
        });
        value.is_empty()
            || !dependency.starts_with(root)
            || dependency.strip_prefix(root).is_ok_and(|relative| {
                relative
                    .components()
                    .any(|part| part.as_os_str() == "target")
            })
    })
}

fn configs_from(directory: &Path, paths: &mut BTreeSet<PathBuf>) -> crate::Result<()> {
    for ancestor in directory.ancestors() {
        for name in ["config", "config.toml"] {
            let path = ancestor.join(".cargo").join(name);
            match fs::symlink_metadata(&path) {
                Ok(metadata) if metadata.is_file() && !is_redirect(&metadata) => {
                    paths.insert(path);
                }
                Ok(_) => return Err("Cargo configuration must be an ordinary file.".into()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.to_string()),
            }
        }
    }
    Ok(())
}

pub(crate) fn fingerprint(
    repository: &Path,
    working: &Path,
    identity: &str,
) -> crate::Result<Option<String>> {
    let environment = std::env::vars_os()
        .filter_map(|(name, value)| {
            name.to_str()
                .filter(|name| build_environment(name))
                .map(|name| {
                    (
                        name.to_ascii_uppercase(),
                        value.to_string_lossy().into_owned(),
                    )
                })
        })
        .collect();
    fingerprint_with(repository, working, identity, &environment)
}

fn fingerprint_with(
    repository: &Path,
    working: &Path,
    identity: &str,
    environment: &BTreeMap<String, String>,
) -> crate::Result<Option<String>> {
    if !repository.is_absolute()
        || !working.is_absolute()
        || repository
            .components()
            .any(|part| matches!(part, Component::ParentDir))
    {
        return Err("Build paths must be expanded and absolute.".into());
    }
    for ancestor in repository.ancestors() {
        if is_redirect(&fs::symlink_metadata(ancestor).map_err(|error| error.to_string())?) {
            return Err("Reparse-point repository storage is unsupported.".into());
        }
    }
    let mut files = BTreeSet::new();
    enumerate(repository, repository, &mut files)?;
    let mut hash = Sha256::new();
    part(&mut hash, b"bonsai-rust-startup-v1");
    part(&mut hash, repository.to_string_lossy().as_bytes());
    part(&mut hash, identity.as_bytes());
    let mut supported = true;
    for file in files {
        let relative = file
            .strip_prefix(repository)
            .map_err(|error| error.to_string())?;
        part(&mut hash, relative.to_string_lossy().as_bytes());
        part(&mut hash, digest_file(&file)?.as_bytes());
        if file.file_name().is_some_and(|name| name == "Cargo.toml") {
            supported &= !external_dependencies(
                &fs::read_to_string(&file).map_err(|error| error.to_string())?,
                &file,
                repository,
            );
        }
    }
    for (name, value) in environment {
        part(&mut hash, name.as_bytes());
        part(&mut hash, value.as_bytes());
    }
    for name in [
        "RUSTC",
        "RUSTC_WRAPPER",
        "RUSTC_WORKSPACE_WRAPPER",
        "CARGO_BUILD_RUSTC",
        "CARGO_BUILD_RUSTC_WRAPPER",
        "CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER",
    ] {
        supported &= environment.get(name).is_none_or(String::is_empty);
    }
    let mut configs = BTreeSet::new();
    configs_from(repository, &mut configs)?;
    configs_from(working, &mut configs)?;
    let home = environment
        .get("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            environment
                .get("USERPROFILE")
                .map(|profile| PathBuf::from(profile).join(".cargo"))
        });
    if let Some(home) = home {
        if home.is_absolute() {
            for name in ["config", "config.toml"] {
                let path = home.join(name);
                match fs::symlink_metadata(&path) {
                    Ok(metadata) if metadata.is_file() && !is_redirect(&metadata) => {
                        configs.insert(path);
                    }
                    Ok(_) => return Err("Cargo configuration must be an ordinary file.".into()),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error.to_string()),
                }
            }
        } else {
            supported = false;
        }
    }
    for config in configs {
        let content = fs::read_to_string(&config).map_err(|error| error.to_string())?;
        part(&mut hash, config.to_string_lossy().as_bytes());
        part(&mut hash, content.as_bytes());
        for key in [
            "paths",
            "path",
            "directory",
            "local-registry",
            "replace-with",
            "rustc",
            "rustc-wrapper",
            "rustc-workspace-wrapper",
            "linker",
        ] {
            supported &= assignments(&content, key).is_empty();
        }
    }
    Ok(supported.then(|| hex(&hash.finalize())))
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Output {
    path: PathBuf,
    sha256: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    version: u32,
    fingerprint: String,
    outputs: BTreeMap<String, Output>,
}

pub(crate) fn matches(
    receipt: &Path,
    fingerprint: &str,
    outputs: &BTreeMap<String, PathBuf>,
) -> bool {
    read_receipt(receipt, fingerprint, outputs).is_ok()
}

fn read_receipt(
    receipt: &Path,
    fingerprint: &str,
    outputs: &BTreeMap<String, PathBuf>,
) -> crate::Result<()> {
    validate_external_path(receipt).map_err(|error| error.to_string())?;
    let mut data = Vec::new();
    source_file(receipt)?
        .take(65537)
        .read_to_end(&mut data)
        .map_err(|error| error.to_string())?;
    if data.len() > 65536 {
        return Err("Build receipt is too large.".into());
    }
    let record: Receipt = serde_json::from_slice(&data).map_err(|error| error.to_string())?;
    if record.version != 1
        || record.fingerprint != fingerprint
        || record.outputs.len() != outputs.len()
    {
        return Err("Build receipt is stale.".into());
    }
    for (name, path) in outputs {
        validate_external_path(path).map_err(|error| error.to_string())?;
        let output = record
            .outputs
            .get(name)
            .ok_or("Build output is missing from receipt.")?;
        if output.path != *path || output.sha256 != digest_file(path)? {
            return Err("Build output digest or path changed.".into());
        }
        assert_arm64(path)?;
    }
    Ok(())
}

pub(crate) fn write_receipt(
    receipt: &Path,
    fingerprint: String,
    outputs: &BTreeMap<String, PathBuf>,
) -> crate::Result<()> {
    validate_external_path(receipt).map_err(|error| error.to_string())?;
    let outputs = outputs
        .iter()
        .map(|(name, path)| {
            Ok((
                name.clone(),
                Output {
                    path: path.clone(),
                    sha256: digest_file(path)?,
                },
            ))
        })
        .collect::<crate::Result<_>>()?;
    let record = Receipt {
        version: 1,
        fingerprint,
        outputs,
    };
    let temporary = receipt.with_extension(format!("{}.tmp", crate::startup_process::unique()));
    validate_external_path(&temporary).map_err(|error| error.to_string())?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|error| error.to_string())?;
    let bytes = serde_json::to_vec(&record).map_err(|error| error.to_string())?;
    file.write_all(&bytes).map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())?;
    drop(file);
    fs::rename(&temporary, receipt).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> crate::Result<(PathBuf, PathBuf)> {
        let scratch = std::env::temp_dir().join(format!(
            "Bonsai-RustCache-{}",
            crate::startup_process::unique()
        ));
        let repo = scratch.join("repository");
        fs::create_dir_all(repo.join("src")).map_err(|error| error.to_string())?;
        fs::write(repo.join("Cargo.toml"), "[package]\nname = 'fixture'\n")
            .map_err(|error| error.to_string())?;
        fs::write(repo.join("src/main.rs"), "first source").map_err(|error| error.to_string())?;
        Ok((scratch, repo))
    }

    #[test]
    fn fingerprint_tracks_contents_config_and_new_inputs() -> crate::Result<()> {
        let (scratch, repo) = fixture()?;
        let env = BTreeMap::new();
        let first = fingerprint_with(&repo, &scratch, "test", &env)?;
        assert!(first.is_some());
        assert_eq!(
            first,
            fingerprint_with(&repo, &scratch.join("another-launch"), "test", &env)?
        );
        let source = repo.join("src/main.rs");
        let old_time = fs::metadata(&source)
            .map_err(|error| error.to_string())?
            .modified()
            .map_err(|error| error.to_string())?;
        fs::write(&source, "other source").map_err(|error| error.to_string())?;
        OpenOptions::new()
            .write(true)
            .open(&source)
            .map_err(|error| error.to_string())?
            .set_modified(old_time)
            .map_err(|error| error.to_string())?;
        let changed = fingerprint_with(&repo, &scratch, "test", &env)?;
        assert_ne!(first, changed);
        fs::write(repo.join("src/new.rs"), "new").map_err(|error| error.to_string())?;
        assert_ne!(changed, fingerprint_with(&repo, &scratch, "test", &env)?);
        fs::remove_file(repo.join("src/new.rs")).map_err(|error| error.to_string())?;
        assert_eq!(changed, fingerprint_with(&repo, &scratch, "test", &env)?);
        fs::create_dir(repo.join(".cargo")).map_err(|error| error.to_string())?;
        fs::write(
            repo.join(".cargo/config.toml"),
            "[patch.crates-io]\nexample = { path = 'C:/external' }\n",
        )
        .map_err(|error| error.to_string())?;
        assert!(fingerprint_with(&repo, &scratch, "test", &env)?.is_none());
        assert!(!build_environment("WT_SESSION"));
        assert!(!build_environment("CODEX_THREAD_ID"));
        assert!(build_environment("RUSTFLAGS"));
        Ok(())
    }

    #[test]
    fn absolute_and_relative_external_sources_disable_reuse() {
        let root = std::env::temp_dir().join("repository");
        let manifest = root.join("src/app/Cargo.toml");
        assert!(external_dependencies(
            "dependency = { path = '../../../outside' }",
            &manifest,
            &root
        ));
        let absolute = std::env::temp_dir().join("outside");
        assert!(external_dependencies(
            &format!("dependency = {{ path = '{}' }}", absolute.display()),
            &manifest,
            &root
        ));
        assert!(!external_dependencies(
            "dependency = { path = '../local' }",
            &manifest,
            &root
        ));
    }

    #[test]
    fn receipts_reject_missing_malformed_changed_and_redirected_outputs() -> crate::Result<()> {
        let (scratch, _) = fixture()?;
        let executable = scratch.join("fixture.exe");
        let mut bytes = vec![0; 512];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[60..64].copy_from_slice(&128_u32.to_le_bytes());
        bytes[128..134].copy_from_slice(&[b'P', b'E', 0, 0, 0x64, 0xaa]);
        fs::write(&executable, bytes).map_err(|error| error.to_string())?;
        let receipt = scratch.join("build.json");
        let outputs = BTreeMap::from([("fixture".into(), executable.clone())]);
        assert!(!matches(&receipt, "fingerprint", &outputs));
        write_receipt(&receipt, "fingerprint".into(), &outputs)?;
        assert!(matches(&receipt, "fingerprint", &outputs));
        assert!(!matches(&receipt, "other", &outputs));
        fs::write(&executable, "changed output").map_err(|error| error.to_string())?;
        assert!(!matches(&receipt, "fingerprint", &outputs));
        fs::write(&receipt, "{").map_err(|error| error.to_string())?;
        assert!(!matches(&receipt, "fingerprint", &outputs));
        Ok(())
    }
}
