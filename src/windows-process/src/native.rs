//! Audited Win32 boundary. Handles use standard-library RAII; attribute buffers
//! remain allocated and aligned until after `DeleteProcThreadAttributeList`.

use std::ffi::{OsStr, c_void};
use std::io;
use std::os::windows::ffi::OsStrExt as _;
use std::os::windows::io::{
    AsRawHandle as _, AsRawSocket as _, AsSocket, FromRawHandle as _, OwnedHandle,
};
use std::path::Path;
use std::ptr::{self, null, null_mut};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{
    ERROR_INSUFFICIENT_BUFFER, ERROR_INVALID_PARAMETER, ERROR_MORE_DATA, FILETIME, HANDLE,
    INVALID_HANDLE_VALUE, WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::NetworkManagement::IpHelper::{
    GetExtendedTcpTable, MIB_TCPROW_OWNER_PID, TCP_TABLE_OWNER_PID_LISTENER,
};
use windows_sys::Win32::Networking::WinSock::{
    AF_INET, SO_EXCLUSIVEADDRUSE, SOCKET_ERROR, SOL_SOCKET, WSAGetLastError, setsockopt,
};
use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
use windows_sys::Win32::Storage::FileSystem::{
    CREATE_NEW, CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE,
    OPEN_EXISTING,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, IsProcessInJob, JOB_OBJECT_LIMIT_ACTIVE_PROCESS,
    JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION, JOB_OBJECT_LIMIT_JOB_MEMORY,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectBasicAccountingInformation,
    JobObjectBasicProcessIdList, JobObjectExtendedLimitInformation, QueryInformationJobObject,
    SetInformationJobObject, TerminateJobObject,
};
use windows_sys::Win32::System::ProcessStatus::{
    K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX,
};
use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows_sys::Win32::System::Threading::{
    CREATE_NO_WINDOW, CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT, CreateProcessW,
    DeleteProcThreadAttributeList, EXTENDED_STARTUPINFO_PRESENT, GetExitCodeProcess,
    GetProcessMitigationPolicy, GetProcessTimes, InitializeProcThreadAttributeList,
    LPPROC_THREAD_ATTRIBUTE_LIST, OpenProcess, PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
    PROC_THREAD_ATTRIBUTE_JOB_LIST, PROC_THREAD_ATTRIBUTE_MITIGATION_POLICY, PROCESS_INFORMATION,
    PROCESS_MITIGATION_POLICY, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
    ProcessExtensionPointDisablePolicy, ProcessImageLoadPolicy, ProcessSystemCallDisablePolicy,
    ResumeThread, STARTF_USESHOWWINDOW, STARTF_USESTDHANDLES, STARTUPINFOEXW, TerminateProcess,
    UpdateProcThreadAttribute, WaitForSingleObject,
};

use crate::{ProcessMemory, ProcessSpec, SandboxPolicy, ToolProcessSpec, invalid};

mod containment;
mod cpu;
mod environment;
mod power;

pub(super) fn cpu_sets() -> io::Result<Vec<crate::CpuSet>> {
    cpu::query()
}

pub(super) fn power_status() -> io::Result<crate::PowerStatus> {
    power::query()
}

pub(super) fn sandbox_available() -> io::Result<()> {
    containment::available()
}

pub(super) fn tool_environment(
    overlay: &std::collections::BTreeMap<String, String>,
) -> io::Result<Option<Vec<u16>>> {
    if overlay.is_empty() {
        return Ok(None);
    }
    environment::build(std::env::vars_os(), overlay).map(Some)
}

struct LaunchSpec<'a> {
    executable: &'a Path,
    working_directory: &'a Path,
    stdout: &'a Path,
    stderr: &'a Path,
    limits: Option<(usize, u32)>,
    mitigation_profile: MitigationProfile,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum MitigationProfile {
    Tool,
    Headless,
    Inspection,
}

impl MitigationProfile {
    const fn flags(self) -> Option<u64> {
        match self {
            Self::Tool => None,
            Self::Headless => Some(HEADLESS_MITIGATIONS),
            Self::Inspection => Some(HEADLESS_MITIGATIONS & !(1 << 28)),
        }
    }
}

pub(super) struct ParentProcess(OwnedHandle);

impl ParentProcess {
    pub(super) fn open(id: u32, expected_creation_time: u64) -> io::Result<Self> {
        if id == 0 || expected_creation_time == 0 {
            return Err(invalid("caller process identity must be nonzero"));
        }
        // SAFETY: OpenProcess takes a numeric PID and returns a fresh owning
        // noninheritable handle with only query and synchronization rights.
        let handle = owned(unsafe {
            OpenProcess(
                PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
                0,
                id,
            )
        })?;
        let actual = creation_file_time(&handle)?;
        if actual != expected_creation_time {
            return Err(invalid("caller process identity changed"));
        }
        let parent = Self(handle);
        parent.ensure_alive()?;
        Ok(parent)
    }

    pub(super) fn ensure_alive(&self) -> io::Result<()> {
        // SAFETY: This noninheritable process synchronization handle remains
        // owned for the entire wait; zero timeout cannot block.
        match unsafe { WaitForSingleObject(self.0.as_raw_handle(), 0) } {
            WAIT_TIMEOUT => Ok(()),
            WAIT_OBJECT_0 => Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "caller process exited",
            )),
            _ => Err(io::Error::last_os_error()),
        }
    }
}

