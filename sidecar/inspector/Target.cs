using System.Runtime.InteropServices;
using Microsoft.Diagnostics.Runtime;

namespace CodeBasics.Inspector;

/// <summary>A failure worth naming, with the code code-basics acts on.</summary>
internal sealed class InspectorException(string code, string message) : Exception(message)
{
    public string Code { get; } = code;
}

/// <summary>
/// Opening what is to be inspected.
///
/// The two sources — a crash dump and a live process — converge on the same
/// <see cref="ClrRuntime"/>, which is what lets one walker serve every
/// situation code-basics can put in front of it.
/// </summary>
internal static class Target
{
    public static (DataTarget Data, ClrRuntime Runtime) Open(TargetDto target, bool suspend)
    {
        var data = target.Kind switch
        {
            "dump" => OpenDump(target),
            "live" => OpenLive(target, suspend),
            _ => throw new InspectorException(
                FailureCodes.BadRequest,
                $"`{target.Kind}` is not a kind of target this inspector understands"),
        };

        try
        {
            return (data, CreateRuntime(data));
        }
        catch
        {
            data.Dispose();
            throw;
        }
    }

    private static DataTarget OpenDump(TargetDto target)
    {
        var path = target.Path
            ?? throw new InspectorException(FailureCodes.BadRequest, "no dump path was given");

        if (!File.Exists(path))
        {
            throw new InspectorException(
                FailureCodes.TargetGone,
                $"there is no dump at {path}; it may have been pruned since it was listed");
        }

        try
        {
            return DataTarget.LoadDump(path);
        }
        catch (Exception e)
        {
            throw new InspectorException(
                FailureCodes.NotManaged,
                $"{path} could not be opened as a .NET crash dump: {e.Message}");
        }
    }

    private static DataTarget OpenLive(TargetDto target, bool suspend)
    {
        var pid = target.Pid
            ?? throw new InspectorException(FailureCodes.BadRequest, "no process id was given");

        try
        {
            // The snapshot copies the process image so the application keeps
            // running; suspending it is opt-in precisely because stopping a
            // service to look at it is a surprise worth asking for.
            return suspend
                ? DataTarget.AttachToProcess(pid, suspend: true)
                : DataTarget.CreateSnapshotAndAttach(pid);
        }
        catch (Exception e) when (e is UnauthorizedAccessException or System.ComponentModel.Win32Exception)
        {
            throw new InspectorException(
                FailureCodes.AccessDenied,
                $"process {pid} could not be opened. It is likely running as a different user; " +
                "starting it from code-basics, or running code-basics elevated, would allow this.");
        }
        catch (ArgumentException)
        {
            throw new InspectorException(
                FailureCodes.TargetGone,
                $"process {pid} is no longer running");
        }
        catch (Exception e)
        {
            throw new InspectorException(
                FailureCodes.NotManaged,
                $"process {pid} could not be inspected: {e.Message}");
        }
    }

    private static ClrRuntime CreateRuntime(DataTarget data)
    {
        // Bitness first: it is the one failure with a specific remedy —
        // code-basics retries with the other build — so diagnosing it as
        // anything else would turn a working capture into a dead end.
        var targetPointerSize = data.DataReader.PointerSize;
        if (targetPointerSize != IntPtr.Size)
        {
            throw new InspectorException(
                FailureCodes.BitnessMismatch,
                $"the target is {targetPointerSize * 8}-bit and this inspector is {IntPtr.Size * 8}-bit");
        }

        var version = data.ClrVersions.FirstOrDefault()
            ?? throw new InspectorException(
                FailureCodes.NotManaged,
                "no .NET runtime was found in the target. It may be a native process, " +
                "or the runtime may not have finished starting when it was captured.");

        try
        {
            return version.CreateRuntime();
        }
        catch (Exception e)
        {
            throw new InspectorException(
                FailureCodes.NotManaged,
                $"the .NET runtime in the target could not be read: {e.Message}");
        }
    }

    /// <summary>What was opened, for the capture to report back.</summary>
    public static TargetSummaryDto Describe(DataTarget data, ClrRuntime runtime, TargetDto request)
    {
        string? name = null;
        try
        {
            name = Recognisable(data.DataReader.DisplayName);
        }
        catch
        {
            // A display name is a nicety; its absence must not cost a capture.
        }

        // A live capture snapshots the target rather than attaching to it, so
        // ClrMD's display name is the throwaway clone's — never the process the
        // user picked. Ask the OS for the real one, by the pid that was asked
        // for. Failure leaves it null, which the UI already renders as a bare
        // pid.
        if (name is null && request.Kind == "live" && request.Pid is { } pid)
        {
            try
            {
                using var process = System.Diagnostics.Process.GetProcessById(pid);
                name = process.ProcessName;
            }
            catch
            {
                // Exited between the capture and this lookup, or another user's.
            }
        }

        return new TargetSummaryDto
        {
            Target = request,
            Bitness = data.DataReader.PointerSize == 8 ? "x64" : "x86",
            RuntimeVersion = runtime.ClrInfo.Version.ToString(),
            ProcessName = name,
        };
    }

    /// <summary>
    /// A display name only when it names something, otherwise nothing.
    ///
    /// ClrMD falls back to <c>pid:&lt;hex&gt;</c> when it has no name to give, and
    /// for a live snapshot that hex is the *clone's* pid, not the target's. The
    /// UI renders this field beside the real pid, so passing it through reads as
    /// "pid:7e60 (pid 33280)" — two contradicting numbers, one of which names no
    /// process that exists. Abstaining leaves a bare, correct pid; the same
    /// trade every threshold in this feature makes.
    /// </summary>
    private static string? Recognisable(string? displayName)
    {
        if (string.IsNullOrWhiteSpace(displayName))
        {
            return null;
        }

        const string prefix = "pid:";
        if (displayName.StartsWith(prefix, StringComparison.OrdinalIgnoreCase))
        {
            var rest = displayName[prefix.Length..];
            if (rest.Length > 0 && rest.All(Uri.IsHexDigit))
            {
                return null;
            }
        }

        return displayName;
    }

    /// <summary>The architecture this build of the inspector is.</summary>
    public static string SelfBitness =>
        RuntimeInformation.ProcessArchitecture is Architecture.X86 or Architecture.Arm ? "x86" : "x64";
}
