//! Cancellable native preparation while the existing chat window is available.
#![allow(
    clippy::redundant_pub_crate,
    reason = "Keep the private preparation worker visible only within the native application."
)]
use bonsai_windows_process::{OwnedProcess, ToolProcessSpec};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs::{self, File},
    io::{self, Read as _},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const MAX_OUTPUT: u64 = 1024 * 1024;

pub(crate) struct Request {
    executable: PathBuf,
    payload: PathBuf,
    working: PathBuf,
    pub(crate) data: PathBuf,
}

impl Request {
    pub(crate) fn parse(arguments: &[OsString]) -> io::Result<Self> {
        if arguments.len() != 6
            || arguments[0] != "--startup-tool"
            || arguments[2] != "--startup-request"
            || arguments[4] != "--startup-data-directory"
        {
            return Err(io::Error::other(
                "Use --startup-tool ABSOLUTE_EXE --startup-request ABSOLUTE_JSON --startup-data-directory ABSOLUTE_DIRECTORY.",
            ));
        }
        let executable = PathBuf::from(&arguments[1]);
        let request = PathBuf::from(&arguments[3]);
        for path in [&executable, &request] {
            bonsai_windows_credentials::validate_external_path(path)?;
            if !path.is_file() {
                return Err(io::Error::other(
                    "Native startup requires existing external tool and request files.",
                ));
            }
        }
        let working = request
            .parent()
            .ok_or_else(|| io::Error::other("Startup request needs an external parent directory."))?
            .to_owned();
        let data = PathBuf::from(&arguments[5]);
        bonsai_windows_credentials::validate_external_path(&data)?;
        Ok(Self {
            executable,
            payload: request,
            working,
            data,
        })
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "PascalCase", deny_unknown_fields)]
struct Prepared {
    arguments: Vec<String>,
}

fn bounded_text(path: &Path) -> io::Result<String> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take(MAX_OUTPUT + 1)
        .read_to_end(&mut bytes)?;
    if u64::try_from(bytes.len()).map_err(io::Error::other)? > MAX_OUTPUT {
        return Err(io::Error::other(
            "Native startup output exceeded its bound.",
        ));
    }
    String::from_utf8(bytes).map_err(io::Error::other)
}

fn configuration(output: &str) -> io::Result<crate::client::Config> {
    let prepared: Prepared = serde_json::from_str(output).map_err(io::Error::other)?;
    if prepared.arguments.len() > 64
        || prepared
            .arguments
            .iter()
            .any(|argument| argument.len() > 32768)
    {
        return Err(io::Error::other(
            "Native startup arguments exceeded their bound.",
        ));
    }
    crate::configuration(prepared.arguments.into_iter().map(OsString::from))
        .map_err(io::Error::other)
}

struct Process(OwnedProcess);

impl Drop for Process {
    fn drop(&mut self) {
        let _stopped = self.0.stop(Duration::from_secs(10));
    }
}

fn execute(request: Request, cancelled: &AtomicBool) -> io::Result<crate::client::Config> {
    let stdout = request.working.join("chat-prepare.stdout");
    let stderr = request.working.join("chat-prepare.stderr");
    let arguments = vec![
        "chat-prepare".into(),
        "--request".into(),
        request.payload.into_os_string(),
    ];
    let output = capture(
        &ToolProcessSpec {
            executable: request.executable,
            arguments,
            working_directory: request.working,
            stdout,
            stderr,
            environment: BTreeMap::new(),
        },
        cancelled,
        // Each download/build retains its existing deadline. In particular,
        // cold multi-gigabyte speech provisioning can legitimately exceed an
        // hour overall; only cancellation bounds the aggregate operation.
        Duration::ZERO,
    )?;
    let config = configuration(&output)?;
    if config.data != request.data {
        return Err(io::Error::other(
            "Native preparation returned a different data directory.",
        ));
    }
    Ok(config)
}

