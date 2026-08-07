using System.Globalization;
using Microsoft.Diagnostics.Runtime;

namespace CodeBasics.Inspector;

/// <summary>
/// Structs that are conceptually one value.
///
/// A <c>decimal</c> on the heap is four integers; a <c>DateTime</c> is a
/// bit-packed <c>ulong</c>. Rendering those as their fields is technically
/// honest and practically useless — and money and timestamps are precisely
/// what someone inspecting a failed calculation came to look at.
///
/// Each of these reconstructs the value from the runtime's documented layout.
/// If any field is missing or unreadable the attempt is abandoned, and the
/// caller falls back to showing the raw fields — a visible struct beats a
/// confidently wrong number.
/// </summary>
internal static class WellKnown
{
    public static bool TryFormat(ClrValueType value, out string text)
    {
        text = "";
        var name = value.Type?.Name;
        if (name is null)
        {
            return false;
        }

        try
        {
            return name switch
            {
                "System.Decimal" => TryDecimal(value, out text),
                "System.DateTime" => TryDateTime(value, out text),
                "System.TimeSpan" => TryTimeSpan(value, out text),
                "System.Guid" => TryGuid(value, out text),
                _ => false,
            };
        }
        catch
        {
            return false;
        }
    }

    private static bool TryRead<T>(ClrValueType value, string field, out T read) where T : unmanaged
    {
        read = default;
        var instance = value.Type?.GetFieldByName(field);
        if (instance is null)
        {
            return false;
        }
        read = instance.Read<T>(value.Address, interior: true);
        return true;
    }

    /// <summary>
    /// `_flags` packs the sign in bit 31 and the scale in bits 16-23; the
    /// magnitude is 96 bits.
    ///
    /// Two field layouts exist and both are live, because a dump may come from
    /// any runtime the machine has: .NET Core 3.0 and later store the
    /// magnitude as <c>_lo64</c> + <c>_hi32</c>, while earlier versions used
    /// three separate <c>_lo</c>/<c>_mid</c>/<c>_hi</c> words. Recognising only
    /// one would silently drop back to showing raw integers — which is exactly
    /// what money must not do.
    /// </summary>
    private static bool TryDecimal(ClrValueType value, out string text)
    {
        text = "";
        if (!TryRead<int>(value, "_flags", out var flags))
        {
            return false;
        }

        int lo, mid, hi;
        if (TryRead<ulong>(value, "_lo64", out var lo64) && TryRead<uint>(value, "_hi32", out var hi32))
        {
            lo = unchecked((int)(uint)lo64);
            mid = unchecked((int)(uint)(lo64 >> 32));
            hi = unchecked((int)hi32);
        }
        else if (TryRead<int>(value, "_lo", out lo)
                 && TryRead<int>(value, "_mid", out mid)
                 && TryRead<int>(value, "_hi", out hi))
        {
            // The pre-3.0 layout, read above by the pattern's out parameters.
        }
        else
        {
            return false;
        }

        var negative = (flags & unchecked((int)0x80000000)) != 0;
        var scale = (byte)((flags >> 16) & 0x7F);
        if (scale > 28)
        {
            // Not a layout this understands; do not fabricate a number.
            return false;
        }

        text = new decimal(lo, mid, hi, negative, scale).ToString(CultureInfo.InvariantCulture);
        return true;
    }

    private static bool TryDateTime(ClrValueType value, out string text)
    {
        text = "";
        if (!TryRead<ulong>(value, "_dateData", out var data))
        {
            return false;
        }

        // Round-trip format, so the value is unambiguous about its kind.
        text = DateTime.FromBinary(unchecked((long)data)).ToString("O", CultureInfo.InvariantCulture);
        return true;
    }

    private static bool TryTimeSpan(ClrValueType value, out string text)
    {
        text = "";
        if (!TryRead<long>(value, "_ticks", out var ticks))
        {
            return false;
        }

        text = TimeSpan.FromTicks(ticks).ToString("c", CultureInfo.InvariantCulture);
        return true;
    }

    private static bool TryGuid(ClrValueType value, out string text)
    {
        text = "";
        if (!TryRead<int>(value, "_a", out var a)
            || !TryRead<short>(value, "_b", out var b)
            || !TryRead<short>(value, "_c", out var c))
        {
            return false;
        }

        Span<byte> tail = stackalloc byte[8];
        var names = new[] { "_d", "_e", "_f", "_g", "_h", "_i", "_j", "_k" };
        for (var i = 0; i < names.Length; i++)
        {
            if (!TryRead<byte>(value, names[i], out var octet))
            {
                return false;
            }
            tail[i] = octet;
        }

        text = new Guid(a, b, c, tail[0], tail[1], tail[2], tail[3], tail[4], tail[5], tail[6], tail[7])
            .ToString();
        return true;
    }
}
