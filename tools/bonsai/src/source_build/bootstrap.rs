//! Immutable, digest-bound maintenance bootstrap publication.

use std::{
    collections::BTreeMap,
    fs,
    io::{Read as _, Write as _},
    path::Path,
    time::Duration,
};

use serde_json::{Value, json};

use crate::{startup_cache as cache, startup_process};

fn snapshot(target: &Path, tool: &Path, digest: &str) -> crate::Result<std::path::PathBuf> {
    let directory = target.join("bootstrap");
    bonsai_artifacts::validate_external_path(&directory).map_err(|error| error.to_string())?;
    fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let destination = directory.join(format!("{digest}.exe"));
    if !destination.exists() {
        let temporary = directory.join(format!("{}.partial", startup_process::unique()));
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|error| error.to_string())?;
        let mut options = fs::OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt as _;
            options.share_mode(1).custom_flags(0x0820_0000);
        }
        let mut input = options.open(tool).map_err(|error| error.to_string())?;
        std::io::copy(&mut input, &mut output).map_err(|error| error.to_string())?;
        output.sync_all().map_err(|error| error.to_string())?;
        drop(output);
        if cache::digest_file(&temporary)? != digest {
            return Err("The maintenance build changed during snapshot publication.".into());
        }
        match fs::hard_link(&temporary, &destination) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.to_string()),
        }
        fs::remove_file(temporary).map_err(|error| error.to_string())?;
    }
    if cache::digest_file(&destination)? != digest {
        return Err("The maintenance bootstrap snapshot changed.".into());
    }
    cache::assert_arm64(&destination)?;
    Ok(destination)
}

fn capabilities(tool: &Path, working: &Path) -> crate::Result<Value> {
    let current = std::env::current_exe().map_err(|error| error.to_string())?;
    // The snapshot digest was checked above. Windows keeps an executing image
    // immutable, so the same image can supply its compiled capabilities directly.
    let version = if current == tool {
        crate::version()
    } else {
        let result = startup_process::run(
            tool.as_os_str(),
            &["--version".into()],
            working,
            &BTreeMap::new(),
            Duration::from_secs(10),
            &working.join("bootstrap-version.log"),
        )?;
        if result.code != 0 || result.stdout.len() > 16_384 {
            return Err("The maintenance bootstrap version probe failed.".into());
        }
        serde_json::from_str(&result.stdout).map_err(|error| error.to_string())?
    };
    if version["Name"] != "bonsai-tools"
        || version["Version"] != "0.1.0"
        || version["ProtocolVersion"] != 5
        || version["StartupProtocolVersion"] != 1
        || version["BootstrapProtocolVersion"] != 1
        || version["Commands"].as_array().is_none_or(|commands| {
            !commands.iter().all(Value::is_string)
                || !commands.iter().any(|command| command == "source-build")
        })
    {
        return Err("Use a current maintenance build with bootstrap protocol 1.".into());
    }
    Ok(version)
}

pub(super) fn publish(target: &Path, working: &Path, tool: &Path) -> crate::Result<Value> {
    let _guard = cache::lock_file(&target.join("bootstrap.lock"))?;
    let digest = cache::digest_file(tool)?;
    let bootstrap = snapshot(target, tool, &digest)?;
    let version = capabilities(&bootstrap, working)?;
    let receipt = json!({"schemaVersion": 1, "sha256": digest, "version": version});
    let bytes = serde_json::to_vec(&receipt).map_err(|error| error.to_string())?;
    let receipt_path = target.join("bootstrap.json");
    let mut existing = Vec::new();
    let matches = fs::File::open(&receipt_path)
        .and_then(|file| file.take(32_769).read_to_end(&mut existing))
        .is_ok_and(|_| existing == bytes);
    if matches {
        return Ok(
            json!({"ToolPath": tool, "BootstrapPath": bootstrap, "ToolSha256": digest, "Version": version}),
        );
    }
    let temporary = target.join(format!("bootstrap-{}.partial", startup_process::unique()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|error| error.to_string())?;
    file.write_all(&bytes).map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())?;
    drop(file);
    fs::rename(&temporary, receipt_path).map_err(|error| error.to_string())?;
    Ok(
        json!({"ToolPath": tool, "BootstrapPath": bootstrap, "ToolSha256": digest, "Version": version}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn immutable_bootstrap_reuses_verified_bytes_and_rejects_tampering() -> crate::Result<()> {
        let target =
            std::env::temp_dir().join(format!("Bonsai-Bootstrap-{}", startup_process::unique()));
        fs::create_dir(&target).map_err(|error| error.to_string())?;
        let tool = target.join("tool.exe");
        let mut bytes = vec![0; 512];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[60..64].copy_from_slice(&128_u32.to_le_bytes());
        bytes[128..134].copy_from_slice(&[b'P', b'E', 0, 0, 0x64, 0xaa]);
        fs::write(&tool, bytes).map_err(|error| error.to_string())?;
        let digest = cache::digest_file(&tool)?;
        let first = snapshot(&target, &tool, &digest)?;
        assert_eq!(first, snapshot(&target, &tool, &digest)?);
        fs::write(&first, "tampered").map_err(|error| error.to_string())?;
        assert!(snapshot(&target, &tool, &digest).is_err());
        fs::remove_dir_all(target).map_err(|error| error.to_string())?;
        Ok(())
    }
}
