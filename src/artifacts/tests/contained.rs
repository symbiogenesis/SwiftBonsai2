//! Model-free verification inside the actual native Windows containment boundary.
#![cfg(windows)]
#![forbid(unsafe_code)]

use std::{ffi::OsString, fs, io, time::Duration};

use bonsai_artifacts::{ArtifactError, VerifiedFile, open_locked_file, validate_external_path};
use bonsai_windows_credentials::{PrivateDirectory, RetainedCredential, is_app_container};
use bonsai_windows_process::{OwnedProcess, ProcessSpec, SandboxPolicy, native_sandbox_available};

const MODEL: &[u8] = b"abc";
const MODEL_HASH: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
const CHILD: &str = "contained_verification_child";

#[test]
fn host_verification_keeps_git_ancestry_rejection_and_retained_handles() -> io::Result<()> {
    assert!(!is_app_container()?, "Run the owner outside containment");
    let directory = PrivateDirectory::create(&std::env::temp_dir())?;
    let storage = directory.path().join("storage");
    fs::create_dir(&storage)?;
    let model = storage.join("model.bin");
    let git_marker = storage.join(".git");
    fs::write(&model, MODEL)?;
    fs::write(&git_marker, b"synthetic repository marker")?;
    assert_eq!(
        VerifiedFile::verify(&model, 3, MODEL_HASH).err(),
        Some(ArtifactError::Path)
    );
    assert!(open_locked_file(&model).is_err());
    fs::remove_file(git_marker)?;
    let model_guard = VerifiedFile::verify(&model, 3, MODEL_HASH).map_err(io::Error::other)?;
    assert!(fs::write(&model, b"changed").is_err());
    drop(model_guard);
    fs::write(&model, MODEL)
}

#[test]
#[ignore = "Requires a supported native PSEC host; invoke explicitly with --ignored and --exact."]
fn contained_verification_retains_exact_artifact_and_credential() -> io::Result<()> {
    assert!(!is_app_container()?, "Run the owner outside containment");
    native_sandbox_available()?;
    let directory = PrivateDirectory::create(&std::env::temp_dir())?;
    let scratch = directory.path().join("worker");
    fs::create_dir(&scratch)?;
    let model = directory.path().join("model.bin");
    let key = directory.path().join("backend.key");
    fs::write(&model, MODEL)?;
    fs::write(&key, [b'a'; 64])?;
    fs::write(directory.path().join("frontend.key"), [b'f'; 64])?;
    validate_external_path(&model).map_err(io::Error::other)?;
    let _model = VerifiedFile::verify(&model, 3, MODEL_HASH).map_err(io::Error::other)?;
    let _key = RetainedCredential::open(&key)?;
    let executable = std::env::current_exe()?;
    let specification = ProcessSpec {
        executable: executable.clone(),
        arguments: ["--exact", CHILD, "--ignored", "--nocapture"]
            .map(OsString::from)
            .into(),
        working_directory: scratch.clone(),
        stdout: directory.path().join("child.stdout"),
        stderr: directory.path().join("child.stderr"),
        memory_limit_bytes: 256 * 1024 * 1024,
        active_process_limit: 1,
    };
    let policy = SandboxPolicy {
        readonly_paths: vec![executable, model, key],
        writable_directory: scratch.clone(),
    };
    let process = OwnedProcess::start_sandboxed(&specification, &policy)?;
    let status = process.wait_for_exit(Duration::from_secs(20))?;
    process.stop(Duration::from_secs(5))?;
    if status != Some(0) || fs::read(scratch.join("verified"))? != b"complete" {
        return Err(io::Error::other(format!(
            "Contained verification failed; retained evidence: {}",
            directory.path().display()
        )));
    }
    Ok(())
}

#[test]
#[ignore = "Spawned only by the native-contained verifier owner regression."]
fn contained_verification_child() -> io::Result<()> {
    assert!(
        is_app_container()?,
        "The verifier must actually be contained"
    );
    let scratch = std::env::current_dir()?;
    let directory = scratch
        .parent()
        .ok_or_else(|| io::Error::other("Missing verification fixture parent"))?;
    let model = directory.join("model.bin");
    // Exact file authority must not silently grant volume-root metadata.
    assert!(validate_external_path(&model).is_err());
    let _model = VerifiedFile::verify(&model, 3, MODEL_HASH).map_err(io::Error::other)?;
    let key = RetainedCredential::open(&directory.join("backend.key"))?;
    assert_eq!(key.bytes(), &[b'a'; 64]);
    assert!(fs::read(directory.join("frontend.key")).is_err());
    assert!(fs::write(&model, b"changed").is_err());
    fs::write(scratch.join("verified"), b"complete")
}