fn creation_file_time(handle: &OwnedHandle) -> io::Result<u64> {
    let mut creation = FILETIME::default();
    let mut exit = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    // SAFETY: The queried handle is live; all four output FILETIMEs are
    // independently allocated writable values for this synchronous call.
    check(unsafe {
        GetProcessTimes(
            handle.as_raw_handle(),
            &raw mut creation,
            &raw mut exit,
            &raw mut kernel,
            &raw mut user,
        )
    })?;
    Ok((u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime))
}

#[cfg(test)]
mod parent_tests {
    use std::io;
    use std::os::windows::process::CommandExt as _;
    use std::process::{Command, Stdio};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, ParentProcess,
        creation_file_time, owned,
    };

    #[test]
    fn retained_parent_identity_rejects_mismatch_and_observes_exit() -> io::Result<()> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_nanos();
        let directory =
            std::env::temp_dir().join(format!("Bonsai-ParentProbe-{}-{nonce}", std::process::id()));
        std::fs::create_dir(&directory)?;
        let executable =
            std::env::var_os("ComSpec").ok_or_else(|| io::Error::other("missing ComSpec"))?;
        let mut child = Command::new(executable)
            .args(["/d", "/c", "set /p bonsai_wait="])
            .current_dir(&directory)
            .creation_flags(0x0800_0000)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        let result = (|| {
            // SAFETY: Query a newly spawned test process by its numeric PID;
            // the new noninheritable owning handle is immediately retained.
            let handle = owned(unsafe {
                OpenProcess(
                    PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                    0,
                    child.id(),
                )
            })?;
            let identity = creation_file_time(&handle)?;
            if ParentProcess::open(child.id(), identity + 1).is_ok() {
                return Err(io::Error::other("reused process identity accepted"));
            }
            let parent = ParentProcess::open(child.id(), identity)?;
            parent.ensure_alive()?;
            child.kill()?;
            let _ = child.wait()?;
            if parent.ensure_alive().is_ok() || ParentProcess::open(child.id(), identity).is_ok() {
                return Err(io::Error::other("exited caller accepted"));
            }
            Ok(())
        })();
        let _ = child.kill();
        let _ = child.wait();
        result
    }
}

pub(super) struct NativeProcess {
    // Drop order closes the Job before the root process handle.
    job: OwnedHandle,
    process: OwnedHandle,
    id: u32,
    // Drop after the Job and retained root: containment outlives the worker.
    containment: Option<containment::Environment>,
}

impl NativeProcess {
    pub(super) fn start(spec: &ProcessSpec, command: Vec<u16>) -> io::Result<Self> {
        Self::launch(
            &LaunchSpec {
                executable: &spec.executable,
                working_directory: &spec.working_directory,
                stdout: &spec.stdout,
                stderr: &spec.stderr,
                limits: Some((spec.memory_limit_bytes, spec.active_process_limit)),
                mitigation_profile: MitigationProfile::Headless,
            },
            command,
            None,
            None,
        )
    }

    pub(super) fn start_inspection(spec: &ProcessSpec, command: Vec<u16>) -> io::Result<Self> {
        Self::launch(
            &LaunchSpec {
                executable: &spec.executable,
                working_directory: &spec.working_directory,
                stdout: &spec.stdout,
                stderr: &spec.stderr,
                limits: Some((spec.memory_limit_bytes, spec.active_process_limit)),
                mitigation_profile: MitigationProfile::Inspection,
            },
            command,
            None,
            None,
        )
    }

    pub(super) fn start_sandboxed(
        spec: &ProcessSpec,
        command: Vec<u16>,
        policy: &SandboxPolicy,
    ) -> io::Result<Self> {
        let containment = containment::Environment::create(spec, policy)?;
        let environment = containment::child_environment(policy)?;
        Self::launch(
            &LaunchSpec {
                executable: &spec.executable,
                working_directory: &spec.working_directory,
                stdout: &spec.stdout,
                stderr: &spec.stderr,
                limits: Some((spec.memory_limit_bytes, spec.active_process_limit)),
                mitigation_profile: MitigationProfile::Headless,
            },
            command,
            Some(&environment),
            Some(containment),
        )
    }

    pub(super) fn start_tool(
        spec: &ToolProcessSpec,
        command: Vec<u16>,
        environment: Option<&[u16]>,
    ) -> io::Result<Self> {
        Self::launch(
            &LaunchSpec {
                executable: &spec.executable,
                working_directory: &spec.working_directory,
                stdout: &spec.stdout,
                stderr: &spec.stderr,
                limits: None,
                mitigation_profile: MitigationProfile::Tool,
            },
            command,
            environment,
            None,
        )
    }

