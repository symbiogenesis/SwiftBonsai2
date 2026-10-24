#![cfg(windows)]
#![forbid(unsafe_code)]
//! Windows sharing-mode regressions with synthetic files; no executable runs.

use bonsai_artifacts::{ArtifactError, RuntimeSpec, VerifiedFile, VerifiedRuntime};
use sha2::{Digest, Sha256};
use std::{
    error::Error,
    fs::{self, OpenOptions},
    io::{Cursor, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

type TestResult = Result<(), Box<dyn Error>>;

struct Fixture {
    root: PathBuf,
    spec: RuntimeSpec,
}

impl Fixture {
    fn new() -> Result<Self, Box<dyn Error>> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let root = std::env::temp_dir().join(format!(
            "SwiftBonsai-artifact-lifetime-{}-{stamp}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root)?;
        let runtime_directory = root.join("runtime");
        fs::create_dir(&runtime_directory)?;
        fs::create_dir(runtime_directory.join("bin"))?;
        fs::create_dir(root.join("downloads"))?;

        let mut image = [0; 128];
        image[..2].copy_from_slice(b"MZ");
        image[60..64].copy_from_slice(&64_u32.to_le_bytes());
        image[64..70].copy_from_slice(&[b'P', b'E', 0, 0, 0x64, 0xaa]);
        let entries: [(&str, &[u8]); 3] = [
            ("llama-server.exe", &image),
            ("bin/ggml-cpu.dll", &image),
            ("LICENSE", b"synthetic artifact test\n"),
        ];
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, bytes) in entries {
            fs::write(runtime_directory.join(name), bytes)?;
            writer.start_file(
                name,
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Deflated),
            )?;
            writer.write_all(bytes)?;
        }
        let archive = writer.finish()?.into_inner();
        let archive_path = root.join("downloads").join("runtime.zip");
        fs::write(&archive_path, &archive)?;
        let spec = RuntimeSpec {
            archive_path,
            runtime_directory,
            archive_bytes: u64::try_from(archive.len())?,
            archive_sha256: digest(&archive),
        };
        Ok(Self { root, spec })
    }

    fn installed(&self, name: &str) -> PathBuf {
        self.spec.runtime_directory.join(name)
    }

    fn remove(self) -> TestResult {
        // Only this test's uniquely created directory is eligible for cleanup.
        assert!(self.root.is_absolute());
        assert_eq!(self.root.parent(), Some(std::env::temp_dir().as_path()));
        assert!(
            self.root
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("SwiftBonsai-artifact-lifetime-"))
        );
        fs::remove_dir_all(self.root)?;
        Ok(())
    }
}

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .into_iter()
        .flat_map(|byte| [byte >> 4, byte & 0x0f])
        .map(|nibble| char::from(b"0123456789abcdef"[usize::from(nibble)]))
        .collect()
}

fn assert_file_locked(path: &Path) {
    assert!(OpenOptions::new().write(true).open(path).is_err());
    assert!(fs::remove_file(path).is_err());
    assert!(fs::rename(path, path.with_extension("renamed")).is_err());
}

fn assert_file_released(path: &Path) -> TestResult {
    drop(OpenOptions::new().write(true).open(path)?);
    let renamed = path.with_extension("renamed");
    fs::rename(path, &renamed)?;
    fs::remove_file(renamed)?;
    Ok(())
}

#[test]
fn runtime_guard_holds_archive_installation_and_ancestors_until_drop() -> TestResult {
    let fixture = Fixture::new()?;
    let verified = VerifiedRuntime::verify(&fixture.spec)?;
    assert_eq!(verified.executable(), fixture.installed("llama-server.exe"));
    assert!(verified.contains_file(&fixture.installed("bin/ggml-cpu.dll")));
    assert!(!verified.contains_file(&fixture.spec.archive_path));
    assert!(!verified.contains_file(&fixture.spec.runtime_directory));
    for path in [
        &fixture.spec.archive_path,
        &fixture.installed("llama-server.exe"),
        &fixture.installed("bin/ggml-cpu.dll"),
        &fixture.installed("LICENSE"),
    ] {
        assert_file_locked(path);
    }
    let moved = fixture.root.with_extension("moved");
    assert!(fs::rename(&fixture.root, &moved).is_err());
    let moved_runtime = fixture.root.join("moved-runtime");
    assert!(fs::rename(&fixture.spec.runtime_directory, &moved_runtime).is_err());

    drop(verified);
    fs::rename(&fixture.spec.runtime_directory, &moved_runtime)?;
    fs::rename(&moved_runtime, &fixture.spec.runtime_directory)?;
    fs::rename(&fixture.root, &moved)?;
    fs::rename(&moved, &fixture.root)?;
    for path in [
        &fixture.spec.archive_path,
        &fixture.installed("llama-server.exe"),
        &fixture.installed("bin/ggml-cpu.dll"),
        &fixture.installed("LICENSE"),
    ] {
        assert_file_released(path)?;
    }
    fixture.remove()
}

