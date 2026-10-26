//! Bounded compiler/tool execution with inherited Windows profile settings.
#![expect(
    clippy::redundant_pub_crate,
    reason = "Private startup modules expose only a crate-local implementation API."
)]

use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    fs::{self, File},
    io::Read as _,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[cfg(not(windows))]
use std::{
    fs::OpenOptions,
    process::{Child, Command, Stdio},
};

#[cfg(windows)]
use bonsai_windows_process::{OwnedProcess, ToolProcessSpec};

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

struct ChildGuard {
    #[cfg(windows)]
    child: OwnedProcess,
    #[cfg(not(windows))]
    child: Child,
    finished: bool,
}

impl ChildGuard {
    #[cfg_attr(
        windows,
        expect(
            clippy::needless_pass_by_ref_mut,
            reason = "The non-Windows std::process::Child wait API requires a mutable borrow."
        )
    )]
    fn try_wait(&mut self) -> crate::Result<Option<u32>> {
        #[cfg(windows)]
        {
            self.child.try_wait().map_err(|error| error.to_string())
        }
        #[cfg(not(windows))]
        {
            self.child
                .try_wait()
                .map(|status| {
                    status.map(|status| {
                        status
                            .code()
                            .map_or(1, |code| u32::from_ne_bytes(code.to_ne_bytes()))
                    })
                })
                .map_err(|error| error.to_string())
        }
    }

    #[cfg_attr(
        windows,
        expect(
            clippy::needless_pass_by_ref_mut,
            reason = "The non-Windows std::process::Child kill/wait API requires a mutable borrow."
        )
    )]
    fn stop(&mut self) -> crate::Result<()> {
        #[cfg(windows)]
        {
            self.child
                .stop(Duration::from_secs(5))
                .map_err(|error| error.to_string())
        }
        #[cfg(not(windows))]
        {
            let _ = self.child.kill();
            self.child
                .wait()
                .map(|_| ())
                .map_err(|error| error.to_string())
        }
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.stop();
        }
    }
}

#[cfg(not(windows))]
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
    run_with_checkpoint(
        executable,
        args,
        working,
        environment,
        timeout,
        log,
        crate::startup_cancel::checkpoint,
    )
}