    fn launch(
        spec: &LaunchSpec<'_>,
        mut command: Vec<u16>,
        environment: Option<&[u16]>,
        containment: Option<containment::Environment>,
    ) -> io::Result<Self> {
        let executable = wide(spec.executable.as_os_str());
        let directory = wide(spec.working_directory.as_os_str());
        // SAFETY: Null attributes/name request a new noninheritable unnamed Job.
        let job = owned(unsafe { CreateJobObjectW(null(), null()) })?;
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
            // A headless engine must terminate on an unhandled exception rather
            // than waiting for a Windows Error Reporting dialog. The Job applies
            // this policy to descendants without changing the owner's error mode.
            | JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION;
        if let Some((memory, processes)) = spec.limits {
            limits.BasicLimitInformation.LimitFlags |=
                JOB_OBJECT_LIMIT_JOB_MEMORY | JOB_OBJECT_LIMIT_ACTIVE_PROCESS;
            limits.BasicLimitInformation.ActiveProcessLimit = processes;
            limits.JobMemoryLimit = memory;
        }
        // SAFETY: The handle is owned/live, class matches the initialized structure,
        // and the exact readable structure size is passed synchronously.
        check(unsafe {
            SetInformationJobObject(
                job.as_raw_handle(),
                JobObjectExtendedLimitInformation,
                ptr::from_ref(&limits).cast(),
                win_size::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>(),
            )
        })?;
        let input = standard_file(
            OsStr::new("NUL"),
            0x8000_0000,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            OPEN_EXISTING,
        )?;
        let output = standard_file(
            spec.stdout.as_os_str(),
            0x4000_0000,
            FILE_SHARE_READ,
            CREATE_NEW,
        )?;
        let error = standard_file(
            spec.stderr.as_os_str(),
            0x4000_0000,
            FILE_SHARE_READ,
            CREATE_NEW,
        )?;
        let attributes = CreationAttributes::new(
            job.as_raw_handle(),
            [
                input.as_raw_handle(),
                output.as_raw_handle(),
                error.as_raw_handle(),
            ],
            spec.mitigation_profile.flags(),
            containment.as_ref().map(containment::Environment::raw),
        )?;
        let mut startup = STARTUPINFOEXW::default();
        startup.StartupInfo.cb = win_size::<STARTUPINFOEXW>();
        startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES | STARTF_USESHOWWINDOW;
        startup.StartupInfo.hStdInput = input.as_raw_handle();
        startup.StartupInfo.hStdOutput = output.as_raw_handle();
        startup.StartupInfo.hStdError = error.as_raw_handle();
        startup.lpAttributeList = attributes.address();
        let mut info = PROCESS_INFORMATION::default();
        // SAFETY: All strings are terminated and mutable command storage is live.
        // Attributes hold stable buffers and live handles. HANDLE_LIST restricts
        // inheritance to the three streams; JOB_LIST assigns the Job atomically
        // before child code can run. An optional complete double-NUL-terminated
        // UTF-16 environment block stays allocated; null inherits unchanged.
        // Both process/thread handles are noninheritable, no breakaway is enabled.
        check(unsafe {
            CreateProcessW(
                executable.as_ptr(),
                command.as_mut_ptr(),
                null(),
                null(),
                1,
                CREATE_SUSPENDED
                    | EXTENDED_STARTUPINFO_PRESENT
                    | CREATE_NO_WINDOW
                    | CREATE_UNICODE_ENVIRONMENT,
                environment.map_or(null(), |block| block.as_ptr().cast()),
                directory.as_ptr(),
                ptr::from_ref(&startup.StartupInfo),
                &raw mut info,
            )
        })?;
        let process = owned(info.hProcess)?;
        let thread = owned(info.hThread)?;
        if let Err(error) = resume_owned(
            &process,
            &thread,
            &job,
            spec.mitigation_profile,
            containment.is_some(),
        ) {
            // PSEC may broker process creation rather than inheriting JOB_LIST.
            // Any failure before confirmed ownership must terminate this exact
            // retained root, even when closing our Job cannot reach it yet.
            terminate_failed_start(&process)?;
            return Err(error);
        }
        Ok(Self {
            job,
            process,
            id: info.dwProcessId,
            containment,
        })
    }

    pub(super) const fn id(&self) -> u32 {
        self.id
    }

    pub(super) fn try_wait(&self) -> io::Result<Option<u32>> {
        self.wait_for_exit(Duration::ZERO)
    }

