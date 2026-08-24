using System.Diagnostics;
using System.Globalization;
using System.Runtime.InteropServices;
using Microsoft.Diagnostics.NETCore.Client;

namespace CodeBasics.Inspector;

/// <summary>
/// Listing the .NET processes on this machine.
///
/// The authority is <see cref="DiagnosticsClient.GetPublishedProcesses"/>: the
/// pids that have published a diagnostics IPC channel, which is exactly the set
/// ClrMD can attach to. Anything richer — an executable path, a start time, a
/// parent — is read separately and may legitimately fail (a process of a
/// different bitness or a higher elevation refuses to describe itself). Every
/// one of those is omitted on failure rather than approximated: code-basics
/// attributes a process to a run configuration by its parent chain, so a
/// guessed parent would confidently point at the wrong application.
///
/// A row survives with nothing but a pid and a name. That is still useful —
/// someone can recognise "Crasher" — whereas a fabricated path is not. A row
/// survives with nothing but a pid, too: the pid is the thing that can be
/// attached to, and a process omitted because Win32 refused to describe it is a
/// process code-basics then explains away with a confident claim about a
/// different one. Every such row carries a warning saying what was refused.
/// </summary>
internal static class Processes
{
    public static ProcessListDto List()
    {
        var result = new ProcessListDto();

        int[] published;
        try
        {
            published = [.. DiagnosticsClient.GetPublishedProcesses()];
        }
        catch (Exception e)
        {
            result.Warnings.Add($"the .NET processes on this machine could not be enumerated: {e.Message}");
            return result;
        }

        // One snapshot for the whole listing, not one per process: this is
        // polled by the UI, and the per-process form would be O(n) walks of
        // the entire process table.
        var parents = ParentMap(result.Warnings);

        // Likewise one command-line snapshot for the whole listing. On Windows
        // that is a single WMI query rather than one per process, which would
        // be far heavier on a polled path.
        var commandLines = CommandLineMap(result.Warnings);

        foreach (var pid in published)
        {
            // The inspector is itself a .NET process and so appears in its own
            // listing, but it exits within milliseconds of writing this file.
            // Offering it would be a row that is stale before it is read.
            if (pid == Environment.ProcessId)
            {
                continue;
            }

            result.Processes.Add(Describe(pid, parents, commandLines, result.Warnings));
        }

        return result;
    }

    /// <summary>
    /// The unnamed row's name. Not a guess at what the process is called — a
    /// statement that it was not readable, which is what the reader needs in
    /// order to decide whether to attach to it.
    /// </summary>
    private const string UnknownName = "unknown";

    /// <summary>
    /// Everything that could be read about one published pid.
    ///
    /// Never returns null. The pid came from
    /// <see cref="DiagnosticsClient.GetPublishedProcesses"/>, so it *is*
    /// attachable, and dropping it because Win32 would not describe it — which
    /// happens for a process in another session or at a higher elevation — is
    /// worse than listing it thinly. Two things went wrong when it was dropped:
    /// code-basics refused a capture of that pid on the grounds that it had
    /// exited, and, if the row was the child of a `dotnet run`, its launcher was
    /// described as "still building" when the application was up and had
    /// published a channel. Both were confident claims produced by a silent
    /// omission.
    /// </summary>
    private static ProcessDto Describe(
        int pid,
        IReadOnlyDictionary<int, int>? parents,
        IReadOnlyDictionary<int, string>? commandLines,
        List<string> warnings)
    {
        var row = new ProcessDto { Pid = pid, Name = UnknownName };

        // The parent comes from the process-table snapshot rather than from the
        // handle, so it survives a process that will not describe itself.
        if (parents is not null && parents.TryGetValue(pid, out var parentPid))
        {
            row.ParentPid = parentPid;
        }

        // The command line comes from the same one-shot snapshot. Absent for a
        // process across an elevation or session boundary, where it is omitted
        // rather than guessed — its only use is preselection.
        if (commandLines is not null && commandLines.TryGetValue(pid, out var commandLine))
        {
            row.CommandLine = commandLine;
        }

        Process handle;
        try
        {
            handle = Process.GetProcessById(pid);
        }
        catch (Exception e)
        {
            // Either it exited between the publish list and here, or it will
            // not identify itself at all.
            warnings.Add(
                $"process {pid} has published a diagnostics channel but could not be described " +
                $"({e.Message}); it is listed by pid alone");
            return row;
        }

        // Disposed promptly: this runs on a poll, and a leaked handle per
        // process per tick is a slow leak in a long-lived session.
        using var process = handle;

        try
        {
            row.Name = process.ProcessName;
        }
        catch (Exception e)
        {
            warnings.Add(
                $"process {pid} has published a diagnostics channel but would not give its name " +
                $"({e.Message}); it is listed by pid alone");
        }

        try
        {
            row.Path = process.MainModule?.FileName;
        }
        catch (Exception)
        {
            // Reading the main module throws across a bitness or elevation
            // boundary. Unknown, so absent.
        }

        try
        {
            row.StartedAt = process.StartTime.ToUniversalTime()
                .ToString("O", CultureInfo.InvariantCulture);
        }
        catch (Exception)
        {
            // Same story: denied for some processes, and never worth inventing.
        }

        return row;
    }

