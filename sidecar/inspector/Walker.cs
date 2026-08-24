using System.Globalization;
using Microsoft.Diagnostics.Runtime;

namespace CodeBasics.Inspector;

/// <summary>
/// Walks the heap and appends nodes.
///
/// Three properties matter more than completeness here:
///
/// <list type="number">
/// <item>It always terminates. An object graph is cyclic and frequently
/// enormous, so every axis it could run away along — depth, breadth, total
/// size — is bounded, and reaching a bound is reported rather than producing a
/// quietly shorter list.</item>
/// <item>It never runs the target's code. ClrMD reads fields directly, so
/// nothing here can throw a user exception, block, or mutate the thing being
/// inspected. That is also why computed properties are invisible: only
/// backing state exists to be read.</item>
/// <item>It never invents a value. Anything it cannot read is emitted as
/// <c>unavailable</c> with a reason. A field shown as <c>0</c> that was never
/// actually read is the failure worth engineering against, because the user
/// believes it and goes and debugs the wrong thing.</item>
/// </list>
///
/// The walk is breadth-first on purpose. Under a total node budget a
/// depth-first walk would spend the entire budget on the first branch it
/// found, leaving the object's other fields unexplored; breadth-first spends
/// it evenly, so what the user sees is the shallow shape of the whole object.
/// </summary>
internal sealed class Walker
{
    private readonly ClrHeap _heap;
    private readonly CapsDto _caps;
    private readonly List<NodeDto> _nodes = [];
    private readonly List<string> _warnings = [];

    /// <summary>Address to the id where it was first emitted, for cycles.</summary>
    private readonly Dictionary<ulong, string> _seen = [];

    private readonly Queue<(ClrObject Obj, NodeDto Node, int Depth)> _pending = new();

    public Walker(ClrHeap heap, CapsDto caps)
    {
        _heap = heap;
        _caps = caps;
    }

    public IReadOnlyList<NodeDto> Nodes => _nodes;
    public IReadOnlyList<string> Warnings => _warnings;

    public void Warn(string message) => _warnings.Add(message);

    /// <summary>Add a root and walk everything reachable within the caps.</summary>
    public NodeDto AddRoot(ClrObject obj, string id, string label)
    {
        var node = Reference(obj, id, parent: null, label);
        _nodes.Add(node);
        _seen[obj.Address] = id;
        _pending.Enqueue((obj, node, 0));
        return node;
    }

    /// <summary>
    /// Add a node the heap walk would not produce.
    ///
    /// Used for the stack trace, which cannot be read as a field: an
    /// exception's `_stackTraceString` stays null until something calls
    /// `.StackTrace`, and nothing here ever runs the target's code. The frames
    /// are available from the runtime directly, and an exception without them
    /// answers half the question.
    /// </summary>
    public NodeDto AddSynthetic(NodeDto parent, string id, string label, string kind, string? text)
    {
        var node = new NodeDto
        {
            Id = id,
            Parent = parent.Id,
            Label = label,
            Kind = kind,
            Text = text,
        };
        _nodes.Add(node);
        return node;
    }

    public void Run()
    {
        while (_pending.Count > 0)
        {
            var (obj, node, depth) = _pending.Dequeue();

            if (depth >= _caps.MaxDepth)
            {
                // There is more here; say so rather than presenting a leaf.
                node.Expandable = true;
                continue;
            }

            if (_nodes.Count >= _caps.MaxNodes)
            {
                node.Expandable = true;
                continue;
            }

            try
            {
                Expand(obj, node, depth);
            }
            catch (Exception e)
            {
                // One unreadable object must not cost the rest of the capture.
                node.Expandable = true;
                Warn($"could not read the contents of {node.TypeName ?? "an object"} at {node.Address}: {e.Message}");
            }
        }
    }

    // -----------------------------------------------------------------------
    // Expanding one object
    // -----------------------------------------------------------------------

    private void Expand(ClrObject obj, NodeDto node, int depth)
    {
        var type = obj.Type;
        if (type is null)
        {
            node.Expandable = false;
            return;
        }

        if (obj.IsArray)
        {
            ExpandArray(obj, node, depth);
            return;
        }

        if (Collections.TryGetElements(obj, out var elements, out var total))
        {
            ExpandElements(elements, total, node, depth);
            return;
        }

        if (Collections.TryGetDictionary(obj, out var pairs, out var pairTotal))
        {
            ExpandDictionary(pairs, pairTotal, node, depth);
            return;
        }

        ExpandFields(type, obj.Address, interior: false, node, depth);
    }