    pub(super) fn wait_for_exit(&self, timeout: Duration) -> io::Result<Option<u32>> {
        let milliseconds = u32::try_from(timeout.as_millis())
            .ok()
            .filter(|value| *value < u32::MAX)
            .ok_or_else(|| invalid("process wait interval exceeds the bounded native limit"))?;
        // SAFETY: The owned process handle remains live; milliseconds is a
        // bounded interval that can never request the native infinite wait.
        match unsafe { WaitForSingleObject(self.process.as_raw_handle(), milliseconds) } {
            WAIT_TIMEOUT => Ok(None),
            WAIT_OBJECT_0 => {
                let mut code = 0;
                // SAFETY: Process handle and writable u32 remain live for the call.
                check(unsafe { GetExitCodeProcess(self.process.as_raw_handle(), &raw mut code) })?;
                Ok(Some(code))
            }
            WAIT_FAILED => Err(io::Error::last_os_error()),
            _ => Err(io::Error::other("unexpected process wait result")),
        }
    }

    pub(super) fn stop(&self, timeout: Duration) -> io::Result<()> {
        if timeout.is_zero() {
            return Err(invalid("cleanup timeout must be positive"));
        }
        let start = Instant::now();
        // Retain synchronization handles before termination: Windows removes a
        // member from Job accounting shortly before signaling its process handle.
        let members = self.members();
        // SAFETY: This owned/live Job has termination rights and contains only
        // processes atomically assigned by this owner and their descendants.
        check(unsafe { TerminateJobObject(self.job.as_raw_handle(), 1) })?;
        let members = members?;
        loop {
            let mut accounting = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
            // SAFETY: The class matches the writable, initialized structure and
            // exact byte count. The optional returned-size pointer is null.
            check(unsafe {
                QueryInformationJobObject(
                    self.job.as_raw_handle(),
                    JobObjectBasicAccountingInformation,
                    ptr::from_mut(&mut accounting).cast(),
                    win_size::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>(),
                    null_mut(),
                )
            })?;
            // Job accounting can reach zero just before the root handle becomes
            // signaled. A successful stop also guarantees try_wait sees exit.
            if accounting.ActiveProcesses == 0
                && self.try_wait()?.is_some()
                && members_exited(&members)?
            {
                return Ok(());
            }
            let Some(remaining) = timeout.checked_sub(start.elapsed()) else {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "owned job did not become empty after cancellation",
                ));
            };
            std::thread::sleep(remaining.min(Duration::from_millis(10)));
        }
    }

    fn members(&self) -> io::Result<Vec<OwnedHandle>> {
        const MAX_MEMBERS: usize = 65_536;
        let header_units = 8_usize.div_ceil(size_of::<usize>());
        let mut capacity = 64;
        for _ in 0..8 {
            let mut storage = vec![0_usize; header_units + capacity];
            let bytes = u32::try_from(size_of_val(storage.as_slice()))
                .map_err(|_| invalid("Job process list is too large"))?;
            // SAFETY: The usize-aligned allocation contains the two u32 header
            // fields and `capacity` pointer-sized IDs. The class and exact
            // writable byte length agree, and the Job handle remains owned.
            let result = unsafe {
                QueryInformationJobObject(
                    self.job.as_raw_handle(),
                    JobObjectBasicProcessIdList,
                    storage.as_mut_ptr().cast(),
                    bytes,
                    null_mut(),
                )
            };
            if result == 0 {
                let error = io::Error::last_os_error();
                if error.raw_os_error() != i32::try_from(ERROR_MORE_DATA).ok() {
                    return Err(error);
                }
                capacity = (capacity * 2).min(MAX_MEMBERS);
                continue;
            }
            // SAFETY: The initialized aligned buffer is at least eight bytes;
            // the native class writes these two u32 counts before its ID array.
            let [assigned, listed] = unsafe { ptr::read(storage.as_ptr().cast::<[u32; 2]>()) };
            let assigned =
                usize::try_from(assigned).map_err(|_| invalid("Job process count overflow"))?;
            let listed =
                usize::try_from(listed).map_err(|_| invalid("Job process count overflow"))?;
            if assigned > MAX_MEMBERS || listed > capacity || listed > assigned {
                return Err(invalid("Job process list exceeded its bound"));
            }
            if assigned > listed {
                capacity = assigned;
                continue;
            }
            let mut handles = Vec::with_capacity(listed);
            for id in &storage[header_units..header_units + listed] {
                let id =
                    u32::try_from(*id).map_err(|_| invalid("Job process identifier overflow"))?;
                // SAFETY: Each numeric PID came from this owned Job. The fresh
                // noninheritable handle has synchronization/query rights only;
                // it is retained immediately and never used to terminate by PID.
                let handle = unsafe {
                    OpenProcess(
                        PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
                        0,
                        id,
                    )
                };
                match owned(handle) {
                    Ok(handle) => {
                        let mut belongs = 0;
                        // SAFETY: Both process and Job handles remain owned;
                        // the writable BOOL lives through this synchronous query.
                        check(unsafe {
                            IsProcessInJob(
                                handle.as_raw_handle(),
                                self.job.as_raw_handle(),
                                &raw mut belongs,
                            )
                        })?;
                        // A PID can exit and be reused between enumeration/open.
                        // Retain only handles still belonging to this exact Job.
                        if belongs != 0 {
                            handles.push(handle);
                        }
                    }
                    Err(error)
                        if error.raw_os_error() == i32::try_from(ERROR_INVALID_PARAMETER).ok() =>
                    {
                        // A member can finish between querying and opening it.
                    }
                    Err(error) => return Err(error),
                }
            }
            return Ok(handles);
        }
        Err(io::Error::other(
            "Job process list changed too often during cleanup",
        ))
    }

    pub(super) fn peak_job_memory_bytes(&self) -> io::Result<usize> {
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        // SAFETY: The owned/live Job, information class, writable structure and
        // exact structure size agree; optional output size is null.
        check(unsafe {
            QueryInformationJobObject(
                self.job.as_raw_handle(),
                JobObjectExtendedLimitInformation,
                ptr::from_mut(&mut limits).cast(),
                win_size::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>(),
                null_mut(),
            )
        })?;
        Ok(limits.PeakJobMemoryUsed)
    }

    pub(super) fn memory_usage(&self) -> io::Result<ProcessMemory> {
        let mut counters = PROCESS_MEMORY_COUNTERS_EX {
            cb: win_size::<PROCESS_MEMORY_COUNTERS_EX>(),
            ..Default::default()
        };
        // SAFETY: EX extends the documented base counters structure; the pointer
        // is aligned, writable and accompanied by its full extended byte size.
        check(unsafe {
            K32GetProcessMemoryInfo(
                self.process.as_raw_handle(),
                ptr::from_mut(&mut counters).cast(),
                win_size::<PROCESS_MEMORY_COUNTERS_EX>(),
            )
        })?;
        Ok(ProcessMemory {
            private_bytes: counters.PrivateUsage,
            working_set_bytes: counters.WorkingSetSize,
            peak_working_set_bytes: counters.PeakWorkingSetSize,
        })
    }
}

