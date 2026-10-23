#nullable enable
using System;
using System.ComponentModel;
using System.Diagnostics;
using System.IO;
using System.Runtime.InteropServices;
using System.Runtime.Versioning;
using System.Text;
using System.Threading;
using Microsoft.Win32.SafeHandles;

namespace SwiftBonsai {
// Native contracts reviewed against Microsoft's CreateProcessW,
// UpdateProcThreadAttribute, JOBOBJECT_EXTENDED_LIMIT_INFORMATION and
// JOBOBJECT_BASIC_LIMIT_INFORMATION documentation. This controls owned process
// lifetime and committed memory, not filesystem access, network access or tokens.
/// <summary>Owns a process tree in a Windows Job with enforced resource limits.</summary>
[SupportedOSPlatform("windows10.0")]
public sealed class OwnedProcess : IDisposable {
    private const uint LimitFlags = 0x2000 | 0x400 | 0x200 | 0x8; // kill on close, no crash dialog, job commit, process count
    private readonly Lock lifetime = new();
    private readonly NativeHandle job;
    private readonly NativeHandle rootHandle;
    private readonly Process rootProcess;
    private bool disposed;

    private OwnedProcess(NativeHandle job, NativeHandle rootHandle, Process process) {
        this.job = job; this.rootHandle = rootHandle; rootProcess = process;
    }

    /// <summary>Gets the root process for waiting and current-memory observations.</summary>
    public Process Process => rootProcess;

    /// <summary>Gets the observed peak job-wide committed bytes, including descendants.</summary>
    /// <returns>The Windows native accounting counter, not physical memory acceptance evidence.</returns>
    public ulong GetPeakJobMemoryBytes() {
        lock (lifetime) {
            ObjectDisposedException.ThrowIf(disposed, this);
            if (!Native.QueryLimits(job, 9, out ExtendedLimits limits, (uint)Marshal.SizeOf<ExtendedLimits>(), IntPtr.Zero))
                throw Error("Query job memory");
            return limits.PeakJobMemoryUsed.ToUInt64();
        }
    }

