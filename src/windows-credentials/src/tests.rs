use super::*;
use std::fs::OpenOptions;
use std::os::windows::fs::symlink_dir;

fn test_lane(name: &str) -> io::Result<PathBuf> {
    let parent = std::env::var_os("BONSAI_CREDENTIAL_TEST_ROOT")
        .map_or_else(std::env::temp_dir, PathBuf::from);
    validate_external_path(&parent)?;
    let mut nonce = [0; 16];
    native::random(&mut nonce)?;
    let lane = parent.join(format!("credentials-{name}-{}", hex(&nonce)));
    fs::create_dir(&lane)?;
    Ok(lane)
}

#[test]
#[allow(
    clippy::used_underscore_binding,
    reason = "Verify the noninheritable reader retained only to protect directory lifetime."
)]
fn private_worker_storage_has_protected_acl_without_extra_credentials() -> io::Result<()> {
    let parent = test_lane("worker-storage")?;
    let directory = PrivateDirectory::create(&parent)?;
    native::Security::new(true)?.verify(&directory._reader)?;
    assert!(native::noninheritable(&directory._reader)?);
    assert_eq!(fs::read_dir(directory.path())?.count(), 0);
    fs::write(directory.path().join("scratch.txt"), b"worker output")?;
    assert!(fs::rename(directory.path(), parent.join("renamed")).is_err());
    let path = directory.path().to_path_buf();
    drop(directory);
    assert_eq!(fs::read(path.join("scratch.txt"))?, b"worker output");
    Ok(())
}

#[test]
#[allow(
    clippy::used_underscore_binding,
    reason = "Check noninheritance of the handle retained only for its lifetime."
)]
fn retained_tokens_validate_content_and_hold_files_until_drop() -> io::Result<()> {
    let lane = test_lane("retained")?;
    let credentials = PrivateCredentials::create(&lane)?;
    let path = credentials.frontend_path().to_owned();
    let retained = RetainedCredential::open(&path)?;
    // Keep assertions value-free even when the secret differs unexpectedly.
    let same_text = retained.as_hex() == credentials.frontend_key();
    let same_bytes = retained.bytes().as_slice() == credentials.frontend_key().as_bytes();
    assert!(same_text);
    assert!(same_bytes);
    assert!(native::noninheritable(&retained._reader)?);
    drop(credentials);
    assert!(OpenOptions::new().write(true).open(&path).is_err());
    assert!(fs::remove_file(&path).is_err());
    drop(retained);
    assert!(OpenOptions::new().write(true).open(&path).is_ok());

    let malformed = lane.join("malformed.key");
    for bytes in [
        Vec::new(),
        vec![b'a'; 63],
        vec![b'a'; 65],
        vec![b'A'; 64],
        vec![b'g'; 64],
        vec![b'\n'; 64],
        vec![0xff; 64],
    ] {
        fs::write(&malformed, bytes)?;
        assert!(RetainedCredential::open(&malformed).is_err());
        // Failed validation releases its handle as well as its secret buffer.
        assert!(OpenOptions::new().write(true).open(&malformed).is_ok());
    }
    assert!(RetainedCredential::open(&lane).is_err());
    let repository = lane.join("repository");
    fs::create_dir(&repository)?;
    fs::write(repository.join(".git"), b"gitdir: elsewhere")?;
    fs::write(repository.join("token.key"), [b'a'; 64])?;
    assert!(RetainedCredential::open(&repository.join("token.key")).is_err());
    Ok(())
}

#[test]
#[allow(
    clippy::used_underscore_binding,
    reason = "Inspect the handles retained only for their lifetime in production."
)]
fn credentials_have_protected_acls_distinct_keys_and_retained_readers() -> io::Result<()> {
    let lane = test_lane("creation")?;
    let credentials = PrivateCredentials::create(&lane)?;
    assert_eq!(credentials.frontend_key().len(), 64);
    assert_eq!(credentials.backend_key().len(), 64);
    assert!(
        credentials
            .frontend_key()
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    );
    assert!(
        credentials
            .backend_key()
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    );
    // Do not let an assertion failure debug-format secret values.
    let distinct = credentials.frontend_key() != credentials.backend_key();
    assert!(distinct);
    let directory_security = native::Security::new(true)?;
    let file_security = native::Security::new(false)?;
    directory_security.verify(&credentials._directory_handle)?;
    file_security.verify(&credentials._frontend_handle)?;
    file_security.verify(&credentials._backend_handle)?;
    assert!(native::noninheritable(&credentials._directory_handle)?);
    assert!(native::noninheritable(&credentials._frontend_handle)?);
    assert!(native::noninheritable(&credentials._backend_handle)?);
    for path in [credentials.frontend_path(), credentials.backend_path()] {
        // ReadAllText-style readers can open while retained handles are live.
        let mut on_disk = Secret::<64>::new();
        let mut reader = native::open_reader(path, false)?;
        reader.read_exact(&mut *on_disk.0)?;
        assert_eq!(
            on_disk.0.len(),
            usize::try_from(reader.metadata()?.len()).map_err(io::Error::other)?
        );
        assert!(native::create_file(path, &file_security).is_err());
        assert!(OpenOptions::new().write(true).open(path).is_err());
        assert!(fs::remove_file(path).is_err());
    }
    let retained_directory = credentials.directory().to_path_buf();
    let retained_frontend = credentials.frontend_path().to_path_buf();
    assert!(fs::rename(&retained_directory, lane.join("moved")).is_err());
    drop(credentials);
    assert!(retained_directory.is_dir());
    assert!(retained_frontend.is_file());
    Ok(())
}