fn process_in_job(process: &OwnedHandle, job: &OwnedHandle) -> io::Result<bool> {
    let mut member = 0;
    // SAFETY: Both uniquely retained handles remain live, and member is writable.
    check(unsafe {
        IsProcessInJob(
            process.as_raw_handle(),
            job.as_raw_handle(),
            &raw mut member,
        )
    })?;
    Ok(member != 0)
}

fn require_mitigation(
    process: &OwnedHandle,
    policy: PROCESS_MITIGATION_POLICY,
    required: u32,
    name: &str,
) -> io::Result<()> {
    // The queried Windows policies all contain one DWORD Flags union. Query the
    // exact aligned ABI size without introducing union field reads.
    let mut flags = 0_u32;
    // SAFETY: This retained process permits queries; the selected policies use
    // one DWORD Flags union, and the exact writable size is supplied.
    check(unsafe {
        GetProcessMitigationPolicy(
            process.as_raw_handle(),
            policy,
            ptr::from_mut(&mut flags).cast(),
            size_of::<u32>(),
        )
    })?;
    if flags & required != required {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            format!(
                "required worker mitigation {name} was not applied (flags={flags:#x}, required={required:#x})"
            ),
        ));
    }
    Ok(())
}

fn resume_owned(
    process: &OwnedHandle,
    thread: &OwnedHandle,
    job: &OwnedHandle,
    mitigation_profile: MitigationProfile,
    native_containment: bool,
) -> io::Result<()> {
    let assigned = !process_in_job(process, job)?;
    if assigned {
        // SAFETY: The retained root was requested suspended and has not been
        // resumed by this owner. The live Job already has its complete limits.
        check(unsafe { AssignProcessToJobObject(job.as_raw_handle(), process.as_raw_handle()) })?;
        if !process_in_job(process, job)? {
            return Err(io::Error::other("worker did not enter its required Job"));
        }
    }
    // Native PSEC's creation broker can omit JOB_LIST. Suspended-root Job
    // assignment above is mandatory; the one-process cap denies child creation
    // for both native and ordinary workers before their first instruction.
    if mitigation_profile == MitigationProfile::Headless {
        require_mitigation(process, ProcessSystemCallDisablePolicy, 0x1, "Win32k")?;
    }
    if mitigation_profile != MitigationProfile::Tool && !native_containment {
        // The PSEC broker can also omit these ordinary creation policies.
        // PSEC uses its native filesystem/UI policy instead; Win32k and
        // verified Job ownership remain mandatory before the first thread.
        require_mitigation(
            process,
            ProcessExtensionPointDisablePolicy,
            0x1,
            "extension points",
        )?;
        require_mitigation(process, ProcessImageLoadPolicy, 0x3, "image loading")?;
    }
    // SAFETY: The retained initial thread was requested suspended. Requiring
    // the returned count to be exactly one detects ignored suspension or extra
    // suspension before accepting a running, fully owned worker.
    match unsafe { ResumeThread(thread.as_raw_handle()) } {
        1 => Ok(()),
        u32::MAX => Err(io::Error::last_os_error()),
        _ => Err(io::Error::other(
            "worker initial suspension was not preserved",
        )),
    }
}

