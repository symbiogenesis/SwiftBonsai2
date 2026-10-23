//! Bounded development runner and explicit, unqualified key-file configuration.
//!
//! No backend is launched or authenticated by process identity. Key-file ACLs,
//! ancestors' ownership and the inference security boundary are not qualified.
//! The executable therefore requires two explicit development acknowledgements.

use crate::{
    scheduler::{GateError, MAX_WAITING},
    service::{AcceptedChat, LocalChatService, ServiceError},
};
use std::{
    ffi::OsString,
    fs::{self, File, OpenOptions},
    future::poll_fn,
    io::Read,
    path::{Component, Path, PathBuf},
    pin::{Pin, pin},
    task::Poll,
};

/// Static help text contains neither keys nor local file paths.
pub const USAGE: &str = concat!(
    "External-backend development mode; daily isolation remains unqualified.\n",
    "Usage: bonsai-broker --development --allow-unverified-key-files\n",
    "  --frontend-port <0-or-1024..65535> --backend-port <1024..65535>\n",
    "  --frontend-key-file <absolute-path> --backend-key-file <absolute-path>\n",
    "  [--max-waiting <0..8>]\n",
    "Keys must be distinct, owner-protected files containing exactly 64 lowercase hex bytes.\n",
    "File ACLs and ancestor ownership are NOT verified; opt-in acknowledges this limitation.\n",
    "The runner binds only 127.0.0.1. Ctrl-C closes admission and drains all requests.\n",
);

/// Value-free CLI or key-file error; never contains arguments or credentials.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfigurationError {
    /// Missing, repeated, unknown, or invalid command-line option.
    Arguments,
    /// Both explicit development acknowledgements are required.
    DevelopmentOptIn,
    /// A key path is relative, redirected, nonregular, or unreadable.
    KeyFile,
    /// A key file is not exactly 64 lowercase hexadecimal ASCII bytes.
    KeyFormat,
    /// The loopback listener could not be configured or bound.
    Service(ServiceError),
}

/// Parsed explicit-development configuration; credentials are never arguments.
pub struct DevelopmentConfig {
    frontend_port: u16,
    backend_port: u16,
    frontend_key: PathBuf,
    backend_key: PathBuf,
    max_waiting: usize,
}

impl DevelopmentConfig {
    /// Parse arguments after the executable name without reading environment values.
    ///
    /// All options are mandatory except `--max-waiting`, which defaults to two.
    /// `--help` is handled separately by the executable. Keys themselves are
    /// accepted only from files, never command-line arguments or environment.
    ///
    /// # Errors
    /// Returns value-free errors for unsupported options or missing opt-ins.
    pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Self, ConfigurationError> {
        let mut args = args.into_iter();
        let mut development = false;
        let mut unverified = false;
        let mut frontend_port = None;
        let mut backend_port = None;
        let mut frontend_key = None;
        let mut backend_key = None;
        let mut max_waiting = None;
        while let Some(argument) = args.next() {
            match argument.to_str() {
                Some("--development") if !development => development = true,
                Some("--allow-unverified-key-files") if !unverified => unverified = true,
                Some("--frontend-port") if frontend_port.is_none() => {
                    frontend_port = Some(number(args.next())?);
                }
                Some("--backend-port") if backend_port.is_none() => {
                    backend_port = Some(number(args.next())?);
                }
                Some("--frontend-key-file") if frontend_key.is_none() => {
                    frontend_key = Some(key_path(args.next())?);
                }
                Some("--backend-key-file") if backend_key.is_none() => {
                    backend_key = Some(key_path(args.next())?);
                }
                Some("--max-waiting") if max_waiting.is_none() => {
                    let value = number(args.next())?;
                    max_waiting = Some(usize::from(value));
                }
                _ => return Err(ConfigurationError::Arguments),
            }
        }
        if !development || !unverified {
            return Err(ConfigurationError::DevelopmentOptIn);
        }
        let frontend_port = frontend_port.ok_or(ConfigurationError::Arguments)?;
        let backend_port = backend_port.ok_or(ConfigurationError::Arguments)?;
        let max_waiting = max_waiting.unwrap_or(2);
        if (frontend_port != 0 && frontend_port < 1024)
            || backend_port < 1024
            || frontend_port == backend_port
            || max_waiting > MAX_WAITING
        {
            return Err(ConfigurationError::Arguments);
        }
        Ok(Self {
            frontend_port,
            backend_port,
            frontend_key: frontend_key.ok_or(ConfigurationError::Arguments)?,
            backend_key: backend_key.ok_or(ConfigurationError::Arguments)?,
            max_waiting,
        })
    }

