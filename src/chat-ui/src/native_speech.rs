//! Shared ownership and memory isolation for explicitly requested native speech.
#![forbid(unsafe_code)]
#![allow(
    clippy::redundant_pub_crate,
    reason = "Only the native speech adapters and worker use this shared owner."
)]

use crate::canary::{Call, Failure};
use bonsai_windows_process::{
    OwnedProcess, ProcessSpec, available_memory_bytes, listener_process_id,
};
use std::{
    ffi::OsString,
    fs, io,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU8, AtomicU64, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, Sender},
    },
    thread::{self, JoinHandle},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const GIB: u64 = 1024 * 1024 * 1024;
const START_AVAILABLE: u64 = 9 * GIB;
const RUNNING_RESERVE: u64 = 5 * GIB;
const SERVICE_COMMIT_LIMIT: usize = 16 * 1024 * 1024 * 1024;
const RESOURCE_POLL: Duration = Duration::from_millis(25);
const RESOURCE_STOP: Duration = Duration::from_secs(5);
const LOW_MEMORY: u8 = 1;
const MEMORY_UNAVAILABLE: u8 = 2;

/// Fixed native executable and local GGUF artifacts; no arbitrary arguments.
#[derive(Clone)]
pub(crate) struct Launch {
    pub(crate) executable: PathBuf,
    pub(crate) model: PathBuf,
    pub(crate) codec: Option<PathBuf>,
    pub(crate) evidence: PathBuf,
}

impl Launch {
    pub(crate) fn validate(&self) -> Result<(), &'static str> {
        for path in [&self.executable, &self.model, &self.evidence]
            .into_iter()
            .chain(self.codec.iter())
        {
            bonsai_windows_credentials::validate_external_path(path)
                .map_err(|_| "Use existing expanded absolute native speech paths outside Git.")?;
        }
        if !self.executable.is_file()
            || !self
                .executable
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
            || !self.evidence.is_dir()
            || !self.model.is_file()
            || !self
                .model
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("gguf"))
            || self.codec.as_ref().is_some_and(|codec| {
                !codec.is_file()
                    || !codec
                        .extension()
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("gguf"))
            })
        {
            return Err(
                "Native speech requires its executable, existing GGUF artifacts and evidence directory.",
            );
        }
        Ok(())
    }

    fn specification(&self, key_file: &std::path::Path, port: u16) -> Result<ProcessSpec, Failure> {
        self.validate().map_err(Failure::Message)?;
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| Failure::Message("Could not create the native speech evidence lane."))?
            .as_nanos();
        let directory = self
            .evidence
            .join(format!("native-speech-{}-{stamp}", std::process::id()));
        fs::create_dir(&directory)
            .map_err(|_| Failure::Message("Could not create the native speech evidence lane."))?;
        let mut arguments = vec![
            OsString::from("--model"),
            self.model.clone().into_os_string(),
        ];
        if let Some(codec) = &self.codec {
            arguments.extend([OsString::from("--codec"), codec.clone().into_os_string()]);
        }
        arguments.extend([
            OsString::from("--token-file"),
            key_file.as_os_str().to_owned(),
            OsString::from("--port"),
            OsString::from(port.to_string()),
        ]);
        Ok(ProcessSpec {
            executable: self.executable.clone(),
            arguments,
            working_directory: directory.clone(),
            stdout: directory.join("service.stdout"),
            stderr: directory.join("service.stderr"),
            memory_limit_bytes: SERVICE_COMMIT_LIMIT,
            active_process_limit: 1,
        })
    }
}

/// Retaining this owner bounds one speech process independently of text chat.
pub(crate) struct Service {
    process: Arc<OwnedProcess>,
    resource_status: Arc<AtomicU8>,
    stop_monitor: Sender<()>,
    monitor: Option<JoinHandle<()>>,
}

impl Service {
    /// Cancellation precedes port checks, evidence creation and model loading.
    pub(crate) fn start(
        launch: &Launch,
        key_file: &std::path::Path,
        port: u16,
        generation: u64,
        current: &AtomicU64,
        mut ready: impl FnMut() -> Result<(), Failure>,
    ) -> Result<Self, Failure> {
        let call = Call::new(generation, current, Duration::from_mins(15));
        call.check()?;
        if listener_process_id(port)
            .map_err(|_| Failure::Message("Could not check the local speech port."))?
            .is_some()
        {
            return Err(Failure::Message(
                "The native speech port is already occupied. Close the conflicting service or choose a distinct speech port.",
            ));
        }
        admit_memory(available_memory_bytes().map_err(|_| Failure::Message("Could not check available memory before loading speech. Text chat remains available."))?)?;
        call.check()?;
        let process = OwnedProcess::start(&launch.specification(key_file, port)?)
            .map_err(|_| Failure::Message("Could not start the native speech executable. Check the retained service evidence; text chat remains available."))?;
        let service = Self::monitored(Arc::new(process), available_memory_bytes)?;
        loop {
            call.check()?;
            service.check()?;
            match ready() {
                Ok(()) => {
                    if listener_process_id(port).ok().flatten() != Some(service.process.id()) {
                        return Err(Failure::Message(
                            "The authenticated speech listener does not belong to the owned native service.",
                        ));
                    }
                    call.check()?;
                    service.check()?;
                    return Ok(service);
                }
                Err(Failure::Cancelled) => return Err(Failure::Cancelled),
                Err(Failure::Message(_)) => thread::sleep(Duration::from_millis(100)),
            }
        }
    }

