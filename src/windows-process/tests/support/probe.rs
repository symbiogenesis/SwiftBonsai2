//! Synthetic subprocess fixture; built only with the explicit test-probe feature.
#![cfg(windows)]

use std::ffi::OsString;
use std::fmt::Write as _;
use std::fs;
use std::io;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::os::windows::ffi::OsStrExt as _;
use std::os::windows::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use bonsai_windows_process::{OwnedProcess, ProcessSpec, SandboxPolicy};
use windows_sys::Win32::Networking::WinSock::WSAEACCES;
use windows_sys::Win32::System::Diagnostics::Debug::{GetErrorMode, RaiseException, SetErrorMode};

fn main() -> io::Result<()> {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let Some(mode) = args.first().and_then(|value| value.to_str()) else {
        return Err(io::Error::other("missing probe mode"));
    };
    match mode {
        "report" => {
            let mut text = format!("cwd={}\n", std::env::current_dir()?.display());
            for arg in args.iter().skip(1) {
                writeln!(text, "arg={:?}", arg.encode_wide().collect::<Vec<_>>())
                    .map_err(io::Error::other)?;
            }
            for name in [
                "SystemDrive",
                "SystemRoot",
                "windir",
                "ProgramData",
                "LOCALAPPDATA",
                "APPDATA",
            ] {
                writeln!(text, "{name}={:?}", std::env::var_os(name)).map_err(io::Error::other)?;
            }
            fs::write("report.txt", text)?;
        }
        "sleep" => wait_forever(),
        "mitigations" => report_mitigations()?,
        "--version" | "--help" => report_inspection_mitigations()?,
        "nested-mitigations" => nested_mitigations()?,
        "sandbox-files" => sandbox_files(&args)?,
        "sandbox-connect-allowed" => sandbox_connect(&args, true)?,
        "sandbox-connect-denied" => sandbox_connect(&args, false)?,
        "sandbox-temp-denied" => sandbox_temp_denied(&args)?,
        "crash" => native_crash(),
        "error-policy" | "error-policy-owner" => error_policy(mode)?,
        "listen" => {
            let port = argument(&args, 1)?
                .parse::<u16>()
                .map_err(io::Error::other)?;
            let _listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))?;
            fs::write("ready.txt", std::process::id().to_string())?;
            wait_forever();
        }
        "tree" | "spawn-denied" => {
            let mut command = Command::new(std::env::current_exe()?);
            command
                .current_dir(std::env::current_dir()?)
                .creation_flags(0x0800_0000);
            if mode == "tree" {
                command.args(["listen", argument(&args, 1)?]);
            } else {
                command.arg("sleep");
            }
            match command.spawn() {
                Ok(mut child) => {
                    fs::write("spawn.txt", "allowed")?;
                    let _ = child.wait()?;
                }
                Err(_) => {
                    fs::write("spawn.txt", "denied")?;
                }
            }
            if mode == "tree" {
                wait_forever();
            }
        }
        "commit" => {
            let mut held = Vec::new();
            for _ in 0..256 {
                let mut allocation = Vec::<u8>::new();
                if allocation.try_reserve_exact(4 * 1024 * 1024).is_err() {
                    fs::write("commit.txt", "denied")?;
                    return Ok(());
                }
                allocation.resize(4 * 1024 * 1024, 0x5a);
                held.push(allocation);
                std::hint::black_box(&held);
            }
            fs::write("commit.txt", "unbounded")?;
        }
        "owner" => plain_owner(&args)?,
        "sandbox-owner" => sandbox_owner(&args)?,
        _ => return Err(io::Error::other("unknown probe mode")),
    }
    Ok(())
}

fn plain_owner(args: &[OsString]) -> io::Result<()> {
    let directory = std::env::current_dir()?;
    let _owned = OwnedProcess::start(&ProcessSpec {
        executable: std::env::current_exe()?,
        arguments: vec![OsString::from("tree"), OsString::from(argument(args, 1)?)],
        working_directory: directory.clone(),
        stdout: directory.join("owned.stdout"),
        stderr: directory.join("owned.stderr"),
        memory_limit_bytes: 256 * 1024 * 1024,
        active_process_limit: 3,
    })?;
    fs::write("owner-ready.txt", "ready")?;
    wait_forever();
    Ok(())
}

