//! Verified, resumable artifact provisioning for native startup.
#![expect(
    clippy::redundant_pub_crate,
    reason = "Sibling startup modules share an explicit crate-local implementation API."
)]

use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io::{Read as _, Seek as _, SeekFrom},
    path::{Path, PathBuf},
    time::Duration,
};

use bonsai_artifacts::{is_redirect, validate_external_path};
use sha2::{Digest as _, Sha256};

fn expected(bytes: u64, sha256: &str) -> crate::Result<()> {
    if bytes == 0
        || bytes > i64::MAX.unsigned_abs()
        || sha256.len() != 64
        || !sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("A positive locked length and SHA-256 are required.".into());
    }
    Ok(())
}

fn digest_matches(file: &mut File, bytes: u64, sha256: &str) -> crate::Result<bool> {
    crate::startup_cancel::checkpoint()?;
    expected(bytes, sha256)?;
    if file.metadata().map_err(|error| error.to_string())?.len() != bytes {
        return Err("Artifact byte length differs from its lock.".into());
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|error| error.to_string())?;
    let mut hash = Sha256::new();
    let mut buffer = vec![0; 1024 * 1024];
    loop {
        crate::startup_cancel::checkpoint()?;
        let read = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        hash.update(&buffer[..read]);
    }
    Ok(crate::startup_cache::hex(&hash.finalize()).eq_ignore_ascii_case(sha256))
}

fn check(file: &mut File, bytes: u64, sha256: &str) -> crate::Result<()> {
    crate::startup_cancel::checkpoint()?;
    if !digest_matches(file, bytes, sha256)? {
        return Err("Artifact SHA-256 differs from its lock; invalid bytes are retained.".into());
    }
    Ok(())
}

fn open_read(path: &Path) -> crate::Result<File> {
    validate_external_path(path).map_err(|error| error.to_string())?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt as _;
        options.share_mode(1).custom_flags(0x0020_0000);
    }
    let file = options.open(path).map_err(|error| error.to_string())?;
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    if !metadata.is_file() || is_redirect(&metadata) {
        return Err("Artifact must be an ordinary file.".into());
    }
    Ok(file)
}

/// Verifies a complete pinned artifact without downloading or modifying it.
pub(crate) fn verify(path: &Path, bytes: u64, sha256: &str) -> crate::Result<()> {
    check(&mut open_read(path)?, bytes, sha256)
}

/// Checks only cached length; native speech verifies the complete hash on use.
pub(crate) fn verify_length(path: &Path, bytes: u64) -> crate::Result<()> {
    if open_read(path)?
        .metadata()
        .map_err(|error| error.to_string())?
        .len()
        != bytes
    {
        return Err("Artifact byte length differs from its lock.".into());
    }
    Ok(())
}

fn download_uri(uri: &str) -> crate::Result<()> {
    let authority = uri
        .strip_prefix("https://")
        .and_then(|suffix| suffix.split(['/', '?', '#']).next())
        .filter(|authority| !authority.is_empty())
        .ok_or("A public HTTPS artifact URL is required.")?;
    if authority.contains(['@', '%', '\\'])
        || uri.chars().any(char::is_whitespace)
        || uri.chars().any(char::is_control)
    {
        return Err("Artifact URL contains credentials or ambiguous characters.".into());
    }
    Ok(())
}

pub(crate) fn system_tool(name: &str) -> crate::Result<PathBuf> {
    let root = PathBuf::from(std::env::var_os("SystemRoot").ok_or("Missing Windows directory.")?);
    validate_external_path(&root).map_err(|error| error.to_string())?;
    let path = root.join("System32").join(name);
    if !path.is_file() {
        return Err(format!(
            "Required Windows tool is unavailable: {}",
            path.display()
        ));
    }
    Ok(path)
}

