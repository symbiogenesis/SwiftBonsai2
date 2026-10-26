//! Bounded compiler/tool execution with inherited Windows profile settings.
#![expect(
    clippy::redundant_pub_crate,
    reason = "Private startup modules expose only a crate-local implementation API."
)]

use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    fs::{self, File, OpenOptions},
    io::Read as _,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use bonsai_artifacts::validate_external_path;

const MAX_OUTPUT: u64 = 16 * 1024 * 1024;
static SERIAL: AtomicU64 = AtomicU64::new(0);

pub(crate) struct ProcessOutput {
    pub(crate) stdout: String,
    pub(crate) stderr: String,
    pub(crate) code: u32,
}

pub(crate) fn unique() -> String {
    format!(
        "{}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        SERIAL.fetch_add(1, Ordering::Relaxed)
    )
}

pub(crate) fn resolve(executable: &OsStr) -> crate::Result<PathBuf> {
    let path = Path::new(executable);
    if path.is_absolute() {
        return path
            .is_file()
            .then(|| native_path(path))
            .ok_or_else(|| "Required executable is missing.".into());
    }
    if path.components().count() != 1 {
        return Err("Executable must be absolute or a single PATH name.".into());
    }
    let search = std::env::var_os("PATH").ok_or("PATH is unavailable.")?;
    for directory in std::env::split_paths(&search) {
        if !directory.is_absolute() {
            continue;
        }
        let candidate = directory.join(executable);
        if candidate.is_file() {
            return Ok(native_path(&candidate));
        }
    }
    Err(format!(
        "Required executable is missing from PATH: {}",
        executable.to_string_lossy()
    ))
}

fn native_path(path: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::{OsStrExt as _, OsStringExt as _};
        // CMD parses a slash in argv[0] as a switch even though CreateProcess
        // accepts it as a path separator. Preserve every other UTF-16 unit.
        let units: Vec<_> = path
            .as_os_str()
            .encode_wide()
            .map(|unit| if unit == 47 { 92 } else { unit })
            .collect();
        PathBuf::from(OsString::from_wide(&units))
    }
    #[cfg(not(windows))]
    {
        path.to_owned()
    }
}

fn hidden(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        command.creation_flags(0x0800_0000);
    }
    #[cfg(not(windows))]
    let _ = command;
}

fn arguments(command: &mut Command, executable: &Path, args: &[OsString]) -> crate::Result<()> {
    #[cfg(windows)]
    if executable
        .file_name()
        .is_some_and(|name| name.eq_ignore_ascii_case("cmd.exe"))
    {
        use std::os::windows::process::CommandExt as _;
        // CMD parses one command string rather than CRT argv. Startup uses
        // only this explicit /d /c form for its generated environment batch.
        let [disable_auto_run, execute, script] = args else {
            return Err("Unsupported command interpreter arguments.".into());
        };
        if disable_auto_run != "/d" || execute != "/c" {
            return Err("Command interpreter startup must disable AutoRun.".into());
        }
        let script = script
            .to_str()
            .ok_or("Command interpreter input must be UTF-8.")?;
        if Path::new(script).is_absolute() {
            if script.contains(['"', '\r', '\n', '%']) {
                return Err("Unsupported environment batch path.".into());
            }
            let script = script.replace('/', "\\");
            command.raw_arg(format!("/d /v:off /s /c \"\"{script}\"\""));
        } else {
            command.raw_arg(format!("/d /v:off /c {script}"));
        }
        return Ok(());
    }
    #[cfg(not(windows))]
    let _ = executable;
    command.args(args);
    Ok(())
}

fn stop_tree(child: &mut Child, working: &Path) {
    #[cfg(windows)]
    if let Some(system) = std::env::var_os("SystemRoot") {
        let taskkill = PathBuf::from(system).join("System32/taskkill.exe");
        if taskkill.is_absolute() {
            let mut command = Command::new(taskkill);
            command
                .args(["/T", "/F", "/PID", &child.id().to_string()])
                .current_dir(working)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            hidden(&mut command);
            if let Ok(mut killer) = command.spawn() {
                let deadline = Instant::now();
                while killer.try_wait().is_ok_and(|status| status.is_none())
                    && deadline.elapsed() < Duration::from_secs(5)
                {
                    thread::sleep(Duration::from_millis(10));
                }
                let _ = killer.kill();
                let _ = killer.wait();
            }
        }
    }
    #[cfg(not(windows))]
    let _ = working;
    let _ = child.kill();
    let _ = child.wait();
}

struct ChildGuard<'a> {
    child: Child,
    working: &'a Path,
    finished: bool,
}

impl Drop for ChildGuard<'_> {
    fn drop(&mut self) {
        if !self.finished {
            stop_tree(&mut self.child, self.working);
        }
    }
}

fn capture(path: &Path) -> crate::Result<File> {
    validate_external_path(path).map_err(|error| error.to_string())?;
    OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| error.to_string())
}

