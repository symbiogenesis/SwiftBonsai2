//! Shared Windows shell and process identity setup for supervision fixtures.
#![expect(
    clippy::redundant_pub_crate,
    reason = "Shared fixture helpers stay visible only inside their owning integration test."
)]

use std::os::windows::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub(crate) fn shell() -> Result<PathBuf> {
    Ok(
        PathBuf::from(std::env::var_os("SystemRoot").ok_or("Missing Windows system root.")?)
            .join("System32/WindowsPowerShell/v1.0/powershell.exe"),
    )
}

pub(crate) fn caller_identity(directory: &Path) -> Result<u64> {
    let output = Command::new(shell()?)
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
        .current_dir(directory)
        .creation_flags(0x0800_0000)
        .stdin(Stdio::null())
        .output()?;
    if !output.status.success() {
        return Err("Cannot observe test caller identity.".into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().parse()?)
}
