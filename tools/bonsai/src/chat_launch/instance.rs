//! Per-user launcher admission retained until the owned app tree has exited.

use std::{
    fs::{self, File, OpenOptions},
    path::Path,
};

use bonsai_artifacts::is_redirect;

pub(super) fn acquire(directory: &Path) -> crate::Result<Option<File>> {
    super::external(directory)?;
    fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    let path = directory.join("chat.lock");
    super::external(&path)?;
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt as _;
        // Keep the sentinel at one path, with no delete-sharing. Open the leaf
        // itself so a replacement cannot redirect the ordinary-file check.
        options.share_mode(0).custom_flags(0x0020_0000);
    }
    let guard = match options.open(&path) {
        Ok(guard) => guard,
        #[cfg(windows)]
        Err(error) if matches!(error.raw_os_error(), Some(32 | 33)) => return Ok(None),
        Err(error) => return Err(format!("Cannot acquire native chat admission: {error}")),
    };
    let metadata = guard.metadata().map_err(|error| error.to_string())?;
    if !metadata.is_file() || is_redirect(&metadata) {
        return Err("Native chat admission requires an ordinary lock file.".into());
    }
    match guard.try_lock() {
        Ok(()) => Ok(Some(guard)),
        Err(fs::TryLockError::WouldBlock) => Ok(None),
        Err(fs::TryLockError::Error(error)) => {
            Err(format!("Cannot lock native chat admission: {error}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn directory(name: &str) -> crate::Result<std::path::PathBuf> {
        let directory = std::env::temp_dir().join(format!(
            "SwiftBonsai-ChatInstance-{name}-{}",
            crate::startup_process::unique()
        ));
        super::super::external(&directory)?;
        fs::create_dir(&directory).map_err(|error| error.to_string())?;
        Ok(directory)
    }

    #[test]
    fn singleton_retains_admission_until_drop_and_keeps_persistent_sentinel() -> crate::Result<()> {
        let directory = directory("lifetime")?;
        let owner = acquire(&directory)?.ok_or("First launcher was rejected.")?;
        assert!(acquire(&directory)?.is_none());
        assert!(directory.join("chat.lock").is_file());
        drop(owner);
        let next = acquire(&directory)?.ok_or("Admission survived owner teardown.")?;
        assert!(directory.join("chat.lock").is_file());
        drop(next);
        fs::remove_dir_all(&directory).map_err(|error| error.to_string())
    }

    #[test]
    fn singleton_storage_errors_are_not_reported_as_duplicates() -> crate::Result<()> {
        let directory = directory("storage-error")?;
        fs::create_dir(directory.join("chat.lock")).map_err(|error| error.to_string())?;
        assert!(acquire(&directory).is_err());
        assert!(acquire(Path::new("relative-storage")).is_err());
        fs::remove_dir_all(&directory).map_err(|error| error.to_string())
    }
}
