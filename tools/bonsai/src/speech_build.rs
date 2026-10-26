//! Native speech build pins and bounded, nonexecuting source ZIP staging.

use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{Read, Seek as _, SeekFrom, Write},
    path::{Path, PathBuf},
};

use bonsai_artifacts::{is_redirect, validate_external_path};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use zip::{HasZipMetadata as _, ZipArchive};

const MAX_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_ENTRIES: usize = 20_000;

#[derive(Deserialize)]
#[serde(tag = "operation", deny_unknown_fields)]
enum Request {
    #[serde(rename = "plan")]
    Plan,
    #[serde(rename = "validate-names", rename_all = "camelCase")]
    ValidateNames {
        archive_root: String,
        names: Vec<String>,
    },
    #[serde(rename = "stage-source", rename_all = "camelCase")]
    StageSource {
        archive_path: PathBuf,
        destination: PathBuf,
        archive_root: String,
        bytes: u64,
        sha256: String,
    },
}

/// Validate compiled build pins or stage bounded source bytes without executing code.
///
/// # Errors
/// Rejects unsupported requests, unsafe paths, malformed pins, ZIP inventory or
/// expansion faults, and source-cache contents that differ from the archive.
pub fn run(request: Value) -> crate::Result<Value> {
    match serde_json::from_value(request).map_err(|_| "Invalid speech build request.")? {
        Request::Plan => build_plan(),
        Request::ValidateNames {
            archive_root,
            names,
        } => {
            if names.is_empty() || names.len() > 100_000 {
                return Err("Compiler archive has too many entries.".into());
            }
            for name in names {
                validate_name(&name, &archive_root)?;
            }
            Ok(json!({}))
        }
        Request::StageSource {
            archive_path,
            destination,
            archive_root,
            bytes,
            sha256,
        } => stage_source(&archive_path, &destination, &archive_root, bytes, &sha256),
    }
}

fn text<'a>(value: &'a Value, name: &str) -> crate::Result<&'a str> {
    value[name]
        .as_str()
        .ok_or_else(|| "Invalid native speech build lock.".into())
}

fn hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn artifact(value: &Value, url: &str) -> crate::Result<()> {
    let filename = text(value, "filename")?;
    if filename.is_empty()
        || !filename
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_.+-".contains(&byte))
        || value["bytes"]
            .as_u64()
            .is_none_or(|bytes| bytes == 0 || bytes > i64::MAX.unsigned_abs())
        || !hex(text(value, "sha256")?, 64)
        || text(value, "url")? != url
    {
        return Err("Invalid native speech artifact identity.".into());
    }
    Ok(())
}

fn build_plan() -> crate::Result<Value> {
    let pin: Value = serde_json::from_str(include_str!("../../../config/native-speech.lock.json"))
        .map_err(|_| "Invalid native build lock JSON.")?;
    let model: Value =
        serde_json::from_str(include_str!("../../../src/speech-tts/model.lock.json"))
            .map_err(|_| "Invalid native speech model lock JSON.")?;
    let compiler = &pin["compiler"];
    let version = text(compiler, "version")?;
    let parts: Vec<_> = version.split('.').collect();
    if pin["schemaVersion"] != 1
        || pin["target"] != "aarch64-pc-windows-msvc"
        || parts.len() != 3
        || parts
            .iter()
            .any(|part| part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()))
        || compiler["directory"] != format!("llvm-{version}-arm64")
        || compiler["root"] != format!("clang+llvm-{version}-aarch64-pc-windows-msvc")
        || compiler["filename"] != format!("clang+llvm-{version}-aarch64-pc-windows-msvc.tar.xz")
    {
        return Err("Invalid native speech compiler lock.".into());
    }
    artifact(
        compiler,
        &format!(
            "https://github.com/llvm/llvm-project/releases/download/llvmorg-{version}/clang%2Bllvm-{version}-aarch64-pc-windows-msvc.tar.xz"
        ),
    )?;
    let executables = compiler["executables"]
        .as_object()
        .ok_or("Missing compiler executable identities.")?;
    if executables.len() != 2 {
        return Err("Invalid compiler executable inventory.".into());
    }
    for name in ["clang-cl.exe", "llvm-rc.exe"] {
        let file = &compiler["executables"][name];
        if file["bytes"]
            .as_u64()
            .is_none_or(|bytes| bytes == 0 || bytes > i64::MAX.unsigned_abs())
            || !hex(text(file, "sha256")?, 64)
        {
            return Err("Invalid compiler executable identity.".into());
        }
    }
    if pin["sources"]
        .as_object()
        .is_none_or(|sources| sources.len() != 2)
    {
        return Err("Invalid native source inventory.".into());
    }
    for key in ["qwen", "ggml"] {
        let source = &pin["sources"][key];
        let repository = text(source, "repository")?;
        let revision = text(source, "revision")?;
        let pieces: Vec<_> = repository.split('/').collect();
        if pieces.len() != 2
            || pieces.iter().any(|piece| {
                piece.is_empty()
                    || !piece
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte))
            })
            || !hex(revision, 40)
            || source["root"] != format!("{}-{revision}", pieces[1])
        {
            return Err("Invalid native source identity.".into());
        }
        artifact(
            source,
            &format!("https://codeload.github.com/{repository}/zip/{revision}"),
        )?;
        let (repository_key, revision_key) = if key == "qwen" {
            ("repository", "revision")
        } else {
            ("ggmlRepository", "ggmlRevision")
        };
        if model["nativeRuntime"][repository_key] != format!("https://github.com/{repository}")
            || model["nativeRuntime"][revision_key] != revision
        {
            return Err("Native source and model runtime revisions disagree.".into());
        }
    }
    Ok(pin)
}

