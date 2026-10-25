//! Packaged offline runtime staging request and receipt contract.
#![cfg(windows)]
#![forbid(unsafe_code)]

use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use std::fs;
use std::io::{Cursor, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};
use zip::{ZipWriter, write::SimpleFileOptions};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn invoke(root: &Path, request: &Value) -> Result<Output, Box<dyn std::error::Error>> {
    let path = root.join("request.json");
    fs::write(&path, serde_json::to_vec(request)?)?;
    Ok(Command::new(env!("CARGO_BIN_EXE_bonsai-tools"))
        .args(["runtime-stage", "--request"])
        .arg(path)
        .current_dir(root)
        .output()?)
}

#[test]
fn runtime_stage_cli_rejects_bad_requests_and_returns_offline_receipt() -> TestResult {
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let root = std::env::temp_dir().join(format!(
        "SwiftBonsai-runtime-command-{}-{stamp}",
        std::process::id()
    ));
    fs::create_dir(&root)?;
    let archive = root.join("runtime.zip");
    let destination = root.join("runtime");
    let mut image = vec![0; 128];
    image[..2].copy_from_slice(b"MZ");
    image[60..64].copy_from_slice(&64_u32.to_le_bytes());
    image[64..70].copy_from_slice(&[b'P', b'E', 0, 0, 0x64, 0xaa]);
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file("llama-server.exe", SimpleFileOptions::default())?;
    zip.write_all(&image)?;
    let bytes = zip.finish()?.into_inner();
    fs::write(&archive, &bytes)?;
    let hash: String = Sha256::digest(&bytes)
        .into_iter()
        .flat_map(|byte| [byte >> 4, byte & 15])
        .map(|nibble| char::from(b"0123456789abcdef"[usize::from(nibble)]))
        .collect();
    let request = json!({
        "schemaVersion": 1,
        "runtime": {"archivePath": archive, "runtimeDirectory": destination,
            "archiveBytes": bytes.len(), "archiveSha256": hash},
    });
    for invalid in [
        json!([1, request["runtime"]]),
        json!({"schemaVersion": 1, "runtime": [archive, destination, bytes.len(), hash]}),
        json!({"schemaVersion": 2, "runtime": request["runtime"]}),
        json!({"schemaVersion": 1, "runtime": request["runtime"], "extra": true}),
        json!({"schemaVersion": 1, "runtime": {"archivePath": archive}}),
    ] {
        let output = invoke(&root, &invalid)?;
        assert!(!output.status.success());
        assert!(!destination.exists());
        assert!(!fs::read_dir(&root)?.any(|entry| {
            entry.is_ok_and(|entry| entry.file_name().to_string_lossy().contains(".staging-"))
        }));
    }
    let output = invoke(&root, &request)?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(receipt["Status"], "staged");
    let staging = PathBuf::from(
        receipt["StagingDirectory"]
            .as_str()
            .ok_or("Missing staging path")?,
    );
    assert_eq!(staging.parent(), destination.parent());
    assert!(!destination.exists());
    assert_eq!(
        receipt["ServerPath"],
        staging.join("llama-server.exe").to_string_lossy().as_ref()
    );
    assert_eq!(
        receipt["RuntimeDirectory"],
        destination.to_string_lossy().as_ref()
    );
    assert_eq!(receipt["RuntimeFiles"][0]["Bytes"], 128);
    assert_eq!(fs::read(staging.join("llama-server.exe"))?, image);
    assert!(receipt.get("ReportedVersion").is_none());
    fs::write(&destination, b"existing destination")?;
    let again = invoke(&root, &request)?;
    assert!(!again.status.success());
    assert_eq!(fs::read(&destination)?, b"existing destination");
    assert_eq!(fs::read(staging.join("llama-server.exe"))?, image);

    assert_eq!(root.parent(), Some(std::env::temp_dir().as_path()));
    assert!(
        root.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("SwiftBonsai-runtime-command-"))
    );
    fs::remove_dir_all(root)?;
    Ok(())
}