    /// <summary>
    /// Every pid's parent, or null when the platform will not say. Null is
    /// propagated all the way out as an omitted field — attribution abstains
    /// rather than guessing at a lineage.
    /// </summary>
    private static IReadOnlyDictionary<int, int>? ParentMap(List<string> warnings)
    {
        try
        {
            if (OperatingSystem.IsWindows())
            {
                return WindowsParents();
            }
            return ProcFsParents();
        }
        catch (Exception e)
        {
            warnings.Add(
                "the parent of each process could not be read, so processes started by " +
                $"code-basics cannot be told apart from the rest: {e.Message}");
            return null;
        }
    }

    /// <summary>
    /// Every pid's command line, or null when the platform will not say. Null is
    /// propagated out as an omitted field: the command line only lets a
    /// `dotnet run` child be preselected over the SDK's own build tools, so its
    /// absence costs preselection and never correctness. A total failure is
    /// warned about once, here; a single unreadable process is simply absent
    /// from the map.
    /// </summary>
    private static IReadOnlyDictionary<int, string>? CommandLineMap(List<string> warnings)
    {
        try
        {
            if (OperatingSystem.IsWindows())
            {
                return WindowsCommandLines();
            }
            return ProcFsCommandLines();
        }
        catch (Exception e)
        {
            warnings.Add(
                "the command line of each process could not be read, so a `dotnet run` child " +
                $"cannot be preselected over the .NET SDK's own build tools: {e.Message}");
            return null;
        }
    }

    // -- Windows ------------------------------------------------------------