    /// <summary>Starts suspended, atomically assigns a Job, then resumes the root.</summary>
    /// <param name="executable">Expanded absolute executable path.</param>
    /// <param name="arguments">Arguments passed using Windows argv escaping, without a shell.</param>
    /// <param name="workingDirectory">Expanded absolute external native working directory.</param>
    /// <param name="stdoutPath">Expanded absolute path to a new standard output evidence file.</param>
    /// <param name="stderrPath">Expanded absolute path to a new standard error evidence file.</param>
    /// <param name="jobMemoryBytes">Maximum job-wide committed memory, in bytes.</param>
    /// <param name="activeProcessLimit">Maximum simultaneously active processes, including the root.</param>
    /// <returns>The owned process and its noninheritable Job handle.</returns>
    public static OwnedProcess Start(string executable, string[] arguments, string workingDirectory,
        string stdoutPath, string stderrPath, ulong jobMemoryBytes, uint activeProcessLimit) {
        ArgumentNullException.ThrowIfNull(arguments);
        AbsolutePath(executable); AbsolutePath(workingDirectory); AbsolutePath(stdoutPath); AbsolutePath(stderrPath);
        if (!File.Exists(executable)) throw new FileNotFoundException("The executable does not exist.", executable);
        if (!Directory.Exists(workingDirectory)) throw new DirectoryNotFoundException(workingDirectory);
        ArgumentOutOfRangeException.ThrowIfZero(jobMemoryBytes);
        ArgumentOutOfRangeException.ThrowIfZero(activeProcessLimit);
        var command = new StringBuilder(Quote(executable));
        foreach (string argument in arguments) { ArgumentNullException.ThrowIfNull(argument); command.Append(' ').Append(Quote(argument)); }
        if (command.Length >= 32767) throw new ArgumentException("The Windows command line is too long.", nameof(arguments));

        NativeHandle? job = null;
        NativeHandle? nativeProcess = null;
        Process? process = null;
        try {
            job = Native.CreateJob(IntPtr.Zero, null);
            if (job.IsInvalid) throw Error("Create job");
            var limits = new ExtendedLimits {
                BasicLimitInformation = new BasicLimits { LimitFlags = LimitFlags, ActiveProcessLimit = activeProcessLimit },
                JobMemoryLimit = new UIntPtr(jobMemoryBytes)
            };
            if (!Native.SetLimits(job, 9, ref limits, (uint)Marshal.SizeOf<ExtendedLimits>())) throw Error("Set job limits");
            using SafeFileHandle input = OpenStandardFile("NUL", 0x80000000, 3, 3);
            // Readers can inspect active evidence, but another writer must not
            // overwrite it while the inherited stream handle is still open.
            using SafeFileHandle output = OpenStandardFile(stdoutPath, 0x40000000, 1, 1);
            using SafeFileHandle error = OpenStandardFile(stderrPath, 0x40000000, 1, 1);
            using var attributes = new CreationAttributes(job, input, output, error);
            var startup = new ExtendedStartupInfo {
                StartupInfo = new StartupInfo {
                    Size = (uint)Marshal.SizeOf<ExtendedStartupInfo>(), Flags = 0x100 | 0x1,
                    StandardInput = input.DangerousGetHandle(), StandardOutput = output.DangerousGetHandle(),
                    StandardError = error.DangerousGetHandle(), ShowWindow = 0
                },
                AttributeList = attributes.Address
            };
            // A null environment preserves every inherited OS/profile variable.
            // The Job-list attribute assigns before any child code can execute,
            // even if this owner dies before CreateProcessW returns. No breakaway
            // flag is enabled in either the Job or process creation flags.
            char[] commandBuffer = (command.ToString() + '\0').ToCharArray();
            if (!Native.CreateProcess(executable, commandBuffer, IntPtr.Zero, IntPtr.Zero, true,
                0x4 | 0x80000 | 0x8000000, IntPtr.Zero, workingDirectory, ref startup, out ProcessInformation info))
                throw Error("Create suspended job-owned process");
            nativeProcess = new NativeHandle(info.Process);
            using var thread = new NativeHandle(info.Thread);
            process = Process.GetProcessById(checked((int)info.ProcessId));
            // Cache a process handle while the native creation handle is alive.
            _ = process.Handle;
            if (Native.ResumeThread(thread) == uint.MaxValue) throw Error("Resume job-owned process");
            var owned = new OwnedProcess(job, nativeProcess, process);
            job = null; nativeProcess = null; process = null; // ownership transferred only after successful resume
            return owned;
        } finally {
            // Closing the last Job handle kills the entire tree, including a
            // suspended root when any post-creation setup operation fails.
            job?.Dispose();
            nativeProcess?.Dispose();
            process?.Dispose();
        }
    }

    /// <summary>Terminates all owned processes and waits for the job to become empty.</summary>
    /// <param name="timeoutMilliseconds">Positive bounded cleanup timeout.</param>
    public void Stop(int timeoutMilliseconds) {
        ArgumentOutOfRangeException.ThrowIfNegativeOrZero(timeoutMilliseconds);
        var timer = Stopwatch.StartNew();
        if (!lifetime.TryEnter(timeoutMilliseconds)) throw new TimeoutException("Timed out waiting for another owned process operation.");
        try {
            ObjectDisposedException.ThrowIf(disposed, this);
            if (timer.ElapsedMilliseconds >= timeoutMilliseconds) throw new TimeoutException("The owned process cleanup deadline expired.");
            if (!Native.TerminateJob(job, 1)) throw Error("Terminate owned job");
            while (true) {
                if (!Native.QueryAccounting(job, 1, out Accounting info, (uint)Marshal.SizeOf<Accounting>(), IntPtr.Zero))
                    throw Error("Query owned job process count");
                // Job accounting can reach zero before the root process is
                // signaled. Retain the creation handle for this completion check
                // independently of the publicly exposed Process component.
                if (info.ActiveProcesses == 0) {
                    uint wait = Native.WaitForSingleObject(rootHandle, 0);
                    if (wait == 0) return;
                    if (wait == uint.MaxValue) throw Error("Wait for owned root process");
                    if (wait != 258) throw new InvalidOperationException("Unexpected owned process wait result.");
                }
                long remaining = timeoutMilliseconds - timer.ElapsedMilliseconds;
                if (remaining <= 0) throw new TimeoutException("The owned job did not complete termination after cancellation.");
                Thread.Sleep((int)Math.Min(10, remaining));
            }
        } finally { lifetime.Exit(); }
    }