    private void ExpandArray(ClrObject obj, NodeDto node, int depth)
    {
        var array = obj.AsArray();
        node.ChildCountTotal = array.Length;

        // A byte array is the one case where per-element rows are actively
        // worse than no rows: nobody reads a payload or a hash a byte at a
        // time, and the CLR hangs several long ones off every exception
        // (`_watsonBuckets` alone is over five thousand). A hex preview says
        // more in one line than five thousand rows.
        if (Bytes.TryPreview(array, _caps.MaxStringLength, out var preview, out var truncated))
        {
            node.Kind = "text";
            node.Text = preview;
            node.Truncated = truncated;
            return;
        }

        var shown = Math.Min(array.Length, _caps.MaxChildren);
        for (var i = 0; i < shown; i++)
        {
            var child = ElementNode(array, i, node, depth);
            _nodes.Add(child);
        }

        if (array.Length > shown)
        {
            _nodes.Add(Elided(node, $"[{shown}…{array.Length - 1}]", "childLimit"));
        }
    }

    private void ExpandElements(IReadOnlyList<ClrObject> elements, int total, NodeDto node, int depth)
    {
        node.ChildCountTotal = total;

        var shown = Math.Min(elements.Count, _caps.MaxChildren);
        for (var i = 0; i < shown; i++)
        {
            var child = ObjectChild(elements[i], node, $"[{i}]", $"{node.Id}[{i}]", depth);
            _nodes.Add(child);
        }

        if (total > shown)
        {
            _nodes.Add(Elided(node, $"[{shown}…{total - 1}]", "childLimit"));
        }
    }

    /// <summary>
    /// Render a dictionary as one <c>pair</c> container per live entry, each
    /// holding a <c>Key</c> and a <c>Value</c> child.
    ///
    /// The container node has no address: it is a grouping the Rust side draws
    /// as a leaf-with-children, never a reference to expand. Its two children
    /// are the Entry struct's <c>key</c> and <c>value</c> fields, read through
    /// the ordinary <see cref="FieldNode"/> machinery at the struct's interior
    /// address (so value-type keys and values read as values), then relabelled
    /// from the CLR's <c>key</c>/<c>value</c> to <c>Key</c>/<c>Value</c>.
    /// </summary>
    private void ExpandDictionary(
        IReadOnlyList<Collections.DictionaryEntry> entries,
        int total,
        NodeDto node,
        int depth)
    {
        node.ChildCountTotal = total;

        var shown = Math.Min(entries.Count, _caps.MaxChildren);
        for (var i = 0; i < shown; i++)
        {
            // Each pair costs three nodes (the container plus a Key and a
            // Value), so the budget is checked before starting one rather than
            // leaving a pair with a Key and no Value.
            if (_nodes.Count + 3 > _caps.MaxNodes)
            {
                _nodes.Add(Elided(node, "…", "nodeLimit"));
                return;
            }

            var entry = entries[i];
            var pair = new NodeDto
            {
                Id = $"{node.Id}[{i}]",
                Parent = node.Id,
                Label = $"[{i}]",
                Kind = "pair",
            };
            _nodes.Add(pair);

            var key = FieldNode(entry.Key, entry.Address, interior: true, pair, depth + 1);
            key.Label = "Key";
            _nodes.Add(key);

            var value = FieldNode(entry.Value, entry.Address, interior: true, pair, depth + 1);
            value.Label = "Value";
            _nodes.Add(value);
        }

        if (total > shown)
        {
            _nodes.Add(Elided(node, $"[{shown}…{total - 1}]", "childLimit"));
        }
    }

    private void ExpandFields(ClrType type, ulong address, bool interior, NodeDto node, int depth)
    {
        var fields = type.Fields;
        var shown = 0;

        foreach (var field in fields)
        {
            if (shown >= _caps.MaxChildren)
            {
                _nodes.Add(Elided(node, $"…{fields.Length - shown} more", "childLimit"));
                node.ChildCountTotal = fields.Length;
                break;
            }

            if (_nodes.Count >= _caps.MaxNodes)
            {
                _nodes.Add(Elided(node, "…", "nodeLimit"));
                break;
            }

            _nodes.Add(FieldNode(field, address, interior, node, depth));
            shown++;
        }
    }

    // -----------------------------------------------------------------------
    // Reading one value
    // -----------------------------------------------------------------------

    private NodeDto FieldNode(ClrInstanceField field, ulong address, bool interior, NodeDto parent, int depth)
    {
        var name = field.Name ?? "<unnamed>";
        var id = $"{parent.Id}.{name}";

        var node = new NodeDto
        {
            Id = id,
            Parent = parent.Id,
            Label = name,
            TypeName = field.Type?.Name,
        };

        try
        {
            Fill(node, field, address, interior, depth);
        }
        catch (Exception e)
        {
            Unavailable(node, $"the field could not be read: {e.Message}");
        }

        return node;
    }

