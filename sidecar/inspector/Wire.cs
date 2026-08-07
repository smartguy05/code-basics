using System.Text.Json;
using System.Text.Json.Serialization;

namespace CodeBasics.Inspector;

/// <summary>
/// The request and result documents exchanged with code-basics.
///
/// These mirror <c>crates/core/src/inspect/model.rs</c> and
/// <c>graph.rs</c> by hand, the same way <c>src/ipc/types.ts</c> does. The Rust
/// side pins the exact JSON keys in tests, so a rename there fails loudly
/// rather than silently producing nulls here.
///
/// The result shape is deliberately flat — a list of nodes with parent links
/// rather than a nested tree. Building the hierarchy is the Rust side's job,
/// which keeps this process to "walk and append" and puts the decisions where
/// they can be tested without a .NET runtime.
/// </summary>
internal static class Wire
{
    public static readonly JsonSerializerOptions Options = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.CamelCase,
        PropertyNameCaseInsensitive = true,
        DefaultIgnoreCondition = JsonIgnoreCondition.WhenWritingNull,
        WriteIndented = true,
    };

    /// <summary>Bumped only for a breaking change; a mismatch is refused, not guessed at.</summary>
    public const int SchemaVersion = 1;
}

// ---------------------------------------------------------------------------
// Request
// ---------------------------------------------------------------------------

internal sealed class RequestDto
{
    public int SchemaVersion { get; set; }
    public TargetDto Target { get; set; } = new();
    public RootDto Root { get; set; } = new();
    public CapsDto Caps { get; set; } = new();
    public bool Suspend { get; set; }
}

/// <summary>A tagged union on the Rust side; every field optional here so an
/// unexpected combination is diagnosed rather than failing to parse.</summary>
internal sealed class TargetDto
{
    public string Kind { get; set; } = "";
    public string? Path { get; set; }
    public int? Pid { get; set; }
}

internal sealed class RootDto
{
    public string Kind { get; set; } = "";
    public string? Name { get; set; }
    public int? Limit { get; set; }
    public string? Address { get; set; }
}

internal sealed class CapsDto
{
    public int MaxDepth { get; set; } = 5;
    public int MaxChildren { get; set; } = 100;
    public int MaxStringLength { get; set; } = 512;
    public int MaxNodes { get; set; } = 5000;
}

// ---------------------------------------------------------------------------
// Result
// ---------------------------------------------------------------------------

internal sealed class ResultDto
{
    public int SchemaVersion { get; set; } = Wire.SchemaVersion;
    public string SnapshotId { get; set; } = "";
    public string CapturedAt { get; set; } = "";
    public TargetSummaryDto Target { get; set; } = new();
    public CapsDto Caps { get; set; } = new();
    public List<NodeDto> Nodes { get; set; } = [];
    public List<string> Warnings { get; set; } = [];

    /// <summary>Why no capture could be taken. Written for a person to read.</summary>
    public string? Failure { get; set; }

    /// <summary>
    /// A stable identifier for the same thing, so code-basics never has to
    /// decide whether to retry by matching on prose.
    /// </summary>
    public string? FailureCode { get; set; }
}

internal sealed class TargetSummaryDto
{
    public TargetDto Target { get; set; } = new();
    public string? Bitness { get; set; }
    public string? RuntimeVersion { get; set; }
    public string? ProcessName { get; set; }
}

/// <summary>
/// One value, as read. <c>Kind</c> drives how the Rust side classifies it, and
/// every variant carries only the fields that variant genuinely needs — a
/// reference without an address is reported as unreadable rather than as a
/// reference, because it could be neither expanded nor matched for a cycle.
/// </summary>
internal sealed class NodeDto
{
    public string Id { get; set; } = "";
    public string? Parent { get; set; }
    public string Label { get; set; } = "";
    public string? TypeName { get; set; }
    public string Kind { get; set; } = "unavailable";
    public string? Text { get; set; }
    public string? Address { get; set; }
    public string? Path { get; set; }
    public string? Reason { get; set; }
    public bool Expandable { get; set; }
    public bool Truncated { get; set; }
    public int? ChildCountTotal { get; set; }
}

// ---------------------------------------------------------------------------
// Process listing
// ---------------------------------------------------------------------------

/// <summary>
/// Every .NET process on the machine that has published a diagnostics channel.
///
/// This exists because `dotnet run` launches the application as a *child*: the
/// pid code-basics supervises is the CLI launcher, whose heap holds none of the
/// user's objects. Enumerating lets the Rust side follow the parent chain and
/// offer the process someone actually meant.
/// </summary>
internal sealed class ProcessListDto
{
    public int SchemaVersion { get; set; } = Wire.SchemaVersion;
    public List<ProcessDto> Processes { get; set; } = [];
    public List<string> Warnings { get; set; } = [];
}

/// <summary>
/// One attachable process. Only <c>Pid</c> and <c>Name</c> are guaranteed;
/// everything else is best effort and is <b>omitted when unknown</b> rather
/// than filled with a plausible value — a wrong parent would attribute a
/// process to the wrong run configuration.
/// </summary>
internal sealed class ProcessDto
{
    public int Pid { get; set; }
    public string Name { get; set; } = "";
    public string? Path { get; set; }
    public int? ParentPid { get; set; }

    /// <summary>Round-trip ("O") UTC, or absent.</summary>
    public string? StartedAt { get; set; }
}

/// <summary>The failure codes code-basics knows how to act on.</summary>
internal static class FailureCodes
{
    public const string BitnessMismatch = "bitnessMismatch";
    public const string NotManaged = "notManaged";
    public const string AccessDenied = "accessDenied";
    public const string TargetGone = "targetGone";
    public const string BadRequest = "badRequest";
}
