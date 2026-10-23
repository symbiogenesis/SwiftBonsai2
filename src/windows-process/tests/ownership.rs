//! Synthetic Windows process/Job behavior tests, requiring test-probe.
#![cfg(all(windows, feature = "test-probe"))]

use std::ffi::OsString;
use std::fs;
use std::io;
use std::net::TcpListener;
use std::os::windows::ffi::{OsStrExt as _, OsStringExt as _};
use std::os::windows::process::CommandExt as _;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use bonsai_windows_process::{
    OwnedProcess, ProcessSpec, available_memory_bytes, listener_process_id,
};

const TIMEOUT: Duration = Duration::from_secs(10);
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct Lane {
    path: PathBuf,
}

impl Lane {
    fn new(name: &str) -> io::Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "bonsai-process-{name}-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path)?;
        Ok(Self { path })
    }

    fn spec(&self, arguments: Vec<OsString>) -> ProcessSpec {
        ProcessSpec {
            executable: PathBuf::from(env!("CARGO_BIN_EXE_bonsai-process-test-probe")),
            arguments,
            working_directory: self.path.clone(),
            stdout: self.path.join("stdout.log"),
            stderr: self.path.join("stderr.log"),
            memory_limit_bytes: 256 * 1024 * 1024,
            active_process_limit: 3,
        }
    }
}

fn until(mut predicate: impl FnMut() -> io::Result<bool>) -> io::Result<()> {
    let start = Instant::now();
    while !predicate()? {
        if start.elapsed() >= TIMEOUT {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "probe condition not reached",
            ));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}

fn unused_port() -> io::Result<u16> {
    Ok(TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?
        .local_addr()?
        .port())
}

#[test]
fn argv_environment_and_native_cwd() -> io::Result<()> {
    let lane = Lane::new("arguments")?;
    let arguments = vec![
        OsString::from("report"),
        OsString::new(),
        OsString::from("with space"),
        OsString::from("a\\\"quoted\"\\"),
        OsString::from_wide(&[0xd800, 0x5c, 0x22]),
    ];
    let process = OwnedProcess::start(&lane.spec(arguments.clone()))?;
    until(|| process.try_wait().map(|code| code.is_some()))?;
    assert_eq!(process.try_wait()?, Some(0));
    let text = fs::read_to_string(lane.path.join("report.txt"))?;
    assert!(text.contains(&format!("cwd={}\n", lane.path.display())));
    for argument in arguments.iter().skip(1) {
        assert!(text.contains(&format!(
            "arg={:?}\n",
            argument.encode_wide().collect::<Vec<_>>()
        )));
    }
    for name in [
        "SystemDrive",
        "SystemRoot",
        "windir",
        "ProgramData",
        "LOCALAPPDATA",
        "APPDATA",
    ] {
        assert!(text.contains(&format!("{name}={:?}\n", std::env::var_os(name))));
    }
    assert!(available_memory_bytes()? > 0);
    // Evidence is create-new, so a repeated launch cannot overwrite it.
    assert!(OwnedProcess::start(&lane.spec(vec![OsString::from("report")])).is_err());
    Ok(())
}

#[test]
fn stop_and_drop_kill_descendants_and_observe_memory() -> io::Result<()> {
    for explicitly_stop in [true, false] {
        let lane = Lane::new("tree")?;
        let port = unused_port()?;
        let process = OwnedProcess::start(&lane.spec(vec![
            OsString::from("tree"),
            OsString::from(port.to_string()),
        ]))?;
        until(|| listener_process_id(port).map(|owner| owner.is_some()))?;
        let descendant =
            listener_process_id(port)?.ok_or_else(|| io::Error::other("listener disappeared"))?;
        assert_ne!(descendant, process.id());
        assert!(process.memory_usage()?.private_bytes > 0);
        assert!(process.memory_usage()?.working_set_bytes > 0);
        assert!(process.peak_job_memory_bytes()? > 0);
        if explicitly_stop {
            assert!(process.stop(Duration::ZERO).is_err());
            process.stop(TIMEOUT)?;
            assert!(process.try_wait()?.is_some());
        }
        drop(process);
        until(|| listener_process_id(port).map(|owner| owner.is_none()))?;
    }
    Ok(())
}

#[test]
fn active_process_limit_blocks_descendants() -> io::Result<()> {
    let lane = Lane::new("process-limit")?;
    let mut spec = lane.spec(vec![OsString::from("spawn-denied")]);
    spec.active_process_limit = 1;
    let process = OwnedProcess::start(&spec)?;
    until(|| process.try_wait().map(|code| code.is_some()))?;
    assert_eq!(process.try_wait()?, Some(0));
    assert_eq!(fs::read_to_string(lane.path.join("spawn.txt"))?, "denied");
    Ok(())
}

#[test]
fn committed_memory_limit_denies_allocations() -> io::Result<()> {
    let lane = Lane::new("memory-limit")?;
    let mut spec = lane.spec(vec![OsString::from("commit")]);
    spec.memory_limit_bytes = 96 * 1024 * 1024;
    let process = OwnedProcess::start(&spec)?;
    until(|| process.try_wait().map(|code| code.is_some()))?;
    assert_eq!(process.try_wait()?, Some(0));
    assert_eq!(fs::read_to_string(lane.path.join("commit.txt"))?, "denied");
    let peak = process.peak_job_memory_bytes()?;
    assert!(peak > 32 * 1024 * 1024);
    assert!(peak <= spec.memory_limit_bytes);
    Ok(())
}

#[test]
fn abrupt_owner_death_kills_entire_tree() -> io::Result<()> {
    let lane = Lane::new("owner-death")?;
    let port = unused_port()?;
    let mut owner = Command::new(env!("CARGO_BIN_EXE_bonsai-process-test-probe"))
        .args(["owner", &port.to_string()])
        .current_dir(&lane.path)
        .creation_flags(0x0800_0000)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let result = until(|| listener_process_id(port).map(|pid| pid.is_some()));
    owner.kill()?;
    let _ = owner.wait()?;
    result?;
    until(|| listener_process_id(port).map(|pid| pid.is_none()))?;
    Ok(())
}

#[test]
fn backend_overrides_are_rejected_without_changing_parent_environment() -> io::Result<()> {
    let lane = Lane::new("environment")?;
    let status = Command::new(env!("CARGO_BIN_EXE_bonsai-process-test-probe"))
        .args(["owner", &unused_port()?.to_string()])
        .env("LLAMA_TEST_OVERRIDE", "1")
        .current_dir(&lane.path)
        .creation_flags(0x0800_0000)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    assert!(!status.success());
    assert!(!lane.path.join("owned.stdout").exists());
    Ok(())
}