    private void Fill(NodeDto node, ClrInstanceField field, ulong address, bool interior, int depth)
    {
        switch (field.ElementType)
        {
            case ClrElementType.Boolean:
                node.Kind = "primitive";
                node.Text = field.Read<bool>(address, interior) ? "true" : "false";
                return;

            case ClrElementType.Char:
                var c = field.Read<char>(address, interior);
                node.Kind = "primitive";
                node.Text = $"'{c}'";
                return;

            case ClrElementType.Int8:
                Primitive(node, field.Read<sbyte>(address, interior)); return;
            case ClrElementType.UInt8:
                Primitive(node, field.Read<byte>(address, interior)); return;
            case ClrElementType.Int16:
                Primitive(node, field.Read<short>(address, interior)); return;
            case ClrElementType.UInt16:
                Primitive(node, field.Read<ushort>(address, interior)); return;
            case ClrElementType.Int32:
                Primitive(node, field.Read<int>(address, interior)); return;
            case ClrElementType.UInt32:
                Primitive(node, field.Read<uint>(address, interior)); return;
            case ClrElementType.Int64:
                Primitive(node, field.Read<long>(address, interior)); return;
            case ClrElementType.UInt64:
                Primitive(node, field.Read<ulong>(address, interior)); return;
            case ClrElementType.Float:
                Primitive(node, field.Read<float>(address, interior)); return;
            case ClrElementType.Double:
                Primitive(node, field.Read<double>(address, interior)); return;

            case ClrElementType.NativeInt:
            case ClrElementType.NativeUInt:
            case ClrElementType.Pointer:
            case ClrElementType.FunctionPointer:
                node.Kind = "primitive";
                node.Text = Hex(field.Read<ulong>(address, interior));
                return;

            case ClrElementType.String:
                FillString(node, field.ReadString(address, interior));
                return;

            case ClrElementType.Struct:
                FillStruct(node, field.ReadStruct(address, interior), depth);
                return;

            case ClrElementType.Object:
            case ClrElementType.Class:
            case ClrElementType.Array:
            case ClrElementType.SZArray:
                FillObject(node, field.ReadObject(address, interior), depth);
                return;

            default:
                // A type this build has no reading strategy for. Naming it
                // beats a placeholder that looks like a value.
                Unavailable(node, $"values of kind `{field.ElementType}` cannot be read by this inspector");
                return;
        }
    }

    private void FillString(NodeDto node, string? value)
    {
        if (value is null)
        {
            node.Kind = "null";
            return;
        }

        node.Kind = "text";
        if (value.Length > _caps.MaxStringLength)
        {
            node.Text = value[.._caps.MaxStringLength];
            node.Truncated = true;
        }
        else
        {
            node.Text = value;
        }
    }

    private void FillStruct(NodeDto node, ClrValueType value, int depth)
    {
        // The handful of structs that are conceptually a single value. Showing
        // a decimal as four raw ints is technically honest and practically
        // useless, and money is exactly what someone inspecting a failed
        // calculation is looking at.
        if (WellKnown.TryFormat(value, out var text))
        {
            node.Kind = "primitive";
            node.Text = text;
            return;
        }

        if (value.Type is null)
        {
            Unavailable(node, "the value's type could not be resolved");
            return;
        }

        // Any other struct is expanded in place: it has no address of its own
        // to revisit, so it cannot participate in a cycle.
        node.Kind = "reference";
        node.Address = Hex(value.Address);
        if (depth + 1 >= _caps.MaxDepth || _nodes.Count >= _caps.MaxNodes)
        {
            node.Expandable = true;
            return;
        }

        ExpandFields(value.Type, value.Address, interior: true, node, depth + 1);
    }

    private void FillObject(NodeDto node, ClrObject value, int depth)
    {
        if (value.IsNull || value.Address == 0)
        {
            node.Kind = "null";
            return;
        }

        node.Kind = "reference";
        node.Address = Hex(value.Address);
        node.TypeName = value.Type?.Name ?? node.TypeName;

        if (_seen.TryGetValue(value.Address, out var first))
        {
            // Already on screen elsewhere. Emitting it again is what makes a
            // naive dumper recurse forever, and on a DAG it also duplicates
            // whole subtrees.
            node.Kind = "cycle";
            node.Path = first;
            return;
        }

        if (depth + 1 >= _caps.MaxDepth || _nodes.Count >= _caps.MaxNodes)
        {
            node.Expandable = true;
            return;
        }

        _seen[value.Address] = node.Id;
        _pending.Enqueue((value, node, depth + 1));
    }

