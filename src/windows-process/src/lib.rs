//! Windows 10+ process ownership using an unnamed, noninheritable Job object.
//!
//! A suspended root receives its Job atomically during creation; dropping its
//! owner kills the complete tree. This enforces lifecycle and committed-memory
//! limits. It does not restrict tokens, files, network access, or GPU allocation.
#![cfg(windows)]

use std::ffi::{OsStr, OsString};
use std::io;
use std::os::windows::ffi::OsStrExt as _;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

// All Win32 calls and their pointer/handle contracts are confined here.
#[allow(unsafe_code)]
mod native;

/// Explicit launch inputs; no shell or executable search path is used.
#[derive(Debug)]
pub struct ProcessSpec {
    /// Expanded absolute path of the executable.
    pub executable: PathBuf,
    /// Arguments encoded with the Windows C runtime argv convention.
    pub arguments: Vec<OsString>,
    /// Expanded absolute native working directory.
    pub working_directory: PathBuf,
    /// Expanded absolute path of a new output evidence file.
    pub stdout: PathBuf,
    /// Expanded absolute path of a new error evidence file.
    pub stderr: PathBuf,
    /// Maximum job-wide committed memory, including descendants.
    pub memory_limit_bytes: usize,
    /// Maximum simultaneously active processes, including the root.
    pub active_process_limit: u32,
}

/// Current root-process counters, distinct from the job-wide commit limit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProcessMemory {
    /// Root-process private committed bytes.
    pub private_bytes: usize,
    /// Root-process resident working-set bytes.
    pub working_set_bytes: usize,
}

/// Owns a complete native process tree; dropping it kills remaining processes.
pub struct OwnedProcess(native::NativeProcess);

impl OwnedProcess {
    /// Creates evidence files, atomically assigns a suspended root, then resumes.
    ///
    /// The full OS/profile environment is inherited without modification.
    /// Model/backend override variables are rejected before creating any files.
    ///
    /// # Errors
    /// Rejects ambiguous paths, invalid limits/arguments, inherited backend
    /// overrides, existing evidence files, or any failed Windows operation.
    pub fn start(spec: &ProcessSpec) -> io::Result<Self> {
        for path in [
            &spec.executable,
            &spec.working_directory,
            &spec.stdout,
            &spec.stderr,
        ] {
            validate_path(path)?;
        }
        if !spec.executable.is_file() || !spec.working_directory.is_dir() {
            return Err(invalid("executable or working directory does not exist"));
        }
        if spec.memory_limit_bytes == 0 || spec.active_process_limit == 0 {
            return Err(invalid(
                "job memory and active-process limits must be positive",
            ));
        }
        for (name, _) in std::env::vars_os() {
            if forbidden_environment_name(&name) {
                return Err(invalid("inherited environment contains a backend override"));
            }
        }
        let command = command_line(&spec.executable, &spec.arguments)?;
        native::NativeProcess::start(spec, command).map(Self)
    }

    /// Returns the root Windows process identifier.
    #[must_use]
    pub const fn id(&self) -> u32 {
        self.0.id()
    }

    /// Polls root completion without waiting; exit code 259 is handled correctly.
    ///
    /// # Errors
    /// Returns failed Windows wait/query operations.
    pub fn try_wait(&self) -> io::Result<Option<u32>> {
        self.0.try_wait()
    }

    /// Terminates the entire tree and waits within a positive timeout.
    ///
    /// # Errors
    /// Returns invalid zero timeouts, Windows failures, or `TimedOut` if active
    /// processes remain after the deadline. Dropping still closes the Job.
    pub fn stop(&self, timeout: Duration) -> io::Result<()> {
        self.0.stop(timeout)
    }

    /// Returns the observed peak job-wide committed bytes.
    ///
    /// # Errors
    /// Returns failed Windows job-accounting operations.
    pub fn peak_job_memory_bytes(&self) -> io::Result<usize> {
        self.0.peak_job_memory_bytes()
    }

    /// Returns current root private commitment and resident working set.
    ///
    /// # Errors
    /// Returns failed Windows process-memory queries.
    pub fn memory_usage(&self) -> io::Result<ProcessMemory> {
        self.0.memory_usage()
    }
}

/// Returns currently available physical memory; this is an observation only.
///
/// # Errors
/// Returns a failed Windows memory-status query.
pub fn available_memory_bytes() -> io::Result<u64> {
    native::available_memory_bytes()
}