fn safe_part(part: &str) -> bool {
    let stem = part
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    !part.is_empty()
        && !part.eq_ignore_ascii_case(".git")
        && part != "."
        && part != ".."
        && !part.ends_with(['.', ' '])
        && !["CON", "PRN", "AUX", "NUL"].contains(&stem.as_str())
        && !((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && stem.as_bytes()[3].is_ascii_digit())
        && !part
            .chars()
            .any(|character| character.is_control() || "\\:<>\"|?*".contains(character))
}

fn validate_name<'a>(name: &'a str, root: &str) -> crate::Result<&'a str> {
    if !safe_part(root)
        || root.contains('/')
        || !name.starts_with(root)
        || name.as_bytes().get(root.len()) != Some(&b'/')
        || !name.trim_end_matches('/').split('/').all(safe_part)
        || name.contains("//")
    {
        return Err("Native archive contains an unsafe or ambiguous path.".into());
    }
    Ok(&name[root.len() + 1..])
}

fn open_locked(path: &Path) -> crate::Result<File> {
    validate_external_path(path).map_err(|error| error.to_string())?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt as _;
        options.share_mode(1).custom_flags(0x0020_0000);
    }
    let file = options
        .open(path)
        .map_err(|_| "Native source file cannot be opened safely.")?;
    let metadata = file
        .metadata()
        .map_err(|_| "Native source metadata is unavailable.")?;
    if !metadata.is_file() || is_redirect(&metadata) {
        return Err("Native source is not a regular file.".into());
    }
    Ok(file)
}

fn digest(
    reader: &mut impl Read,
    expected: u64,
    output: &mut impl Write,
) -> crate::Result<Vec<u8>> {
    let mut hash = Sha256::new();
    let mut count = 0_u64;
    let mut buffer = vec![0; 64 * 1024];
    loop {
        let copied = reader
            .read(&mut buffer)
            .map_err(|_| "Cannot read native source bytes.")?;
        if copied == 0 {
            break;
        }
        count = count
            .checked_add(u64::try_from(copied).map_err(|_| "Source read overflow.")?)
            .ok_or("Source read overflow.")?;
        if count > expected {
            return Err("Native source entry exceeded its byte bound.".into());
        }
        hash.update(&buffer[..copied]);
        output
            .write_all(&buffer[..copied])
            .map_err(|_| "Cannot write native source bytes.")?;
    }
    if count != expected {
        return Err("Native source byte length differs from its archive.".into());
    }
    Ok(hash.finalize().to_vec())
}