    // -----------------------------------------------------------------------
    // Nodes
    // -----------------------------------------------------------------------

    private NodeDto ElementNode(ClrArray array, int index, NodeDto parent, int depth)
    {
        var id = $"{parent.Id}[{index}]";
        var node = new NodeDto
        {
            Id = id,
            Parent = parent.Id,
            Label = $"[{index}]",
            TypeName = array.Type?.ComponentType?.Name,
        };

        try
        {
            var component = array.Type?.ComponentType;
            if (component is null)
            {
                Unavailable(node, "the array's element type could not be resolved");
            }
            else if (component.IsObjectReference)
            {
                FillObject(node, array.GetObjectValue(index), depth);
            }
            // Primitives must be read as values. Left to the struct path they
            // come back as one row per element wrapping a nested `m_value`,
            // which doubles the node count and buries the actual numbers.
            else if (TryPrimitiveElement(node, array, index, component.ElementType))
            {
                // Filled.
            }
            else if (component.IsValueType)
            {
                FillStruct(node, array.GetStructValue(index), depth);
            }
            else
            {
                Unavailable(node, $"array elements of kind `{component.ElementType}` cannot be read by this inspector");
            }
        }
        catch (Exception e)
        {
            Unavailable(node, $"the element could not be read: {e.Message}");
        }

        return node;
    }

    /// <summary>
    /// Read one element of a primitive array, or return false if it is not a
    /// primitive.
    /// </summary>
    private static bool TryPrimitiveElement(NodeDto node, ClrArray array, int index, ClrElementType element)
    {
        switch (element)
        {
            case ClrElementType.Boolean:
                node.Kind = "primitive";
                node.Text = array.GetValue<bool>(index) ? "true" : "false";
                return true;
            case ClrElementType.Char:
                node.Kind = "primitive";
                node.Text = $"'{array.GetValue<char>(index)}'";
                return true;
            case ClrElementType.Int8: Primitive(node, array.GetValue<sbyte>(index)); return true;
            case ClrElementType.UInt8: Primitive(node, array.GetValue<byte>(index)); return true;
            case ClrElementType.Int16: Primitive(node, array.GetValue<short>(index)); return true;
            case ClrElementType.UInt16: Primitive(node, array.GetValue<ushort>(index)); return true;
            case ClrElementType.Int32: Primitive(node, array.GetValue<int>(index)); return true;
            case ClrElementType.UInt32: Primitive(node, array.GetValue<uint>(index)); return true;
            case ClrElementType.Int64: Primitive(node, array.GetValue<long>(index)); return true;
            case ClrElementType.UInt64: Primitive(node, array.GetValue<ulong>(index)); return true;
            case ClrElementType.Float: Primitive(node, array.GetValue<float>(index)); return true;
            case ClrElementType.Double: Primitive(node, array.GetValue<double>(index)); return true;
            case ClrElementType.NativeInt:
            case ClrElementType.NativeUInt:
            case ClrElementType.Pointer:
                node.Kind = "primitive";
                node.Text = Hex(array.GetValue<ulong>(index));
                return true;
            default:
                return false;
        }
    }

    private NodeDto ObjectChild(ClrObject obj, NodeDto parent, string label, string id, int depth)
    {
        var node = new NodeDto
        {
            Id = id,
            Parent = parent.Id,
            Label = label,
            TypeName = obj.Type?.Name,
        };
        FillObject(node, obj, depth);
        return node;
    }

    private NodeDto Reference(ClrObject obj, string id, string? parent, string label) => new()
    {
        Id = id,
        Parent = parent,
        Label = label,
        TypeName = obj.Type?.Name,
        Kind = "reference",
        Address = Hex(obj.Address),
    };

    private static NodeDto Elided(NodeDto parent, string label, string reason) => new()
    {
        Id = $"{parent.Id}…{reason}",
        Parent = parent.Id,
        Label = label,
        Kind = "elided",
        Reason = reason,
    };

    private static void Primitive<T>(NodeDto node, T value) where T : IFormattable
    {
        node.Kind = "primitive";
        node.Text = value.ToString(null, CultureInfo.InvariantCulture);
    }

    private static void Unavailable(NodeDto node, string reason)
    {
        node.Kind = "unavailable";
        node.Reason = reason;
        node.Text = null;
        node.Address = null;
    }

    /// <summary>
    /// Addresses cross as hex strings. They exceed what a JavaScript number
    /// holds exactly, and the address is the identity used to expand a node —
    /// a rounded one would open the wrong object.
    /// </summary>
    public static string Hex(ulong address) => "0x" + address.ToString("x", CultureInfo.InvariantCulture);
}