fn curl_arguments(uri: &str, partial: &Path, offset: u64, bytes: u64) -> Vec<OsString> {
    let mut arguments: Vec<OsString> = [
        "--disable",
        "--fail",
        "--silent",
        "--show-error",
        "--location",
        "--max-redirs",
        "10",
        "--disallow-username-in-url",
        "--proto",
        "=https",
        "--proto-redir",
        "=https",
        "--header",
        "Accept-Encoding: identity",
        "--connect-timeout",
        "60",
        "--max-time",
        "7200",
        "--max-filesize",
    ]
    .into_iter()
    .map(OsString::from)
    .collect();
    arguments.push(bytes.saturating_sub(offset).to_string().into());
    if offset > 0 {
        arguments.extend([OsString::from("--continue-at"), offset.to_string().into()]);
    }
    arguments.extend([
        OsString::from("--output"),
        partial.as_os_str().to_owned(),
        uri.into(),
    ]);
    arguments
}

fn compatible_curl(output: &str) -> bool {
    let mut words = output.split_whitespace();
    if words.next() != Some("curl") {
        return false;
    }
    let mut version = words.next().unwrap_or_default().split('.');
    matches!((version.next().and_then(|part| part.parse::<u64>().ok()), version.next().and_then(|part| part.parse::<u64>().ok())),
        (Some(major), Some(minor)) if major > 8 || (major == 8 && minor >= 4))
}

fn download_tool(working: &Path) -> crate::Result<PathBuf> {
    let curl = system_tool("curl.exe")?;
    let result = crate::startup_process::run(
        curl.as_os_str(),
        &["--disable".into(), "--version".into()],
        working,
        &BTreeMap::new(),
        Duration::from_secs(10),
        &working.join(format!(
            "curl-version-{}.log",
            crate::native_startup::nonce()?
        )),
    )?;
    if result.code != 0 || !compatible_curl(&result.stdout) {
        return Err(
            "Artifact provisioning requires Windows curl 8.4 or newer for bounded downloads."
                .into(),
        );
    }
    Ok(curl)
}

fn transfer(
    uri: &str,
    partial: &Path,
    offset: u64,
    bytes: u64,
    working: &Path,
) -> crate::Result<()> {
    let curl = download_tool(working)?;
    let log = working.join(format!("download-{}.log", crate::native_startup::nonce()?));
    let mut result = crate::startup_process::run(
        curl.as_os_str(),
        &curl_arguments(uri, partial, offset, bytes),
        working,
        &BTreeMap::new(),
        Duration::from_secs(7_230),
        &log,
    )?;
    if result.code == 33 && offset > 0 {
        // curl refuses to append a full response when a server ignores Range.
        // Restart this same locked partial rather than combining responses.
        validate_external_path(partial).map_err(|error| error.to_string())?;
        OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(partial)
            .map_err(|error| error.to_string())?;
        result = crate::startup_process::run(
            curl.as_os_str(),
            &curl_arguments(uri, partial, 0, bytes),
            working,
            &BTreeMap::new(),
            Duration::from_secs(7_230),
            &working.join(format!(
                "download-restart-{}.log",
                crate::native_startup::nonce()?
            )),
        )?;
    }
    if result.code != 0 {
        return Err(format!(
            "Pinned download failed ({}): {}; partial bytes retained at {}",
            result.code,
            result.stderr.trim(),
            partial.display()
        ));
    }
    Ok(())
}

/// Downloads a pinned artifact, resuming partial bytes and publishing exclusively.
pub(crate) fn receive(
    uri: &str,
    path: &Path,
    bytes: u64,
    sha256: &str,
    working: &Path,
) -> crate::Result<()> {
    receive_with(uri, path, bytes, sha256, working, transfer)
}