fn sandbox_owner(args: &[OsString]) -> io::Result<()> {
    let directory = std::env::current_dir()?;
    let scratch = directory.join("scratch");
    fs::create_dir(&scratch)?;
    let executable = std::env::current_exe()?;
    let process = OwnedProcess::start_sandboxed(
        &ProcessSpec {
            executable: executable.clone(),
            arguments: vec![OsString::from("listen"), OsString::from(argument(args, 1)?)],
            working_directory: scratch.clone(),
            stdout: directory.join("owned.stdout"),
            stderr: directory.join("owned.stderr"),
            memory_limit_bytes: 256 * 1024 * 1024,
            active_process_limit: 1,
        },
        &SandboxPolicy {
            readonly_paths: vec![executable],
            writable_directory: scratch,
        },
    )?;
    fs::write(
        "sandbox-owner-ready.txt",
        format!(
            "{}\n{}\n",
            process.id(),
            process_creation_file_time(process.id())?
        ),
    )?;
    wait_forever();
    Ok(())
}

fn sandbox_files(args: &[OsString]) -> io::Result<()> {
    let model = argument(args, 1)?;
    let key = argument(args, 2)?;
    let unrelated = argument(args, 3)?;
    let model_text =
        fs::read_to_string(model).map_err(|error| sandbox_error("model read", &error))?;
    let key_text =
        fs::read_to_string(key).map_err(|error| sandbox_error("backend key read", &error))?;
    if model_text != "approved model" || key_text != "backend key" {
        return Err(io::Error::other("approved sandbox input changed"));
    }
    sandbox_ancestors(Path::new(model))?;
    if fs::read(unrelated).is_ok() {
        return Err(io::Error::other(
            "sandbox read unrelated frontend credential",
        ));
    }
    if fs::hard_link(unrelated, "stolen.key").is_ok()
        && (fs::read("stolen.key").is_ok()
            || fs::OpenOptions::new()
                .write(true)
                .open("stolen.key")
                .is_ok())
    {
        return Err(io::Error::other(
            "sandbox accessed unrelated file through a hardlink alias",
        ));
    }
    if fs::OpenOptions::new().write(true).open(model).is_ok() {
        return Err(io::Error::other("sandbox opened read-only model writable"));
    }
    fs::write("scratch.txt", "private scratch")
        .map_err(|error| sandbox_error("scratch write", &error))?;
    let directory = std::env::current_dir()?;
    sandbox_temp()?;
    let child_policy = child_process_policy()?;
    fs::write("child-policy.txt", child_policy.to_string())?;
    let mut child = Command::new(std::env::current_exe()?)
        .arg("sleep")
        .current_dir(&directory)
        .creation_flags(0x0800_0000)
        .spawn();
    if let Ok(child) = &mut child {
        let recorded = child_creation_file_time(child).and_then(|creation| {
            fs::write(
                "unexpected-child.txt",
                format!(
                    "pid={}\ncreation={creation}\nexecutable={}\n",
                    child.id(),
                    std::env::current_exe()?.display()
                ),
            )
        });
        let stopped = child.kill().and_then(|()| child.wait().map(|_| ()));
        recorded?;
        stopped?;
        return Err(io::Error::other(
            "sandbox escaped its one-process Job limit",
        ));
    }
    let port = argument(args, 4)?
        .parse::<u16>()
        .map_err(io::Error::other)?;
    let _listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))
        .map_err(|error| sandbox_error("loopback listener bind", &error))?;
    fs::write("sandbox-ready.txt", std::process::id().to_string())?;
    wait_forever();
    Ok(())
}

#[allow(
    unsafe_code,
    reason = "Query the disposable probe's native child-process mitigation without transferring handles."
)]
fn child_process_policy() -> io::Result<u32> {
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, GetProcessMitigationPolicy, ProcessChildProcessPolicy,
    };

    let mut flags = 0_u32;
    // SAFETY: Borrow the current process pseudo handle without closing it.
    let process = unsafe { GetCurrentProcess() };
    // SAFETY: The documented child-policy structure is one DWORD Flags union.
    // The aligned writable buffer and exact byte count agree and remain live.
    let queried = unsafe {
        GetProcessMitigationPolicy(
            process,
            ProcessChildProcessPolicy,
            std::ptr::from_mut(&mut flags).cast(),
            size_of_val(&flags),
        )
    };
    if queried == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(flags)
}

fn child_creation_file_time(child: &std::process::Child) -> io::Result<u64> {
    use std::os::windows::io::AsHandle as _;

    handle_creation_file_time(child.as_handle())
}