#[test]
fn protected_acl_precedes_bytes_and_create_new_never_overwrites() -> io::Result<()> {
    let lane = test_lane("exclusive")?;
    let directory = lane.join("private");
    let directory_security = native::Security::new(true)?;
    native::create_directory(&directory, &directory_security)?;
    let directory_handle = native::open_reader(&directory, true)?;
    directory_security.verify(&directory_handle)?;
    assert!(native::create_directory(&directory, &directory_security).is_err());
    let file_security = native::Security::new(false)?;
    let path = directory.join("empty.key");
    let mut writer = native::create_file(&path, &file_security)?;
    assert_eq!(writer.metadata()?.len(), 0);
    file_security.verify(&writer)?;
    writer.write_all(b"sentinel")?;
    writer.sync_all()?;
    assert!(native::create_file(&path, &file_security).is_err());
    drop(writer);
    assert_eq!(fs::read(&path)?, b"sentinel");
    Ok(())
}

#[test]
fn path_validation_rejects_aliases_git_and_reparse_ancestors() -> io::Result<()> {
    let lane = test_lane("paths")?;
    validate_external_path(Path::new(&format!("{}\\", lane.display())))?;
    validate_external_path(&lane.join("new-leaf"))?;
    validate_external_path(&lane.join("emoji-🦀"))?;
    for path in [
        "relative",
        "C:",
        "C:\\",
        "C:relative",
        "\\\\server\\share\\key",
        "\\\\?\\C:\\keys",
        "//./C:/keys",
        "C:\\keys:stream",
        "C:\\%TEMP%\\keys",
        "C:\\a\\.\\key",
        "C:\\a\\..\\key",
        "C:\\a\\key\0tail",
        "C:\\a\\CON.key",
        "C:\\a\\COM1",
        "C:\\a\\LPT9.key",
        "C:\\a\\key.",
        "C:\\a\\key ",
    ] {
        assert!(validate_external_path(Path::new(path)).is_err());
    }
    assert!(validate_external_path(&lane.join("missing").join("leaf")).is_err());
    let repository = lane.join("repository");
    fs::create_dir(&repository)?;
    fs::write(repository.join(".git"), b"gitdir: elsewhere")?;
    assert!(validate_external_path(&repository.join("new-leaf")).is_err());
    let target = lane.join("target");
    fs::create_dir(&target)?;
    let alias = lane.join("alias");
    symlink_dir(&target, &alias)?;
    assert!(validate_external_path(&alias.join("new-leaf")).is_err());
    assert!(!reserved_component("🦀"));
    Ok(())
}

#[test]
fn volatile_scrub_zeroes_the_owned_buffer() {
    let mut bytes = [0x5a; 64];
    native::scrub(&mut bytes);
    assert!(bytes.iter().all(|byte| *byte == 0));
}

#[test]
fn path_validation_rejects_win32_device_aliases_and_control_characters() -> io::Result<()> {
    let lane = test_lane("device-aliases")?;
    for component in [
        "NUL .key",
        "CONIN$",
        "CONOUT$",
        "COM\u{b9}",
        "com\u{b2}.key",
        "COM\u{b3}.log",
        "LPT\u{b9}.key",
        "lpt\u{b2}",
        "LPT\u{b3}.txt",
        "key\u{1}file",
        "key\tfile",
        "key\u{1f}file",
    ] {
        assert!(validate_external_path(&lane.join(component)).is_err());
    }
    for component in ["COM0", "COM10.key", "LPT10", "company.key", "\u{1f980}"] {
        validate_external_path(&lane.join(component))?;
        assert!(!reserved_component(component));
    }
    Ok(())
}
