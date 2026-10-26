//! Optional encrypted recovery. Disk and DPAPI work run on a dedicated worker.
#![allow(
    clippy::redundant_pub_crate,
    reason = "Keep the private persistence worker visible only within the native application."
)]
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, SyncSender, TrySendError},
    thread::JoinHandle,
};

const MAX_BYTES: usize = 8 * 1024 * 1024;
const MAX_SEALED: u64 = 8 * 1024 * 1024 + 65_536;

enum Command {
    Save(Vec<u8>),
    Erase,
    Shutdown,
}

pub(crate) enum Event {
    Saved,
    Erased,
    Failed,
}

pub(crate) struct Recovery {
    commands: SyncSender<Command>,
    pub(crate) events: Receiver<Event>,
    worker: Option<JoinHandle<()>>,
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
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock_path)?;
        lock.try_lock().map_err(io::Error::other)?;
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
                        Command::Save(bytes) => {
                            if save(&path, &bytes).is_ok() {
                                Event::Saved
                            } else {
                                Event::Failed
                            }
                        }
                        Command::Erase => {
                            if erase(&path).is_ok() {
                                Event::Erased
                            } else {
                                Event::Failed
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
        self.commands.try_send(Command::Save(bytes)).is_ok()
    }

    pub(crate) fn erase(&self) -> bool {
        match self.commands.try_send(Command::Erase) {
            Ok(()) => true,
            Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => false,
        }
    }

    /// Flushes the last snapshot during window shutdown, preserving ordering
    /// with an erase or save that is already in progress.
    pub(crate) fn finish(&self, bytes: Option<Vec<u8>>) {
        if let Some(bytes) = bytes.filter(|bytes| !bytes.is_empty() && bytes.len() <= MAX_BYTES) {
            let _sent = self.commands.send(Command::Save(bytes));
        }
    }

    pub(crate) fn finish_erase(&self) {
        let _sent = self.commands.send(Command::Erase);
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
            Ok(Event::Saved)
        ));
        assert_eq!(read(&path)?, Some(text));
        let replacement = b"Revised draft".to_vec();
        store.finish(Some(replacement.clone()));
        assert!(matches!(
            store.events.recv_timeout(std::time::Duration::from_secs(5)),
            Ok(Event::Saved)
        ));
        assert_eq!(read(&path)?, Some(replacement));
        assert!(store.erase());
        assert!(matches!(
            store.events.recv_timeout(std::time::Duration::from_secs(5)),
            Ok(Event::Erased)
        ));
        assert!(!path.exists());
        drop(store);
        fs::remove_file(directory.join("chat-recovery.lock"))?;
        fs::remove_dir(directory)?;
        Ok(())
    }
}
