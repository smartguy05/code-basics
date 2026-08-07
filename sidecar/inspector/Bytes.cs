using System.Text;
using Microsoft.Diagnostics.Runtime;

namespace CodeBasics.Inspector;

/// <summary>
/// Byte arrays, shown as bytes rather than as rows.
///
/// Every other array is worth one row per element. A <c>byte[]</c> is not:
/// nobody reads a hash, a payload or a serialised blob one row at a time, and
/// the CLR hangs several long ones off every exception it creates —
/// <c>_watsonBuckets</c> alone is over five thousand bytes. Rendered as rows
/// they consume the entire node budget and push the user's own data off the
/// screen, which is how a capture ends up technically complete and practically
/// useless.
/// </summary>
internal static class Bytes
{
    /// <summary>
    /// Format a byte array as hex, if that is what it is.
    /// </summary>
    /// <param name="maxLength">Character budget, shared with strings.</param>
    public static bool TryPreview(ClrArray array, int maxLength, out string preview, out bool truncated)
    {
        preview = "";
        truncated = false;

        if (array.Type?.ComponentType?.ElementType is not ClrElementType.UInt8)
        {
            return false;
        }

        // Two hex characters and a separator per byte.
        var affordable = Math.Max(1, maxLength / 3);
        var shown = Math.Min(array.Length, affordable);

        var builder = new StringBuilder(shown * 3 + 24);
        for (var i = 0; i < shown; i++)
        {
            if (i > 0)
            {
                builder.Append(' ');
            }

            try
            {
                builder.Append(array.GetValue<byte>(i).ToString("x2"));
            }
            catch
            {
                // A partially readable array is still worth showing up to the
                // point it stopped being readable.
                builder.Append("??");
                truncated = true;
                break;
            }
        }

        if (array.Length > shown)
        {
            truncated = true;
        }

        // The count goes in the text because the value is the array, not a
        // list of children — there is no row to hang a count off.
        builder.Insert(0, $"byte[{array.Length}] ");
        preview = builder.ToString();
        return true;
    }
}