fn terminate_failed_start(process: &OwnedHandle) -> io::Result<()> {
    // SAFETY: This is the exact newly created root, retained against PID reuse.
    let terminated = unsafe { TerminateProcess(process.as_raw_handle(), 1) };
    // SAFETY: Wait only on the same live root handle, with a bounded timeout.
    match unsafe { WaitForSingleObject(process.as_raw_handle(), 10_000) } {
        WAIT_OBJECT_0 => Ok(()),
        WAIT_TIMEOUT => Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "failed worker startup did not terminate",
        )),
        _ if terminated == 0 => Err(io::Error::last_os_error()),
        _ => Err(io::Error::other(
            "failed worker startup exit could not be confirmed",
        )),
    }
}

impl Drop for NativeProcess {
    fn drop(&mut self) {
        if self.containment.is_some() {
            // Retain PSEC while requesting complete tree termination and waiting
            // for the exact root. Closing the Job remains the final kill guard
            // even when a native query/wait fails during best-effort Drop.
            let _ = self.stop(Duration::from_secs(10));
        }
    }
}

fn members_exited(members: &[OwnedHandle]) -> io::Result<bool> {
    for member in members {
        // SAFETY: Each noninheritable synchronization handle remains owned for
        // this zero-timeout wait. No raw ownership is transferred.
        match unsafe { WaitForSingleObject(member.as_raw_handle(), 0) } {
            WAIT_OBJECT_0 => (),
            WAIT_TIMEOUT => return Ok(false),
            _ => return Err(io::Error::last_os_error()),
        }
    }
    Ok(true)
}

pub(super) fn available_memory_bytes() -> io::Result<u64> {
    let mut status = MEMORYSTATUSEX {
        dwLength: win_size::<MEMORYSTATUSEX>(),
        ..Default::default()
    };
    // SAFETY: Structure is initialized with its required exact length and writable.
    check(unsafe { GlobalMemoryStatusEx(&raw mut status) })?;
    Ok(status.ullAvailPhys)
}

pub(super) fn require_exclusive_address(socket: &impl AsSocket) -> io::Result<()> {
    let socket = socket.as_socket();
    let handle = usize::try_from(socket.as_raw_socket())
        .map_err(|_| io::Error::other("socket handle overflow"))?;
    let enabled = 1_i32;
    // SAFETY: AsSocket keeps the live socket borrowed throughout this synchronous
    // call. SOL_SOCKET/SO_EXCLUSIVEADDRUSE requires a readable BOOL-sized integer;
    // its pointer and exact byte count agree. No raw ownership is transferred.
    let result = unsafe {
        setsockopt(
            handle,
            SOL_SOCKET,
            SO_EXCLUSIVEADDRUSE,
            ptr::from_ref(&enabled).cast(),
            i32::try_from(size_of_val(&enabled))
                .map_err(|_| io::Error::other("socket option size overflow"))?,
        )
    };
    if result == SOCKET_ERROR {
        // SAFETY: Retrieve this thread's Winsock error immediately after failure.
        return Err(io::Error::from_raw_os_error(unsafe { WSAGetLastError() }));
    }
    Ok(())
}

