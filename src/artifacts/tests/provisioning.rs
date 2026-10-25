//! Focused offline runtime installation regressions with synthetic ZIPs.
#![cfg(windows)]
#![forbid(unsafe_code)]

use bonsai_artifacts::{ArtifactError, RuntimeSpec, VerifiedRuntime, stage_runtime};
use sha2::{Digest as _, Sha256};
use std::fs;
use std::io::{Cursor, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

type TestResult = Result<(), Box<dyn std::error::Error>>;

struct Fixture {
    root: PathBuf,
    staging: PathBuf,
    spec: RuntimeSpec,
}

impl Fixture {
    fn new(bytes: &[u8]) -> Result<Self, Box<dyn std::error::Error>> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let root = std::env::temp_dir().join(format!(
            "SwiftBonsai-runtime-install-{}-{stamp}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root)?;
        let archive_path = root.join("runtime.zip");
        fs::write(&archive_path, bytes)?;
        Ok(Self {
            staging: root.join("runtime.staging"),
            spec: RuntimeSpec {
                archive_path,
                runtime_directory: root.join("runtime"),
                archive_bytes: u64::try_from(bytes.len())?,
                archive_sha256: digest(bytes),
            },
            root,
        })
    }

    fn remove(self) -> TestResult {
        assert!(self.root.is_absolute());
        assert_eq!(self.root.parent(), Some(std::env::temp_dir().as_path()));
        assert!(
            self.root
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("SwiftBonsai-runtime-install-"))
        );
        fs::remove_dir_all(self.root)?;
        Ok(())
    }
}

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .into_iter()
        .flat_map(|byte| [byte >> 4, byte & 15])
        .map(|nibble| char::from(b"0123456789abcdef"[usize::from(nibble)]))
        .collect()
}

fn image() -> Vec<u8> {
    let mut bytes = vec![0; 128];
    bytes[..2].copy_from_slice(b"MZ");
    bytes[60..64].copy_from_slice(&64_u32.to_le_bytes());
    bytes[64..70].copy_from_slice(&[b'P', b'E', 0, 0, 0x64, 0xaa]);
    bytes
}

fn archive(entries: &[(&str, &[u8])]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    for &(name, content) in entries {
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        if name.ends_with('/') {
            zip.add_directory(name, options)?;
        } else {
            zip.start_file(name, options)?;
            zip.write_all(content)?;
        }
    }
    Ok(zip.finish()?.into_inner())
}

fn assert_absent(path: &Path) {
    assert!(fs::symlink_metadata(path).is_err());
}

#[test]
fn runtime_stage_prepares_complete_tree_without_publishing() -> TestResult {
    let image = image();
    let bytes = archive(&[
        ("llama-server.exe", &image),
        ("bin/ggml.dll", &image),
        ("LICENSE", b"synthetic test"),
        ("empty/", b""),
    ])?;
    let fixture = Fixture::new(&bytes)?;
    let files = stage_runtime(&fixture.spec, &fixture.staging)?;
    assert_absent(&fixture.spec.runtime_directory);
    assert_eq!(files.len(), 3);
    assert!(
        files
            .iter()
            .all(|file| file.path().starts_with(&fixture.staging))
    );
    assert_eq!(fs::read(&fixture.spec.archive_path)?, bytes);
    assert_eq!(fs::read(fixture.staging.join("bin/ggml.dll"))?, image);
    assert!(fixture.staging.join("empty").is_dir());
    let staged_spec = RuntimeSpec {
        runtime_directory: fixture.staging.clone(),
        ..fixture.spec.clone()
    };
    let verified = VerifiedRuntime::verify(&staged_spec)?;
    assert_eq!(verified.files(), files);
    drop(verified);
    fixture.remove()
}

#[test]
fn runtime_stage_rejects_unsafe_archive_policy_before_staging() -> TestResult {
    let image = image();
    for entries in [
        vec![
            ("llama-server.exe", image.as_slice()),
            ("../escape", b"x".as_slice()),
        ],
        vec![("llama-server.exe", &image), ("NUL.dll", b"x")],
        vec![("llama-server.exe", &image), ("trailing./", b"")],
        vec![("llama-server.exe", &image), ("trailing /entry", b"x")],
        vec![("llama-server.exe", &image), ("bad\\entry", b"x")],
        vec![("llama-server.exe", &image), ("FILE", b"x"), ("file", b"y")],
        vec![
            ("llama-server.exe", &image),
            ("bin", b"x"),
            ("bin/file", b"y"),
        ],
        vec![
            ("llama-server.exe", &image),
            ("bin/llama-server.exe", &image),
        ],
        vec![("LICENSE", b"no server")],
    ] {
        let fixture = Fixture::new(&archive(&entries)?)?;
        assert_eq!(
            stage_runtime(&fixture.spec, &fixture.staging),
            Err(ArtifactError::Archive)
        );
        assert_absent(&fixture.spec.runtime_directory);
        assert_absent(&fixture.staging);
        assert_absent(&fixture.root.join("escape"));
        fixture.remove()?;
    }
    Ok(())
}