fn run_with_checkpoint(
    executable: &OsStr,
    args: &[OsString],
    working: &Path,
    environment: &BTreeMap<String, String>,
    timeout: Duration,
    log: &Path,
    mut checkpoint: impl FnMut() -> crate::Result<()>,
) -> crate::Result<ProcessOutput> {
    checkpoint()?;
    validate_external_path(working).map_err(|error| error.to_string())?;
    validate_external_path(log).map_err(|error| error.to_string())?;
    if !working.is_dir() {
        return Err("Native working directory must exist.".into());
    }
    let serial = unique();
    let stdout_path = log.with_extension(format!("{serial}.stdout"));
    let stderr_path = log.with_extension(format!("{serial}.stderr"));
    let executable = resolve(executable)?;
    checkpoint()?;
    #[cfg(windows)]
    let child = OwnedProcess::start_tool(&ToolProcessSpec {
        executable,
        arguments: args.to_vec(),
        working_directory: working.to_owned(),
        stdout: stdout_path.clone(),
        stderr: stderr_path.clone(),
        environment: environment.clone(),
    })
    .map_err(|error| error.to_string())?;
    #[cfg(not(windows))]
    let child = {
        let stdout = capture(&stdout_path)?;
        let stderr = capture(&stderr_path)?;
        Command::new(&executable)
            .args(args)
            .current_dir(working)
            .envs(environment)
            .stdin(Stdio::null())
            .stdout(stdout)
            .stderr(stderr)
            .spawn()
            .map_err(|error| error.to_string())?
    };
    let mut guard = ChildGuard {
        child,
        finished: false,
    };
    let started = Instant::now();
    let status = loop {
        if let Err(error) = checkpoint() {
            fs::write(
                log,
                format!(
                    "{error}\nstdout: {}\nstderr: {}\n",
                    stdout_path.display(),
                    stderr_path.display()
                ),
            )
            .map_err(|error| error.to_string())?;
            return Err(format!("{error} Evidence: {}", log.display()));
        }
        if let Some(status) = guard.try_wait()? {
            break status;
        }
        if (!timeout.is_zero() && started.elapsed() >= timeout)
            || fs::metadata(&stdout_path)
                .map_err(|error| error.to_string())?
                .len()
                > MAX_OUTPUT
            || fs::metadata(&stderr_path)
                .map_err(|error| error.to_string())?
                .len()
                > MAX_OUTPUT
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
    // A compiler root can exit before a descendant. Empty the owned Job before
    // accepting its result, so no compiler outlives the enclosing build lock.
    guard.stop()?;
    checkpoint()?;
    let output = ProcessOutput {
        stdout: text(&stdout_path)?,
        stderr: text(&stderr_path)?,
        code: status,
    };
    fs::write(log, format!("{}{}", output.stdout, output.stderr))
        .map_err(|error| error.to_string())?;
    checkpoint()?;
    guard.finished = true;
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
        let scratch =
            std::env::temp_dir().join(format!("Bonsai-Process with spaces & (group)-{}", unique()));
        fs::create_dir(&scratch).map_err(|error| error.to_string())?;
        let batch = scratch.join("environment with spaces & (group).cmd");
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

    #[test]
    fn cancelled_before_spawn_has_no_filesystem_side_effects() -> crate::Result<()> {
        let scratch = std::env::temp_dir().join(format!("Bonsai-Process-{}", unique()));
        fs::create_dir(&scratch).map_err(|error| error.to_string())?;
        let result = run_with_checkpoint(
            OsStr::new("missing"),
            &[],
            &scratch,
            &BTreeMap::new(),
            Duration::ZERO,
            &scratch.join("not-created.log"),
            || Err("Synthetic caller cancellation.".into()),
        );
        assert!(result.is_err_and(|error| error == "Synthetic caller cancellation."));
        assert_eq!(
            fs::read_dir(&scratch)
                .map_err(|error| error.to_string())?
                .count(),
            0
        );
        Ok(())
    }

    #[cfg(windows)]
    fn quoted(path: &Path) -> String {
        path.to_string_lossy().replace('\'', "''")
    }

    #[cfg(windows)]
    fn process_tree(
        scratch: &Path,
        wait: bool,
    ) -> crate::Result<(PathBuf, Vec<OsString>, PathBuf)> {
        let executable = resolve(OsStr::new("pwsh.exe"))?;
        let marker = scratch.join("descendant.txt");
        let leaf = scratch.join("leaf.ps1");
        let parent = scratch.join("parent.ps1");
        fs::write(
            &leaf,
            format!(
                "$ErrorActionPreference = 'Stop'\n\
                 $process = Get-Process -Id $PID\n\
                 [IO.File]::WriteAllText('{marker}.partial', \"$PID`n$($process.StartTime.ToUniversalTime().ToFileTimeUtc())\")\n\
                 [IO.File]::Move('{marker}.partial', '{marker}')\n\
                 $deadline = [DateTime]::UtcNow.AddSeconds(30)\n\
                 while ([DateTime]::UtcNow -lt $deadline) {{ Start-Sleep -Milliseconds 100 }}\n",
                marker = quoted(&marker)
            ),
        )
        .map_err(|error| error.to_string())?;
        fs::write(
            &parent,
            format!(
                "$ErrorActionPreference = 'Stop'\n\
                 $start = [Diagnostics.ProcessStartInfo]::new('{executable}')\n\
                 $start.UseShellExecute = $false\n\
                 $start.CreateNoWindow = $true\n\
                 $start.WorkingDirectory = '{working}'\n\
                 foreach ($argument in @('-NoLogo', '-NoProfile', '-NonInteractive', '-File', '{leaf}')) {{ $start.ArgumentList.Add($argument) }}\n\
                 $child = [Diagnostics.Process]::Start($start)\n\
                 while (-not [IO.File]::Exists('{marker}')) {{ if ($child.HasExited) {{ throw 'Leaf failed.' }}; Start-Sleep -Milliseconds 10 }}\n\
                 {wait}\n",
                executable = quoted(&executable),
                working = quoted(scratch),
                leaf = quoted(&leaf),
                marker = quoted(&marker),
                wait = if wait { "$child.WaitForExit()" } else { "exit 0" },
            ),
        )
        .map_err(|error| error.to_string())?;
        Ok((
            executable,
            vec![
                "-NoLogo".into(),
                "-NoProfile".into(),
                "-NonInteractive".into(),
                "-File".into(),
                parent.into_os_string(),
            ],
            marker,
        ))
    }

    #[cfg(windows)]
    fn descendant(marker: &Path) -> crate::Result<Option<bonsai_windows_process::ParentProcess>> {
        if !marker.is_file() {
            return Ok(None);
        }
        let identity = fs::read_to_string(marker).map_err(|error| error.to_string())?;
        let (id, creation) = identity
            .split_once('\n')
            .ok_or("Incomplete descendant identity.")?;
        let id = id
            .trim()
            .parse()
            .map_err(|error| format!("Invalid descendant PID: {error}"))?;
        let creation = creation
            .trim()
            .parse()
            .map_err(|error| format!("Invalid descendant creation identity: {error}"))?;
        bonsai_windows_process::ParentProcess::open(id, creation)
            .map(Some)
            .map_err(|error| error.to_string())
    }

    #[cfg(windows)]
    #[test]
    fn cancelled_build_stops_descendants_before_releasing_lock() -> crate::Result<()> {
        let scratch = std::env::temp_dir().join(format!("Bonsai-Process-Cancel-{}", unique()));
        fs::create_dir(&scratch).map_err(|error| error.to_string())?;
        let (executable, args, marker) = process_tree(&scratch, true)?;
        let lock_path = scratch.join("build.lock");
        let lock = crate::startup_cache::lock_file(&lock_path)?;
        let mut child = None;
        let result = run_with_checkpoint(
            executable.as_os_str(),
            &args,
            &scratch,
            &BTreeMap::new(),
            Duration::from_secs(30),
            &scratch.join("cancel.log"),
            || {
                child = descendant(&marker)?;
                if child.is_some() {
                    Err("Synthetic caller cancellation.".into())
                } else {
                    Ok(())
                }
            },
        );
        assert!(result.is_err_and(|error| error.contains("Synthetic caller cancellation.")));
        let child = child.ok_or("Synthetic compiler descendant never became ready.")?;
        assert!(
            child.ensure_alive().is_err(),
            "Compiler descendant survived cancellation."
        );
        assert!(crate::startup_cache::lock_file(&lock_path).is_err());
        drop(lock);
        let _next_build = crate::startup_cache::lock_file(&lock_path)?;
        assert!(
            fs::read_to_string(scratch.join("cancel.log"))
                .map_err(|error| error.to_string())?
                .contains("Synthetic caller cancellation.")
        );
        Ok(())
    }

    #[cfg(windows)]
    #[test]
    fn timed_out_build_stops_descendants() -> crate::Result<()> {
        let scratch = std::env::temp_dir().join(format!("Bonsai-Process-Timeout-{}", unique()));
        fs::create_dir(&scratch).map_err(|error| error.to_string())?;
        let (executable, args, marker) = process_tree(&scratch, true)?;
        let mut child = None;
        let result = run_with_checkpoint(
            executable.as_os_str(),
            &args,
            &scratch,
            &BTreeMap::new(),
            Duration::from_secs(10),
            &scratch.join("timeout.log"),
            || {
                if child.is_none() {
                    child = descendant(&marker)?;
                }
                Ok(())
            },
        );
        assert!(result.is_err_and(|error| error.contains("timed out")));
        let child = child.ok_or("Synthetic compiler descendant never became ready.")?;
        assert!(
            child.ensure_alive().is_err(),
            "Compiler descendant survived timeout."
        );
        Ok(())
    }

    #[cfg(windows)]
    #[test]
    fn completed_root_cannot_leave_compiler_descendants() -> crate::Result<()> {
        let scratch = std::env::temp_dir().join(format!("Bonsai-Process-RootExit-{}", unique()));
        fs::create_dir(&scratch).map_err(|error| error.to_string())?;
        let (executable, args, marker) = process_tree(&scratch, false)?;
        let output = run(
            executable.as_os_str(),
            &args,
            &scratch,
            &BTreeMap::new(),
            Duration::from_secs(10),
            &scratch.join("root-exit.log"),
        )?;
        assert_eq!(output.code, 0);
        assert!(marker.is_file());
        assert!(
            descendant(&marker).is_err(),
            "Compiler descendant survived root exit."
        );
        Ok(())
    }

    #[cfg(windows)]
    #[test]
    fn compiler_overlay_preserves_inherited_profile_and_unicode() -> crate::Result<()> {
        let scratch = std::env::temp_dir().join(format!("Bonsai-Process-Environment-{}", unique()));
        fs::create_dir(&scratch).map_err(|error| error.to_string())?;
        let executable = resolve(OsStr::new("pwsh.exe"))?;
        let original_path = std::env::var("PATH").map_err(|error| error.to_string())?;
        let overlay = BTreeMap::from([
            (
                "BONSAI_STARTUP_UNICODE".into(),
                "\u{30d1}\u{30b9}\u{1f332}".into(),
            ),
            (
                "pAtH".into(),
                format!("{original_path};C:\\compiler-marker"),
            ),
        ]);
        let output = run(
            executable.as_os_str(),
            &[
                "-NoLogo".into(), "-NoProfile".into(), "-NonInteractive".into(), "-Command".into(),
                "[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false); @{Value=$env:BONSAI_STARTUP_UNICODE; Path=$env:PATH; SystemRoot=$env:SystemRoot; Local=$env:LOCALAPPDATA; Roaming=$env:APPDATA} | ConvertTo-Json -Compress".into(),
            ],
            &scratch,
            &overlay,
            Duration::from_secs(10),
            &scratch.join("environment.log"),
        )?;
        assert_eq!(output.code, 0, "{}", output.stderr);
        let actual: serde_json::Value =
            serde_json::from_str(&output.stdout).map_err(|error| error.to_string())?;
        assert_eq!(actual["Value"], overlay["BONSAI_STARTUP_UNICODE"]);
        // PowerShell itself prepends $PSHOME; the complete compiler overlay
        // remains the inherited suffix rather than changing the parent PATH.
        assert!(
            actual["Path"]
                .as_str()
                .is_some_and(|path| path.ends_with(&overlay["pAtH"]))
        );
        for (field, name) in [
            ("SystemRoot", "SystemRoot"),
            ("Local", "LOCALAPPDATA"),
            ("Roaming", "APPDATA"),
        ] {
            assert_eq!(
                actual[field],
                std::env::var(name).map_err(|error| error.to_string())?
            );
        }
        assert_eq!(
            std::env::var("PATH").map_err(|error| error.to_string())?,
            original_path
        );
        Ok(())
    }
}
