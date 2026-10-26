//! Native speech pins, bounded source staging, provisioning and startup builds.

use std::{
    collections::{BTreeMap, BTreeSet},
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
    #[serde(rename = "stage-sources", rename_all = "camelCase")]
    StageSources {
        source_directory: PathBuf,
        qwen_archive_path: PathBuf,
        ggml_archive_path: PathBuf,
    },
    #[serde(rename = "environment", rename_all = "camelCase")]
    Environment {
        data_directory: PathBuf,
        working_directory: PathBuf,
    },
    #[serde(rename = "build", rename_all = "camelCase")]
    Build {
        repository_root: PathBuf,
        data_directory: PathBuf,
        working_directory: PathBuf,
        #[serde(default = "all_models")]
        model: String,
        #[serde(default)]
        force_build: bool,
    },
    #[serde(rename = "models", rename_all = "camelCase")]
    Models {
        data_directory: PathBuf,
        working_directory: PathBuf,
        #[serde(default = "all_models")]
        model: String,
        #[serde(default = "default_quantization")]
        tts_quantization: String,
        #[serde(default)]
        verify_only: bool,
        #[serde(default)]
        defer_cached_verification: bool,
    },
}

fn all_models() -> String {
    "All".into()
}
fn default_quantization() -> String {
    "Q8_0".into()
}

