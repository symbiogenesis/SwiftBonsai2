//! Source-build entry points and unchanged-build reuse for native startup.

use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use bonsai_artifacts::validate_external_path;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

use crate::{startup_cache as cache, startup_process};

pub(crate) struct BuildSpec {
    pub(crate) repository_root: PathBuf,
    pub(crate) working_directory: PathBuf,
    pub(crate) target_directory: PathBuf,
    pub(crate) packages: Vec<String>,
    pub(crate) channel: String,
    pub(crate) target: Option<String>,
    pub(crate) identity: String,
    pub(crate) force: bool,
}

pub(crate) fn channel(repository: &Path) -> crate::Result<String> {
    if !repository.is_absolute() {
        return Err("RepositoryRoot must be expanded and absolute.".into());
    }
    for name in ["Cargo.toml", "Cargo.lock", "rust-toolchain.toml"] {
        if !repository.join(name).is_file() {
            return Err(format!("Source build input is missing: {name}"));
        }
    }
    let toolchain = fs::read_to_string(repository.join("rust-toolchain.toml"))
        .map_err(|error| error.to_string())?;
    let value = toolchain
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once('=')?;
            (name.trim() == "channel").then(|| value.trim().trim_matches('"'))
        })
        .ok_or("Source builds require the repository-pinned Rust compiler.")?;
    if value.len() != 18
        || !value.starts_with("nightly-")
        || !value[8..].bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 4 | 7) {
                byte == b'-'
            } else {
                byte.is_ascii_digit()
            }
        })
    {
        return Err("Source builds require a dated nightly compiler pin.".into());
    }
    Ok(format!("{value}-aarch64-pc-windows-msvc"))
}

pub(crate) fn cache_key(repository: &Path, channel: &str) -> String {
    crate::identity::hex(&Sha256::digest(
        format!("{}\n{channel}", repository.to_string_lossy().to_uppercase()).as_bytes(),
    ))
    // MSVC still rejects some archive paths beyond MAX_PATH. The full source
    // identity stays in the verified receipt; a 96-bit directory label leaves
    // room for Cargo's target, package and artifact names on ordinary profiles.
    .chars()
    .take(24)
    .collect()
}

fn outputs(spec: &BuildSpec) -> crate::Result<BTreeMap<String, PathBuf>> {
    let mut directory = spec.target_directory.clone();
    if let Some(target) = &spec.target {
        if target != "aarch64-pc-windows-msvc" {
            return Err("Source builds require the native ARM64 target.".into());
        }
        directory.push(target);
    }
    directory.push("release");
    let mut outputs = BTreeMap::new();
    if spec.packages.is_empty() {
        return Err("Source builds require at least one package.".into());
    }
    for package in &spec.packages {
        if package.is_empty()
            || !package
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        {
            return Err("Invalid source-build package name.".into());
        }
        let path = directory.join(format!("{package}.exe"));
        validate_external_path(&path).map_err(|error| error.to_string())?;
        outputs.insert(package.clone(), path);
    }
    Ok(outputs)
}

fn identity(spec: &BuildSpec) -> String {
    format!(
        "{}\n{}\n{}\n{}",
        spec.identity,
        spec.channel,
        spec.target.as_deref().unwrap_or("host"),
        spec.packages
            .iter()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    )
}

fn receipt_path(spec: &BuildSpec, identity: &str) -> PathBuf {
    spec.target_directory.join(format!(
        "startup-build-{}.json",
        crate::identity::hex(&Sha256::digest(identity.as_bytes()))
    ))
}

fn cargo_arguments(spec: &BuildSpec) -> Vec<OsString> {
    let mut args = vec![
        format!("+{}", spec.channel).into(),
        "build".into(),
        "--locked".into(),
        "--release".into(),
        "--manifest-path".into(),
        spec.repository_root.join("Cargo.toml").into_os_string(),
        "--target-dir".into(),
        spec.target_directory.clone().into_os_string(),
    ];
    if let Some(target) = &spec.target {
        args.extend(["--target".into(), target.into()]);
    }
    for package in &spec.packages {
        args.extend(["--package".into(), package.into()]);
    }
    args
}

