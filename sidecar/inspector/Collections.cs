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