    /// Read bounded key files, retain read-only handles, and bind numeric loopback.
    ///
    /// Files must be regular and have no symlink/reparse components. On Windows
    /// only read sharing is allowed while this owner exists, refusing concurrent
    /// write/delete opens. Ancestor replacement races and ACLs remain unverified;
    /// this is explicitly a development facility, not secure key provisioning.
    ///
    /// # Errors
    /// Returns value-free file, format, or service errors without logging values.
    pub async fn bind(self) -> Result<DevelopmentService, ConfigurationError> {
        let (frontend_file, frontend_key) = read_key(&self.frontend_key)?;
        let (backend_file, backend_key) = read_key(&self.backend_key)?;
        let service = LocalChatService::bind(
            self.frontend_port,
            &frontend_key,
            self.backend_port,
            &backend_key,
            self.max_waiting,
        )
        .await
        .map_err(ConfigurationError::Service)?;
        Ok(DevelopmentService {
            service,
            _frontend_file: frontend_file,
            _backend_file: backend_file,
        })
    }
}

/// Bound development service retaining its read-only credential file handles.
pub struct DevelopmentService {
    service: LocalChatService,
    _frontend_file: File,
    _backend_file: File,
}

impl DevelopmentService {
    /// Borrow the service while its key-file read handles remain open.
    pub const fn service(&self) -> &LocalChatService {
        &self.service
    }
}

fn number(value: Option<OsString>) -> Result<u16, ConfigurationError> {
    crate::configuration::decimal_u16(value).ok_or(ConfigurationError::Arguments)
}

fn key_path(value: Option<OsString>) -> Result<PathBuf, ConfigurationError> {
    let value = PathBuf::from(value.ok_or(ConfigurationError::Arguments)?);
    if !value.is_absolute()
        || value
            .components()
            .any(|part| matches!(part, Component::ParentDir))
    {
        return Err(ConfigurationError::KeyFile);
    }
    // UNC/device/verbatim paths are deliberately outside this development CLI.
    #[cfg(windows)]
    if !matches!(
        value.components().next(),
        Some(Component::Prefix(prefix)) if matches!(prefix.kind(), std::path::Prefix::Disk(_))
    ) {
        return Err(ConfigurationError::KeyFile);
    }
    Ok(value)
}

fn read_key(path: &Path) -> Result<(File, String), ConfigurationError> {
    for ancestor in path.ancestors() {
        let metadata = fs::symlink_metadata(ancestor).map_err(|_| ConfigurationError::KeyFile)?;
        if metadata.file_type().is_symlink() {
            return Err(ConfigurationError::KeyFile);
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt as _;
            if metadata.file_attributes() & 0x400 != 0 {
                return Err(ConfigurationError::KeyFile);
            }
        }
        if (ancestor == path && !metadata.is_file()) || (ancestor != path && !metadata.is_dir()) {
            return Err(ConfigurationError::KeyFile);
        }
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt as _;
        options.share_mode(1); // FILE_SHARE_READ only; deny write/delete sharing.
        options.custom_flags(0x0020_0000); // FILE_FLAG_OPEN_REPARSE_POINT.
    }
    let mut file = options
        .open(path)
        .map_err(|_| ConfigurationError::KeyFile)?;
    let metadata = file.metadata().map_err(|_| ConfigurationError::KeyFile)?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt as _;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err(ConfigurationError::KeyFile);
        }
    }
    if !metadata.is_file() || metadata.len() != 64 {
        return Err(ConfigurationError::KeyFormat);
    }
    let mut bytes = [0; 64];
    file.read_exact(&mut bytes)
        .map_err(|_| ConfigurationError::KeyFile)?;
    let mut surplus = [0];
    if file
        .read(&mut surplus)
        .map_err(|_| ConfigurationError::KeyFile)?
        != 0
        || !bytes
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
    {
        return Err(ConfigurationError::KeyFormat);
    }
    let key = String::from_utf8(bytes.to_vec()).map_err(|_| ConfigurationError::KeyFormat)?;
    Ok((file, key))
}