#[test]
fn file_guard_holds_file_and_ancestors_until_drop() -> TestResult {
    let fixture = Fixture::new()?;
    let path = fixture.installed("LICENSE");
    let bytes = fs::read(&path)?;
    let verified = VerifiedFile::verify(&path, u64::try_from(bytes.len())?, &digest(&bytes))?;
    assert_file_locked(&path);
    let moved = fixture.root.join("moved-runtime");
    assert!(fs::rename(&fixture.spec.runtime_directory, &moved).is_err());
    drop(verified);
    fs::rename(&fixture.spec.runtime_directory, &moved)?;
    fs::rename(&moved, &fixture.spec.runtime_directory)?;
    assert_file_released(&path)?;
    fixture.remove()
}

#[test]
fn runtime_verification_rejects_an_existing_writer() -> TestResult {
    let fixture = Fixture::new()?;
    for path in [
        &fixture.spec.archive_path,
        &fixture.installed("bin/ggml-cpu.dll"),
    ] {
        let writer = OpenOptions::new().write(true).open(path)?;
        assert_eq!(
            VerifiedRuntime::verify(&fixture.spec).err(),
            Some(ArtifactError::Path)
        );
        drop(writer);
        drop(VerifiedRuntime::verify(&fixture.spec)?);
    }
    fixture.remove()
}

#[test]
fn file_verification_rejects_an_existing_writer() -> TestResult {
    let fixture = Fixture::new()?;
    let path = fixture.installed("LICENSE");
    let bytes = fs::read(&path)?;
    let writer = OpenOptions::new().write(true).open(&path)?;
    assert_eq!(
        VerifiedFile::verify(&path, u64::try_from(bytes.len())?, &digest(&bytes)).err(),
        Some(ArtifactError::Path)
    );
    drop(writer);
    drop(VerifiedFile::verify(
        &path,
        u64::try_from(bytes.len())?,
        &digest(&bytes),
    )?);
    fixture.remove()
}

#[test]
fn runtime_verification_rejects_changed_and_unexpected_installed_files() -> TestResult {
    let fixture = Fixture::new()?;
    // This entry sorts last, so rejection also exercises cleanup of earlier files.
    let path = fixture.installed("llama-server.exe");
    let original = fs::read(&path)?;
    let mut changed = original.clone();
    changed[127] ^= 1;
    fs::write(&path, &changed)?;
    assert_eq!(
        VerifiedRuntime::verify(&fixture.spec).err(),
        Some(ArtifactError::Installation)
    );
    fs::write(&path, original)?;
    let unexpected = fixture.installed("unexpected.txt");
    fs::write(&unexpected, b"not in the archive")?;
    assert_eq!(
        VerifiedRuntime::verify(&fixture.spec).err(),
        Some(ArtifactError::Installation)
    );
    fs::remove_file(unexpected)?;
    drop(VerifiedRuntime::verify(&fixture.spec)?);
    fixture.remove()
}

#[test]
fn cancelled_runtime_verification_releases_open_handles() -> TestResult {
    let fixture = Fixture::new()?;
    let installed = fixture.installed("bin/ggml-cpu.dll");
    drop(OpenOptions::new().write(true).open(&installed)?);
    let mut cancelled_after_file_lock = false;
    assert_eq!(
        VerifiedRuntime::verify_with(&fixture.spec, || {
            if OpenOptions::new().write(true).open(&installed).is_err() {
                assert!(
                    OpenOptions::new()
                        .write(true)
                        .open(&fixture.spec.archive_path)
                        .is_err()
                );
                cancelled_after_file_lock = true;
                Err(ArtifactError::Cancelled)
            } else {
                Ok(())
            }
        })
        .err(),
        Some(ArtifactError::Cancelled)
    );
    assert!(cancelled_after_file_lock);
    assert_file_released(&fixture.spec.archive_path)?;
    assert_file_released(&installed)?;
    assert_file_released(&fixture.installed("llama-server.exe"))?;
    let moved = fixture.root.with_extension("moved");
    fs::rename(&fixture.root, &moved)?;
    fs::rename(&moved, &fixture.root)?;
    fixture.remove()
}
