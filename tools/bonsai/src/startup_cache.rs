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
    crate::startup_cancel::checkpoint()?;
    let mut file = source_file(path)?;
    let mut hash = Sha256::new();
    let mut buffer = vec![0; 65536];
    loop {
        crate::startup_cancel::checkpoint()?;
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

fn non_build_input(root: &Path, path: &Path) -> bool {
    let Ok(relative) = path.strip_prefix(root) else {
        return false;
    };
    let mut parts = relative.components();
    let Some(first) = parts.next() else {
        return false;
    };
    let first = first.as_os_str();
    matches!(first.to_str(), Some("docs" | ".github" | "scripts"))
        || (parts.next().is_none()
            && path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("md")))
        || (first == "tests"
            && path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("ps1")))
}

fn enumerate(root: &Path, directory: &Path, files: &mut BTreeSet<PathBuf>) -> crate::Result<()> {
    crate::startup_cancel::checkpoint()?;
    for entry in fs::read_dir(directory).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if non_build_input(root, &path)
            || matches!(
                name.as_ref(),
                ".git" | ".agents" | ".codex" | "target" | "node_modules"
            )
            || (directory == root && matches!(name.as_ref(), "bin" | "obj" | "artifacts"))
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
    if matches!(
        name.as_str(),
        "RUST_LOG" | "RUST_BACKTRACE" | "RUST_LIB_BACKTRACE"
    ) {
        return false;
    }
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

#[derive(Debug)]
enum RustToken<'a> {
    Word(&'a str),
    Literal(Option<String>),
    Punctuation(u8),
}

// Only include expressions need parsing. Skip comments and complete literals so
// examples, tests and documentation mentioning include macros are never inputs.
fn rust_tokens(source: &str) -> Vec<RustToken<'_>> {
    let bytes = source.as_bytes();
    let mut index = 0;
    let mut tokens = Vec::new();
    while index < bytes.len() {
        if bytes[index..].starts_with(b"//") {
            index += bytes[index..]
                .iter()
                .position(|byte| *byte == b'\n')
                .unwrap_or(bytes.len() - index);
        } else if bytes[index..].starts_with(b"/*") {
            index += 2;
            let mut depth = 1;
            while index < bytes.len() && depth > 0 {
                if bytes[index..].starts_with(b"/*") {
                    depth += 1;
                    index += 2;
                } else if bytes[index..].starts_with(b"*/") {
                    depth -= 1;
                    index += 2;
                } else {
                    index += 1;
                }
            }
        } else if let Some((end, literal)) = rust_literal(source, index) {
            tokens.push(RustToken::Literal(literal));
            index = end;
        } else if bytes[index].is_ascii_alphabetic() || bytes[index] == b'_' {
            let start = index;
            index += 1;
            while index < bytes.len()
                && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_')
            {
                index += 1;
            }
            tokens.push(RustToken::Word(&source[start..index]));
        } else {
            if !bytes[index].is_ascii_whitespace() {
                tokens.push(RustToken::Punctuation(bytes[index]));
            }
            index += 1;
        }
    }
    tokens
}

