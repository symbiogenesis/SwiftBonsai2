//! Optional encrypted recovery. Disk and DPAPI work run on a dedicated worker.
#![allow(
    clippy::redundant_pub_crate,
    reason = "Keep the private persistence worker visible only within the native application."
)]
use std::{
    cell::Cell,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    os::windows::fs::{MetadataExt as _, OpenOptionsExt as _},
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, SyncSender, TrySendError},
    thread::JoinHandle,
};

const MAX_BYTES: usize = 8 * 1024 * 1024;
const MAX_SEALED: u64 = 8 * 1024 * 1024 + 65_536;

enum Command {
    Save(u64, Vec<u8>),
    Erase(u64),
    Shutdown,
}

pub(crate) enum Event {
    Saved(u64),
    Erased(u64),
    Failed(u64),
}

pub(crate) struct Recovery {
    commands: SyncSender<Command>,
    pub(crate) events: Receiver<Event>,
    worker: Option<JoinHandle<()>>,
    operation: Cell<u64>,
}

pub(crate) struct Opened {
    pub(crate) store: Recovery,
    pub(crate) saved: Option<Vec<u8>>,
    pub(crate) load_failed: bool,
}

impl Recovery {
    /// Opens one recovery store before starting the native event loop. A file
    /// lock prevents concurrent windows from overwriting each other's recovery.
    pub(crate) fn open(data: &Path) -> io::Result<Opened> {
        let path = data.join("chat-recovery.dpapi");
        let lock_path = data.join("chat-recovery.lock");
        for candidate in [data, path.as_path(), lock_path.as_path()] {
            bonsai_windows_credentials::validate_external_path(candidate)?;
        }
        let lock = open_lock(&lock_path)?;
        let loaded = read(&path);
        let load_failed = loaded.is_err();
        let saved = loaded.unwrap_or_default();
        let (commands, incoming) = mpsc::sync_channel(1);
        let (events, outgoing) = mpsc::channel();
        let worker = std::thread::Builder::new()
            .name("swift-chat-recovery".to_owned())
            .spawn(move || {
                let _lock = lock;
                while let Ok(command) = incoming.recv() {
                    let event = match command {
                        Command::Save(operation, bytes) => {
                            if save(&path, &bytes).is_ok() {
                                Event::Saved(operation)
                            } else {
                                Event::Failed(operation)
                            }
                        }
                        Command::Erase(operation) => {
                            if erase(&path).is_ok() {
                                Event::Erased(operation)
                            } else {
                                Event::Failed(operation)
                            }
                        }
                        Command::Shutdown => break,
                    };
                    if events.send(event).is_err() {
                        break;
                    }
                }
            })?;
        Ok(Opened {
            store: Self {
                commands,
                events: outgoing,
                worker: Some(worker),
                operation: Cell::new(0),
            },
            saved,
            load_failed,
        })
    }

    /// Returns false if the worker is occupied; retain dirty state and retry
    /// later. The channel holds at most one bounded plaintext snapshot.
    pub(crate) fn save(&self, bytes: Vec<u8>) -> bool {
        if bytes.is_empty() || bytes.len() > MAX_BYTES {
            return false;
        }
        let operation = self.operation.get().saturating_add(1);
        if self
            .commands
            .try_send(Command::Save(operation, bytes))
            .is_ok()
        {
            self.operation.set(operation);
            true
        } else {
            false
        }
    }

    pub(crate) fn erase(&self) -> bool {
        let operation = self.operation.get().saturating_add(1);
        match self.commands.try_send(Command::Erase(operation)) {
            Ok(()) => {
                self.operation.set(operation);
                true
            }
            Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => false,
        }
    }

    /// Stale acknowledgments cannot clear a newer save/erase intention.
    pub(crate) const fn is_current(&self, event: &Event) -> bool {
        let operation = match event {
            Event::Saved(operation) | Event::Erased(operation) | Event::Failed(operation) => {
                *operation
            }
        };
        operation == self.operation.get()
    }

