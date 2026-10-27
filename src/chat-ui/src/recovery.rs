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
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender, TrySendError},
    },
    thread::JoinHandle,
};

const MAX_BYTES: usize = 8 * 1024 * 1024;
const MAX_SEALED: u64 = 8 * 1024 * 1024 + 65_536;

enum Command {
    Save(u64, Vec<u8>, SnapshotSlot),
    Erase(u64),
    Fence(SyncSender<()>),
    Shutdown,
}

// Own the plaintext budget from before serialization until the worker has
// finished encryption and replacement. Dropping a rejected command also frees it.
struct SnapshotSlot(Arc<AtomicBool>);

impl SnapshotSlot {
    fn reserve(occupied: &Arc<AtomicBool>) -> Option<Self> {
        occupied
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .ok()
            .map(|_| Self(Arc::clone(occupied)))
    }
}

impl Drop for SnapshotSlot {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
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
    snapshot_occupied: Arc<AtomicBool>,
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
                        Command::Save(operation, bytes, slot) => {
                            let saved = save(&path, &bytes).is_ok();
                            drop(bytes);
                            drop(slot);
                            if saved {
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
                        Command::Fence(complete) => {
                            let _sent = complete.send(());
                            continue;
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
                snapshot_occupied: Arc::new(AtomicBool::new(false)),
            },
            saved,
            load_failed,
        })
    }

    /// Reserve the only plaintext snapshot before encoding. An occupied worker
    /// skips encoding entirely; retain dirty state and retry the latest state.
    pub(crate) fn save_with(
        &self,
        encode: impl FnOnce() -> io::Result<Vec<u8>>,
    ) -> io::Result<bool> {
        let Some(slot) = SnapshotSlot::reserve(&self.snapshot_occupied) else {
            return Ok(false);
        };
        let bytes = encode()?;
        if bytes.is_empty() || bytes.len() > MAX_BYTES {
            return Ok(false);
        }
        let operation = self.operation.get().saturating_add(1);
        if self
            .commands
            .try_send(Command::Save(operation, bytes, slot))
            .is_ok()
        {
            self.operation.set(operation);
            Ok(true)
        } else {
            Ok(false)
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
    pub(crate) fn finish_with(&self, encode: impl FnOnce() -> Option<Vec<u8>>) {
        // Shutdown already waits for the worker. Wait before encoding its final
        // state as well, so a slow save never overlaps a second plaintext copy.
        let (complete, finished) = mpsc::sync_channel(0);
        if self.commands.send(Command::Fence(complete)).is_ok() && finished.recv().is_ok() {
            let _saved = self.save_with(|| Ok(encode().unwrap_or_default()));
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
            Command::Save(operation, _, _slot) => Ok(Event::Saved(operation)),
            Command::Erase(operation) => Ok(Event::Erased(operation)),
            Command::Fence(_) => Err(io::Error::other("Unexpected recovery fence.")),
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
                snapshot_occupied: Arc::new(AtomicBool::new(false)),
            },
            ControlledRecovery { incoming, events },
        )
    }
}

impl Drop for Recovery {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            let _sent = self.commands.send(Command::Shutdown);
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
    fn recovery_reserves_snapshot_before_encoding_through_worker_completion() -> io::Result<()> {
        let (store, controlled) = Recovery::controlled();
        let encoded = Cell::new(0);
        let encode = || {
            encoded.set(encoded.get() + 1);
            Ok(b"Latest bounded workspace".to_vec())
        };
        assert!(store.save_with(encode)?);
        assert!(!store.save_with(encode)?);
        assert_eq!(encoded.get(), 1);

        // Model a worker that took the command but is still encrypting/writing.
        let active = controlled
            .incoming
            .recv_timeout(std::time::Duration::from_secs(5))
            .map_err(io::Error::other)?;
        assert!(!store.save_with(encode)?);
        assert_eq!(encoded.get(), 1);
        drop(active);

        assert!(store.save_with(encode)?);
        assert_eq!(encoded.get(), 2);
        let _saved = controlled.next_completion()?;
        assert!(
            store
                .save_with(|| Err(io::Error::other("Synthetic encoding failure.")))
                .is_err()
        );
        assert!(!store.save_with(|| Ok(Vec::new()))?);
        assert!(store.save_with(encode)?);
        assert_eq!(encoded.get(), 3);
        let _saved = controlled.next_completion()?;
        drop(controlled);
        Ok(())
    }

    #[test]
    fn recovery_final_snapshot_waits_for_prior_plaintext_before_encoding() -> io::Result<()> {
        let (store, controlled) = Recovery::controlled();
        assert!(store.save_with(|| Ok(b"Prior workspace".to_vec()))?);
        let active = controlled
            .incoming
            .recv_timeout(std::time::Duration::from_secs(5))
            .map_err(io::Error::other)?;
        let encoded = Arc::new(AtomicBool::new(false));
        let observed = Arc::clone(&encoded);
        let occupied = Arc::clone(&store.snapshot_occupied);
        let previous_slot = Arc::clone(&occupied);
        let finalizing = std::thread::spawn(move || {
            store.finish_with(|| {
                // The final encoder has reserved the now-free slot itself.
                assert!(occupied.load(Ordering::Acquire));
                observed.store(true, Ordering::Release);
                Some(b"Final workspace".to_vec())
            });
            store
        });
        let Command::Fence(complete) = controlled
            .incoming
            .recv_timeout(std::time::Duration::from_secs(5))
            .map_err(io::Error::other)?
        else {
            return Err(io::Error::other("Expected a final-save fence."));
        };
        assert!(!encoded.load(Ordering::Acquire));
        // The worker finishes its previous command before acknowledging the
        // fence. Only then may shutdown construct its final plaintext snapshot.
        assert!(previous_slot.load(Ordering::Acquire));
        drop(active);
        assert!(!previous_slot.load(Ordering::Acquire));
        complete.send(()).map_err(io::Error::other)?;
        let _store = finalizing
            .join()
            .map_err(|_| io::Error::other("Final-save worker panicked."))?;
        assert!(encoded.load(Ordering::Acquire));
        let Command::Save(_, bytes, _slot) = controlled
            .incoming
            .recv_timeout(std::time::Duration::from_secs(5))
            .map_err(io::Error::other)?
        else {
            return Err(io::Error::other("Expected the final workspace."));
        };
        assert_eq!(bytes, b"Final workspace");
        Ok(())
    }

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
        assert!(store.save_with(|| Ok(text.clone()))?);
        assert!(matches!(
            store.events.recv_timeout(std::time::Duration::from_secs(5)),
            Ok(Event::Saved(_))
        ));
        assert_eq!(read(&path)?, Some(text));
        let replacement = b"Revised draft".to_vec();
        store.finish_with(|| Some(replacement.clone()));
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
        store.finish_with(|| Some(b"First saved draft".to_vec()));
        store.finish_erase();
        store.finish_with(|| Some(b"Newer queued saved draft".to_vec()));
        store.finish_erase();
        drop(store);
        assert!(!directory.join("chat-recovery.dpapi").exists());
        fs::remove_file(directory.join("chat-recovery.lock"))?;
        fs::remove_dir(directory)?;
        Ok(())
    }
}