    pub(crate) fn monitored(
        process: Arc<OwnedProcess>,
        available: impl FnMut() -> io::Result<u64> + Send + 'static,
    ) -> Result<Self, Failure> {
        let resource_status = Arc::new(AtomicU8::new(0));
        let (stop_monitor, stopped) = mpsc::channel();
        let monitored_process = Arc::clone(&process);
        let status = Arc::clone(&resource_status);
        let monitor = thread::Builder::new()
            .name("bonsai-speech-memory".to_owned())
            .spawn(move || monitor_resources(&monitored_process, &status, &stopped, available))
            .map_err(|_| {
                let _stopped = process.stop(RESOURCE_STOP);
                Failure::Message(
                    "Could not start native speech memory monitoring. Text chat remains available.",
                )
            })?;
        Ok(Self {
            process,
            resource_status,
            stop_monitor,
            monitor: Some(monitor),
        })
    }

    pub(crate) fn check(&self) -> Result<(), Failure> {
        match self.resource_status.load(Ordering::Acquire) {
            LOW_MEMORY => {
                return Err(Failure::Message(
                    "Speech stopped because it was using memory needed to keep text chat running. Close other apps and try again; completed answers are retained.",
                ));
            }
            MEMORY_UNAVAILABLE => {
                return Err(Failure::Message(
                    "Speech stopped because available memory could not be checked. Text chat remains available.",
                ));
            }
            _ => {}
        }
        if self
            .process
            .try_wait()
            .map_err(|_| Failure::Message("Could not inspect the native speech process."))?
            .is_some()
        {
            if self
                .process
                .peak_job_memory_bytes()
                .is_ok_and(|peak| peak >= SERVICE_COMMIT_LIMIT)
            {
                return Err(Failure::Message(
                    "Speech stopped after reaching its memory limit. Text chat and completed answers remain available.",
                ));
            }
            return Err(Failure::Message(
                "The native speech model stopped. Check retained service evidence and enable speech again to retry; text chat remains available.",
            ));
        }
        Ok(())
    }
}

fn monitor_resources(
    process: &OwnedProcess,
    status: &AtomicU8,
    stopped: &Receiver<()>,
    mut available: impl FnMut() -> io::Result<u64>,
) {
    loop {
        let failure = match available() {
            Ok(bytes) if bytes >= RUNNING_RESERVE => 0,
            Ok(_) => LOW_MEMORY,
            Err(_) => MEMORY_UNAVAILABLE,
        };
        if failure != 0 {
            status.store(failure, Ordering::Release);
            let _stopped = process.stop(RESOURCE_STOP);
            return;
        }
        match stopped.recv_timeout(RESOURCE_POLL) {
            Ok(()) | Err(RecvTimeoutError::Disconnected) => return,
            Err(RecvTimeoutError::Timeout) => {}
        }
    }
}

const fn admit_memory(available: u64) -> Result<(), Failure> {
    if available < START_AVAILABLE {
        return Err(Failure::Message(
            "Speech needs 9 GiB of available memory before loading alongside Swift. Close other apps and try again; text chat and completed answers are retained.",
        ));
    }
    Ok(())
}

impl Drop for Service {
    fn drop(&mut self) {
        let _stopped = self.process.stop(RESOURCE_STOP);
        let _sent = self.stop_monitor.send(());
        if let Some(monitor) = self.monitor.take() {
            let _joined = monitor.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Failure, GIB, RUNNING_RESERVE, START_AVAILABLE, Service, admit_memory};
    use bonsai_windows_process::{OwnedProcess, ProcessSpec};
    use std::{
        ffi::OsString,
        fs, io, thread,
        time::{Duration, SystemTime, UNIX_EPOCH},
    };
    #[test]
    fn speech_memory_admission_preserves_room_for_the_running_chat_model() {
        assert!(admit_memory(START_AVAILABLE).is_ok());
        assert!(admit_memory(START_AVAILABLE - 1).is_err());
        assert!(admit_memory(RUNNING_RESERVE).is_err());
        assert!(admit_memory(4 * GIB).is_err());
    }

    #[test]
    #[ignore = "Spawned only by the resource monitor regression as a disposable child."]
    fn resource_guard_fake_service() {
        thread::sleep(Duration::from_secs(30));
    }

    #[test]
    fn speech_memory_monitor_stops_only_its_owned_service() -> io::Result<()> {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("Bonsai-TTS-Memory-{stamp}"));
        fs::create_dir(&directory)?;
        let start = |name: &str| {
            OwnedProcess::start(&ProcessSpec {
                executable: std::env::current_exe()?,
                arguments: vec![
                    OsString::from("--exact"),
                    OsString::from("native_speech::tests::resource_guard_fake_service"),
                    OsString::from("--ignored"),
                ],
                working_directory: directory.clone(),
                stdout: directory.join(format!("{name}.stdout")),
                stderr: directory.join(format!("{name}.stderr")),
                memory_limit_bytes: 512 * 1024 * 1024,
                active_process_limit: 1,
            })
        };
        let unrelated = start("independent-chat-sentinel")?;
        let speech = Service::monitored(std::sync::Arc::new(start("speech")?), || {
            Ok(RUNNING_RESERVE - 1)
        })
        .map_err(|_| io::Error::other("Could not start the test memory monitor"))?;
        assert!(
            speech
                .process
                .wait_for_exit(Duration::from_secs(5))?
                .is_some(),
            "Speech memory pressure did not stop the owned child"
        );
        assert!(
            matches!(speech.check(), Err(Failure::Message(message)) if message.contains("memory") && message.contains("text chat"))
        );
        assert!(
            unrelated.try_wait()?.is_none(),
            "Speech pressure stopped an independent Job"
        );
        drop(speech);
        unrelated.stop(Duration::from_secs(5))?;
        Ok(())
    }
}
