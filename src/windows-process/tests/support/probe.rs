//! Synthetic subprocess fixture; built only with the explicit test-probe feature.
#![cfg(windows)]

use std::ffi::OsString;
use std::fmt::Write as _;
use std::fs;
use std::io;
use std::net::TcpListener;
use std::os::windows::ffi::OsStrExt as _;
use std::os::windows::process::CommandExt as _;
use std::process::Command;
use std::time::Duration;

use bonsai_windows_process::{OwnedProcess, ProcessSpec};
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
        "nested-mitigations" => nested_mitigations()?,
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
        "owner" => {
            let directory = std::env::current_dir()?;
            let _owned = OwnedProcess::start(&ProcessSpec {
                executable: std::env::current_exe()?,
                arguments: vec![OsString::from("tree"), OsString::from(argument(&args, 1)?)],
                working_directory: directory.clone(),
                stdout: directory.join("owned.stdout"),
                stderr: directory.join("owned.stderr"),
                memory_limit_bytes: 256 * 1024 * 1024,
                active_process_limit: 3,
            })?;
            fs::write("owner-ready.txt", "ready")?;
            wait_forever();
        }
        _ => return Err(io::Error::other("unknown probe mode")),
    }
    Ok(())
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

#[allow(
    unsafe_code,
    reason = "Query mitigation flags for this disposable probe's borrowed pseudo handle."
)]
fn report_mitigations() -> io::Result<()> {
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, GetProcessMitigationPolicy, ProcessExtensionPointDisablePolicy,
        ProcessImageLoadPolicy, ProcessSystemCallDisablePolicy,
    };

    let mut report = String::new();
    for (name, policy) in [
        ("win32k", ProcessSystemCallDisablePolicy),
        ("extensions", ProcessExtensionPointDisablePolicy),
        ("images", ProcessImageLoadPolicy),
    ] {
        let mut flags = 0_u32;
        // SAFETY: Borrow the current process pseudo handle without closing it.
        let process = unsafe { GetCurrentProcess() };
        // SAFETY: These three documented policy structures each consist of a
        // single DWORD Flags union. The aligned writable buffer has that exact
        // size and remains live for the synchronous query; no ownership moves.
        let queried = unsafe {
            GetProcessMitigationPolicy(
                process,
                policy,
                std::ptr::from_mut(&mut flags).cast(),
                size_of_val(&flags),
            )
        };
        if queried == 0 {
            return Err(io::Error::last_os_error());
        }
        writeln!(report, "{name}={flags}").map_err(io::Error::other)?;
    }
    fs::write("mitigations.txt", report)
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
