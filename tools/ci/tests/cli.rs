//! End-to-end command checks with real Git and external output files.
#![forbid(unsafe_code)]

use std::error::Error;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn empty_repository_diff_writes_unselected_actions_outputs() -> Result<(), Box<dyn Error>> {
    let temporary_root = fs::canonicalize(std::env::temp_dir())?;
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let scratch = temporary_root.join(format!("Bonsai-CiCommand-{}-{nonce}", std::process::id()));
    fs::create_dir(&scratch)?;
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output_path = scratch.join("output.txt");
    let summary_path = scratch.join("summary.txt");
    let output = Command::new(env!("CARGO_BIN_EXE_bonsai-ci"))
        .args(["impact", "--before", "HEAD", "--after", "HEAD", "--root"])
        .arg(root)
        .env("GITHUB_EVENT_NAME", "push")
        .env("GITHUB_OUTPUT", &output_path)
        .env("GITHUB_STEP_SUMMARY", &summary_path)
        .current_dir(&scratch)
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let plan: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(plan["full"], false, "{plan}");
    assert_eq!(plan["packages"], serde_json::json!([]));
    assert_eq!(plan["reason"], "changes since successful validation");
    assert!(fs::read_to_string(output_path)?.contains("packages=[]\n"));
    assert!(fs::read_to_string(summary_path)?.contains("Affected crates: none\n"));
    // Only remove the uniquely owned child verified beneath system temp.
    if scratch.parent() != Some(temporary_root.as_path()) {
        return Err("Unexpected command-test scratch ownership.".into());
    }
    fs::remove_dir_all(scratch)?;
    Ok(())
}