    /// Flushes the last snapshot during window shutdown, preserving ordering
    /// with an erase or save that is already in progress.
    pub(crate) fn finish(&self, bytes: Option<Vec<u8>>) {
        if let Some(bytes) = bytes.filter(|bytes| !bytes.is_empty() && bytes.len() <= MAX_BYTES) {
            let operation = self.operation.get().saturating_add(1);
            if self.commands.send(Command::Save(operation, bytes)).is_ok() {
                self.operation.set(operation);
            }
        }
    }

    pub(crate) fn finish_erase(&self) {
        let operation = self.operation.get().saturating_add(1);
        if self.commands.send(Command::Erase(operation)).is_ok() {
            self.operation.set(operation);
        }
    }
}

#[cfg(test)]
pub(crate) struct ControlledRecovery {
    incoming: Receiver<Command>,
    events: mpsc::Sender<Event>,
}

#[cfg(test)]
impl ControlledRecovery {
    pub(crate) fn next_completion(&self) -> io::Result<Event> {
        match self
            .incoming
            .recv_timeout(std::time::Duration::from_secs(5))
            .map_err(io::Error::other)?
        {
            Command::Save(operation, _) => Ok(Event::Saved(operation)),
            Command::Erase(operation) => Ok(Event::Erased(operation)),
            Command::Shutdown => Err(io::Error::other("No recovery operation.")),
        }
    }

    pub(crate) fn complete(&self, event: Event) -> io::Result<()> {
        self.events.send(event).map_err(io::Error::other)
    }
}

#[cfg(test)]
impl Recovery {
    pub(crate) fn controlled() -> (Self, ControlledRecovery) {
        let (commands, incoming) = mpsc::sync_channel(1);
        let (events, outgoing) = mpsc::channel();
        (
            Self {
                commands,
                events: outgoing,
                worker: None,
                operation: Cell::new(0),
            },
            ControlledRecovery { incoming, events },
        )
    }
}

impl Drop for Recovery {
    fn drop(&mut self) {
        let _sent = self.commands.send(Command::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _joined = worker.join();
        }
    }
}

fn open_lock(path: &Path) -> io::Result<File> {
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        // FILE_SHARE_READ | FILE_SHARE_WRITE: contenders can open and test
        // the byte-range lock, but cannot delete/replace its pathname.
        .share_mode(3)
        // FILE_FLAG_OPEN_REPARSE_POINT: examine the leaf actually opened if it
        // was replaced after the caller's path validation.
        .custom_flags(0x0020_0000)
        .open(path)?;
    let metadata = lock.metadata()?;
    if !metadata.is_file() || metadata.file_attributes() & 0x400 != 0 {
        return Err(io::Error::other(
            "Conversation recovery locks must be ordinary files.",
        ));
    }
    lock.try_lock().map_err(io::Error::other)?;
    Ok(lock)
}

fn read(path: &Path) -> io::Result<Option<Vec<u8>>> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if file.metadata()?.len() > MAX_SEALED {
        return Err(io::Error::other(
            "Saved conversations exceed their storage bound.",
        ));
    }
    let mut sealed = Vec::new();
    file.take(MAX_SEALED + 1).read_to_end(&mut sealed)?;
    if u64::try_from(sealed.len()).map_err(io::Error::other)? > MAX_SEALED {
        return Err(io::Error::other(
            "Saved conversations exceed their storage bound.",
        ));
    }
    bonsai_windows_credentials::unprotect_private_data(&sealed).map(Some)
}

fn save(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if bytes.is_empty() || bytes.len() > MAX_BYTES {
        return Err(io::Error::other(
            "Saved conversations exceed their storage bound.",
        ));
    }
    bonsai_windows_credentials::validate_external_path(path)?;
    let sealed = bonsai_windows_credentials::protect_private_data(bytes)?;
    let temporary = temporary_path(path);
    bonsai_windows_credentials::validate_external_path(&temporary)?;
    // A unique create-new sibling receives ciphertext only. The old recovery
    // file survives failed encryption/writes and is replaced only after sync.
    let mut created = false;
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        created = true;
        file.write_all(&sealed)?;
        file.sync_all()?;
        drop(file);
        bonsai_windows_credentials::validate_external_path(path)?;
        fs::rename(&temporary, path)
    })();
    if result.is_err() && created {
        let _removed = fs::remove_file(&temporary);
    }
    result
}