pub(crate) fn cached_build(
    spec: &BuildSpec,
    prepare: impl FnOnce() -> crate::Result<BTreeMap<String, String>>,
) -> crate::Result<BTreeMap<String, PathBuf>> {
    cached_build_with(spec, prepare, |spec, environment, arguments, log| {
        let result = startup_process::run(
            "cargo.exe".as_ref(),
            arguments,
            &spec.working_directory,
            environment,
            Duration::from_secs(3600),
            log,
        )?;
        if result.code != 0 {
            return Err(format!(
                "Source build failed; see {}\n{}",
                log.display(),
                result.stderr
            ));
        }
        Ok(())
    })
}

fn cached_build_with(
    spec: &BuildSpec,
    prepare: impl FnOnce() -> crate::Result<BTreeMap<String, String>>,
    build: impl FnOnce(&BuildSpec, &BTreeMap<String, String>, &[OsString], &Path) -> crate::Result<()>,
) -> crate::Result<BTreeMap<String, PathBuf>> {
    crate::startup_cancel::checkpoint()?;
    validate_external_path(&spec.target_directory).map_err(|error| error.to_string())?;
    validate_external_path(&spec.working_directory).map_err(|error| error.to_string())?;
    fs::create_dir_all(&spec.target_directory).map_err(|error| error.to_string())?;
    fs::create_dir_all(&spec.working_directory).map_err(|error| error.to_string())?;
    let _guard = cache::lock_file(&spec.target_directory.join("build.lock"))?;
    let outputs = outputs(spec)?;
    let identity = identity(spec);
    let fingerprint =
        cache::fingerprint(&spec.repository_root, &spec.working_directory, &identity)?;
    let receipt = receipt_path(spec, &identity);
    cache::remove_abandoned_receipts(&receipt)?;
    if !spec.force
        && fingerprint
            .as_ref()
            .is_some_and(|fingerprint| cache::matches(&receipt, &identity, fingerprint, &outputs))
    {
        return Ok(outputs);
    }
    match fs::remove_file(&receipt) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.to_string()),
    }
    // Cargo tracks source contents, but does not attest a manually replaced
    // top-level executable. Remove only these fixed, externally owned outputs.
    for path in outputs.values() {
        match fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.to_string()),
        }
    }
    let mut environment = prepare()?;
    environment.insert("CARGO_BUILD_FINGERPRINT".into(), "content".into());
    environment.insert("CARGO_UNSTABLE_CHECKSUM_FRESHNESS".into(), "true".into());
    let arguments = cargo_arguments(spec);
    let log = spec
        .working_directory
        .join(format!("source-build-{}.log", startup_process::unique()));
    build(spec, &environment, &arguments, &log)?;
    for path in outputs.values() {
        cache::assert_arm64(path)?;
    }
    if let Some(fingerprint) = fingerprint
        && cache::fingerprint(&spec.repository_root, &spec.working_directory, &identity)?.as_ref()
            == Some(&fingerprint)
    {
        cache::write_receipt(&receipt, identity, fingerprint, &outputs)?;
    }
    Ok(outputs)
}

#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "kebab-case", deny_unknown_fields)]
enum Request {
    #[serde(rename_all = "camelCase")]
    Tool {
        repository_root: PathBuf,
        working_directory: PathBuf,
        target_directory: PathBuf,
        #[serde(default)]
        force_build: bool,
    },
    #[serde(rename_all = "camelCase")]
    Chat {
        repository_root: PathBuf,
        working_directory: PathBuf,
        data_directory: PathBuf,
        packages: Vec<String>,
        #[serde(default)]
        force_build: bool,
    },
}

