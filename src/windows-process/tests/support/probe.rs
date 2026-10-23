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
