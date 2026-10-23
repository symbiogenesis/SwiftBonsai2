#nullable enable
using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.Diagnostics;
using System.Diagnostics.CodeAnalysis;
using System.Globalization;
using System.IO;
using System.Runtime.InteropServices;
using System.Runtime.Versioning;
using System.Text.Json;
using System.Threading;

namespace SwiftBonsai.Validation {
[SupportedOSPlatform("windows10.0")]
internal static class ProcessLifecycleProbe {
    [SuppressMessage("Globalization", "CA1303:Do not pass literals as localized parameters", Justification = "Stable machine-readable synthetic test evidence, not localized user-facing prose.")]
    private static int Main(string[] arguments) {
        ArgumentNullException.ThrowIfNull(arguments);
        switch (arguments[0]) {
            case "echo":
                Console.Write(JsonSerializer.Serialize(new Dictionary<string, object> {
                    { "Arguments", arguments[1..] }, { "WorkingDirectory", Environment.CurrentDirectory },
                    { "SystemRoot", Environment.GetEnvironmentVariable("SystemRoot") ?? string.Empty },
                    { "UserProfile", Environment.GetEnvironmentVariable("USERPROFILE") ?? string.Empty },
                    { "InputClosed", Console.Read() == -1 }
                }));
                Console.Error.Write("stderr-evidence");
                return 0;
            case "wait":
                File.WriteAllText(arguments[1], Environment.ProcessId.ToString(CultureInfo.InvariantCulture));
                Thread.Sleep(Timeout.Infinite);
                return 0;
            case "callback-lifetime":
                return VerifyCallbackLifetime(arguments[1]);
            case "crash":
                ThrowNativeException(0xe0424242, 1, 0, IntPtr.Zero);
                return 20;
            case "error-policy":
                using (Process child = StartSelf("crash", arguments[1])) {
                    if (!child.WaitForExit(10000)) { child.Kill(); child.WaitForExit(); return 17; }
                    File.WriteAllText(arguments[1] + ".child", child.ExitCode.ToString(CultureInfo.InvariantCulture));
                    return child.ExitCode == 0 ? 21 : 0;
                }
            case "error-policy-owner":
                // Only this disposable owner changes its process-wide mode. The
                // Job must handle root and descendant crashes without changing
                // this owner's error mode or waiting for interactive dialogs.
                _ = SetErrorMode(0);
                File.WriteAllText(arguments[1] + ".before", GetErrorMode().ToString(CultureInfo.InvariantCulture));
                foreach (string mode in new[] { "crash", "error-policy" }) {
                    using OwnedProcess owned = OwnedProcess.Start(Environment.ProcessPath!, [mode, arguments[1]], Environment.CurrentDirectory,
                        arguments[1] + "." + mode + ".stdout", arguments[1] + "." + mode + ".stderr", 512UL * 1024 * 1024, 2);
                    if (!owned.Process.WaitForExit(10000)) { owned.Stop(10000); return 19; }
                    if (mode == "crash") File.WriteAllText(arguments[1] + ".root", owned.Process.ExitCode.ToString(CultureInfo.InvariantCulture));
                    else if (owned.Process.ExitCode != 0) return owned.Process.ExitCode;
                }
                File.WriteAllText(arguments[1] + ".after", GetErrorMode().ToString(CultureInfo.InvariantCulture));
                return 0;
            case "tree":
                using (Process child = StartSelf("wait", arguments[1] + ".child")) {
                    File.WriteAllText(arguments[1], string.Create(CultureInfo.InvariantCulture, $"{Environment.ProcessId},{child.Id}"));
                    Thread.Sleep(Timeout.Infinite);
                }
                return 0;
            case "owner":
                using (OwnedProcess owned = OwnedProcess.Start(Environment.ProcessPath!, ["tree", arguments[1]], Environment.CurrentDirectory,
                    arguments[1] + ".stdout", arguments[1] + ".stderr", 512UL * 1024 * 1024, 2)) {
                    Thread.Sleep(Timeout.Infinite);
                }
                return 0;
            case "spawn":
                try {
                    using Process child = StartSelf("wait", arguments[1]);
                    if (!child.WaitForExit(2000)) { child.Kill(); child.WaitForExit(); }
                } catch (Win32Exception) { }
                Console.Write("spawn-attempted");
                return 0;
            case "hold-memory":
                IntPtr held = Allocate(IntPtr.Zero, new UIntPtr(300U * 1024 * 1024), 0x3000, 4);
                File.WriteAllText(arguments[1], held == IntPtr.Zero ? "failed" : "ready");
                if (held == IntPtr.Zero) return 13;
                try { Thread.Sleep(Timeout.Infinite); }
                finally { _ = Free(held, UIntPtr.Zero, 0x8000); }
                return 0;
            case "aggregate":
                using (Process child = StartSelf("hold-memory", arguments[1])) {
                    var timer = Stopwatch.StartNew();
                    while (!File.Exists(arguments[1]) || new FileInfo(arguments[1]).Length == 0) {
                        if (timer.ElapsedMilliseconds >= 10000) return 14;
                        Thread.Sleep(10);
                    }
                    if (!string.Equals(File.ReadAllText(arguments[1]), "ready", StringComparison.Ordinal)) return 15;
                    IntPtr additional = Allocate(IntPtr.Zero, new UIntPtr(300U * 1024 * 1024), 0x3000, 4);
                    if (additional != IntPtr.Zero) { _ = Free(additional, UIntPtr.Zero, 0x8000); return 16; }
                    Console.Write("aggregate-commit-denied");
                    return 0;
                }
            case "memory":
                // Commit rather than just reserve: the job commit limit applies
                // independently of whether these private pages are touched.
                IntPtr small = Allocate(IntPtr.Zero, new UIntPtr(16U * 1024 * 1024), 0x3000, 4);
                if (small == IntPtr.Zero) return 10;
                try {
                    IntPtr large = Allocate(IntPtr.Zero, new UIntPtr(512U * 1024 * 1024), 0x3000, 4);
                    if (large != IntPtr.Zero) { _ = Free(large, UIntPtr.Zero, 0x8000); return 11; }
                    Console.Write("commit-denied");
                    return 0;
                } finally { _ = Free(small, UIntPtr.Zero, 0x8000); }
            default: return 12;
        }
    }