fn rust_literal(source: &str, start: usize) -> Option<(usize, Option<String>)> {
    let bytes = source.as_bytes();
    if matches!(bytes[start], b'b' | b'c')
        && bytes
            .get(start + 1)
            .is_some_and(|byte| matches!(*byte, b'r' | b'"' | b'\''))
    {
        return rust_literal(source, start + 1).map(|(end, _)| (end, None));
    }
    if bytes[start] == b'\'' {
        let content = &source[start + 1..];
        let character = content.chars().next()?;
        let mut end = start + 1 + character.len_utf8();
        if character == '\\' {
            let escaped = *bytes.get(end)?;
            end += 1;
            if escaped == b'x' {
                end += 2;
            } else if escaped == b'u' {
                end += source.get(end..)?.find('}')? + 1;
            }
        }
        return (bytes.get(end) == Some(&b'\'')).then_some((end + 1, None));
    }
    if bytes[start] == b'r' {
        let hashes = bytes[start + 1..]
            .iter()
            .take_while(|byte| **byte == b'#')
            .count();
        let quote = start + hashes + 1;
        if bytes.get(quote) == Some(&b'"') {
            let ending = format!("\"{}", "#".repeat(hashes));
            let content = quote + 1;
            let end = source[content..]
                .find(&ending)
                .map_or(bytes.len(), |end| content + end);
            return Some((
                (end + ending.len()).min(bytes.len()),
                Some(source[content..end].into()),
            ));
        }
    }
    if bytes[start] != b'"' {
        return None;
    }
    let mut end = start + 1;
    while end < bytes.len() {
        if bytes[end] == b'\\' {
            end = (end + 2).min(bytes.len());
        } else if bytes[end] == b'"' {
            end += 1;
            return Some((end, serde_json::from_str(&source[start..end]).ok()));
        } else {
            end += 1;
        }
    }
    Some((end, None))
}