/// Builds native startup executables only when their source or output identities change.
///
/// # Errors
/// Rejects unsupported requests, unsafe cache paths, concurrent preparation,
/// missing compiler inputs, failed builds and non-ARM64 executable outputs.
pub fn run(request: Value) -> crate::Result<Value> {
    let request: Request = serde_json::from_value(request).map_err(|error| error.to_string())?;
    match request {
        Request::Tool {
            repository_root,
            working_directory,
            target_directory,
            force_build,
        } => {
            let channel = channel(&repository_root)?;
            let spec = BuildSpec {
                repository_root,
                working_directory,
                target_directory,
                packages: vec!["bonsai-tools".into()],
                channel,
                target: None,
                identity: "maintenance".into(),
                force: force_build,
            };
            let paths = cached_build(&spec, || Ok(BTreeMap::new()))?;
            Ok(
                json!({"ToolPath": paths.get("bonsai-tools").ok_or("Missing maintenance build output.")?}),
            )
        }
        Request::Chat {
            repository_root,
            working_directory,
            data_directory,
            packages,
            force_build,
        } => {
            if packages
                .iter()
                .any(|name| !matches!(name.as_str(), "bonsai-chat" | "bonsai-broker"))
            {
                return Err("Unsupported native chat package.".into());
            }
            let channel = channel(&repository_root)?;
            let target_directory = data_directory
                .join("build/chat")
                .join(cache_key(&repository_root, &channel));
            let spec = BuildSpec {
                repository_root,
                working_directory,
                target_directory,
                packages,
                channel,
                target: Some("aarch64-pc-windows-msvc".into()),
                identity: "chat".into(),
                force: force_build,
            };
            let paths = cached_build(&spec, || Ok(BTreeMap::new()))?;
            Ok(
                json!({"ChatPath": paths.get("bonsai-chat"), "BrokerPath": paths.get("bonsai-broker"), "TargetDirectory": spec.target_directory}),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn cache_labels_leave_room_for_windows_linker_artifacts() {
        let repository = Path::new("C:/Users/symbi/source/repos/SwiftBonsai2Runner");
        let compiler = "nightly-2026-10-22-aarch64-pc-windows-msvc";
        let key = cache_key(repository, compiler);
        assert_eq!(key.len(), 24);
        assert_eq!(key, cache_key(repository, compiler));
        assert_ne!(key, cache_key(Path::new("C:/another-checkout"), compiler));
        let artifact = Path::new("C:/Users/symbi/AppData/Local/SwiftBonsai2")
            .join("build/native-speech")
            .join(key)
            .join("aarch64-pc-windows-msvc/release/build/bonsai-windows-credentials")
            .join("032c9bd086e4ceb8/out/libbonsai_windows_credentials-032c9bd086e4ceb8.rlib");
        assert!(artifact.as_os_str().len() < 240);
    }

    fn fixture() -> crate::Result<BuildSpec> {
        let scratch =
            std::env::temp_dir().join(format!("Bonsai-SourceBuild-{}", startup_process::unique()));
        let repository = scratch.join("repository");
        fs::create_dir_all(repository.join("src")).map_err(|error| error.to_string())?;
        fs::write(repository.join("src/main.rs"), "source").map_err(|error| error.to_string())?;
        Ok(BuildSpec {
            repository_root: repository,
            working_directory: scratch.join("work"),
            target_directory: scratch.join("target"),
            packages: vec!["bonsai-chat".into(), "bonsai-broker".into()],
            channel: "nightly-2026-10-22-aarch64-pc-windows-msvc".into(),
            target: Some("aarch64-pc-windows-msvc".into()),
            identity: "fixture".into(),
            force: false,
        })
    }

    fn fake_build(
        spec: &BuildSpec,
        environment: &BTreeMap<String, String>,
        arguments: &[OsString],
        _log: &Path,
    ) -> crate::Result<()> {
        assert_eq!(
            environment
                .get("CARGO_BUILD_FINGERPRINT")
                .map(String::as_str),
            Some("content")
        );
        assert_eq!(
            environment
                .get("CARGO_UNSTABLE_CHECKSUM_FRESHNESS")
                .map(String::as_str),
            Some("true")
        );
        assert_eq!(
            arguments.get(1).map(OsString::as_os_str),
            Some(std::ffi::OsStr::new("build"))
        );
        assert!(arguments.contains(&"--locked".into()));
        for path in outputs(spec)?.values() {
            fs::create_dir_all(path.parent().ok_or("Missing output parent.")?)
                .map_err(|error| error.to_string())?;
            let mut bytes = vec![0; 512];
            bytes[..2].copy_from_slice(b"MZ");
            bytes[60..64].copy_from_slice(&128_u32.to_le_bytes());
            bytes[128..134].copy_from_slice(&[b'P', b'E', 0, 0, 0x64, 0xaa]);
            fs::write(path, bytes).map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    #[test]
    fn warm_build_skips_preparation_and_cargo_but_repairs_changed_output() -> crate::Result<()> {
        let mut spec = fixture()?;
        let preparations = Cell::new(0);
        let builds = Cell::new(0);
        let prepare = || {
            preparations.set(preparations.get() + 1);
            Ok(BTreeMap::new())
        };
        let build =
            |spec: &BuildSpec, env: &BTreeMap<String, String>, args: &[OsString], log: &Path| {
                builds.set(builds.get() + 1);
                fake_build(spec, env, args, log)
            };
        let first = cached_build_with(&spec, prepare, build)?;
        let second = cached_build_with(&spec, prepare, build)?;
        assert_eq!(first, second);
        assert_eq!((preparations.get(), builds.get()), (1, 1));
        fs::write(
            first.get("bonsai-chat").ok_or("Missing chat fixture.")?,
            "modified executable",
        )
        .map_err(|error| error.to_string())?;
        let _ = cached_build_with(&spec, prepare, build)?;
        assert_eq!((preparations.get(), builds.get()), (2, 2));
        spec.force = true;
        let _ = cached_build_with(&spec, prepare, build)?;
        assert_eq!((preparations.get(), builds.get()), (3, 3));
        spec.force = false;
        let source = spec.repository_root.join("src/main.rs");
        let stamp = fs::metadata(&source)
            .map_err(|error| error.to_string())?
            .modified()
            .map_err(|error| error.to_string())?;
        fs::write(&source, "edited").map_err(|error| error.to_string())?;
        fs::OpenOptions::new()
            .write(true)
            .open(&source)
            .map_err(|error| error.to_string())?
            .set_modified(stamp)
            .map_err(|error| error.to_string())?;
        let _ = cached_build_with(&spec, prepare, build)?;
        assert_eq!((preparations.get(), builds.get()), (4, 4));
        Ok(())
    }

    #[test]
    fn failures_and_concurrent_builds_never_publish_paths() -> crate::Result<()> {
        let spec = fixture()?;
        assert!(
            cached_build_with(&spec, || Err("Preparation failed.".into()), fake_build).is_err()
        );
        assert!(!receipt_path(&spec, &identity(&spec)).exists());
        let guard = cache::lock_file(&spec.target_directory.join("build.lock"))?;
        assert!(cached_build_with(&spec, || Ok(BTreeMap::new()), fake_build).is_err());
        drop(guard);
        assert!(
            cached_build_with(
                &spec,
                || Ok(BTreeMap::new()),
                |_, _, _, _| Err("Compiler failed.".into())
            )
            .is_err()
        );
        assert!(!receipt_path(&spec, &identity(&spec)).exists());
        Ok(())
    }

    #[test]
    fn package_variants_reuse_receipts_only_while_exact_outputs_match() -> crate::Result<()> {
        let mut spec = fixture()?;
        let builds = Cell::new(0);
        let build =
            |spec: &BuildSpec, env: &BTreeMap<String, String>, args: &[OsString], log: &Path| {
                builds.set(builds.get() + 1);
                fake_build(spec, env, args, log)
            };
        let _ = cached_build_with(&spec, || Ok(BTreeMap::new()), build)?;
        let complete = receipt_path(&spec, &identity(&spec));
        spec.packages = vec!["bonsai-chat".into()];
        let narrow = receipt_path(&spec, &identity(&spec));
        let _ = cached_build_with(&spec, || Ok(BTreeMap::new()), build)?;
        assert!(complete.is_file() && narrow.is_file());
        spec.packages.push("bonsai-broker".into());
        spec.packages.reverse();
        let outputs = cached_build_with(&spec, || Err("Warm prepare ran.".into()), build)?;
        assert_eq!(builds.get(), 2);
        let chat = outputs.get("bonsai-chat").ok_or("Missing chat fixture.")?;
        let mut changed = fs::read(chat).map_err(|error| error.to_string())?;
        changed.push(1);
        fs::write(chat, changed).map_err(|error| error.to_string())?;
        let _ = cached_build_with(&spec, || Ok(BTreeMap::new()), build)?;
        assert_eq!(builds.get(), 3);
        Ok(())
    }

    #[test]
    fn documentation_edits_keep_warm_build_but_embedded_and_config_edits_rebuild()
    -> crate::Result<()> {
        let spec = fixture()?;
        fs::create_dir(spec.repository_root.join("docs")).map_err(|error| error.to_string())?;
        fs::create_dir(spec.repository_root.join("config")).map_err(|error| error.to_string())?;
        fs::write(spec.repository_root.join("README.md"), "embedded")
            .map_err(|error| error.to_string())?;
        fs::write(
            spec.repository_root.join("src/main.rs"),
            "const TEXT: &str = include_str!(\"../README.md\");",
        )
        .map_err(|error| error.to_string())?;
        let builds = Cell::new(0);
        let build =
            |spec: &BuildSpec, env: &BTreeMap<String, String>, args: &[OsString], log: &Path| {
                builds.set(builds.get() + 1);
                fake_build(spec, env, args, log)
            };
        let _ = cached_build_with(&spec, || Ok(BTreeMap::new()), build)?;
        fs::write(spec.repository_root.join("docs/guide.md"), "updated guide")
            .map_err(|error| error.to_string())?;
        let _ = cached_build_with(&spec, || Err("Documentation rebuilt.".into()), build)?;
        assert_eq!(builds.get(), 1);
        fs::write(spec.repository_root.join("README.md"), "changed embedded")
            .map_err(|error| error.to_string())?;
        let _ = cached_build_with(&spec, || Ok(BTreeMap::new()), build)?;
        assert_eq!(builds.get(), 2);
        fs::write(spec.repository_root.join("config/input.json"), "{}")
            .map_err(|error| error.to_string())?;
        let _ = cached_build_with(&spec, || Ok(BTreeMap::new()), build)?;
        assert_eq!(builds.get(), 3);
        Ok(())
    }

    #[test]
    fn interrupted_and_malformed_receipts_recover_without_blessing_partial_outputs()
    -> crate::Result<()> {
        let spec = fixture()?;
        let receipt = receipt_path(&spec, &identity(&spec));
        let _ = cached_build_with(&spec, || Ok(BTreeMap::new()), fake_build)?;
        fs::write(&receipt, "{\"version\":2,").map_err(|error| error.to_string())?;
        assert!(
            cached_build_with(
                &spec,
                || Ok(BTreeMap::new()),
                |spec, _, _, _| {
                    for path in outputs(spec)?.values() {
                        fs::write(path, "interrupted").map_err(|error| error.to_string())?;
                    }
                    Err("Compiler was interrupted.".into())
                }
            )
            .is_err()
        );
        assert!(!receipt.exists());
        let _ = cached_build_with(&spec, || Ok(BTreeMap::new()), fake_build)?;
        let abandoned = receipt.with_file_name(format!(
            "{}.1-2-3.tmp",
            receipt
                .file_name()
                .ok_or("Missing receipt name.")?
                .to_string_lossy()
        ));
        fs::write(&abandoned, "{partial").map_err(|error| error.to_string())?;
        let unrelated = spec.target_directory.join("unrelated.tmp");
        fs::write(&unrelated, "keep").map_err(|error| error.to_string())?;
        let _ = cached_build_with(
            &spec,
            || Err("Warm prepare ran.".into()),
            |_, _, _, _| Err("Warm Cargo ran.".into()),
        )?;
        assert!(!abandoned.exists());
        assert!(unrelated.exists());
        Ok(())
    }

    #[test]
    fn source_commands_reject_unknown_fields_and_packages() {
        assert!(run(json!({"operation": "chat", "unknown": true})).is_err());
        assert!(run(json!({"operation": "chat", "repositoryRoot": "relative", "workingDirectory": "relative", "dataDirectory": "relative", "packages": ["arbitrary"]})).is_err());
    }
}
