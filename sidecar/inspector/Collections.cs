using Microsoft.Diagnostics.Runtime;

namespace CodeBasics.Inspector;

/// <summary>
/// Recognising the collections whose internals are noise.
///
/// A <c>List&lt;T&gt;</c> on the heap is an <c>_items</c> array plus a
/// <c>_size</c>, and showing it that way buries the elements one level down
/// behind an array that is usually longer than the list. Since the whole point
/// of this feature is reading a value at a glance, the common containers are
/// unwrapped into their elements.
///
/// Only shapes that can be recognised with certainty are unwrapped. Anything
/// else falls through to ordinary field rendering, which is honest if ugly —
/// guessing that a type is a collection because it happens to have an
/// <c>_items</c> field would misrepresent it.
/// </summary>
internal static class Collections
{
    /// <summary>
    /// True when <paramref name="obj"/> is a container this understands.
    /// <paramref name="total"/> is the real element count, which may exceed
    /// what is returned.
    /// </summary>
    public static bool TryGetElements(
        ClrObject obj,
        out IReadOnlyList<ClrObject> elements,
        out int total)
    {
        elements = [];
        total = 0;

        var name = obj.Type?.Name;
        if (name is null)
        {
            return false;
        }

        // `List<T>` and the handful of types built on the same two fields.
        if (name.StartsWith("System.Collections.Generic.List<", StringComparison.Ordinal)
            || name.StartsWith("System.Collections.ObjectModel.Collection<", StringComparison.Ordinal))
        {
            return TryReadBackingArray(obj, out elements, out total);
        }

        return false;
    }

    /// <summary>
    /// One live dictionary entry: the interior address of the <c>Entry</c>
    /// struct in the backing array, plus the struct's <c>key</c> and
    /// <c>value</c> fields, so the caller can read each through the ordinary
    /// field machinery (which is what covers value-type keys and values that the
    /// object-reference path in <see cref="TryReadBackingArray"/> would miss).
    /// </summary>
    public readonly struct DictionaryEntry(ulong address, ClrInstanceField key, ClrInstanceField value)
    {
        public ulong Address { get; } = address;
        public ClrInstanceField Key { get; } = key;
        public ClrInstanceField Value { get; } = value;
    }

    /// <summary>
    /// True when <paramref name="obj"/> is a <c>Dictionary&lt;K,V&gt;</c> whose
    /// entries can be read with certainty. <paramref name="total"/> is the count
    /// of live entries.
    ///
    /// A <c>Dictionary&lt;K,V&gt;</c> is a <c>_buckets</c> array, an
    /// <c>_entries</c> array of a private <c>Entry</c> struct, and a
    /// <c>_count</c> high-water mark. Rendered as fields it buries the actual
    /// key/value pairs behind those internals, so — like <c>List&lt;T&gt;</c> —
    /// it is unwrapped into its entries. Anything surprising in the layout
    /// abstains back to ordinary field rendering rather than misrepresenting the
    /// contents.
    /// </summary>
    public static bool TryGetDictionary(
        ClrObject obj,
        out IReadOnlyList<DictionaryEntry> entries,
        out int total)
    {
        entries = [];
        total = 0;

        var name = obj.Type?.Name;
        if (name is null
            || !name.StartsWith("System.Collections.Generic.Dictionary<", StringComparison.Ordinal))
        {
            return false;
        }

        try
        {
            var entriesObj = obj.ReadObjectField("_entries");
            if (entriesObj.IsNull || !entriesObj.IsArray)
            {
                return false;
            }

            // `_count` is the high-water mark of used slots in `_entries`,
            // including any later freed. It bounds the scan; the free-list
            // filter below removes the dead slots.
            var count = obj.ReadField<int>("_count");
            var array = entriesObj.AsArray();
            if (count < 0 || count > array.Length)
            {
                // The two disagree, so one was misread. Fall back rather than
                // trusting either.
                return false;
            }

            var entryType = array.Type?.ComponentType;
            if (entryType is null || !entryType.IsValueType)
            {
                return false;
            }

            var keyField = entryType.GetFieldByName("key");
            var valueField = entryType.GetFieldByName("value");
            var nextField = entryType.GetFieldByName("next");
            if (keyField is null || valueField is null || nextField is null)
            {
                // Not the Entry struct we know; do not guess at its shape.
                return false;
            }

            var live = new List<DictionaryEntry>();
            for (var i = 0; i < count; i++)
            {
                var entry = array.GetStructValue(i);

                // A live entry has `next >= -1` (-1 marks the last in its
                // bucket chain). A freed slot encodes its free-list link as
                // `next = StartOfFreeList - nextIndex` with StartOfFreeList = -3,
                // so every free slot is `next < -1`. Showing one would render a
                // key/value the dictionary no longer contains.
                var next = nextField.Read<int>(entry.Address, interior: true);
                if (next < -1)
                {
                    continue;
                }

                live.Add(new DictionaryEntry(entry.Address, keyField, valueField));
            }

            entries = live;
            total = live.Count;
            return true;
        }
        catch
        {
            // Any surprise in the layout means this is not the type we think.
            return false;
        }
    }

    /// <summary>
    /// Read the <c>_items</c>/<c>_size</c> pair.
    ///
    /// <c>_size</c> matters: the backing array is grown in powers of two, so
    /// its tail holds stale references to objects the list no longer contains.
    /// Showing those would be showing values that are not in the collection.
    /// </summary>
    private static bool TryReadBackingArray(
        ClrObject obj,
        out IReadOnlyList<ClrObject> elements,
        out int total)
    {
        elements = [];
        total = 0;

        try
        {
            var items = obj.ReadObjectField("_items");
            if (items.IsNull || !items.IsArray)
            {
                return false;
            }

            var size = obj.ReadField<int>("_size");
            var array = items.AsArray();
            if (size < 0 || size > array.Length)
            {
                // The two disagree, which means one of them was misread. Fall
                // back rather than trusting either.
                return false;
            }

            var component = array.Type?.ComponentType;
            if (component is null || !component.IsObjectReference)
            {
                // A list of primitives or structs. The generic element reader
                // does not cover those, so ordinary field rendering at least
                // shows the backing array rather than an empty collection.
                return false;
            }

            var read = new List<ClrObject>(size);
            for (var i = 0; i < size; i++)
            {
                read.Add(array.GetObjectValue(i));
            }

            elements = read;
            total = size;
            return true;
        }
        catch
        {
            // Any surprise in the layout means this is not the type we think.
            return false;
        }
    }
}