fn punctuation(tokens: &[RustToken<'_>], index: &mut usize, value: u8) -> Option<()> {
    if matches!(tokens.get(*index), Some(RustToken::Punctuation(found)) if *found == value) {
        *index += 1;
        Some(())
    } else {
        None
    }
}

fn include_expression(
    tokens: &[RustToken<'_>],
    index: &mut usize,
    manifest: &Path,
    depth: usize,
) -> Option<String> {
    if depth > 8 {
        return None;
    }
    if let Some(RustToken::Literal(value)) = tokens.get(*index) {
        *index += 1;
        return value.clone();
    }
    let Some(RustToken::Word(name @ ("concat" | "env"))) = tokens.get(*index) else {
        return None;
    };
    *index += 1;
    punctuation(tokens, index, b'!')?;
    punctuation(tokens, index, b'(')?;
    if *name == "env" {
        let value = include_expression(tokens, index, manifest, depth + 1)?;
        if value != "CARGO_MANIFEST_DIR" {
            return None;
        }
        punctuation(tokens, index, b')')?;
        return Some(manifest.to_string_lossy().into_owned());
    }
    let mut value = String::new();
    loop {
        value.push_str(&include_expression(tokens, index, manifest, depth + 1)?);
        if punctuation(tokens, index, b',').is_none() {
            break;
        }
        if matches!(tokens.get(*index), Some(RustToken::Punctuation(b')'))) {
            break;
        }
    }
    punctuation(tokens, index, b')')?;
    Some(value)
}

fn embedded_inputs(repository: &Path, files: &mut BTreeSet<PathBuf>) -> crate::Result<bool> {
    let mut pending = files
        .iter()
        .filter(|path| path.extension().is_some_and(|extension| extension == "rs"))
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut parsed = BTreeSet::new();
    let mut supported = true;
    while let Some(file) = pending.pop_first() {
        if !parsed.insert(file.clone()) {
            continue;
        }
        let source = fs::read_to_string(&file).map_err(|error| error.to_string())?;
        if !source.contains("include") {
            continue;
        }
        let tokens = rust_tokens(&source);
        let manifest = file
            .ancestors()
            .find(|path| path.join("Cargo.toml").is_file())
            .unwrap_or(repository);
        for (index, token) in tokens.iter().enumerate() {
            let RustToken::Word(name @ ("include" | "include_str" | "include_bytes")) = token
            else {
                continue;
            };
            let mut cursor = index + 1;
            if punctuation(&tokens, &mut cursor, b'!').is_none() {
                continue;
            }
            let ending = match tokens.get(cursor) {
                Some(RustToken::Punctuation(b'(')) => b')',
                Some(RustToken::Punctuation(b'{')) => b'}',
                Some(RustToken::Punctuation(b'[')) => b']',
                _ => {
                    supported = false;
                    continue;
                }
            };
            cursor += 1;
            let Some(value) = include_expression(&tokens, &mut cursor, manifest, 0) else {
                supported = false;
                continue;
            };
            let _ = punctuation(&tokens, &mut cursor, b',');
            if punctuation(&tokens, &mut cursor, ending).is_none() {
                supported = false;
                continue;
            }
            let input = normalized(&file.parent().unwrap_or(repository).join(value));
            if !input.starts_with(repository) {
                supported = false;
                continue;
            }
            if !files.contains(&input) {
                for ancestor in input
                    .ancestors()
                    .take_while(|path| path.starts_with(repository))
                {
                    if is_redirect(
                        &fs::symlink_metadata(ancestor).map_err(|error| error.to_string())?,
                    ) {
                        return Err("Reparse-point embedded inputs are unsupported.".into());
                    }
                }
            }
            files.insert(input.clone());
            if *name == "include" {
                pending.insert(input);
            }
        }
    }
    Ok(supported)
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
            || non_build_input(root, &dependency)
            || dependency.strip_prefix(root).is_ok_and(|relative| {
                relative.components().any(|part| {
                    matches!(
                        part.as_os_str().to_str(),
                        Some(".git" | ".agents" | ".codex" | "target" | "node_modules")
                    )
                }) || relative.starts_with("bin")
                    || relative.starts_with("obj")
                    || relative.starts_with("artifacts")
                    || relative.starts_with("tools/validation/bin")
                    || relative.starts_with("tools/validation/obj")
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
    crate::startup_cancel::checkpoint()?;
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
    let mut supported = embedded_inputs(repository, &mut files)?;
    let mut hash = Sha256::new();
    part(&mut hash, b"bonsai-rust-startup-v2");
    part(&mut hash, repository.to_string_lossy().as_bytes());
    part(&mut hash, identity.as_bytes());
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
    identity: String,
    fingerprint: String,
    outputs: BTreeMap<String, Output>,
}

pub(crate) fn matches(
    receipt: &Path,
    identity: &str,
    fingerprint: &str,
    outputs: &BTreeMap<String, PathBuf>,
) -> bool {
    read_receipt(receipt, identity, fingerprint, outputs).is_ok()
}

fn read_receipt(
    receipt: &Path,
    identity: &str,
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
    if record.version != 2
        || record.identity != identity
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

// Called under the target-directory build lock. A crash can leave only an
// unpublished temporary; it must never be read as a receipt or block a retry.
pub(crate) fn remove_abandoned_receipts(receipt: &Path) -> crate::Result<()> {
    validate_external_path(receipt).map_err(|error| error.to_string())?;
    let directory = receipt.parent().ok_or("Missing receipt directory.")?;
    let prefix = format!(
        "{}.",
        receipt
            .file_name()
            .ok_or("Missing receipt name.")?
            .to_string_lossy()
    );
    for entry in fs::read_dir(directory).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let Some(unique) = name
            .strip_prefix(&prefix)
            .and_then(|name| name.strip_suffix(".tmp"))
        else {
            continue;
        };
        let parts = unique.split('-').collect::<Vec<_>>();
        if parts.len() != 3
            || parts
                .iter()
                .any(|part| part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()))
        {
            continue;
        }
        let path = entry.path();
        validate_external_path(&path).map_err(|error| error.to_string())?;
        fs::remove_file(path).map_err(|error| error.to_string())?;
    }
    Ok(())
}

struct TemporaryReceipt(PathBuf);

impl Drop for TemporaryReceipt {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

pub(crate) fn write_receipt(
    receipt: &Path,
    identity: String,
    fingerprint: String,
    outputs: &BTreeMap<String, PathBuf>,
) -> crate::Result<()> {
    validate_external_path(receipt).map_err(|error| error.to_string())?;
    let outputs = outputs
        .iter()
        .map(|(name, path)| {
            assert_arm64(path)?;
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
        version: 2,
        identity,
        fingerprint,
        outputs,
    };
    let temporary = receipt.with_file_name(format!(
        "{}.{}.tmp",
        receipt
            .file_name()
            .ok_or("Missing receipt name.")?
            .to_string_lossy(),
        crate::startup_process::unique()
    ));
    validate_external_path(&temporary).map_err(|error| error.to_string())?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|error| error.to_string())?;
    let temporary = TemporaryReceipt(temporary);
    let bytes = serde_json::to_vec(&record).map_err(|error| error.to_string())?;
    file.write_all(&bytes).map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())?;
    drop(file);
    fs::rename(&temporary.0, receipt).map_err(|error| error.to_string())
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
        assert!(external_dependencies(
            "dependency = { path = '../../docs/local' }",
            &manifest,
            &root
        ));
    }

    #[test]
    fn non_build_edits_are_ignored_but_all_embedded_inputs_are_hashed() -> crate::Result<()> {
        let (scratch, repo) = fixture()?;
        let environment = BTreeMap::new();
        let before = fingerprint_with(&repo, &scratch, "test", &environment)?;
        for (path, content) in [
            ("README.md", "readme"),
            ("docs/guide.md", "guide"),
            (".github/workflows/build.yml", "workflow"),
            ("scripts/Start-Chat.ps1", "script"),
            ("tests/Test-Chat.ps1", "test"),
        ] {
            let path = repo.join(path);
            fs::create_dir_all(path.parent().ok_or("Missing fixture directory.")?)
                .map_err(|error| error.to_string())?;
            fs::write(path, content).map_err(|error| error.to_string())?;
        }
        assert_eq!(
            before,
            fingerprint_with(&repo, &scratch, "test", &environment)?
        );
        fs::write(
            repo.join("docs/source.inc"),
            "const NESTED: &str = include_str!(\"../README.md\");",
        )
        .map_err(|error| error.to_string())?;
        fs::write(
            repo.join("src/main.rs"),
            r##"
            let quote = b'"'; let character = '\''; let value: &'static str = "";
            let example = r#"include_str!(unknown!())"#;
            /* include_bytes!(unknown!()) /* nested */ */
            const README: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/README.md"));
            const SCRIPT: &[u8] = include_bytes!(r#"../scripts/Start-Chat.ps1"#);
            include!("../docs/source.inc");
        "##,
        )
        .map_err(|error| error.to_string())?;
        let embedded = fingerprint_with(&repo, &scratch, "test", &environment)?;
        assert!(embedded.is_some());
        fs::write(
            repo.join("scripts/Start-Chat.ps1"),
            "edited embedded script",
        )
        .map_err(|error| error.to_string())?;
        assert_ne!(
            embedded,
            fingerprint_with(&repo, &scratch, "test", &environment)?
        );
        fs::write(
            repo.join("src/main.rs"),
            "include_str!(concat!(env!(\"UNKNOWN_INPUT\"), \"/file\"));",
        )
        .map_err(|error| error.to_string())?;
        assert!(fingerprint_with(&repo, &scratch, "test", &environment)?.is_none());
        assert!(!build_environment("RUST_LOG"));
        assert!(!build_environment("RUST_BACKTRACE"));
        Ok(())
    }

    #[test]
    fn checked_in_repository_embeds_have_complete_content_identities() -> crate::Result<()> {
        let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .ok_or("Missing workspace root.")?;
        let mut files = BTreeSet::new();
        enumerate(repository, repository, &mut files)?;
        assert!(embedded_inputs(repository, &mut files)?);
        Ok(())
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
        assert!(!matches(&receipt, "variant", "fingerprint", &outputs));
        write_receipt(&receipt, "variant".into(), "fingerprint".into(), &outputs)?;
        assert!(matches(&receipt, "variant", "fingerprint", &outputs));
        assert!(!matches(
            &receipt,
            "another variant",
            "fingerprint",
            &outputs
        ));
        assert!(!matches(&receipt, "variant", "other", &outputs));
        fs::write(&executable, "changed output").map_err(|error| error.to_string())?;
        assert!(!matches(&receipt, "variant", "fingerprint", &outputs));
        fs::write(&receipt, "{").map_err(|error| error.to_string())?;
        assert!(!matches(&receipt, "variant", "fingerprint", &outputs));
        Ok(())
    }
}