    private static Process StartSelf(string mode, string marker) {
        var start = new ProcessStartInfo(Environment.ProcessPath!) {
            WorkingDirectory = Environment.CurrentDirectory, UseShellExecute = false, CreateNoWindow = true
        };
        start.ArgumentList.Add(mode); start.ArgumentList.Add(marker);
        return Process.Start(start) ?? throw new InvalidOperationException("The synthetic child did not start.");
    }

    private static int VerifyCallbackLifetime(string marker) {
        using OwnedProcess owned = OwnedProcess.Start(Environment.ProcessPath!, ["wait", marker], Environment.CurrentDirectory,
            marker + ".inner.stdout", marker + ".inner.stderr", 512UL * 1024 * 1024, 1);
        using var callbackFinished = new ManualResetEventSlim();
        var disposer = new Thread(owned.Dispose) { IsBackground = true };
        var callback = new CallbackState();
        owned.Process.Exited += (_, _) => {
            try {
                // Process's completion callback holds its monitor. A concurrent
                // owner Dispose must release its own lock before Process.Close;
                // this reentry must observe disposal rather than time out on it.
                disposer.Start();
                var timer = Stopwatch.StartNew();
                while (true) {
                    try { owned.Stop(50); }
                    catch (ObjectDisposedException) { return; }
                    catch (TimeoutException exception) { callback.Failure = exception; return; }
                    catch (Win32Exception exception) { callback.Failure = exception; return; }
                    catch (InvalidOperationException exception) { callback.Failure = exception; return; }
                    if (timer.ElapsedMilliseconds >= 3000) {
                        callback.Failure = new TimeoutException("Concurrent owner disposal did not become visible.");
                        return;
                    }
                    _ = Thread.Yield();
                }
            } finally { callbackFinished.Set(); }
        };
        owned.Process.EnableRaisingEvents = true;
        owned.Process.Kill();
        if (!callbackFinished.Wait(10000)) throw new TimeoutException("The exit callback did not complete.");
        if (!disposer.Join(10000)) throw new TimeoutException("Concurrent owner disposal did not complete.");
        if (callback.Failure is Exception failure) throw new InvalidOperationException("The exit callback could not reenter the disposing owner.", failure);
        return 0;
    }

    private sealed class CallbackState {
        internal Exception? Failure { get; set; }
    }

    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    [DllImport("kernel32.dll", EntryPoint="VirtualAlloc", ExactSpelling=true, SetLastError=true)]
    private static extern IntPtr Allocate(IntPtr address, UIntPtr size, uint allocationType, uint protection);
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    [DllImport("kernel32.dll", EntryPoint="VirtualFree", ExactSpelling=true, SetLastError=true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool Free(IntPtr address, UIntPtr size, uint freeType);
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    [DllImport("kernel32.dll", EntryPoint="GetErrorMode", ExactSpelling=true)]
    private static extern uint GetErrorMode();
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    [DllImport("kernel32.dll", EntryPoint="SetErrorMode", ExactSpelling=true)]
    private static extern uint SetErrorMode(uint mode);
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    [DllImport("kernel32.dll", EntryPoint="RaiseException", ExactSpelling=true)]
    private static extern void ThrowNativeException(uint code, uint flags, uint count, IntPtr arguments);
}
}