#[allow(
    unsafe_code,
    reason = "Retain query access to the exact disposable contained worker before reporting its creation identity."
)]
fn process_creation_file_time(id: u32) -> io::Result<u64> {
    use std::os::windows::io::{AsHandle as _, FromRawHandle as _, OwnedHandle};
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

    // SAFETY: Open only the newly owned worker's numeric PID for querying.
    let raw = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, id) };
    if raw.is_null() {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: A successful OpenProcess returned a fresh, uniquely owned handle.
    let process = unsafe { OwnedHandle::from_raw_handle(raw) };
    handle_creation_file_time(process.as_handle())
}

#[allow(
    unsafe_code,
    reason = "Read creation identity through a live borrowed fixture process handle."
)]
fn handle_creation_file_time(process: std::os::windows::io::BorrowedHandle<'_>) -> io::Result<u64> {
    use std::os::windows::io::AsRawHandle as _;
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::Threading::GetProcessTimes;

    let mut creation = FILETIME::default();
    let mut exit = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    // SAFETY: The caller retains the live process handle; four nonaliasing aligned
    // FILETIME buffers stay writable for the synchronous identity query.
    let queried = unsafe {
        GetProcessTimes(
            process.as_raw_handle(),
            &raw mut creation,
            &raw mut exit,
            &raw mut kernel,
            &raw mut user,
        )
    };
    if queried == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok((u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime))
}

fn sandbox_temp() -> io::Result<()> {
    let directory = std::env::temp_dir();
    if !directory.is_absolute() {
        return Err(io::Error::other("sandbox temporary directory is relative"));
    }
    for name in ["TEMP", "TMP"] {
        let value = std::env::var_os(name)
            .ok_or_else(|| io::Error::other("sandbox temporary variable is missing"))?;
        if fs::canonicalize(PathBuf::from(value))? != fs::canonicalize(&directory)? {
            return Err(io::Error::other("sandbox TEMP/TMP disagree"));
        }
    }
    let file = directory.join("sandbox-private-temp.txt");
    fs::write(&file, "private kernel temp")?;
    fs::write("kernel-temp-path.txt", file.to_string_lossy().as_bytes())
}

fn sandbox_temp_denied(args: &[OsString]) -> io::Result<()> {
    if fs::read(argument(args, 1)?).is_ok() {
        return Err(io::Error::other(
            "sandbox read another worker's private temp",
        ));
    }
    sandbox_temp()
}

fn sandbox_error(stage: &str, error: &io::Error) -> io::Error {
    io::Error::new(error.kind(), format!("{stage}: {error}"))
}

fn sandbox_ancestors(model: &Path) -> io::Result<()> {
    let parent = model
        .parent()
        .ok_or_else(|| io::Error::other("approved sandbox input lacks a parent"))?;
    for ancestor in parent.ancestors() {
        // The owner validates volume roots before granting these exact paths.
        // The worker receives no root grant and checks all non-root ancestors.
        if ancestor.parent().is_none() {
            continue;
        }
        let _metadata = fs::symlink_metadata(ancestor).map_err(|error| {
            sandbox_error(&format!("ancestor metadata {}", ancestor.display()), &error)
        })?;
        match fs::symlink_metadata(ancestor.join(".git")) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(sandbox_error(
                    &format!("Git ancestry lookup {}", ancestor.display()),
                    &error,
                ));
            }
            Ok(_) => return Err(io::Error::other("sandbox fixture has a Git ancestor")),
        }
    }
    Ok(())
}

fn sandbox_connect(args: &[OsString], allowed: bool) -> io::Result<()> {
    let endpoint = argument(args, 1)?
        .parse::<SocketAddr>()
        .map_err(io::Error::other)?;
    match TcpStream::connect_timeout(&endpoint, Duration::from_secs(2)) {
        Ok(_) if allowed => fs::write("network-allowed.txt", "connected"),
        Err(error) if !allowed && error.raw_os_error() == Some(WSAEACCES) => {
            fs::write("network-denied.txt", "WSAEACCES")
        }
        Ok(_) => Err(io::Error::other(
            "sandbox unexpectedly connected to denied endpoint",
        )),
        Err(error) => Err(sandbox_error("unexpected network outcome", &error)),
    }
}

fn nested_mitigations() -> io::Result<()> {
    let directory = std::env::current_dir()?;
    let process = OwnedProcess::start(&ProcessSpec {
        executable: std::env::current_exe()?,
        arguments: vec![OsString::from("mitigations")],
        working_directory: directory.clone(),
        stdout: directory.join("nested.stdout"),
        stderr: directory.join("nested.stderr"),
        memory_limit_bytes: 256 * 1024 * 1024,
        active_process_limit: 1,
    })?;
    if process.wait_for_exit(Duration::from_secs(10))? != Some(0) {
        return Err(io::Error::other("nested mitigation probe failed"));
    }
    Ok(())
}