fn temporary_path(path: &Path) -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    path.with_extension(format!("{}.{stamp}.pending", std::process::id()))
}

fn erase(path: &Path) -> io::Result<()> {
    bonsai_windows_credentials::validate_external_path(path)?;
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_lock_prevents_path_replacement_until_owner_exits() -> io::Result<()> {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("Bonsai-recovery-lock-{stamp}"));
        fs::create_dir(&directory)?;
        let path = directory.join("chat-recovery.lock");
        let renamed = directory.join("renamed.lock");
        let opened = Recovery::open(&directory)?;
        assert!(Recovery::open(&directory).is_err());
        assert!(fs::remove_file(&path).is_err());
        assert!(fs::rename(&path, &renamed).is_err());
        assert!(path.exists());
        drop(opened);
        let reopened = Recovery::open(&directory)?;
        drop(reopened);
        fs::rename(&path, &renamed)?;
        fs::remove_file(renamed)?;
        fs::remove_dir(directory)?;
        Ok(())
    }

    #[test]
    fn recovery_lock_rejects_a_leaf_redirect_at_open() -> io::Result<()> {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("Bonsai-recovery-redirect-{stamp}"));
        fs::create_dir(&directory)?;
        let target = directory.join("ordinary.lock");
        let alias = directory.join("redirect.lock");
        fs::write(&target, b"Retain this unrelated file.")?;
        std::os::windows::fs::symlink_file(&target, &alias)?;
        // Call the open boundary directly to model a redirect installed after
        // Recovery::open validated the pathname, without a timing-dependent race.
        assert!(open_lock(&alias).is_err());
        assert_eq!(fs::read(&target)?, b"Retain this unrelated file.");
        drop(open_lock(&target)?);
        fs::remove_file(alias)?;
        fs::remove_file(target)?;
        fs::remove_dir(directory)?;
        Ok(())
    }

    #[test]
    fn encrypted_recovery_replaces_atomically_locks_and_erases() -> io::Result<()> {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("Bonsai-recovery-{stamp}"));
        fs::create_dir(&directory)?;
        let path = directory.join("chat-recovery.dpapi");
        let opened = Recovery::open(&directory)?;
        assert!(opened.saved.is_none());
        assert!(!opened.load_failed);
        let store = opened.store;
        assert!(Recovery::open(&directory).is_err());
        let text = "A saved draft 🌱".as_bytes().to_vec();
        assert!(store.save(text.clone()));
        assert!(matches!(
            store.events.recv_timeout(std::time::Duration::from_secs(5)),
            Ok(Event::Saved(_))
        ));
        assert_eq!(read(&path)?, Some(text));
        let replacement = b"Revised draft".to_vec();
        store.finish(Some(replacement.clone()));
        assert!(matches!(
            store.events.recv_timeout(std::time::Duration::from_secs(5)),
            Ok(Event::Saved(_))
        ));
        assert_eq!(read(&path)?, Some(replacement));
        assert!(store.erase());
        assert!(matches!(
            store.events.recv_timeout(std::time::Duration::from_secs(5)),
            Ok(Event::Erased(_))
        ));
        assert!(!path.exists());
        drop(store);
        fs::remove_file(directory.join("chat-recovery.lock"))?;
        fs::remove_dir(directory)?;
        Ok(())
    }

    #[test]
    fn encrypted_recovery_finishes_erase_after_queued_saves() -> io::Result<()> {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("Bonsai-recovery-order-{stamp}"));
        fs::create_dir(&directory)?;
        let opened = Recovery::open(&directory)?;
        let store = opened.store;
        store.finish(Some(b"First saved draft".to_vec()));
        store.finish_erase();
        store.finish(Some(b"Newer queued saved draft".to_vec()));
        store.finish_erase();
        drop(store);
        assert!(!directory.join("chat-recovery.dpapi").exists());
        fs::remove_file(directory.join("chat-recovery.lock"))?;
        fs::remove_dir(directory)?;
        Ok(())
    }
}