pub(super) fn listener_process_id(port: u16) -> io::Result<Option<u32>> {
    let mut bytes = 0;
    // SAFETY: The documented size-query call uses null storage and a valid size
    // pointer; IPv4 and the PID-listener table class are compatible.
    let result = unsafe {
        GetExtendedTcpTable(
            null_mut(),
            &raw mut bytes,
            0,
            u32::from(AF_INET),
            TCP_TABLE_OWNER_PID_LISTENER,
            0,
        )
    };
    if result != ERROR_INSUFFICIENT_BUFFER {
        return Err(table_error(result));
    }
    // Retry boundedly if listeners change between sizing and reading. Bound both
    // bytes and entry counts before reading the variable-length native table.
    for _ in 0..4 {
        if !(4..=16_777_216).contains(&bytes) {
            return Err(io::Error::other("invalid TCP table size"));
        }
        let capacity =
            usize::try_from(bytes).map_err(|_| io::Error::other("TCP table size overflow"))?;
        let mut storage = vec![0_u32; capacity.div_ceil(size_of::<u32>())];
        let mut returned = u32::try_from(size_of_val(storage.as_slice()))
            .map_err(|_| io::Error::other("TCP table size overflow"))?;
        // SAFETY: The aligned allocation is writable for `returned` bytes and
        // stays live; the same IPv4/class query produced the sizing request.
        let result = unsafe {
            GetExtendedTcpTable(
                storage.as_mut_ptr().cast(),
                &raw mut returned,
                0,
                u32::from(AF_INET),
                TCP_TABLE_OWNER_PID_LISTENER,
                0,
            )
        };
        if result == ERROR_INSUFFICIENT_BUFFER {
            bytes = returned;
            continue;
        }
        if result != 0 {
            return Err(table_error(result));
        }
        let returned =
            usize::try_from(returned).map_err(|_| io::Error::other("TCP table size overflow"))?;
        let count = usize::try_from(storage[0])
            .map_err(|_| io::Error::other("TCP entry count overflow"))?;
        if returned > size_of_val(storage.as_slice())
            || returned < 4
            || count > (returned - 4) / size_of::<MIB_TCPROW_OWNER_PID>()
        {
            return Err(io::Error::other("malformed TCP listener table"));
        }
        let mut owner = None;
        for index in 0..count {
            // SAFETY: Count and returned bytes were bounded above against this
            // allocation. read_unaligned avoids relying on native row alignment.
            let row = unsafe {
                ptr::read_unaligned(
                    storage
                        .as_ptr()
                        .cast::<u8>()
                        .wrapping_add(4 + index * size_of::<MIB_TCPROW_OWNER_PID>())
                        .cast::<MIB_TCPROW_OWNER_PID>(),
                )
            };
            let port_bytes = row.dwLocalPort.to_ne_bytes();
            if row.dwLocalAddr.to_ne_bytes() == [127, 0, 0, 1]
                && u16::from_be_bytes([port_bytes[0], port_bytes[1]]) == port
                && owner.replace(row.dwOwningPid).is_some()
            {
                return Err(io::Error::other("ambiguous loopback listener ownership"));
            }
        }
        return Ok(owner);
    }
    Err(io::Error::other("TCP listener table changed repeatedly"))
}

fn table_error(code: u32) -> io::Error {
    i32::try_from(code).map_or_else(
        |_| io::Error::other("invalid TCP query status"),
        io::Error::from_raw_os_error,
    )
}

fn standard_file(
    path: &OsStr,
    access: u32,
    share: u32,
    disposition: u32,
) -> io::Result<OwnedHandle> {
    let name = wide(path);
    let security = SECURITY_ATTRIBUTES {
        nLength: win_size::<SECURITY_ATTRIBUTES>(),
        lpSecurityDescriptor: null_mut(),
        bInheritHandle: 1,
    };
    // SAFETY: Name is terminated, security is readable, and flags request a
    // synchronous file. Only this inheritable handle enters the explicit list.
    owned(unsafe {
        CreateFileW(
            name.as_ptr(),
            access,
            share,
            &raw const security,
            disposition,
            FILE_ATTRIBUTE_NORMAL,
            null_mut(),
        )
    })
}

fn owned(handle: HANDLE) -> io::Result<OwnedHandle> {
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: Only freshly created valid owning handles enter this function; the
    // caller immediately transfers its unique close obligation to OwnedHandle.
    Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
}

fn check(result: i32) -> io::Result<()> {
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn wide(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}

const fn win_size<T>() -> u32 {
    // All callers instantiate fixed Win32 structures, much smaller than u32::MAX.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::as_conversions,
        reason = "Fixed Win32 structure sizes fit in their u32 byte-count fields."
    )]
    {
        size_of::<T>() as u32
    }
}

// Documented PROCESS_CREATION_MITIGATION_POLICY_* ALWAYS_ON values from winnt.h:
// heap termination, Win32k syscall denial, legacy extension-point denial, and
// remote/low-integrity image denial. These headless CPU workers need no desktop
// calls or injected UI extensions. Unlike Job UI limits, creation mitigations
// permit nested Jobs. This does not restrict ordinary file or network access.
// https://learn.microsoft.com/windows/win32/api/processthreadsapi/nf-processthreadsapi-updateprocthreadattribute
const HEADLESS_MITIGATIONS: u64 = (1 << 12) | (1 << 28) | (1 << 32) | (1 << 52) | (1 << 56);

struct CreationAttributes {
    storage: Vec<usize>,
    handles: Box<[HANDLE; 3]>,
    jobs: Box<[HANDLE; 1]>,
    mitigations: Option<Box<u64>>,
    security_environment: Option<Box<HANDLE>>,
    initialized: bool,
}