fn text(path: &Path) -> crate::Result<String> {
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|error| error.to_string())?
        .take(MAX_OUTPUT + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if u64::try_from(bytes.len()).map_err(|error| error.to_string())? > MAX_OUTPUT {
        return Err("Tool output exceeded its bound.".into());
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

pub(crate) fn run(
    executable: &OsStr,
    args: &[OsString],
    working: &Path,
    environment: &BTreeMap<String, String>,
    timeout: Duration,
    log: &Path,
) -> crate::Result<ProcessOutput> {
    validate_external_path(working).map_err(|error| error.to_string())?;
    validate_external_path(log).map_err(|error| error.to_string())?;
    if !working.is_dir() {
        return Err("Native working directory must exist.".into());
    }
    let serial = unique();
    let stdout_path = log.with_extension(format!("{serial}.stdout"));
    let stderr_path = log.with_extension(format!("{serial}.stderr"));
    let stdout = capture(&stdout_path)?;
    let stderr = capture(&stderr_path)?;
    let executable = resolve(executable)?;
    let mut command = Command::new(&executable);
    arguments(&mut command, &executable, args)?;
    command
        .current_dir(working)
        .envs(environment)
        .stdin(Stdio::null())
        .stdout(stdout.try_clone().map_err(|error| error.to_string())?)
        .stderr(stderr.try_clone().map_err(|error| error.to_string())?);
    hidden(&mut command);
    let mut guard = ChildGuard {
        child: command.spawn().map_err(|error| error.to_string())?,
        working,
        finished: false,
    };
    let started = Instant::now();
    let status = loop {
        if let Some(status) = guard.child.try_wait().map_err(|error| error.to_string())? {
            break status;
        }
        if (!timeout.is_zero() && started.elapsed() >= timeout)
            || stdout.metadata().map_err(|error| error.to_string())?.len() > MAX_OUTPUT
            || stderr.metadata().map_err(|error| error.to_string())?.len() > MAX_OUTPUT
        {
            fs::write(
                log,
                format!(
                    "Tool timed out or exceeded its output bound.\nstdout: {}\nstderr: {}\n",
                    stdout_path.display(),
                    stderr_path.display()
                ),
            )
            .map_err(|error| error.to_string())?;
            return Err(format!(
                "Tool timed out or exceeded its output bound. Evidence: {}",
                log.display()
            ));
        }
        thread::sleep(Duration::from_millis(20));
    };
    // An ordinary compiler child may leave background descendants behind. The
    // owned root is complete; captures use files, so no inherited pipe can hang us.
    guard.finished = true;
    let output = ProcessOutput {
        stdout: text(&stdout_path)?,
        stderr: text(&stderr_path)?,
        code: status
            .code()
            .map_or(1, |code| u32::from_ne_bytes(code.to_ne_bytes())),
    };
    fs::write(log, format!("{}{}", output.stdout, output.stderr))
        .map_err(|error| error.to_string())?;
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_native_working_directories_reject_before_spawn() {
        assert!(
            run(
                OsStr::new("missing"),
                &[],
                Path::new("relative"),
                &BTreeMap::new(),
                Duration::ZERO,
                Path::new("relative.log")
            )
            .is_err()
        );
    }

    #[test]
    fn captures_both_streams_and_nonzero_exit() -> crate::Result<()> {
        let scratch = std::env::temp_dir().join(format!("Bonsai-Process-{}", unique()));
        fs::create_dir(&scratch).map_err(|error| error.to_string())?;
        #[cfg(windows)]
        let (exe, args) = (
            std::env::var_os("SystemRoot")
                .map(|value| PathBuf::from(value).join("System32/cmd.exe"))
                .ok_or("Missing Windows directory.")?,
            vec![
                "/d".into(),
                "/c".into(),
                "echo output & echo diagnostic 1>&2 & exit /b 7".into(),
            ],
        );
        #[cfg(not(windows))]
        let (exe, args) = (
            PathBuf::from("/bin/sh"),
            vec![
                "-c".into(),
                "printf output; printf diagnostic >&2; exit 7".into(),
            ],
        );
        let result = run(
            exe.as_os_str(),
            &args,
            &scratch,
            &BTreeMap::new(),
            Duration::from_secs(5),
            &scratch.join("capture.log"),
        )?;
        assert_eq!(result.code, 7);
        assert!(result.stdout.contains("output"));
        assert!(result.stderr.contains("diagnostic"));
        Ok(())
    }

    #[cfg(windows)]
    #[test]
    fn command_batch_path_preserves_spaces() -> crate::Result<()> {
        let scratch = std::env::temp_dir().join(format!("Bonsai-Process with spaces-{}", unique()));
        fs::create_dir(&scratch).map_err(|error| error.to_string())?;
        let batch = scratch.join("environment with spaces.cmd");
        fs::write(&batch, "@echo batch-output\r\n@exit /b 0\r\n")
            .map_err(|error| error.to_string())?;
        let exe =
            PathBuf::from(std::env::var_os("SystemRoot").ok_or("Missing Windows directory.")?)
                .join("System32/cmd.exe");
        let output = run(
            exe.as_os_str(),
            &["/d".into(), "/c".into(), batch.into_os_string()],
            &scratch,
            &BTreeMap::new(),
            Duration::from_secs(5),
            &scratch.join("batch.log"),
        )?;
        assert_eq!(output.code, 0);
        assert!(output.stdout.contains("batch-output"));
        Ok(())
    }
}