/// Finds the process listening specifically on IPv4 `127.0.0.1:port`.
///
/// Wildcard and nonloopback listeners are not accepted. An ambiguous duplicate
/// listener is an error. This observation does not reserve or authenticate a port.
///
/// # Errors
/// Returns invalid port zero, malformed/oversized tables, ambiguity, or Windows
/// TCP-table query failures.
pub fn listener_process_id(port: u16) -> io::Result<Option<u32>> {
    if port == 0 {
        return Err(invalid("listener port must be nonzero"));
    }
    native::listener_process_id(port)
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

fn forbidden_environment_name(name: &OsStr) -> bool {
    let name = name.to_string_lossy().to_ascii_uppercase();
    ["LLAMA_", "LLAMACPP_", "GGML_", "AIP_", "MTMD_"]
        .iter()
        .any(|prefix| name.starts_with(prefix))
        || ["HF_TOKEN", "HF_ENDPOINT", "MODEL_ENDPOINT"].contains(&name.as_str())
}

fn validate_path(path: &Path) -> io::Result<()> {
    // Inspect raw segments as Path::components normalizes embedded `.` segments.
    let text: Vec<u16> = path.as_os_str().encode_wide().collect();
    let ambiguous = text.split(|unit| *unit == 92 || *unit == 47).any(|part| {
        part == [46]
            || part == [46, 46]
            || part.last().is_some_and(|last| *last == 46 || *last == 32)
    });
    if !path.is_absolute() || text.contains(&0) || text.contains(&37) || ambiguous {
        return Err(invalid(
            "native paths must be expanded absolute paths without ambiguous segments",
        ));
    }
    // Device/verbatim paths can bypass ordinary Win32 file-name semantics.
    if !matches!(path.components().next(), Some(Component::Prefix(prefix)) if matches!(prefix.kind(), std::path::Prefix::Disk(_) | std::path::Prefix::UNC(_, _)))
    {
        return Err(invalid("native paths must use a drive or UNC prefix"));
    }
    Ok(())
}

fn command_line(executable: &Path, arguments: &[OsString]) -> io::Result<Vec<u16>> {
    let mut output = Vec::new();
    for value in
        std::iter::once(executable.as_os_str()).chain(arguments.iter().map(OsString::as_os_str))
    {
        if !output.is_empty() {
            output.push(32);
        }
        quote(value, &mut output)?;
        if output.len() >= 32_767 {
            return Err(invalid("Windows command line exceeds 32767 UTF-16 units"));
        }
    }
    output.push(0);
    Ok(output)
}

fn quote(value: &OsStr, output: &mut Vec<u16>) -> io::Result<()> {
    output.push(34);
    let mut slashes = 0;
    for unit in value.encode_wide() {
        match unit {
            0 => return Err(invalid("arguments cannot contain NUL")),
            92 => {
                slashes += 1;
                continue;
            }
            34 => output.extend(std::iter::repeat_n(92, slashes * 2 + 1)),
            _ => output.extend(std::iter::repeat_n(92, slashes)),
        }
        output.push(unit);
        slashes = 0;
    }
    output.extend(std::iter::repeat_n(92, slashes * 2));
    output.push(34);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_argument_quoting() -> io::Result<()> {
        for (input, expected) in [
            ("", "\"\""),
            ("a b", "\"a b\""),
            ("a\\", "\"a\\\\\""),
            ("a\"b", "\"a\\\"b\""),
        ] {
            let mut output = Vec::new();
            quote(OsStr::new(input), &mut output)?;
            assert_eq!(String::from_utf16_lossy(&output), expected);
        }
        assert!(
            command_line(
                Path::new("C:\\probe.exe"),
                &[OsString::from("x".repeat(32_767))]
            )
            .is_err()
        );
        assert!(command_line(Path::new("C:\\probe.exe"), &[OsString::from("a\0b")]).is_err());
        Ok(())
    }

    #[test]
    fn path_and_environment_policy() {
        for path in [
            "relative",
            "C:relative",
            "C:\\a\\..\\b",
            "C:\\a\\.\\b",
            "C:\\a.\\b",
            "C:\\%TEMP%\\b",
            "\\\\?\\C:\\a",
        ] {
            assert!(validate_path(Path::new(path)).is_err(), "{path}");
        }
        assert!(validate_path(Path::new("C:\\native cwd\\probe.exe")).is_ok());
        for name in ["llama_arg", "GGML_CUDA", "HF_TOKEN", "model_endpoint"] {
            assert!(forbidden_environment_name(OsStr::new(name)));
        }
        for name in [
            "SystemDrive",
            "SystemRoot",
            "windir",
            "ProgramData",
            "LOCALAPPDATA",
            "APPDATA",
            "PATH",
            "HF_HOME",
        ] {
            assert!(!forbidden_environment_name(OsStr::new(name)));
        }
    }
}