/// Plans, provisions or builds native speech through explicit operation requests.
///
/// Planning and source-staging operations remain nonexecuting. Model provisioning,
/// environment preparation and builds use the pinned native startup boundary.
///
/// # Errors
/// Rejects unsupported requests, unsafe paths, malformed pins, ZIP inventory or
/// expansion faults, source-cache contents that differ from the archive, invalid
/// host prerequisites, bounded download failures and native compiler failures.
pub fn run(request: Value) -> crate::Result<Value> {
    match serde_json::from_value(request).map_err(|_| "Invalid speech build request.")? {
        Request::Plan => build_plan(),
        Request::Environment {
            data_directory,
            working_directory,
        } => crate::native_startup::environment(&data_directory, &working_directory),
        Request::Build {
            repository_root,
            data_directory,
            working_directory,
            model,
            force_build,
        } => crate::native_startup::build(
            &repository_root,
            &data_directory,
            &working_directory,
            &model,
            force_build,
        ),
        Request::Models {
            data_directory,
            working_directory,
            model,
            tts_quantization,
            verify_only,
            defer_cached_verification,
        } => crate::native_startup::models(
            &data_directory,
            &model,
            &tts_quantization,
            verify_only,
            defer_cached_verification,
            &working_directory,
        ),
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
        Request::StageSources {
            source_directory,
            qwen_archive_path,
            ggml_archive_path,
        } => {
            let pin = build_plan()?;
            let mut sources = Vec::new();
            for (key, archive_path, destination) in [
                ("qwen", qwen_archive_path, source_directory.clone()),
                ("ggml", ggml_archive_path, source_directory.join("ggml")),
            ] {
                let source = &pin["sources"][key];
                sources.push(SourceSpec {
                    archive_path,
                    destination,
                    archive_root: text(source, "root")?.into(),
                    bytes: source["bytes"].as_u64().ok_or("Invalid source size.")?,
                    sha256: text(source, "sha256")?.into(),
                });
            }
            stage_sources(&source_directory, &sources)
        }
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

pub(crate) fn build_plan() -> crate::Result<Value> {
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
    buffer: &mut [u8],
) -> crate::Result<[u8; 32]> {
    let mut hash = Sha256::new();
    let mut count = 0_u64;
    loop {
        let copied = reader
            .read(buffer)
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
    Ok(hash.finalize().into())
}

struct PreparedSource {
    zip: ZipArchive<File>,
    entries: Vec<SourceEntry>,
}

struct SourceSpec {
    archive_path: PathBuf,
    destination: PathBuf,
    archive_root: String,
    bytes: u64,
    sha256: String,
}

fn stage_source(
    archive_path: &Path,
    destination: &Path,
    archive_root: &str,
    bytes: u64,
    sha256: &str,
) -> crate::Result<Value> {
    check_source_tree(destination, None)?;
    let mut buffer = vec![0; 64 * 1024];
    let mut source = prepare_source(
        archive_path,
        destination,
        archive_root,
        bytes,
        sha256,
        &mut buffer,
    )?;
    stage_prepared(&mut source, &mut buffer)?;
    let files: Vec<_> = source
        .entries
        .iter()
        .filter(|(_, _, directory, _)| !directory)
        .map(|(_, path, _, _)| path)
        .collect();
    Ok(json!({"Files": files}))
}

fn stage_sources(root: &Path, specs: &[SourceSpec]) -> crate::Result<Value> {
    check_source_tree(root, None)?;
    let mut buffer = vec![0; 64 * 1024];
    let mut sources = Vec::new();
    for spec in specs {
        sources.push(prepare_source(
            &spec.archive_path,
            &spec.destination,
            &spec.archive_root,
            spec.bytes,
            &spec.sha256,
            &mut buffer,
        )?);
    }
    let expected = expected_files(&sources)?;
    for source in &mut sources {
        stage_prepared(source, &mut buffer)?;
    }
    check_source_tree(root, Some(&expected))?;
    // Keep both read-only ZIP handles until the complete union has been checked.
    Ok(
        json!({"SourceDirectory": root.components().collect::<PathBuf>(), "FilesVerified": expected.len()}),
    )
}

fn path_key(path: &Path) -> String {
    path.components()
        .collect::<PathBuf>()
        .to_string_lossy()
        .replace('\\', "/")
        .to_uppercase()
        .to_lowercase()
}

fn expected_files(sources: &[PreparedSource]) -> crate::Result<BTreeSet<String>> {
    let mut entries = BTreeMap::new();
    for source in sources {
        for (_, path, directory, _) in &source.entries {
            if entries
                .insert(path_key(path), *directory)
                .is_some_and(|previous| previous != *directory)
            {
                return Err("Native source archives have conflicting path types.".into());
            }
        }
    }
    for name in entries.keys() {
        let mut ancestor = name.as_str();
        while let Some((parent, _)) = ancestor.rsplit_once('/') {
            if entries.get(parent) == Some(&false) {
                return Err("Native source archives have a file/directory collision.".into());
            }
            ancestor = parent;
        }
    }
    Ok(entries
        .into_iter()
        .filter_map(|(name, directory)| (!directory).then_some(name))
        .collect())
}

fn check_source_tree(root: &Path, expected: Option<&BTreeSet<String>>) -> crate::Result<()> {
    validate_external_path(root).map_err(|error| error.to_string())?;
    match fs::symlink_metadata(root) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Ok(metadata) if metadata.is_dir() && !is_redirect(&metadata) => {}
        _ => return Err("Native source cache is not an ordinary directory.".into()),
    }
    let mut directories = vec![root.to_path_buf()];
    while let Some(directory) = directories.pop() {
        for entry in fs::read_dir(&directory).map_err(|_| "Cannot inspect native source cache.")? {
            let path = entry
                .map_err(|_| "Cannot inspect native source cache entry.")?
                .path();
            let metadata = fs::symlink_metadata(&path)
                .map_err(|_| "Cannot inspect native source cache metadata.")?;
            if is_redirect(&metadata) {
                return Err("Native source cache contains a reparse entry.".into());
            }
            if metadata.is_dir() {
                directories.push(path);
            } else if !metadata.is_file()
                || expected.is_some_and(|files| !files.contains(&path_key(&path)))
            {
                return Err("Native source cache contains an unpinned file.".into());
            }
        }
    }
    Ok(())
}

fn digest_hex(hash: &[u8]) -> String {
    hash.iter()
        .flat_map(|byte| {
            [
                char::from(b"0123456789abcdef"[usize::from(byte >> 4)]),
                char::from(b"0123456789abcdef"[usize::from(byte & 15)]),
            ]
        })
        .collect()
}

fn prepare_source(
    archive_path: &Path,
    destination: &Path,
    archive_root: &str,
    bytes: u64,
    sha256: &str,
    buffer: &mut [u8],
) -> crate::Result<PreparedSource> {
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
    let actual = digest_hex(&digest(&mut file, bytes, &mut std::io::sink(), buffer)?);
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
    Ok(PreparedSource { zip, entries })
}

fn stage_prepared(source: &mut PreparedSource, buffer: &mut [u8]) -> crate::Result<()> {
    for (index, path, directory, bytes) in &source.entries {
        validate_external_path(path).map_err(|error| error.to_string())?;
        if *directory {
            fs::create_dir_all(path).map_err(|_| "Cannot create native source directory.")?;
            continue;
        }
        fs::create_dir_all(path.parent().ok_or("Missing source parent directory.")?)
            .map_err(|_| "Cannot create native source parent.")?;
        validate_external_path(path).map_err(|error| error.to_string())?;
        let mut entry = source
            .zip
            .by_index(*index)
            .map_err(|_| "Cannot reopen source ZIP entry.")?;
        if path.exists() {
            let expected = digest(&mut entry, *bytes, &mut std::io::sink(), buffer)?;
            let mut existing = open_locked(path)?;
            if existing
                .metadata()
                .map_err(|_| "Cannot inspect source cache.")?
                .len()
                != *bytes
                || digest(&mut existing, *bytes, &mut std::io::sink(), buffer)? != expected
            {
                return Err("Native source cache differs from its pinned archive.".into());
            }
        } else {
            let mut output = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
                .map_err(|_| "Cannot create native source file.")?;
            digest(&mut entry, *bytes, &mut output, buffer)?;
            output
                .flush()
                .map_err(|_| "Cannot flush native source file.")?;
        }
    }
    Ok(())
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
    use super::{SourceSpec, build_plan, stage_source, stage_sources, validate_name};
    use sha2::{Digest as _, Sha256};
    use std::{
        error::Error,
        fs,
        io::Write as _,
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };
    use zip::{ZipWriter, write::SimpleFileOptions};

    type TestResult<T = ()> = Result<T, Box<dyn Error>>;

    fn scratch(name: &str) -> TestResult<PathBuf> {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let path = std::env::temp_dir().join(format!(
            "Bonsai-SourceCache-{name}-{}-{nonce}",
            std::process::id()
        ));
        bonsai_artifacts::validate_external_path(&path)?;
        fs::create_dir(&path)?;
        Ok(path)
    }

    fn source_fixture(
        scratch: &Path,
        destination: &Path,
        root: &str,
        names: &[&str],
        link: bool,
    ) -> TestResult<SourceSpec> {
        let path = scratch.join(format!("{root}.zip"));
        let mut zip = ZipWriter::new(fs::File::create(&path)?);
        for name in names {
            if link {
                zip.add_symlink(*name, "fixture", SimpleFileOptions::default())?;
            } else {
                zip.start_file(*name, SimpleFileOptions::default())?;
                zip.write_all(b"fixture")?;
            }
        }
        zip.finish()?;
        let bytes = fs::read(&path)?;
        Ok(SourceSpec {
            archive_path: path,
            destination: destination.into(),
            archive_root: root.into(),
            bytes: u64::try_from(bytes.len())?,
            sha256: super::digest_hex(&Sha256::digest(&bytes)),
        })
    }

    fn stage(spec: &SourceSpec) -> crate::Result<serde_json::Value> {
        stage_source(
            &spec.archive_path,
            &spec.destination,
            &spec.archive_root,
            spec.bytes,
            &spec.sha256,
        )
    }

    #[test]
    fn source_staging_preserves_bytes_and_rejects_changed_archive_or_cache() -> TestResult {
        let scratch = scratch("identity")?;
        let root = scratch.join("source");
        let mut spec = source_fixture(
            &scratch,
            &root,
            "source",
            &["source/src/engine.cpp", "source/CMakeLists.txt"],
            false,
        )?;
        let expected = spec.sha256.clone();
        spec.sha256 = "0".repeat(64);
        assert!(stage(&spec).is_err());
        assert!(!root.exists());
        spec.sha256 = expected;
        assert_eq!(stage(&spec)?["Files"].as_array().map(Vec::len), Some(2));
        assert_eq!(fs::read(root.join("src/engine.cpp"))?, b"fixture");
        stage(&spec)?;
        fs::write(root.join("src/engine.cpp"), b"changed")?;
        assert!(stage(&spec).is_err());
        assert_eq!(fs::read(root.join("src/engine.cpp"))?, b"changed");
        Ok(())
    }

    #[test]
    fn source_inventory_rejects_hostile_names_before_writing() -> TestResult {
        let scratch = scratch("inventory")?;
        let root = scratch.join("source");
        for names in [
            vec!["source/../outside.cpp"],
            vec!["source/sub/../../outside.cpp"],
            vec!["/absolute.cpp"],
            vec!["source/file:stream.cpp"],
            vec!["source/CON.cpp"],
            vec!["source/file. "],
            vec!["source/src/engine.cpp", "source/SRC/engine.cpp"],
            vec!["source/a", "source/a/b"],
            vec!["source/source.cpp", "source/ſource.cpp"],
            vec!["source/a/b", "source/A"],
            vec!["source/.git/config"],
            vec!["source//file"],
            vec![],
        ] {
            let spec = source_fixture(&scratch, &root, "source", &names, false)?;
            assert!(
                stage(&spec).is_err(),
                "Accepted hostile source entries: {names:?}"
            );
            assert!(!root.exists());
        }
        Ok(())
    }

    #[test]
    fn source_symlink_entries_become_regular_text() -> TestResult {
        let scratch = scratch("link-bytes")?;
        let root = scratch.join("source");
        let spec = source_fixture(&scratch, &root, "source", &["source/doc/link.md"], true)?;
        stage(&spec)?;
        let path = root.join("doc/link.md");
        assert!(!bonsai_artifacts::is_redirect(&fs::symlink_metadata(
            &path
        )?));
        assert_eq!(fs::read(path)?, b"fixture");
        Ok(())
    }

    #[test]
    fn combined_sources_verify_complete_cache_with_native_path_spelling() -> TestResult {
        let scratch = scratch("union")?;
        let root = scratch.join("source");
        let specs = [
            source_fixture(&scratch, &root, "qwen", &["qwen/src/engine.cpp"], false)?,
            source_fixture(
                &scratch,
                &root.join("ggml"),
                "ggml",
                &["ggml/src/ggml.c"],
                false,
            )?,
        ];
        assert_eq!(stage_sources(&root, &specs)?["FilesVerified"], 2);
        let alternate = PathBuf::from(root.to_str().ok_or("Non-UTF-8 scratch")?.replace('\\', "/"));
        assert_eq!(stage_sources(&alternate, &specs)?["FilesVerified"], 2);
        fs::write(root.join("unreviewed.cpp"), b"unreviewed")?;
        assert!(stage_sources(&root, &specs).is_err());
        assert_eq!(fs::read(root.join("unreviewed.cpp"))?, b"unreviewed");
        Ok(())
    }

    #[test]
    fn combined_sources_preflight_both_archives_before_writing() -> TestResult {
        let scratch = scratch("preflight")?;
        let root = scratch.join("source");
        let specs = [
            source_fixture(&scratch, &root, "qwen", &["qwen/src/engine.cpp"], false)?,
            source_fixture(
                &scratch,
                &root.join("ggml"),
                "ggml",
                &["ggml/../escaped.cpp"],
                false,
            )?,
        ];
        assert!(stage_sources(&root, &specs).is_err());
        assert!(!root.exists());
        Ok(())
    }

    #[cfg(windows)]
    #[test]
    fn source_cache_rejects_junctions_before_staging() -> TestResult {
        use std::os::windows::process::CommandExt as _;
        let scratch = scratch("junction")?;
        let root = scratch.join("source");
        let target = scratch.join("external");
        fs::create_dir(&root)?;
        fs::create_dir(&target)?;
        fs::write(target.join("sentinel"), b"unchanged")?;
        let junction = root.join("redirected");
        let system =
            PathBuf::from(std::env::var_os("SystemRoot").ok_or("Missing Windows directory")?);
        assert!(
            system.is_absolute(),
            "Use an expanded absolute Windows directory"
        );
        for path in [&junction, &target] {
            let spelling = path.to_str().ok_or("Non-UTF-8 fixture path")?;
            assert!(path.is_absolute() && !spelling.contains(['"', '\r', '\n', '%']));
        }
        // cmd builtins parse one command string rather than CRT-style argv.
        let command = format!(
            "/d /v:off /c mklink /J \"{}\" \"{}\"",
            junction.to_string_lossy().replace('/', "\\"),
            target.to_string_lossy().replace('/', "\\")
        );
        let output = std::process::Command::new(system.join("System32").join("cmd.exe"))
            .raw_arg(&command)
            .current_dir(&scratch)
            .output()?;
        assert!(
            output.status.success(),
            "Cannot create owned junction fixture ({command}): {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let spec = source_fixture(&scratch, &root, "source", &["source/src/engine.cpp"], false)?;
        assert!(stage(&spec).is_err());
        assert!(stage_sources(&root, &[spec]).is_err());
        assert!(!root.join("src").exists());
        assert_eq!(fs::read(target.join("sentinel"))?, b"unchanged");
        fs::remove_dir(junction)?;
        Ok(())
    }

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
