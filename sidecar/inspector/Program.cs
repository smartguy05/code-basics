using System.Globalization;
using System.Text.Json;
using Microsoft.Diagnostics.Runtime;

namespace CodeBasics.Inspector;

/// <summary>
/// One capture — or one process listing — then exit.
///
/// The contract with code-basics is deliberately narrow: read
/// <c>--request</c>, write <c>--result</c>, exit 0. stdout carries human
/// diagnostics that end up in the console the user is already watching;
/// structured data goes in the file and nowhere else, so a stray
/// <c>Console.WriteLine</c> can never corrupt a capture.
///
/// A failure that can be described is *still written to the result file* and
/// still exits 0. The sidecar ran and explained itself, which is a successful
/// exchange — a non-zero exit would throw away the explanation and leave
/// code-basics guessing.
///
/// <c>--list-processes</c> is a second mode under the same contract: no
/// request file, a differently shaped result file, and the same "exit 0 means
/// the file is there" promise.
/// </summary>
internal static class Program
{
    private const string Usage =
        "usage: cb-inspector --request <path> --result <path>\n" +
        "       cb-inspector --list-processes --result <path>";

    private static int Main(string[] args)
    {
        string? requestPath = null;
        string? resultPath = null;
        var listProcesses = false;

        // Stepped one argument at a time rather than in pairs, because
        // --list-processes is a flag and pairwise stepping would read the
        // following option as its value.
        for (var i = 0; i < args.Length; i++)
        {
            switch (args[i])
            {
                case "--list-processes":
                    listProcesses = true;
                    break;
                case "--request" when i + 1 < args.Length:
                    requestPath = args[++i];
                    break;
                case "--result" when i + 1 < args.Length:
                    resultPath = args[++i];
                    break;
                default:
                    Console.Error.WriteLine($"unrecognised argument `{args[i]}`");
                    Console.Error.WriteLine(Usage);
                    return 2;
            }
        }

        if (resultPath is null)
        {
            Console.Error.WriteLine(Usage);
            return 2;
        }

        if (listProcesses)
        {
            return ListProcesses(resultPath);
        }

        if (requestPath is null)
        {
            Console.Error.WriteLine(Usage);
            return 2;
        }

        RequestDto request;
        try
        {
            var json = File.ReadAllText(requestPath);
            request = JsonSerializer.Deserialize<RequestDto>(json, Wire.Options)
                ?? throw new InvalidOperationException("the request was empty");
        }
        catch (Exception e)
        {
            // Nothing can be written to the result file usefully: without a
            // request there is no target to describe.
            Console.Error.WriteLine($"could not read the request at {requestPath}: {e.Message}");
            return 2;
        }

        var result = Capture(request);

        try
        {
            File.WriteAllText(resultPath, JsonSerializer.Serialize(result, Wire.Options));
        }
        catch (Exception e)
        {
            Console.Error.WriteLine($"could not write the result to {resultPath}: {e.Message}");
            return 2;
        }

        Console.WriteLine(result.Failure is null
            ? $"read {result.Nodes.Count} values"
            : $"no capture: {result.Failure}");

        return 0;
    }

    /// <summary>
    /// The listing never fails as a whole: an enumeration that turned nothing
    /// up is an empty list with a warning, which code-basics can show, rather
    /// than an error it would have to translate.
    /// </summary>
    private static int ListProcesses(string resultPath)
    {
        var listing = Processes.List();

        try
        {
            File.WriteAllText(resultPath, JsonSerializer.Serialize(listing, Wire.Options));
        }
        catch (Exception e)
        {
            Console.Error.WriteLine($"could not write the result to {resultPath}: {e.Message}");
            return 2;
        }

        Console.WriteLine($"found {listing.Processes.Count} .NET processes");
        return 0;
    }