fn stage_source(
    archive_path: &Path,
    destination: &Path,
    archive_root: &str,
    bytes: u64,
    sha256: &str,
) -> crate::Result<Value> {
    validate_external_path(destination).map_err(|error| error.to_string())?;
    let mut file = open_locked(archive_path)?;
    if bytes == 0 || bytes > MAX_BYTES || !hex(sha256, 64) {
        return Err("Invalid native source archive identity.".into());
    }
    if file
        .metadata()
        .map_err(|_| "Cannot inspect source archive.")?
        .len()
        > MAX_BYTES
    {
        return Err("Source archive exceeds its byte bound.".into());
    }
    let actual: String = digest(&mut file, bytes, &mut std::io::sink())?
        .iter()
        .flat_map(|byte| {
            [
                char::from(b"0123456789abcdef"[usize::from(byte >> 4)]),
                char::from(b"0123456789abcdef"[usize::from(byte & 15)]),
            ]
        })
        .collect();
    if actual != sha256 {
        return Err("Native source archive differs from its pin.".into());
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|_| "Cannot rewind native source archive.")?;
    let mut zip = ZipArchive::new(file).map_err(|_| "Cannot read native source ZIP.")?;
    if zip.len() > MAX_ENTRIES {
        return Err("Source ZIP has too many entries.".into());
    }
    let entries = inventory(&mut zip, destination, archive_root)?;
    let mut files: Vec<PathBuf> = Vec::new();
    for (index, path, directory, bytes) in entries {
        validate_external_path(&path).map_err(|error| error.to_string())?;
        if directory {
            fs::create_dir_all(&path).map_err(|_| "Cannot create native source directory.")?;
            continue;
        }
        fs::create_dir_all(path.parent().ok_or("Missing source parent directory.")?)
            .map_err(|_| "Cannot create native source parent.")?;
        validate_external_path(&path).map_err(|error| error.to_string())?;
        let mut entry = zip
            .by_index(index)
            .map_err(|_| "Cannot reopen source ZIP entry.")?;
        if path.exists() {
            let expected = digest(&mut entry, bytes, &mut std::io::sink())?;
            let mut existing = open_locked(&path)?;
            if existing
                .metadata()
                .map_err(|_| "Cannot inspect source cache.")?
                .len()
                != bytes
                || digest(&mut existing, bytes, &mut std::io::sink())? != expected
            {
                return Err("Native source cache differs from its pinned archive.".into());
            }
        } else {
            let mut output = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .map_err(|_| "Cannot create native source file.")?;
            digest(&mut entry, bytes, &mut output)?;
            output
                .flush()
                .map_err(|_| "Cannot flush native source file.")?;
        }
        files.push(path);
    }
    Ok(json!({"Files": files}))
}

type SourceEntry = (usize, PathBuf, bool, u64);

fn inventory(
    zip: &mut ZipArchive<File>,
    destination: &Path,
    archive_root: &str,
) -> crate::Result<Vec<SourceEntry>> {
    let mut entries: Vec<SourceEntry> = Vec::new();
    let mut paths = BTreeMap::<String, bool>::new();
    let mut expanded = 0_u64;
    for index in 0..zip.len() {
        let entry = zip
            .by_index(index)
            .map_err(|_| "Cannot inspect native source ZIP entry.")?;
        let name = entry
            .name()
            .map_err(|_| "Invalid native source filename encoding.")?;
        let relative = validate_name(&name, archive_root)?;
        if relative.is_empty() {
            continue;
        }
        let directory = name.ends_with('/');
        let mode = (entry.get_metadata().external_attributes >> 16) & 0xf000;
        if ![0, 0x8000, 0x4000, 0xa000].contains(&mode)
            || (directory && [0x8000, 0xa000].contains(&mode))
            || (!directory && mode == 0x4000)
        {
            return Err("Source ZIP has an unsupported file type.".into());
        }
        // Uppercase first also catches Windows aliases such as long-s versus s;
        // lowercasing alone leaves those equivalent path spellings distinct.
        let key = relative.trim_end_matches('/').to_uppercase().to_lowercase();
        if paths.insert(key, directory).is_some() {
            return Err("Source ZIP has duplicate paths.".into());
        }
        expanded = expanded
            .checked_add(entry.size())
            .ok_or("Source ZIP expansion overflow.")?;
        if expanded > MAX_BYTES {
            return Err("Source ZIP exceeds its expansion limit.".into());
        }
        let path: PathBuf = destination
            .join(relative.trim_end_matches('/'))
            .components()
            .collect();
        validate_external_path(&path).map_err(|error| error.to_string())?;
        entries.push((index, path, directory, entry.size()));
    }
    for name in paths.keys() {
        let mut ancestor = name.as_str();
        while let Some((parent, _)) = ancestor.rsplit_once('/') {
            if paths.get(parent) == Some(&false) {
                return Err("Source ZIP has a file/directory collision.".into());
            }
            ancestor = parent;
        }
    }
    if entries.is_empty() {
        return Err("Source ZIP contains no source entries.".into());
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::{build_plan, validate_name};

    #[test]
    fn compiled_build_pins_match_native_runtime() -> crate::Result<()> {
        let pin = build_plan()?;
        assert_eq!(
            pin["compiler"]["executables"]
                .as_object()
                .map(serde_json::Map::len),
            Some(2)
        );
        Ok(())
    }

    #[test]
    fn archive_names_reject_windows_aliases_and_traversal() {
        for name in [
            "source/../outside",
            "source/sub/../../outside",
            "/absolute",
            "source/a:stream",
            "source/CON.cpp",
            "source/file. ",
            "source//file",
            "source\\file",
        ] {
            assert!(validate_name(name, "source").is_err());
        }
        assert_eq!(
            validate_name("source/src/engine.cpp", "source").as_deref(),
            Ok("src/engine.cpp")
        );
    }
}