fn capture(
    spec: &ToolProcessSpec,
    cancelled: &AtomicBool,
    deadline: Duration,
) -> io::Result<String> {
    if cancelled.load(Ordering::Acquire) {
        return Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "Native startup was cancelled.",
        ));
    }
    let child = Process(OwnedProcess::start_tool(spec)?);
    let started = Instant::now();
    let status = loop {
        if cancelled.load(Ordering::Acquire) {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "Native startup was cancelled.",
            ));
        }
        if fs::metadata(&spec.stdout)?.len() > MAX_OUTPUT
            || fs::metadata(&spec.stderr)?.len() > MAX_OUTPUT
        {
            return Err(io::Error::other(
                "Native startup output exceeded its bound.",
            ));
        }
        if let Some(status) = child.0.try_wait()? {
            break status;
        }
        if !deadline.is_zero() && started.elapsed() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Native startup timed out.",
            ));
        }
        thread::sleep(Duration::from_millis(20));
    };
    child.0.stop(Duration::from_secs(10))?;
    if status != 0 {
        return Err(io::Error::other(format!(
            "Preparation failed. Close this window and launch again. {} Evidence: {}",
            bounded_text(&spec.stderr)?.trim(),
            spec.working_directory.display()
        )));
    }
    bounded_text(&spec.stdout)
}
pub(crate) struct Session {
    incoming: Receiver<io::Result<crate::client::Config>>,
    cancelled: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl Session {
    pub(crate) fn start(request: Request) -> io::Result<Self> {
        let (outgoing, incoming) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let cancellation = Arc::clone(&cancelled);
        let worker = thread::Builder::new()
            .name("swift-chat-prepare".into())
            .spawn(move || {
                let result = execute(request, &cancellation);
                let _sent = outgoing.send(result);
            })?;
        Ok(Self {
            incoming,
            cancelled,
            worker: Some(worker),
        })
    }

    pub(crate) fn poll(&self) -> Option<io::Result<crate::client::Config>> {
        match self.incoming.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Err(io::Error::other(
                "Native preparation worker stopped. Close this window and launch again.",
            ))),
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _joined = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepared_configuration_rejects_unknown_fields_and_unbounded_arguments() {
        assert!(configuration(r#"{"Arguments":[],"Extra":true}"#).is_err());
        assert!(
            configuration(&serde_json::json!({"Arguments": vec!["x";65]}).to_string()).is_err()
        );
        assert!(
            configuration(&serde_json::json!({"Arguments":["x".repeat(32769)]}).to_string())
                .is_err()
        );
    }

    fn scratch(name: &str) -> io::Result<PathBuf> {
        let directory = std::env::temp_dir().join(format!(
            "Bonsai-Startup-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(io::Error::other)?
                .as_nanos()
        ));
        fs::create_dir(&directory)?;
        Ok(directory)
    }

    fn quoted(path: &Path) -> String {
        path.to_string_lossy().replace('\'', "''")
    }

    fn powershell() -> io::Result<PathBuf> {
        let search =
            std::env::var_os("PATH").ok_or_else(|| io::Error::other("PATH unavailable."))?;
        std::env::split_paths(&search)
            .map(|directory| directory.join("pwsh.exe"))
            .find(|path| path.is_absolute() && path.is_file())
            .ok_or_else(|| {
                io::Error::other(
                    "PowerShell 7 is required for the synthetic startup process fixture.",
                )
            })
    }

    fn spec(directory: &Path, script: &str) -> io::Result<ToolProcessSpec> {
        let path = directory.join("prepare.ps1");
        fs::write(&path, script)?;
        Ok(ToolProcessSpec {
            executable: powershell()?,
            arguments: vec![
                "-NoLogo".into(),
                "-NoProfile".into(),
                "-NonInteractive".into(),
                "-File".into(),
                path.into_os_string(),
            ],
            working_directory: directory.into(),
            stdout: directory.join("prepare.stdout"),
            stderr: directory.join("prepare.stderr"),
            environment: BTreeMap::new(),
        })
    }

    #[test]
    fn cancelled_startup_stops_native_descendants_before_returning() -> io::Result<()> {
        let directory = scratch("Cancel")?;
        let marker = directory.join("descendant.txt");
        let leaf = directory.join("leaf.ps1");
        fs::write(
            &leaf,
            format!(
                "$process = Get-Process -Id $PID\n[IO.File]::WriteAllText('{marker}.partial', \"$PID`n$($process.StartTime.ToUniversalTime().ToFileTimeUtc())\")\n[IO.File]::Move('{marker}.partial', '{marker}')\nStart-Sleep -Seconds 60\n",
                marker = quoted(&marker)
            ),
        )?;
        let script = format!(
            "$ErrorActionPreference = 'Stop'\n$start = [Diagnostics.ProcessStartInfo]::new('{executable}')\n$start.UseShellExecute = $false\n$start.CreateNoWindow = $true\n$start.WorkingDirectory = '{directory}'\nforeach ($argument in @('-NoLogo', '-NoProfile', '-NonInteractive', '-File', '{leaf}')) {{ $start.ArgumentList.Add($argument) }}\n$child = [Diagnostics.Process]::Start($start)\n$child.WaitForExit()\n",
            executable = quoted(&powershell()?),
            directory = quoted(&directory),
            leaf = quoted(&leaf)
        );
        let spec = spec(&directory, &script)?;
        let cancelled = Arc::new(AtomicBool::new(false));
        let cancellation = Arc::clone(&cancelled);
        let worker = thread::spawn(move || capture(&spec, &cancellation, Duration::from_secs(30)));
        let started = Instant::now();
        while !marker.is_file() && started.elapsed() < Duration::from_secs(20) {
            thread::sleep(Duration::from_millis(20));
        }
        if !marker.is_file() {
            cancelled.store(true, Ordering::Release);
            let _joined = worker.join();
            return Err(io::Error::other(
                "Synthetic startup descendant did not start.",
            ));
        }
        let identity = fs::read_to_string(&marker)?;
        let (id, creation) = identity
            .split_once('\n')
            .ok_or_else(|| io::Error::other("Incomplete descendant identity."))?;
        let child = bonsai_windows_process::ParentProcess::open(
            id.trim().parse().map_err(io::Error::other)?,
            creation.trim().parse().map_err(io::Error::other)?,
        )?;
        cancelled.store(true, Ordering::Release);
        let result = worker
            .join()
            .map_err(|_| io::Error::other("Synthetic startup worker panicked."))?;
        assert!(result.is_err_and(|error| error.kind() == io::ErrorKind::Interrupted));
        assert!(
            child.ensure_alive().is_err(),
            "Startup descendant survived cancellation."
        );
        drop(child);
        fs::remove_dir_all(&directory)?;
        Ok(())
    }

    #[test]
    fn native_startup_bounds_output_and_reports_preparation_failure() -> io::Result<()> {
        for (name, script, expected) in [
            (
                "Failure",
                "[Console]::Error.WriteLine('Synthetic failure'); exit 7",
                "Synthetic failure",
            ),
            (
                "Bound",
                "[Console]::Write('x' * 1048577); Start-Sleep -Seconds 10",
                "exceeded its bound",
            ),
        ] {
            let directory = scratch(name)?;
            let result = capture(
                &spec(&directory, script)?,
                &AtomicBool::new(false),
                Duration::from_secs(30),
            );
            assert!(result.is_err_and(|error| error.to_string().contains(expected)));
            fs::remove_dir_all(&directory)?;
        }
        Ok(())
    }
}