    private static ResultDto Capture(RequestDto request)
    {
        var result = new ResultDto
        {
            SnapshotId = "snap-" + Guid.NewGuid().ToString("n")[..12],
            CapturedAt = DateTime.UtcNow.ToString("O", CultureInfo.InvariantCulture),
            Caps = request.Caps,
            Target = new TargetSummaryDto { Target = request.Target, Bitness = Target.SelfBitness },
        };

        if (request.SchemaVersion != Wire.SchemaVersion)
        {
            result.Failure =
                $"code-basics asked for a version {request.SchemaVersion} capture, but this " +
                $"inspector produces version {Wire.SchemaVersion}. The bundled inspector is out " +
                "of step with the application.";
            result.FailureCode = FailureCodes.BadRequest;
            return result;
        }

        DataTarget? data = null;
        ClrRuntime? runtime = null;
        try
        {
            (data, runtime) = Target.Open(request.Target, request.Suspend);
            result.Target = Target.Describe(data, runtime, request.Target);

            var walker = new Walker(runtime.Heap, request.Caps);
            Roots.Add(walker, runtime, request.Root);
            walker.Run();

            result.Nodes = [.. walker.Nodes];
            result.Warnings = [.. walker.Warnings];
        }
        catch (InspectorException e)
        {
            result.Failure = e.Message;
            result.FailureCode = e.Code;
        }
        catch (Exception e)
        {
            result.Failure = $"the capture failed: {e.Message}";
        }
        finally
        {
            runtime?.Dispose();
            data?.Dispose();
        }

        return result;
    }
}

/// <summary>
/// Finding what to walk from.
///
/// There is deliberately no "pick something interesting" option. A live heap
/// holds millions of objects, and a heuristic that chose one would be wrong
/// often enough to send someone down the wrong path — worse than asking them
/// which type they meant.
/// </summary>
internal static class Roots
{
    public static void Add(Walker walker, ClrRuntime runtime, RootDto root)
    {
        switch (root.Kind)
        {
            case "crashException":
                AddCrashException(walker, runtime);
                break;
            case "exceptions":
                AddExceptions(walker, runtime, root.Limit ?? 50);
                break;
            case "type":
                AddOfType(walker, runtime, root.Name, root.Limit ?? 50);
                break;
            case "statics":
                AddStatics(walker, runtime, root.Name);
                break;
            case "address":
                AddAddress(walker, runtime, root.Address);
                break;
            default:
                throw new InspectorException(
                    FailureCodes.BadRequest,
                    $"`{root.Kind}` is not a kind of root this inspector understands");
        }
    }

    private static void AddCrashException(Walker walker, ClrRuntime runtime)
    {
        var found = 0;
        foreach (var thread in runtime.Threads)
        {
            var exception = thread.CurrentException;
            if (exception is null)
            {
                continue;
            }

            var obj = runtime.Heap.GetObject(exception.Address);
            if (obj.IsUnusable())
            {
                continue;
            }

            var node = walker.AddRoot(obj, $"thread{thread.OSThreadId}", $"exception on thread {thread.OSThreadId}");
            AddFrames(walker, node, exception);
            found++;
        }

        if (found == 0)
        {
            walker.Warn(
                "no thread in this capture was holding an exception. The process may have exited " +
                "for another reason, or the dump may have been taken after the exception was handled.");
        }
    }

    /// <summary>
    /// Attach the exception's frames.
    ///
    /// These are read from the runtime rather than from the object, because
    /// `_stackTraceString` is populated lazily by `.StackTrace` and is
    /// therefore almost always null in a dump — the field is there, and empty,
    /// which without this would read as "there was no stack trace".
    /// </summary>
    private static void AddFrames(Walker walker, NodeDto parent, ClrException exception)
    {
        List<ClrStackFrame> frames;
        try
        {
            frames = [.. exception.StackTrace];
        }
        catch (Exception e)
        {
            walker.Warn($"the stack trace could not be read: {e.Message}");
            return;
        }

        if (frames.Count == 0)
        {
            return;
        }

        var container = walker.AddSynthetic(
            parent,
            $"{parent.Id}.stackTrace",
            "stack trace",
            "primitive",
            $"{frames.Count} frames");

        for (var i = 0; i < frames.Count; i++)
        {
            var method = frames[i].Method;
            var text = method is null
                ? frames[i].ToString() ?? "(unknown frame)"
                : $"{method.Type?.Name}.{method.Name}";

            walker.AddSynthetic(container, $"{container.Id}[{i}]", $"#{i}", "text", text);
        }
    }

    private static void AddExceptions(Walker walker, ClrRuntime runtime, int limit)
    {
        var found = 0;
        foreach (var obj in runtime.Heap.EnumerateObjects())
        {
            if (found >= limit)
            {
                break;
            }
            if (obj.IsUnusable() || !DerivesFromException(obj.Type))
            {
                continue;
            }

            walker.AddRoot(obj, $"ex{found}", obj.Type?.Name ?? "exception");
            found++;
        }

        if (found == 0)
        {
            walker.Warn("no exception objects were found on the heap.");
        }
        else if (found >= limit)
        {
            walker.Warn($"showing the first {limit} exceptions found; there may be more.");
        }
    }

