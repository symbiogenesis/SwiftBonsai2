//! Synthetic child regression for runtime guards spanning native execution.
#![forbid(unsafe_code)]
#![cfg(all(windows, target_arch = "aarch64"))]

use std::fs::{self, File, OpenOptions};
use std::io::{Write as _, stdout};
use std::os::windows::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use bonsai_artifacts::RuntimeSpec;
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use zip::{ZipWriter, write::SimpleFileOptions};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .into_iter()
        .flat_map(|byte| [byte >> 4, byte & 15])
        .map(|nibble| char::from(b"0123456789abcdef"[usize::from(nibble)]))
        .collect()
}

fn fixture() -> Result<(PathBuf, RuntimeSpec)> {
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let root = std::env::temp_dir().join(format!(
        "Bonsai-RuntimeChild-{}-{stamp}",
        std::process::id()
    ));
    let directory = root.join("runtime");
    fs::create_dir(&root)?;
    fs::create_dir(&directory)?;
    let executable = fs::read(std::env::current_exe()?)?;
    let mut library = vec![0; 128];
    library[..2].copy_from_slice(b"MZ");
    library[60] = 64;
    library[64..70].copy_from_slice(&[b'P', b'E', 0, 0, 0x64, 0xaa]);
    let archive_path = root.join("runtime.zip");
    let mut archive = ZipWriter::new(File::create(&archive_path)?);
    for (name, bytes) in [
        ("llama-server.exe", &executable),
        ("llama-completion.exe", &executable),
        ("ggml.dll", &library),
    ] {
        fs::write(directory.join(name), bytes)?;
        archive.start_file(name, SimpleFileOptions::default())?;
        archive.write_all(bytes)?;
    }
    archive.finish()?;
    let bytes = fs::read(&archive_path)?;
    let spec = RuntimeSpec {
        archive_path,
        runtime_directory: directory,
        archive_bytes: u64::try_from(bytes.len())?,
        archive_sha256: digest(&bytes),
    };
    Ok((root, spec))
}

fn caller_identity(root: &Path) -> Result<u64> {
    let shell = PathBuf::from(std::env::var_os("SystemRoot").ok_or("Missing system root")?)
        .join("System32/WindowsPowerShell/v1.0/powershell.exe");
    let output = Command::new(shell)
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &format!(
                "(Get-Process -Id {}).StartTime.ToUniversalTime().ToFileTimeUtc()",
                std::process::id()
            ),
        ])
        .current_dir(root)
        .creation_flags(0x0800_0000)
        .output()?;
    if !output.status.success() {
        return Err("Cannot observe caller identity".into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().parse()?)
}

#[test]
fn runtime_child_observes_guards() -> Result<()> {
    let executable = std::env::current_exe()?;
    if executable
        .file_name()
        .is_none_or(|name| name != "llama-completion.exe")
    {
        return Ok(());
    }
    let directory = executable.parent().ok_or("Missing runtime directory")?;
    let root = directory.parent().ok_or("Missing fixture root")?;
    for path in [root.join("runtime.zip"), directory.join("ggml.dll")] {
        assert!(OpenOptions::new().write(true).open(&path).is_err());
        assert!(fs::remove_file(&path).is_err());
    }
    writeln!(stdout().lock(), "retained-runtime")?;
    Ok(())
}

#[test]
fn runtime_guard_survives_child_execution_and_releases_after_exit() -> Result<()> {
    let (root, runtime) = fixture()?;
    let evidence = root.join("evidence");
    fs::create_dir(&evidence)?;
    let creation = caller_identity(&root)?;
    let request = json!({
        "schemaVersion": 1, "executable": runtime.runtime_directory.join("llama-completion.exe"),
        "arguments": ["--exact", "runtime_child_observes_guards", "--nocapture"],
        "workingDirectory": root, "evidenceDirectory": evidence, "artifacts": [], "runtime": runtime,
        "memoryLimitBytes": 536_870_912, "requiredAvailableBytes": 0, "physicalReserveBytes": 0,
        "timeoutSeconds": 10, "stdoutLimitBytes": 32768, "stderrLimitBytes": 32768,
        "requireNonemptyStdout": true, "ciOnly": false,
        "parentProcessId": std::process::id(), "parentCreationTimeFileTime": creation,
    });
    let rejected_evidence = root.join("rejected");
    fs::create_dir(&rejected_evidence)?;
    let mut outside = request.clone();
    outside["executable"] = json!(std::env::current_exe()?);
    outside["evidenceDirectory"] = json!(rejected_evidence);
    assert!(bonsai_tools::completion::run(outside).is_err());
    assert!(!rejected_evidence.join("stdout.txt").exists());
    let result = bonsai_tools::completion::run(request)?;
    assert_eq!(result["Status"], "completed");
    assert_eq!(
        result["RuntimeVerification"],
        "verified-and-retained-through-exit"
    );
    let files = result["RuntimeFiles"]
        .as_array()
        .ok_or("Missing runtime file identities")?;
    assert_eq!(files.len(), 3);
    assert!(fs::read_to_string(evidence.join("stdout.txt"))?.contains("retained-runtime"));
    for path in [
        runtime.archive_path,
        runtime.runtime_directory.join("ggml.dll"),
    ] {
        let _writer = OpenOptions::new().write(true).open(&path)?;
    }
    let receipt: Value = serde_json::from_slice(&fs::read(evidence.join("supervision.json"))?)?;
    assert_eq!(receipt["RuntimeFiles"], result["RuntimeFiles"]);
    Ok(())
}
