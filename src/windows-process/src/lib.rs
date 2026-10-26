//! Windows 10+ process ownership using an unnamed, noninheritable Job object.
//!
//! A suspended root receives its Job atomically during creation; dropping its
//! owner kills the complete tree. This enforces lifecycle and committed-memory
//! limits. Explicit sandboxed launches additionally require native Windows PSEC
//! filesystem, network and desktop containment; no weaker fallback is used.
#![cfg(windows)]

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::io;
use std::os::windows::ffi::OsStrExt as _;
use std::os::windows::io::AsSocket;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

// All Win32 calls and their pointer/handle contracts are confined here.
#[allow(
    unsafe_code,
    reason = "Keep audited Win32 calls in this native boundary."
)]
#[expect(
    clippy::redundant_pub_crate,
    reason = "Explicit parent-only visibility keeps the audited boundary private."
)]
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

/// Least-authority native worker filesystem grants.
///
/// Networking denies external ingress and egress, with a per-container host
/// loopback exception for the authenticated local HTTP service. Loopback access
/// is not limited to the broker. Required Windows runtime directories are added
/// by the native boundary; caller profile and data directories are not.
#[derive(Debug)]
pub struct SandboxPolicy {
    /// Exact existing model, credential, executable and runtime library files.
    pub readonly_paths: Vec<PathBuf>,
    /// Existing private worker scratch, also used as its working directory.
    /// PSEC may substitute its own private per-container TEMP/TMP directory.
    pub writable_directory: PathBuf,
}

/// Requires native PSEC 1.1, host-loopback ingress and security-environment creation.
///
/// This never changes host ACLs, firewall settings or installed Windows features.
/// It does not launch a process or establish actual-model compatibility.
/// Recognized ambient credential variable names reject with a value-free error;
/// other inherited entries remain intact, so unknown secrets remain a residual risk.
///
/// # Errors
/// Returns `Unsupported` when the native contract is unavailable, or a native
/// failure when the host cannot create the required restricted environment, or
/// `InvalidInput` for recognized ambient credential variables.
pub fn native_sandbox_available() -> io::Result<()> {
    validate_sandbox_environment()?;
    native::sandbox_available()
}

/// Startup compiler/tool inputs with a child-only inherited-environment overlay.
///
/// The unnamed Job owns the complete tree without applying model-specific
/// memory or process-count limits. Descendants cannot escape this ownership.
#[derive(Debug)]
pub struct ToolProcessSpec {
    /// Expanded absolute executable path; no executable search is performed.
    pub executable: PathBuf,
    /// Windows CRT arguments, or the explicit `cmd.exe /d /c` startup grammar.
    pub arguments: Vec<OsString>,
    /// Expanded absolute native working directory.
    pub working_directory: PathBuf,
    /// Expanded absolute path of a new standard-output evidence file.
    pub stdout: PathBuf,
    /// Expanded absolute path of a new standard-error evidence file.
    pub stderr: PathBuf,
    /// Child-only overrides; Windows system/profile variables cannot be replaced.
    pub environment: BTreeMap<String, String>,
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

/// Retained identity of the caller whose lifetime bounds a supervised operation.
pub struct ParentProcess(native::ParentProcess);

impl ParentProcess {
    /// Opens a noninheritable synchronization handle and checks creation identity.
    ///
    /// `creation_file_time` is the caller's Windows UTC creation FILETIME, in
    /// 100-nanosecond ticks since 1601. Checking it prevents accepting a reused PID.
    ///
    /// # Errors
    /// Rejects inaccessible, exited or mismatched processes, including PID zero.
    pub fn open(id: u32, creation_file_time: u64) -> io::Result<Self> {
        native::ParentProcess::open(id, creation_file_time).map(Self)
    }

    /// Confirms that the retained caller process has not exited.
    ///
    /// # Errors
    /// Returns a failed native wait or `BrokenPipe` after caller exit.
    pub fn ensure_alive(&self) -> io::Result<()> {
        self.0.ensure_alive()
    }
}

impl OwnedProcess {
    /// Starts a natively contained worker with atomic Job and mitigation attributes.
    ///
    /// The exact worker retains the existing PID, memory, process-count and
    /// lifetime contracts. Writable application storage is private scratch;
    /// Windows also supplies its own per-container private temporary directory.
    /// OS/profile environment entries are inherited; TEMP/TMP request scratch
    /// but PSEC can replace them with that OS-managed temporary directory.
    /// All policy and capability failures reject without an uncontained retry.
    ///
    /// # Errors
    /// Returns invalid launch/grant paths, reparse paths, overlapping read/write
    /// grants, unavailable native PSEC support or failed native creation.
    pub fn start_sandboxed(spec: &ProcessSpec, policy: &SandboxPolicy) -> io::Result<Self> {
        validate_process(spec)?;
        validate_sandbox_environment()?;
        let command = command_line(&spec.executable, &spec.arguments)?;
        native::NativeProcess::start_sandboxed(spec, command, policy).map(Self)
    }