    private static void AddOfType(Walker walker, ClrRuntime runtime, string? name, int limit)
    {
        if (string.IsNullOrWhiteSpace(name))
        {
            throw new InspectorException(FailureCodes.BadRequest, "no type name was given");
        }

        var found = 0;
        foreach (var obj in runtime.Heap.EnumerateObjects())
        {
            if (found >= limit)
            {
                break;
            }
            if (obj.IsUnusable() || !Matches(obj.Type?.Name, name))
            {
                continue;
            }

            walker.AddRoot(obj, $"i{found}", $"{Simple(obj.Type?.Name)} #{found}");
            found++;
        }

        if (found == 0)
        {
            walker.Warn(
                $"no instances of `{name}` are on the heap. The name must match the type as the " +
                "runtime sees it — try the full namespace, or a different capture.");
        }
        else if (found >= limit)
        {
            walker.Warn($"showing the first {limit} instances; there may be more.");
        }
    }

    private static void AddStatics(Walker walker, ClrRuntime runtime, string? name)
    {
        if (string.IsNullOrWhiteSpace(name))
        {
            throw new InspectorException(FailureCodes.BadRequest, "no type name was given");
        }

        var domain = runtime.AppDomains.FirstOrDefault()
            ?? throw new InspectorException(FailureCodes.NotManaged, "the target has no app domain");

        var found = 0;
        foreach (var module in runtime.EnumerateModules())
        {
            var type = module.GetTypeByName(name);
            if (type is null)
            {
                continue;
            }

            foreach (var field in type.StaticFields)
            {
                var obj = field.ReadObject(domain);
                if (obj.IsUnusable())
                {
                    continue;
                }
                walker.AddRoot(obj, $"static{found}", field.Name ?? "static");
                found++;
            }
            break;
        }

        if (found == 0)
        {
            walker.Warn(
                $"`{name}` has no readable static object fields in this capture. Statics holding " +
                "primitives are not shown, and the type may not be loaded.");
        }
    }

    private static void AddAddress(Walker walker, ClrRuntime runtime, string? address)
    {
        if (string.IsNullOrWhiteSpace(address))
        {
            throw new InspectorException(FailureCodes.BadRequest, "no address was given");
        }

        var text = address.StartsWith("0x", StringComparison.OrdinalIgnoreCase) ? address[2..] : address;
        if (!ulong.TryParse(text, NumberStyles.HexNumber, CultureInfo.InvariantCulture, out var parsed))
        {
            throw new InspectorException(
                FailureCodes.BadRequest,
                $"`{address}` is not a hexadecimal address");
        }

        var obj = runtime.Heap.GetObject(parsed);
        if (obj.IsUnusable())
        {
            throw new InspectorException(
                FailureCodes.TargetGone,
                $"there is no live object at {address}. On a running process it may have moved or " +
                "been collected since the last capture; take a new one.");
        }

        walker.AddRoot(obj, "root", Simple(obj.Type?.Name) ?? "object");
    }

    private static bool DerivesFromException(ClrType? type)
    {
        for (var current = type; current is not null; current = current.BaseType)
        {
            if (current.Name == "System.Exception")
            {
                return true;
            }
        }
        return false;
    }

    /// <summary>Match on the full name, or on the simple name for convenience.</summary>
    private static bool Matches(string? actual, string wanted)
    {
        if (actual is null)
        {
            return false;
        }
        return actual == wanted
            || string.Equals(Simple(actual), wanted, StringComparison.Ordinal);
    }

    private static string? Simple(string? name)
    {
        if (name is null)
        {
            return null;
        }
        var generic = name.IndexOf('<', StringComparison.Ordinal);
        var head = generic < 0 ? name : name[..generic];
        var dot = head.LastIndexOf('.');
        return dot < 0 ? name : name[(dot + 1)..];
    }
}

internal static class ObjectExtensions
{
    /// <summary>
    /// True when there is nothing worth walking — null, or a reference that
    /// does not point at a live object with a resolvable type. Heap
    /// enumeration on a dump routinely turns up all three.
    /// </summary>
    public static bool IsUnusable(this ClrObject obj) => obj.IsNull || !obj.IsValid || obj.Type is null;
}