/// Why the bounded runner stopped accepting requests.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunExit {
    /// The supplied shutdown future completed successfully (for example Ctrl-C).
    Shutdown,
    /// Shutdown notification registration failed; requests were still drained.
    ShutdownFailure,
    /// Admission closed, including the unacknowledged-generation failure latch.
    AdmissionClosed,
    /// Accepting failed or another caller consumed the runner's capacity.
    AcceptFailure,
}

/// Value-free accounting after every accepted operation has been polled to exit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunReport {
    /// Shutdown, notification failure, closed admission, or accepting failure.
    pub exit: RunExit,
    /// Successfully delivered responses; saturates at `usize::MAX`.
    pub completed: usize,
    /// Failed or shutdown-cancelled accepted operations; saturates at `usize::MAX`.
    pub failed: usize,
    /// Largest number of simultaneously owned accepted request futures.
    pub peak_in_flight: usize,
}

type RequestFuture<'a> = Pin<Box<dyn Future<Output = Result<(), ServiceError>> + 'a>>;

enum Event<'a> {
    Stop(RunExit),
    Accepted(AcceptedChat<'a>),
    Completed(usize, Result<(), ServiceError>),
}

/// Serve with no detached tasks and at most one plus `max_waiting` request futures.
///
/// This function must exclusively own acceptance for the service. A full budget
/// pauses acceptance until a request completes; no busy retry or body allocation
/// occurs. Shutdown is polled first, closes admission and polls every owned
/// request to completion so sockets and reservations are released before return.
/// Dropping this runner also closes admission and drops all accepted operations.
/// The listener itself stays bound until its owner drops the service.
///
/// Pass `tokio::signal::ctrl_c()` for console cancellation, or an application
/// notification future with the same `io::Result<()>` output. A notification
/// failure causes the same cancellation/drain and a distinct failure report.
#[allow(
    clippy::future_not_send,
    reason = "the bounded request futures and shutdown source may be local; no task is spawned"
)]
pub async fn run(
    service: &LocalChatService,
    shutdown: impl Future<Output = std::io::Result<()>>,
) -> RunReport {
    let mut shutdown = pin!(shutdown);
    let mut requests: Vec<RequestFuture<'_>> = Vec::with_capacity(service.capacity());
    let _close_on_drop = CloseOnDrop(service);
    let mut report = RunReport {
        exit: RunExit::Shutdown,
        completed: 0,
        failed: 0,
        peak_in_flight: 0,
    };
    loop {
        let accepting = service.accept();
        let mut accepting = pin!(accepting);
        let event = poll_fn(|cx| {
            if let Poll::Ready(result) = shutdown.as_mut().poll(cx) {
                return Poll::Ready(Event::Stop(if result.is_ok() {
                    RunExit::Shutdown
                } else {
                    RunExit::ShutdownFailure
                }));
            }
            for (index, request) in requests.iter_mut().enumerate() {
                if let Poll::Ready(result) = request.as_mut().poll(cx) {
                    return Poll::Ready(Event::Completed(index, result));
                }
            }
            if requests.len() < service.capacity() {
                return accepting.as_mut().poll(cx).map(|result| match result {
                    Ok(request) => Event::Accepted(request),
                    Err(ServiceError::Gate(GateError::Closed)) => {
                        Event::Stop(RunExit::AdmissionClosed)
                    }
                    Err(_) => Event::Stop(RunExit::AcceptFailure),
                });
            }
            Poll::Pending
        })
        .await;
        match event {
            Event::Accepted(request) => {
                requests.push(Box::pin(request.serve()));
                report.peak_in_flight = report.peak_in_flight.max(requests.len());
            }
            Event::Completed(index, result) => {
                let _finished = requests.remove(index);
                record(&mut report, result);
            }
            Event::Stop(reason) => {
                report.exit = reason;
                break;
            }
        }
    }
    service.close();
    for request in requests {
        record(&mut report, request.await);
    }
    report
}

const fn record(report: &mut RunReport, result: Result<(), ServiceError>) {
    if result.is_ok() {
        report.completed = report.completed.saturating_add(1);
    } else {
        report.failed = report.failed.saturating_add(1);
    }
}

struct CloseOnDrop<'a>(&'a LocalChatService);

impl Drop for CloseOnDrop<'_> {
    fn drop(&mut self) {
        self.0.close();
    }
}