fn report_mitigations() -> io::Result<()> {
    use windows_sys::Win32::System::Threading::{
        ProcessExtensionPointDisablePolicy, ProcessImageLoadPolicy, ProcessSystemCallDisablePolicy,
    };

    let mut report = String::new();
    for (name, policy) in [
        ("win32k", ProcessSystemCallDisablePolicy),
        ("extensions", ProcessExtensionPointDisablePolicy),
        ("images", ProcessImageLoadPolicy),
    ] {
        let flags = mitigation_flags(policy)?;
        writeln!(report, "{name}={flags}").map_err(io::Error::other)?;
    }
    fs::write("mitigations.txt", report)
}

fn report_inspection_mitigations() -> io::Result<()> {
    report_mitigations()?;
    let attempted = Command::new(std::env::current_exe()?)
        .arg("sleep")
        .current_dir(std::env::current_dir()?)
        .creation_flags(0x0800_0000)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    let result = match attempted {
        Ok(mut child) => {
            child.kill()?;
            let _ = child.wait()?;
            "spawn=allowed\n"
        }
        Err(_) => "spawn=denied\n",
    };
    fs::write("inspection.txt", result)
}

#[allow(
    unsafe_code,
    reason = "Query mitigation flags for this disposable probe's borrowed pseudo handle."
)]
fn mitigation_flags(
    policy: windows_sys::Win32::System::Threading::PROCESS_MITIGATION_POLICY,
) -> io::Result<u32> {
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessMitigationPolicy};
    let mut flags = 0_u32;
    // SAFETY: Borrow the current process pseudo handle without closing it.
    let process = unsafe { GetCurrentProcess() };
    // SAFETY: The queried policy structures consist of a single DWORD Flags
    // union. The aligned writable buffer has that exact size and remains live
    // for the synchronous query; no ownership moves.
    let queried = unsafe {
        GetProcessMitigationPolicy(
            process,
            policy,
            std::ptr::from_mut(&mut flags).cast(),
            size_of_val(&flags),
        )
    };
    if queried == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(flags)
    }
}

fn argument(args: &[OsString], index: usize) -> io::Result<&str> {
    args.get(index)
        .and_then(|value| value.to_str())
        .ok_or_else(|| io::Error::other("missing probe argument"))
}

fn wait_forever() {
    loop {
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[allow(
    unsafe_code,
    reason = "Read this disposable probe process's native error mode."
)]
fn error_mode() -> u32 {
    // SAFETY: GetErrorMode has no arguments or pointer/handle requirements.
    unsafe { GetErrorMode() }
}

#[allow(
    unsafe_code,
    reason = "Reset only this disposable owner process for an inheritance control."
)]
fn reset_error_mode() {
    // SAFETY: The single-threaded probe changes only its own process-wide mode.
    let _ = unsafe { SetErrorMode(0) };
}

#[allow(
    unsafe_code,
    reason = "Raise a synthetic native exception only in this disposable crash fixture."
)]
fn native_crash() {
    // SAFETY: The test-only noncontinuable exception takes no arguments. Its
    // documented effect is process termination; no pointers are dereferenced.
    unsafe { RaiseException(0xe042_4242, 1, 0, std::ptr::null()) };
}

fn error_policy(mode: &str) -> io::Result<()> {
    if mode == "error-policy" {
        let status = Command::new(std::env::current_exe()?)
            .arg("crash")
            .current_dir(std::env::current_dir()?)
            .creation_flags(0x0800_0000)
            .status()?;
        fs::write("child-crash.txt", format!("{:?}", status.code()))?;
        return Ok(());
    }
    reset_error_mode();
    fs::write("owner-error-mode-before.txt", error_mode().to_string())?;
    let directory = std::env::current_dir()?;
    for mode in ["crash", "error-policy"] {
        let process = OwnedProcess::start(&ProcessSpec {
            executable: std::env::current_exe()?,
            arguments: vec![OsString::from(mode)],
            working_directory: directory.clone(),
            stdout: directory.join(format!("{mode}.stdout")),
            stderr: directory.join(format!("{mode}.stderr")),
            memory_limit_bytes: 256 * 1024 * 1024,
            active_process_limit: 2,
        })?;
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(code) = process.try_wait()? {
                fs::write(format!("{mode}-exit.txt"), code.to_string())?;
                break;
            }
            if std::time::Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "crash fixture hung",
                ));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    fs::write("owner-error-mode-after.txt", error_mode().to_string())?;
    Ok(())
}