    /// <summary>Closes the final Job handle, killing every remaining owned process.</summary>
    public void Dispose() {
        // Keep Dispose from closing handles between a native operation and its
        // completion check. SafeHandle marshaling protects one call at a time.
        lock (lifetime) {
            if (disposed) return;
            disposed = true;
            job.Dispose();
            rootHandle.Dispose();
        }
        // Process.Close takes its own monitor. An Exited callback can hold that
        // monitor while reentering this owner, so never acquire it under lifetime.
        rootProcess.Dispose();
    }

    private static Win32Exception Error(string operation) { return new Win32Exception(Marshal.GetLastWin32Error(), operation); }
    private static void AbsolutePath(string path) {
        ArgumentException.ThrowIfNullOrWhiteSpace(path);
        if (!Path.IsPathFullyQualified(path) || path.Contains('%', StringComparison.Ordinal) || path.Contains('\0', StringComparison.Ordinal))
            throw new ArgumentException("Native process paths must be expanded, absolute and contain no NUL.", nameof(path));
    }
    private static string Quote(string value) {
        ArgumentNullException.ThrowIfNull(value);
        if (value.Contains('\0', StringComparison.Ordinal)) throw new ArgumentException("Arguments cannot contain NUL.", nameof(value));
        var text = new StringBuilder("\"");
        int slashes = 0;
        foreach (char character in value) {
            if (character == '\\') { slashes++; continue; }
            if (character == '"') text.Append('\\', checked(slashes * 2 + 1)); else text.Append('\\', slashes);
            text.Append(character); slashes = 0;
        }
        return text.Append('\\', checked(slashes * 2)).Append('"').ToString();
    }
    private static SafeFileHandle OpenStandardFile(string path, uint access, uint share, uint disposition) {
        var security = new SecurityAttributes { Length = (uint)Marshal.SizeOf<SecurityAttributes>(), InheritHandle = 1 };
        SafeFileHandle handle = Native.CreateFile(path, access, share, ref security, disposition, 0x80, IntPtr.Zero);
        if (!handle.IsInvalid) return handle;
        Win32Exception failure = Error("Open process standard stream");
        handle.Dispose(); throw failure;
    }

    private sealed class NativeHandle : SafeHandleZeroOrMinusOneIsInvalid {
        private NativeHandle() : base(true) { }
        internal NativeHandle(IntPtr value) : base(true) { SetHandle(value); }
        protected override bool ReleaseHandle() { return Native.CloseHandle(handle); }
    }