    /// Atomically owns a startup compiler/tool tree and then resumes its root.
    ///
    /// The complete Unicode environment is inherited and merged case-insensitively
    /// with the child-only overlay. OS/profile entries remain unchanged. Only the
    /// three evidence streams are inherited as handles, and the window is hidden.
    /// Closing the owner kills descendants even if the root already exited.
    ///
    /// # Errors
    /// Rejects invalid paths, malformed arguments or environment entries, protected
    /// system/profile overrides, existing evidence files, or native launch failures.
    pub fn start_tool(spec: &ToolProcessSpec) -> io::Result<Self> {
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
        let command = tool_command_line(&spec.executable, &spec.arguments)?;
        let environment = native::tool_environment(&spec.environment)?;
        native::NativeProcess::start_tool(spec, command, environment.as_deref()).map(Self)
    }

    /// Creates evidence files, atomically assigns a suspended root, then resumes.
    ///
    /// The full OS/profile environment is inherited without modification.
    /// Model/backend override variables are rejected before creating any files.
    /// Headless CPU workers deny Win32k calls, legacy extension points and
    /// remote/low-integrity executable images, with heap termination enabled.
    /// These creation-time mitigations preserve nested Jobs but do not restrict
    /// ordinary filesystem access, sockets or the caller's security identity.
    ///
    /// # Errors
    /// Rejects ambiguous paths, invalid limits/arguments, inherited backend
    /// overrides, existing evidence files, or any failed Windows operation,
    /// including unsupported required process-creation mitigations.
    pub fn start(spec: &ProcessSpec) -> io::Result<Self> {
        validate_process(spec)?;
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

    /// Waits up to a bounded interval for root completion, waking immediately on exit.
    ///
    /// # Errors
    /// Rejects intervals at or above the native infinite-wait sentinel, or a failed wait.
    pub fn wait_for_exit(&self, timeout: Duration) -> io::Result<Option<u32>> {
        self.0.wait_for_exit(timeout)
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
    /// Windows may include denied allocation requests in this diagnostic counter;
    /// it is not a measurement of successfully resident or accepted allocations.
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

fn validate_process(spec: &ProcessSpec) -> io::Result<()> {
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
    Ok(())
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

/// Requires exclusive Windows ownership of a socket's bound address.
///
/// Call this before binding a server socket or reserving a backend port. Without
/// exclusivity, some conflicting `SO_REUSEADDR` binds can share an
/// address and make delivery indeterminate. The socket stays borrowed;
/// this function does not bind it or take ownership of its handle.
///
/// # Errors
/// Returns the Winsock error if `SO_EXCLUSIVEADDRUSE` cannot be enabled.
pub fn require_exclusive_address(socket: &impl AsSocket) -> io::Result<()> {
    native::require_exclusive_address(socket)
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

fn validate_sandbox_environment() -> io::Result<()> {
    if std::env::vars_os().any(|(name, _)| credential_environment_name(&name)) {
        return Err(invalid(
            "sandbox launch rejects inherited credential environment variables",
        ));
    }
    Ok(())
}

fn credential_environment_name(name: &OsStr) -> bool {
    let name = name.to_string_lossy().to_ascii_uppercase();
    [
        "_TOKEN",
        "_SECRET",
        "_PASSWORD",
        "_API_KEY",
        "_PRIVATE_KEY",
        "_ACCESS_KEY",
        "_CONNECTION_STRING",
        "_CONNECTIONSTRING",
        "_DATABASE_URL",
        "_REDIS_URL",
    ]
    .iter()
    .any(|suffix| name.ends_with(suffix))
        || [
            "TOKEN",
            "SECRET",
            "PASSWORD",
            "API_KEY",
            "PRIVATE_KEY",
            "ACCESS_KEY",
            "AWS_ACCESS_KEY_ID",
            "AZURE_OPENAI_KEY",
            "AZURE_STORAGE_KEY",
            "AZURE_STORAGE_CONNECTION_STRING",
            "GOOGLE_APPLICATION_CREDENTIALS",
            "CONNECTION_STRING",
            "CONNECTIONSTRING",
            "DATABASE_URL",
            "REDIS_URL",
        ]
        .contains(&name.as_str())
        || [
            "CONNECTIONSTRINGS__",
            "SQLCONNSTR_",
            "SQLAZURECONNSTR_",
            "MYSQLCONNSTR_",
            "CUSTOMCONNSTR_",
            "POSTGRESQLCONNSTR_",
        ]
        .iter()
        .any(|prefix| name.starts_with(prefix))
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
    for component in path.components() {
        if let Component::Normal(name) = component {
            // CreateFileW also opens DOS devices and NTFS alternate streams.
            // Evidence paths must name ordinary files, even when fully qualified.
            if name
                .encode_wide()
                .any(|unit| unit < 32 || matches!(unit, 34 | 42 | 58 | 60 | 62 | 63 | 124))
                || reserved_device(name)
            {
                return Err(invalid(
                    "native paths must name ordinary files and directories",
                ));
            }
        }
    }
    Ok(())
}

fn reserved_device(name: &OsStr) -> bool {
    let text = name.to_string_lossy();
    let base = text
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end_matches(' ');
    let base = base.to_ascii_uppercase();
    matches!(
        base.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || (base.starts_with("COM") || base.starts_with("LPT"))
        && matches!(
            base.get(3..),
            Some(
                "1" | "2"
                    | "3"
                    | "4"
                    | "5"
                    | "6"
                    | "7"
                    | "8"
                    | "9"
                    | "\u{b9}"
                    | "\u{b2}"
                    | "\u{b3}"
            )
        )
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

fn tool_command_line(executable: &Path, arguments: &[OsString]) -> io::Result<Vec<u16>> {
    if !executable
        .file_name()
        .is_some_and(|name| name.eq_ignore_ascii_case("cmd.exe"))
    {
        return command_line(executable, arguments);
    }
    let [disable_auto_run, execute, script] = arguments else {
        return Err(invalid("unsupported command interpreter arguments"));
    };
    if disable_auto_run != "/d" || execute != "/c" {
        return Err(invalid("command interpreter startup must disable AutoRun"));
    }
    let script = script
        .to_str()
        .ok_or_else(|| invalid("command interpreter input must be Unicode"))?;
    if script.contains(['\0', '\r', '\n']) {
        return Err(invalid("unsupported command interpreter input"));
    }
    let command = if Path::new(script).is_absolute() {
        validate_path(Path::new(script))?;
        if script.contains('"') {
            return Err(invalid("unsupported environment batch path"));
        }
        format!(" /d /v:off /s /c \"\"{}\"\"", script.replace('/', "\\"))
    } else {
        format!(" /d /v:off /c {script}")
    };
    let mut output = Vec::new();
    quote(executable.as_os_str(), &mut output)?;
    output.extend(command.encode_utf16());
    if output.len() >= 32_767 {
        return Err(invalid("Windows command line exceeds 32767 UTF-16 units"));
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

    #[test]
    fn native_paths_reject_devices_streams_and_reserved_characters() {
        for name in [
            "NUL",
            "nul.txt",
            "NUL .txt",
            "Con",
            "PRN.log",
            "AUX",
            "CONIN$",
            "CONOUT$",
            "COM1",
            "com9.txt",
            "LPT1",
            "lpt9.log",
            "COM\u{b9}",
            "com\u{b2}.txt",
            "COM\u{b3}",
            "LPT\u{b9}",
            "lpt\u{b2}.txt",
            "LPT\u{b3}",
            "file.log:stream",
            "file.log::$DATA",
            "wild?card",
            "wild*card",
            "quoted\"name",
            "pipe|name",
            "less<name",
            "greater>name",
            "control\u{1f}name",
        ] {
            assert!(
                validate_path(&Path::new("C:\\evidence").join(name)).is_err(),
                "{name}"
            );
            assert!(
                validate_path(&Path::new("C:\\evidence").join(name).join("output.log")).is_err(),
                "{name}"
            );
        }
        for name in [
            "console.log",
            "nulled.txt",
            "COM10",
            "LPT10",
            ".config",
            "ordinary name",
        ] {
            assert!(
                validate_path(&Path::new("C:\\evidence").join(name)).is_ok(),
                "{name}"
            );
        }
    }
}
#[cfg(test)]
#[test]
fn sandbox_environment_classifies_credential_names_without_changing_environment() {
    for name in [
        "OPENAI_API_KEY",
        "openai_api_key",
        "ANTHROPIC_API_KEY",
        "AZURE_OPENAI_KEY",
        "AZURE_STORAGE_CONNECTION_STRING",
        "AWS_ACCESS_KEY_ID",
        "AWS_SECRET_ACCESS_KEY",
        "AWS_SESSION_TOKEN",
        "GH_TOKEN",
        "GITHUB_TOKEN",
        "HF_TOKEN",
        "service_TOKEN",
        "service_SECRET",
        "service_PASSWORD",
        "service_API_KEY",
        "service_PRIVATE_KEY",
        "service_ACCESS_KEY",
        "TOKEN",
        "PASSWORD",
        "GOOGLE_APPLICATION_CREDENTIALS",
        "DATABASE_URL",
        "REDIS_URL",
        "APP_DATABASE_URL",
        "APP_REDIS_URL",
        "APP_CONNECTION_STRING",
        "APP_CONNECTIONSTRING",
        "ConnectionStrings__DefaultConnection",
        "SQLCONNSTR_Default",
        "SQLAZURECONNSTR_Default",
        "MYSQLCONNSTR_Default",
        "CUSTOMCONNSTR_Default",
        "POSTGRESQLCONNSTR_Default",
    ] {
        assert!(credential_environment_name(OsStr::new(name)), "{name}");
    }
    for name in [
        "SystemDrive",
        "SystemRoot",
        "windir",
        "ProgramData",
        "LOCALAPPDATA",
        "APPDATA",
        "USERPROFILE",
        "HOMEDRIVE",
        "HOMEPATH",
        "PATH",
        "TEMP",
        "TMP",
        "ComSpec",
        "ProgramFiles",
        "HF_HOME",
        "TOKENIZERS_PARALLELISM",
        "PASSWORD_POLICY",
    ] {
        assert!(!credential_environment_name(OsStr::new(name)), "{name}");
    }
}