impl CreationAttributes {
    fn new(
        job: HANDLE,
        handles: [HANDLE; 3],
        mitigations: Option<u64>,
        security_environment: Option<HANDLE>,
    ) -> io::Result<Self> {
        let count =
            2 + u32::from(mitigations.is_some()) + u32::from(security_environment.is_some());
        let mut bytes = 0;
        // SAFETY: Documented null sizing call; bytes points to writable storage.
        let _ = unsafe { InitializeProcThreadAttributeList(null_mut(), count, 0, &raw mut bytes) };
        if bytes == 0 || bytes > 1_048_576 {
            return Err(io::Error::other("invalid process attribute size"));
        }
        let mut value = Self {
            storage: vec![0; bytes.div_ceil(size_of::<usize>())],
            handles: Box::new(handles),
            jobs: Box::new([job]),
            mitigations: mitigations.map(Box::new),
            security_environment: security_environment.map(Box::new),
            initialized: false,
        };
        // SAFETY: Pointer-aligned buffer is writable for at least the requested
        // bytes. No other references alias its native attribute-list contents.
        check(unsafe {
            InitializeProcThreadAttributeList(value.address(), count, 0, &raw mut bytes)
        })?;
        value.initialized = true;
        value.update(
            PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
            value.handles.as_ptr().cast(),
            size_of_val(value.handles.as_ref()),
        )?;
        value.update(
            PROC_THREAD_ATTRIBUTE_JOB_LIST,
            value.jobs.as_ptr().cast(),
            size_of_val(value.jobs.as_ref()),
        )?;
        if let Some(mitigations) = value.mitigations.as_ref() {
            value.update(
                PROC_THREAD_ATTRIBUTE_MITIGATION_POLICY,
                ptr::from_ref(mitigations.as_ref()).cast(),
                size_of_val(mitigations.as_ref()),
            )?;
        }
        if let Some(environment) = value.security_environment.as_ref() {
            // Process security-environment attribute from the pinned MXC ABI.
            // The value and owning PSEC handle stay live through CreateProcessW.
            value.update(
                0x0023 | 0x0002_0000,
                ptr::from_ref(environment.as_ref()).cast(),
                size_of::<HANDLE>(),
            )?;
        }
        Ok(value)
    }

    const fn address(&self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        self.storage.as_ptr().cast_mut().cast()
    }

    fn update(&self, kind: u32, data: *const c_void, bytes: usize) -> io::Result<()> {
        // SAFETY: List is initialized, value buffer has the exact attribute size
        // and persists in a Box until this list is deleted; optional args are null.
        check(unsafe {
            UpdateProcThreadAttribute(
                self.address(),
                0,
                usize::try_from(kind).map_err(|_| io::Error::other("attribute kind overflow"))?,
                data,
                bytes,
                null_mut(),
                null(),
            )
        })
    }
}

impl Drop for CreationAttributes {
    fn drop(&mut self) {
        if self.initialized {
            // SAFETY: This uniquely owned list was successfully initialized and
            // all backing storage/attribute values remain live during deletion.
            unsafe { DeleteProcThreadAttributeList(self.address()) };
        }
    }
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, TcpListener};
    use std::os::windows::io::{FromRawSocket as _, OwnedSocket};

    use windows_sys::Win32::Networking::WinSock::{
        INVALID_SOCKET, IPPROTO_TCP, SOCK_STREAM, getsockopt, socket,
    };

    use super::*;

    #[test]
    fn exclusive_socket_option_is_enabled_on_the_borrowed_unbound_socket() -> io::Result<()> {
        // Let std initialize Winsock without taking responsibility for global
        // WSAStartup/WSACleanup lifetime inside this native test.
        let _initialized = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        // SAFETY: Winsock is initialized; AF_INET/SOCK_STREAM/IPPROTO_TCP agree.
        let raw = unsafe { socket(i32::from(AF_INET), SOCK_STREAM, IPPROTO_TCP) };
        if raw == INVALID_SOCKET {
            // SAFETY: Read the failing socket creation's thread-local error.
            return Err(io::Error::from_raw_os_error(unsafe { WSAGetLastError() }));
        }
        // SAFETY: A successful socket() returned a uniquely owned fresh socket.
        let socket = unsafe {
            OwnedSocket::from_raw_socket(
                u64::try_from(raw).map_err(|_| io::Error::other("socket handle overflow"))?,
            )
        };
        assert_eq!(exclusive_option(&socket)?, 0);
        require_exclusive_address(&socket)?;
        assert_ne!(exclusive_option(&socket)?, 0);
        Ok(())
    }

    fn exclusive_option(socket: &impl AsSocket) -> io::Result<i32> {
        let socket = socket.as_socket();
        let handle = usize::try_from(socket.as_raw_socket())
            .map_err(|_| io::Error::other("socket handle overflow"))?;
        let mut enabled = 0_i32;
        let mut bytes = i32::try_from(size_of_val(&enabled))
            .map_err(|_| io::Error::other("socket option size overflow"))?;
        // SAFETY: Socket stays borrowed; the writable integer buffer and its
        // byte count match the documented SO_EXCLUSIVEADDRUSE BOOL result.
        let result = unsafe {
            getsockopt(
                handle,
                SOL_SOCKET,
                SO_EXCLUSIVEADDRUSE,
                ptr::from_mut(&mut enabled).cast(),
                &raw mut bytes,
            )
        };
        if result == SOCKET_ERROR {
            // SAFETY: Read the failing getsockopt call's thread-local error.
            return Err(io::Error::from_raw_os_error(unsafe { WSAGetLastError() }));
        }
        Ok(enabled)
    }
}