    private sealed class CreationAttributes : IDisposable {
        private readonly IntPtr list;
        private readonly IntPtr handles;
        private readonly IntPtr jobs;
        private bool initialized;
        private bool disposed;
        internal IntPtr Address => list;
        internal CreationAttributes(NativeHandle job, SafeFileHandle input, SafeFileHandle output, SafeFileHandle error) {
            ArgumentNullException.ThrowIfNull(job); ArgumentNullException.ThrowIfNull(input);
            ArgumentNullException.ThrowIfNull(output); ArgumentNullException.ThrowIfNull(error);
            UIntPtr size = UIntPtr.Zero;
            _ = Native.InitializeAttributes(IntPtr.Zero, 2, 0, ref size);
            if (size == UIntPtr.Zero) throw Error("Size process creation attributes");
            try {
                list = Marshal.AllocHGlobal(checked((nint)size.ToUInt64()));
                handles = Marshal.AllocHGlobal(checked(3 * IntPtr.Size));
                jobs = Marshal.AllocHGlobal(IntPtr.Size);
                if (!Native.InitializeAttributes(list, 2, 0, ref size)) throw Error("Initialize process creation attributes");
                initialized = true;
                Marshal.WriteIntPtr(handles, 0, input.DangerousGetHandle());
                Marshal.WriteIntPtr(handles, IntPtr.Size, output.DangerousGetHandle());
                Marshal.WriteIntPtr(handles, 2 * IntPtr.Size, error.DangerousGetHandle());
                Marshal.WriteIntPtr(jobs, job.DangerousGetHandle());
                if (!Native.UpdateAttribute(list, 0, new UIntPtr(0x20002), handles, new UIntPtr((uint)(3 * IntPtr.Size)), IntPtr.Zero, IntPtr.Zero))
                    throw Error("Set explicit inherited standard handles");
                if (!Native.UpdateAttribute(list, 0, new UIntPtr(0x2000D), jobs, new UIntPtr((uint)IntPtr.Size), IntPtr.Zero, IntPtr.Zero))
                    throw Error("Set atomic process job assignment");
            } catch { Dispose(); throw; }
        }
        public void Dispose() {
            if (disposed) return;
            disposed = true;
            if (initialized) { Native.DeleteAttributes(list); initialized = false; }
            Marshal.FreeHGlobal(jobs); Marshal.FreeHGlobal(handles); Marshal.FreeHGlobal(list);
        }
    }

    [StructLayout(LayoutKind.Sequential)]
    private record struct SecurityAttributes { public uint Length; public IntPtr Descriptor; public int InheritHandle; }
    [StructLayout(LayoutKind.Sequential)]
    private record struct BasicLimits {
        public long PerProcessUserTimeLimit, PerJobUserTimeLimit;
        public uint LimitFlags;
        public UIntPtr MinimumWorkingSetSize, MaximumWorkingSetSize;
        public uint ActiveProcessLimit;
        public UIntPtr Affinity;
        public uint PriorityClass, SchedulingClass;
    }
    [StructLayout(LayoutKind.Sequential)]
    private record struct IoCounters { public ulong ReadOperationCount, WriteOperationCount, OtherOperationCount, ReadTransferCount, WriteTransferCount, OtherTransferCount; }
    [StructLayout(LayoutKind.Sequential)]
    private record struct ExtendedLimits {
        public BasicLimits BasicLimitInformation;
        public IoCounters IoInfo;
        public UIntPtr ProcessMemoryLimit, JobMemoryLimit, PeakProcessMemoryUsed, PeakJobMemoryUsed;
    }
    [StructLayout(LayoutKind.Sequential)]
    private record struct Accounting {
        public long TotalUserTime, TotalKernelTime, ThisPeriodTotalUserTime, ThisPeriodTotalKernelTime;
        public uint TotalPageFaultCount, TotalProcesses, ActiveProcesses, TotalTerminatedProcesses;
    }
    [StructLayout(LayoutKind.Sequential)]
    private record struct StartupInfo {
        public uint Size;
        public IntPtr Reserved, Desktop, Title;
        public uint X, Y, XSize, YSize, XCountChars, YCountChars, FillAttribute, Flags;
        public ushort ShowWindow, ReservedSize;
        public IntPtr ReservedBytes, StandardInput, StandardOutput, StandardError;
    }
    [StructLayout(LayoutKind.Sequential)]
    private record struct ExtendedStartupInfo { public StartupInfo StartupInfo; public IntPtr AttributeList; }
    [StructLayout(LayoutKind.Sequential)]
    private record struct ProcessInformation { public IntPtr Process, Thread; public uint ProcessId, ThreadId; }