#[test]
fn runtime_stage_rejects_identity_architecture_and_corrupt_payloads() -> TestResult {
    let image = image();
    let bytes = archive(&[("llama-server.exe", &image)])?;
    let mut fixture = Fixture::new(&bytes)?;
    fixture.spec.archive_sha256 = "0".repeat(64);
    assert_eq!(
        stage_runtime(&fixture.spec, &fixture.staging),
        Err(ArtifactError::Identity)
    );
    assert_absent(&fixture.staging);
    assert_absent(&fixture.spec.runtime_directory);
    fixture.remove()?;

    let mut wrong = image.clone();
    wrong[68..70].copy_from_slice(&0x8664_u16.to_le_bytes());
    let fixture = Fixture::new(&archive(&[("llama-server.exe", &wrong)])?)?;
    assert_eq!(
        stage_runtime(&fixture.spec, &fixture.staging),
        Err(ArtifactError::Architecture)
    );
    assert_absent(&fixture.spec.runtime_directory);
    assert!(fixture.staging.join("llama-server.exe").is_file());
    fixture.remove()?;

    let mut corrupt = bytes;
    let position = corrupt
        .windows(image.len())
        .position(|window| window == image)
        .ok_or("Missing stored ZIP payload")?;
    corrupt[position + 127] ^= 1;
    let fixture = Fixture::new(&corrupt)?;
    assert_eq!(
        stage_runtime(&fixture.spec, &fixture.staging),
        Err(ArtifactError::Archive)
    );
    assert_absent(&fixture.spec.runtime_directory);
    assert!(fixture.staging.is_dir());
    fixture.remove()
}

#[test]
fn runtime_stage_rejects_entry_and_expansion_budgets() -> TestResult {
    let image = image();
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file("llama-server.exe", SimpleFileOptions::default())?;
    zip.write_all(&image)?;
    for index in 0..4096 {
        zip.start_file(format!("entry-{index}"), SimpleFileOptions::default())?;
    }
    let fixture = Fixture::new(&zip.finish()?.into_inner())?;
    assert_eq!(
        stage_runtime(&fixture.spec, &fixture.staging),
        Err(ArtifactError::Archive)
    );
    assert_absent(&fixture.spec.runtime_directory);
    assert_absent(&fixture.staging);
    fixture.remove()?;

    let mut bytes = archive(&[("llama-server.exe", &image)])?;
    let central = bytes
        .windows(4)
        .position(|window| window == b"PK\x01\x02")
        .ok_or("Missing ZIP central header")?;
    bytes[central + 24..central + 28].copy_from_slice(&(512_u32 * 1024 * 1024 + 1).to_le_bytes());
    let fixture = Fixture::new(&bytes)?;
    assert_eq!(
        stage_runtime(&fixture.spec, &fixture.staging),
        Err(ArtifactError::Archive)
    );
    assert_absent(&fixture.spec.runtime_directory);
    assert_absent(&fixture.staging);
    fixture.remove()
}

#[test]
fn runtime_stage_preserves_existing_staging_and_destination() -> TestResult {
    let image = image();
    let bytes = archive(&[("llama-server.exe", &image)])?;
    for is_directory in [false, true] {
        let fixture = Fixture::new(&bytes)?;
        if is_directory {
            fs::create_dir(&fixture.spec.runtime_directory)?;
        } else {
            fs::write(&fixture.spec.runtime_directory, b"existing destination")?;
        }
        assert_eq!(
            stage_runtime(&fixture.spec, &fixture.staging),
            Err(ArtifactError::Path)
        );
        assert_absent(&fixture.staging);
        if is_directory {
            assert_eq!(fs::read_dir(&fixture.spec.runtime_directory)?.count(), 0);
        } else {
            assert_eq!(
                fs::read(&fixture.spec.runtime_directory)?,
                b"existing destination"
            );
        }
        fixture.remove()?;
    }
    let fixture = Fixture::new(&bytes)?;
    fs::create_dir(&fixture.staging)?;
    fs::write(fixture.staging.join("keep.txt"), b"existing staging")?;
    assert_eq!(
        stage_runtime(&fixture.spec, &fixture.staging),
        Err(ArtifactError::Path)
    );
    assert_eq!(
        fs::read(fixture.staging.join("keep.txt"))?,
        b"existing staging"
    );
    assert_absent(&fixture.spec.runtime_directory);
    fixture.remove()
}