    private const uint TH32CS_SNAPPROCESS = 0x00000002;
    private static readonly IntPtr InvalidHandle = new(-1);

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    private struct PROCESSENTRY32
    {
        public uint dwSize;
        public uint cntUsage;
        public uint th32ProcessID;
        public IntPtr th32DefaultHeapID;
        public uint th32ModuleID;
        public uint cntThreads;
        public uint th32ParentProcessID;
        public int pcPriClassBase;
        public uint dwFlags;

        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 260)]
        public string szExeFile;
    }

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern IntPtr CreateToolhelp32Snapshot(uint flags, uint processId);

    [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool Process32First(IntPtr snapshot, ref PROCESSENTRY32 entry);

    [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool Process32Next(IntPtr snapshot, ref PROCESSENTRY32 entry);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool CloseHandle(IntPtr handle);

    /// <summary>
    /// The whole process table in one ToolHelp32 snapshot.
    ///
    /// There is no managed API for a parent pid, and the alternatives (WMI, a
    /// per-process NtQueryInformationProcess) cost far more per poll than one
    /// pass over this snapshot.
    /// </summary>
    private static IReadOnlyDictionary<int, int>? WindowsParents()
    {
        var snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if (snapshot == InvalidHandle || snapshot == IntPtr.Zero)
        {
            return null;
        }

        try
        {
            var entry = new PROCESSENTRY32
            {
                dwSize = (uint)Marshal.SizeOf<PROCESSENTRY32>(),
                szExeFile = string.Empty,
            };

            if (!Process32First(snapshot, ref entry))
            {
                return null;
            }

            var parents = new Dictionary<int, int>();
            do
            {
                // Pid 0 is the idle process and is nobody's meaningful parent;
                // recording it would invite a false lineage.
                if (entry.th32ProcessID != 0 && entry.th32ParentProcessID != 0)
                {
                    parents[(int)entry.th32ProcessID] = (int)entry.th32ParentProcessID;
                }
            }
            while (Process32Next(snapshot, ref entry));

            return parents;
        }
        finally
        {
            CloseHandle(snapshot);
        }
    }

    /// <summary>
    /// Every process's command line, in one WMI query.
    ///
    /// There is no command line in the ToolHelp32 snapshot and no managed API
    /// for another process's, so this is a single
    /// <c>SELECT ProcessId, CommandLine FROM Win32_Process</c> for the whole
    /// listing rather than a per-process call. <c>Win32_Process.CommandLine</c>
    /// is null across an elevation or session boundary; such rows are skipped,
    /// which leaves the pid without a command line rather than failing the map.
    /// </summary>
    [System.Runtime.Versioning.SupportedOSPlatform("windows")]
    private static IReadOnlyDictionary<int, string>? WindowsCommandLines()
    {
        var map = new Dictionary<int, string>();
        using var searcher = new System.Management.ManagementObjectSearcher(
            "SELECT ProcessId, CommandLine FROM Win32_Process");
        using var results = searcher.Get();
        foreach (var obj in results)
        {
            using var entry = obj;
            if (entry["ProcessId"] is not { } pidValue
                || entry["CommandLine"] is not string commandLine
                || commandLine.Length == 0)
            {
                continue;
            }

            map[Convert.ToInt32(pidValue, CultureInfo.InvariantCulture)] = commandLine;
        }

        return map;
    }

    // -- Linux and anything else with /proc ---------------------------------

    /// <summary>
    /// Field 4 of <c>/proc/&lt;pid&gt;/stat</c>. The command name in field 2 is
    /// unquoted and may itself contain spaces and brackets, so the scan starts
    /// after the last close bracket rather than splitting the whole line.
    /// </summary>
    private static IReadOnlyDictionary<int, int>? ProcFsParents()
    {
        if (!Directory.Exists("/proc"))
        {
            return null;
        }

        var parents = new Dictionary<int, int>();
        foreach (var dir in Directory.EnumerateDirectories("/proc"))
        {
            var leaf = System.IO.Path.GetFileName(dir);
            if (!int.TryParse(leaf, NumberStyles.None, CultureInfo.InvariantCulture, out var pid))
            {
                continue;
            }

            string line;
            try
            {
                line = File.ReadAllText(System.IO.Path.Combine(dir, "stat"));
            }
            catch (Exception)
            {
                // The process exited mid-walk, or is not ours to read.
                continue;
            }

            var close = line.LastIndexOf(')');
            if (close < 0)
            {
                continue;
            }

            var fields = line[(close + 1)..]
                .Split(' ', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries);

            // fields[0] is the state, fields[1] the parent.
            if (fields.Length > 1
                && int.TryParse(fields[1], NumberStyles.Integer, CultureInfo.InvariantCulture, out var ppid)
                && ppid != 0)
            {
                parents[pid] = ppid;
            }
        }

        return parents;
    }

    /// <summary>
    /// Every process's command line, from <c>/proc/&lt;pid&gt;/cmdline</c>. The
    /// arguments are NUL-separated there; they are joined with spaces so the
    /// Rust side sees the same shape Windows produces. A process that exited
    /// mid-walk, or is not ours to read, is skipped rather than failing the map.
    /// </summary>
    private static IReadOnlyDictionary<int, string>? ProcFsCommandLines()
    {
        if (!Directory.Exists("/proc"))
        {
            return null;
        }

        var map = new Dictionary<int, string>();
        foreach (var dir in Directory.EnumerateDirectories("/proc"))
        {
            var leaf = System.IO.Path.GetFileName(dir);
            if (!int.TryParse(leaf, NumberStyles.None, CultureInfo.InvariantCulture, out var pid))
            {
                continue;
            }

            string raw;
            try
            {
                raw = File.ReadAllText(System.IO.Path.Combine(dir, "cmdline"));
            }
            catch (Exception)
            {
                continue;
            }

            // A kernel thread has an empty cmdline; nothing to preselect on.
            var commandLine = raw.Replace('\0', ' ').Trim();
            if (commandLine.Length == 0)
            {
                continue;
            }

            map[pid] = commandLine;
        }

        return map;
    }
}