    private static class Native {
        [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
        [DllImport("kernel32.dll", EntryPoint="CreateJobObjectW", CharSet=CharSet.Unicode, ExactSpelling=true, SetLastError=true)]
        internal static extern NativeHandle CreateJob(IntPtr attributes, string? name);
        [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
        [DllImport("kernel32.dll", EntryPoint="SetInformationJobObject", ExactSpelling=true, SetLastError=true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        internal static extern bool SetLimits(NativeHandle job, int informationClass, ref ExtendedLimits limits, uint length);
        [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
        [DllImport("kernel32.dll", EntryPoint="QueryInformationJobObject", ExactSpelling=true, SetLastError=true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        internal static extern bool QueryLimits(NativeHandle job, int informationClass, out ExtendedLimits limits, uint length, IntPtr returnedLength);
        [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
        [DllImport("kernel32.dll", EntryPoint="QueryInformationJobObject", ExactSpelling=true, SetLastError=true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        internal static extern bool QueryAccounting(NativeHandle job, int informationClass, out Accounting info, uint length, IntPtr returnedLength);
        [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
        [DllImport("kernel32.dll", EntryPoint="TerminateJobObject", ExactSpelling=true, SetLastError=true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        internal static extern bool TerminateJob(NativeHandle job, uint exitCode);
        [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
        [DllImport("kernel32.dll", EntryPoint="CreateFileW", CharSet=CharSet.Unicode, ExactSpelling=true, SetLastError=true)]
        internal static extern SafeFileHandle CreateFile(string path, uint access, uint share, ref SecurityAttributes security, uint disposition, uint flags, IntPtr template);
        [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
        [DllImport("kernel32.dll", EntryPoint="InitializeProcThreadAttributeList", ExactSpelling=true, SetLastError=true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        internal static extern bool InitializeAttributes(IntPtr list, uint count, uint flags, ref UIntPtr size);
        [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
        [DllImport("kernel32.dll", EntryPoint="UpdateProcThreadAttribute", ExactSpelling=true, SetLastError=true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        internal static extern bool UpdateAttribute(IntPtr list, uint flags, UIntPtr attribute, IntPtr value, UIntPtr size, IntPtr previous, IntPtr returnedSize);
        [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
        [DllImport("kernel32.dll", EntryPoint="DeleteProcThreadAttributeList", ExactSpelling=true)]
        internal static extern void DeleteAttributes(IntPtr list);
        [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
        [DllImport("kernel32.dll", EntryPoint="CreateProcessW", CharSet=CharSet.Unicode, ExactSpelling=true, SetLastError=true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        internal static extern bool CreateProcess(string application, [In, Out, MarshalAs(UnmanagedType.LPArray, ArraySubType=UnmanagedType.U2)] char[] command, IntPtr processAttributes, IntPtr threadAttributes,
            [MarshalAs(UnmanagedType.Bool)] bool inheritHandles, uint flags, IntPtr environment, string directory, ref ExtendedStartupInfo startup, out ProcessInformation info);
        [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
        [DllImport("kernel32.dll", EntryPoint="ResumeThread", ExactSpelling=true, SetLastError=true)]
        internal static extern uint ResumeThread(NativeHandle thread);
        [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
        [DllImport("kernel32.dll", EntryPoint="WaitForSingleObject", ExactSpelling=true, SetLastError=true)]
        internal static extern uint WaitForSingleObject(NativeHandle handle, uint milliseconds);
        [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
        [DllImport("kernel32.dll", EntryPoint="CloseHandle", ExactSpelling=true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        internal static extern bool CloseHandle(IntPtr handle);
    }
}
}
