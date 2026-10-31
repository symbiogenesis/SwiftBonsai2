//! Bounded on-disk observations of current-worker module candidates.

use std::{
    io::Read,
    time::{Duration, Instant},
};

use bonsai_windows_process::VulkanModuleFileCandidate;
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 64 * 1024 * 1024;
const MAX_FILES: usize = 16;

struct Budget {
    remaining: u64,
    deadline: Instant,
}

fn digest(
    reader: &mut impl Read,
    expected_bytes: u64,
    budget: &mut Budget,
) -> Result<[u8; 32], &'static str> {
    // Reserve an extra byte to detect growth/truncation without accepting a
    // prefix hash as a complete-file identity. Failed reads consume the budget.
    if expected_bytes == 0 || expected_bytes > MAX_FILE_BYTES {
        return Err("file-size-unavailable-or-exceeds-bound");
    }
    if expected_bytes >= budget.remaining {
        return Err("byte-budget-exhausted");
    }
    let mut reader = reader.take(expected_bytes + 1);
    let mut hash = Sha256::new();
    let mut bytes = 0_u64;
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        if Instant::now() >= budget.deadline {
            return Err("time-budget-exhausted");
        }
        let count = reader
            .read(&mut buffer)
            .map_err(|_| "file-read-unavailable")?;
        let count_bytes = u64::try_from(count).map_err(|_| "file-read-unavailable")?;
        budget.remaining -= count_bytes;
        bytes += count_bytes;
        if bytes > expected_bytes {
            return Err("file-size-changed");
        }
        if count == 0 {
            return if bytes == expected_bytes {
                Ok(hash.finalize().into())
            } else {
                Err("file-size-changed")
            };
        }
        hash.update(&buffer[..count]);
    }
}

fn observe_file(candidate: &VulkanModuleFileCandidate, budget: &mut Budget) -> Value {
    let mut report = json!({"file": candidate.name, "status": "unknown",
        "bytes": null, "sha256": null, "reason": null});
    let result = (|| {
        if Instant::now() >= budget.deadline {
            return Err("time-budget-exhausted");
        }
        let path = candidate.path.as_ref().ok_or("module-path-unavailable")?;
        // The existing artifact opener rejects redirects/nonlocal paths and
        // denies write/delete sharing while the handle is read. A path swap
        // before open still cannot bind these bytes to an image mapped earlier.
        let mut file =
            bonsai_artifacts::open_locked_file(path).map_err(|_| "file-open-unavailable")?;
        let metadata = file.metadata().map_err(|_| "file-metadata-unavailable")?;
        if !metadata.is_file() {
            return Err("module-path-is-not-file");
        }
        let hash = digest(&mut file, metadata.len(), budget)?;
        let after = file.metadata().map_err(|_| "file-metadata-unavailable")?;
        if after.len() != metadata.len() || after.modified().ok() != metadata.modified().ok() {
            return Err("file-metadata-changed");
        }
        Ok((metadata.len(), crate::identity::hex(&hash)))
    })();
    match result {
        Ok((bytes, hash)) => {
            report["status"] = json!("observed");
            report["bytes"] = json!(bytes);
            report["sha256"] = json!(hash);
        }
        Err(reason) => report["reason"] = json!(reason),
    }
    report
}

pub(super) fn observe(candidates: Option<&[VulkanModuleFileCandidate]>) -> Value {
    let Some(candidates) = candidates else {
        return Value::Null;
    };
    let mut budget = Budget {
        remaining: MAX_TOTAL_BYTES,
        deadline: Instant::now() + Duration::from_millis(500),
    };
    let files: Vec<_> = candidates
        .iter()
        .take(MAX_FILES)
        .map(|candidate| observe_file(candidate, &mut budget))
        .collect();
    json!({
        "scope": "on-disk-files-at-post-instance-module-paths",
        "files": files,
        "maximum_files": MAX_FILES, "maximum_file_bytes": MAX_FILE_BYTES,
        "maximum_total_read_bytes": MAX_TOTAL_BYTES, "read_bytes": MAX_TOTAL_BYTES - budget.remaining,
        "work_budget_milliseconds": 500,
        "loaded_image_identity_verified": false,
        "signature_verified": false, "dependencies_verified": false,
    })
}

#[cfg(test)]
mod tests {
    use std::io::{self, Cursor};

    use super::*;

    fn budget(remaining: u64) -> Budget {
        Budget {
            remaining,
            deadline: Instant::now() + Duration::from_secs(10),
        }
    }

    #[test]
    fn module_file_hashes_reject_prefixes_and_charge_failed_reads() {
        let mut allowance = budget(16);
        let hash = digest(&mut Cursor::new(b"abc"), 3, &mut allowance);
        assert_eq!(hash, Ok(Sha256::digest(b"abc").into()));
        assert_eq!(allowance.remaining, 13);
        assert_eq!(
            digest(&mut Cursor::new(b"ab"), 3, &mut allowance),
            Err("file-size-changed")
        );
        assert_eq!(allowance.remaining, 11);
        assert_eq!(
            digest(&mut Cursor::new(b"abcd"), 3, &mut allowance),
            Err("file-size-changed")
        );
        assert_eq!(allowance.remaining, 7);
    }

    #[test]
    fn module_file_caps_and_deadlines_reject_before_reading() {
        let mut reader = Cursor::new(b"abc");
        assert_eq!(
            digest(&mut reader, 3, &mut budget(3)),
            Err("byte-budget-exhausted")
        );
        assert_eq!(
            digest(
                &mut reader,
                MAX_FILE_BYTES + 1,
                &mut budget(MAX_TOTAL_BYTES)
            ),
            Err("file-size-unavailable-or-exceeds-bound")
        );
        let mut expired = Budget {
            remaining: 16,
            deadline: Instant::now(),
        };
        assert_eq!(
            digest(&mut reader, 3, &mut expired),
            Err("time-budget-exhausted")
        );
        assert_eq!(reader.position(), 0);
    }

    #[test]
    fn module_file_read_failures_and_private_paths_never_enter_json() {
        struct Failing;
        impl Read for Failing {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::other("private-path-and-driver-error"))
            }
        }
        assert_eq!(
            digest(&mut Failing, 3, &mut budget(16)),
            Err("file-read-unavailable")
        );
        let report = observe(Some(&[VulkanModuleFileCandidate {
            name: "driver.dll".into(),
            path: Some(std::path::PathBuf::from(
                r"C:\private-path-and-driver-error\<invalid>.dll",
            )),
        }]));
        assert_eq!(report["files"][0]["reason"], "file-open-unavailable");
        assert!(report["files"][0]["sha256"].is_null());
        assert_eq!(report["loaded_image_identity_verified"], false);
        assert!(!report.to_string().contains("private-path-and-driver-error"));
    }
}