fn receive_with(
    uri: &str,
    path: &Path,
    bytes: u64,
    sha256: &str,
    working: &Path,
    mut transfer: impl FnMut(&str, &Path, u64, u64, &Path) -> crate::Result<()>,
) -> crate::Result<()> {
    crate::startup_cancel::checkpoint()?;
    expected(bytes, sha256)?;
    download_uri(uri)?;
    validate_external_path(path).map_err(|error| error.to_string())?;
    validate_external_path(working).map_err(|error| error.to_string())?;
    if path.exists() {
        return verify(path, bytes, sha256);
    }
    fs::create_dir_all(path.parent().ok_or("Artifact has no parent directory.")?)
        .map_err(|error| error.to_string())?;
    fs::create_dir_all(working).map_err(|error| error.to_string())?;
    let lock = path.with_file_name(format!(
        "{}.download.lock",
        path.file_name()
            .ok_or("Artifact has no filename.")?
            .to_string_lossy()
    ));
    let _guard = crate::startup_cache::lock_file(&lock)?;
    if path.exists() {
        return verify(path, bytes, sha256);
    }
    let partial = path.with_file_name(format!(
        "{}.partial",
        path.file_name()
            .ok_or("Artifact has no filename.")?
            .to_string_lossy()
    ));
    validate_external_path(&partial).map_err(|error| error.to_string())?;
    let mut offset = match fs::symlink_metadata(&partial) {
        Ok(metadata) if metadata.is_file() && !is_redirect(&metadata) => metadata.len(),
        Ok(_) => return Err("Partial artifact is not an ordinary file.".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
        Err(error) => return Err(error.to_string()),
    };
    if offset > bytes {
        return Err("Partial artifact exceeds its locked length; bytes are retained.".into());
    }
    let mut quarantined: Option<PathBuf> = None;
    let completed = loop {
        crate::startup_cancel::checkpoint()?;
        if offset < bytes {
            transfer(uri, &partial, offset, bytes, working)?;
        }
        let mut completed = open_read(&partial)?;
        if digest_matches(&mut completed, bytes, sha256)? {
            break completed;
        }
        if let Some(quarantined) = &quarantined {
            return Err(format!(
                "Fresh artifact SHA-256 differs from its lock; invalid bytes retained at {} and {}.",
                partial.display(),
                quarantined.display()
            ));
        }
        let quarantine = partial.with_file_name(format!(
            ".bonsai-invalid-download-{}",
            crate::startup_process::unique()
        ));
        validate_external_path(&partial).map_err(|error| error.to_string())?;
        validate_external_path(&quarantine).map_err(|error| error.to_string())?;
        // Link exclusively while the checked read handle still denies writes.
        // A crash before unlinking only leaves another safe evidence alias.
        fs::hard_link(&partial, &quarantine)
            .map_err(|error| format!("Cannot preserve invalid download bytes: {error}"))?;
        drop(completed);
        fs::remove_file(&partial).map_err(|error| {
            format!(
                "Cannot restart partial download; invalid bytes retained at {}: {error}",
                quarantine.display()
            )
        })?;
        quarantined = Some(quarantine);
        offset = 0;
    };
    crate::startup_cancel::checkpoint()?;
    match fs::hard_link(&partial, path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            verify(path, bytes, sha256)?;
        }
        Err(error) => return Err(format!("Cannot publish verified artifact: {error}")),
    }
    drop(completed);
    // Retaining an extra hard-link alias is safe if another reader keeps it open.
    let _ = fs::remove_file(&partial);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recovery_fixture() -> crate::Result<(PathBuf, PathBuf, PathBuf, String)> {
        let working = std::env::temp_dir().join(format!(
            "Bonsai-DownloadRecovery-{}",
            crate::native_startup::nonce()?
        ));
        validate_external_path(&working).map_err(|error| error.to_string())?;
        fs::create_dir(&working).map_err(|error| error.to_string())?;
        let path = working.join("artifact.bin");
        let partial = working.join("artifact.bin.partial");
        fs::write(&partial, b"wrong").map_err(|error| error.to_string())?;
        let hash = crate::startup_cache::hex(&Sha256::digest(b"right"));
        Ok((working, path, partial, hash))
    }

    fn quarantine_files(working: &Path) -> crate::Result<Vec<PathBuf>> {
        fs::read_dir(working)
            .map_err(|error| error.to_string())?
            .filter_map(|entry| match entry {
                Ok(entry)
                    if entry
                        .file_name()
                        .to_string_lossy()
                        .starts_with(".bonsai-invalid-download-") =>
                {
                    Some(Ok(entry.path()))
                }
                Ok(_) => None,
                Err(error) => Some(Err(error.to_string())),
            })
            .collect()
    }

    #[test]
    fn corrupt_completed_partial_is_preserved_and_fresh_retry_publishes() -> crate::Result<()> {
        let (working, path, partial, hash) = recovery_fixture()?;
        let mut transfers = 0;
        receive_with(
            "https://example.invalid/artifact",
            &path,
            5,
            &hash,
            &working,
            |_, destination, offset, bytes, _| {
                transfers += 1;
                assert_eq!((offset, bytes), (0, 5));
                assert_eq!(destination, partial);
                assert!(!destination.exists());
                fs::write(destination, b"right").map_err(|error| error.to_string())
            },
        )?;
        assert_eq!(transfers, 1);
        verify(&path, 5, &hash)?;
        assert!(!partial.exists());
        let evidence = quarantine_files(&working)?;
        assert_eq!(evidence.len(), 1);
        assert_eq!(
            fs::read(evidence.first().ok_or("Missing quarantined download.")?)
                .map_err(|error| error.to_string())?,
            b"wrong"
        );
        Ok(())
    }

    #[test]
    fn corrupt_fresh_retry_stops_without_publishing_or_replacing_existing_artifacts()
    -> crate::Result<()> {
        let (working, path, partial, hash) = recovery_fixture()?;
        let mut transfers = 0;
        assert!(
            receive_with(
                "https://example.invalid/artifact",
                &path,
                5,
                &hash,
                &working,
                |_, destination, offset, _, _| {
                    transfers += 1;
                    assert_eq!(offset, 0);
                    fs::write(destination, b"again").map_err(|error| error.to_string())
                },
            )
            .is_err()
        );
        assert_eq!(transfers, 1);
        assert!(!path.exists());
        assert_eq!(
            fs::read(&partial).map_err(|error| error.to_string())?,
            b"again"
        );
        let evidence = quarantine_files(&working)?;
        assert_eq!(evidence.len(), 1);
        assert_eq!(
            fs::read(evidence.first().ok_or("Missing quarantined download.")?)
                .map_err(|error| error.to_string())?,
            b"wrong"
        );
        fs::write(&path, b"final").map_err(|error| error.to_string())?;
        assert!(
            receive_with(
                "https://example.invalid/artifact",
                &path,
                5,
                &hash,
                &working,
                |_, _, _, _, _| {
                    transfers += 1;
                    Err("Published artifacts must never trigger a replacement transfer.".into())
                },
            )
            .is_err()
        );
        assert_eq!(transfers, 1);
        assert_eq!(fs::read(path).map_err(|error| error.to_string())?, b"final");
        assert_eq!(quarantine_files(&working)?, evidence);
        Ok(())
    }

    #[test]
    fn locked_download_contract_rejects_unsafe_urls_and_bounds() {
        for uri in [
            "http://example.com/file",
            "https://user@example.com/file",
            "https://example.com/a b",
            "https://",
        ] {
            assert!(download_uri(uri).is_err());
        }
        assert!(download_uri("https://codeload.github.com/owner/repo/zip/revision").is_ok());
        assert!(expected(0, &"0".repeat(64)).is_err());
        assert!(expected(1, "bad").is_err());
        assert!(compatible_curl("curl 8.4.0 (Windows) libcurl/8.4.0"));
        assert!(compatible_curl("curl 9.0.0"));
        assert!(!compatible_curl("curl 8.3.1"));
        assert!(!compatible_curl("unknown"));
        let arguments = curl_arguments("https://example.com/file", Path::new("partial"), 7, 11);
        assert!(
            arguments
                .windows(2)
                .any(|pair| pair == ["--max-filesize", "4"])
        );
        assert!(
            arguments
                .windows(2)
                .any(|pair| pair == ["--continue-at", "7"])
        );
        assert!(
            arguments
                .windows(2)
                .any(|pair| pair == ["--proto-redir", "=https"])
        );
    }

    #[test]
    fn cached_artifacts_reject_corruption_and_retain_oversized_partials() -> crate::Result<()> {
        let working = std::env::temp_dir().join(format!(
            "Bonsai-DownloadTest-{}",
            crate::native_startup::nonce()?
        ));
        validate_external_path(&working).map_err(|error| error.to_string())?;
        fs::create_dir(&working).map_err(|error| error.to_string())?;
        let path = working.join("artifact.bin");
        let hash = crate::startup_cache::hex(&Sha256::digest(b"hello"));
        fs::write(&path, b"hello").map_err(|error| error.to_string())?;
        receive(
            "https://example.invalid/artifact",
            &path,
            5,
            &hash,
            &working,
        )?;
        assert!(!working.join("artifact.bin.download.lock").exists());
        fs::write(&path, b"jello").map_err(|error| error.to_string())?;
        verify_length(&path, 5)?;
        assert!(verify(&path, 5, &hash).is_err());
        assert!(
            receive(
                "https://example.invalid/artifact",
                &path,
                5,
                &hash,
                &working
            )
            .is_err()
        );
        assert_eq!(
            fs::read(&path).map_err(|error| error.to_string())?,
            b"jello"
        );
        let missing = working.join("missing.bin");
        let partial = working.join("missing.bin.partial");
        fs::write(&partial, b"too long").map_err(|error| error.to_string())?;
        assert!(
            receive(
                "https://example.invalid/artifact",
                &missing,
                5,
                &hash,
                &working
            )
            .is_err()
        );
        assert!(!missing.exists());
        assert_eq!(
            fs::read(&partial).map_err(|error| error.to_string())?,
            b"too long"
        );
        Ok(())
    }

    #[cfg(windows)]
    #[test]
    fn curl_resumes_with_a_bound_on_remaining_response_bytes() -> crate::Result<()> {
        use std::{io::Write as _, net::TcpListener, thread, time::Instant};
        let working = std::env::temp_dir().join(format!(
            "Bonsai-RangeTest-{}",
            crate::native_startup::nonce()?
        ));
        validate_external_path(&working).map_err(|error| error.to_string())?;
        fs::create_dir(&working).map_err(|error| error.to_string())?;
        let partial = working.join("artifact.partial");
        fs::write(&partial, b"hello").map_err(|error| error.to_string())?;
        let listener = TcpListener::bind("127.0.0.1:0").map_err(|error| error.to_string())?;
        let port = listener
            .local_addr()
            .map_err(|error| error.to_string())?
            .port();
        listener
            .set_nonblocking(true)
            .map_err(|error| error.to_string())?;
        let server = thread::spawn(move || -> crate::Result<String> {
            let deadline = Instant::now() + Duration::from_secs(10);
            while Instant::now() < deadline {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream
                            .set_read_timeout(Some(Duration::from_secs(3)))
                            .map_err(|error| error.to_string())?;
                        let mut request = [0; 4096];
                        let count = stream
                            .read(&mut request)
                            .map_err(|error| error.to_string())?;
                        stream.write_all(b"HTTP/1.1 206 Partial Content\r\nContent-Length: 5\r\nContent-Range: bytes 5-9/10\r\nConnection: close\r\n\r\nworld").map_err(|error| error.to_string())?;
                        return Ok(String::from_utf8_lossy(&request[..count]).into_owned());
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => return Err(error.to_string()),
                }
            }
            Err("Range fixture received no request.".into())
        });
        let uri = format!("http://127.0.0.1:{port}/artifact");
        let mut arguments = curl_arguments(&uri, &partial, 5, 10);
        // Isolate curl's resume semantics on loopback. Production arguments
        // remain HTTPS-only and the receiver rejects this fixture's URL.
        for argument in &mut arguments {
            if argument == "=https" {
                *argument = "=http".into();
            }
        }
        let output = crate::startup_process::run(
            system_tool("curl.exe")?.as_os_str(),
            &arguments,
            &working,
            &BTreeMap::new(),
            Duration::from_secs(10),
            &working.join("range.log"),
        )?;
        let request = server.join().map_err(|_| "Range fixture panicked.")??;
        assert_eq!(output.code, 0, "{}", output.stderr);
        assert!(request.to_ascii_lowercase().contains("range: bytes=5-"));
        assert_eq!(
            fs::read(&partial).map_err(|error| error.to_string())?,
            b"helloworld"
        );
        Ok(())
    }
}
