namespace Crasher;

public sealed class Quote
{
    public decimal Total { get; set; }
    public string Currency { get; set; } = "USD";
    public Guid Reference { get; set; }
    public DateTime RequestedAt { get; set; }
    public TimeSpan Duration { get; set; }
    public Customer? Customer { get; set; }
    public List<Leg> Legs { get; } = [];
    private readonly int[] _weights = [10, 20, 30];
}

public sealed class Leg
{
    public int Id { get; set; }
    public string Origin { get; set; } = "";
    public string Destination { get; set; } = "";
    /// <summary>The back-reference that makes the graph cyclic.</summary>
    public Quote? Quote { get; set; }
}

public sealed class Customer
{
    public string Name { get; set; } = "";
    public string? Email { get; set; }
}

internal static class Program
{
    private static void Main(string[] args)
    {
        var quote = new Quote
        {
            Total = 12450.75m,
            Reference = Guid.Parse("6f9619ff-8b86-d011-b42d-00c04fc964ff"),
            RequestedAt = new DateTime(2026, 8, 6, 14, 32, 7, DateTimeKind.Utc),
            Duration = TimeSpan.FromMinutes(215),
            // Left null on purpose: a genuine null must be distinguishable from
            // a value the inspector could not read.
            Customer = null,
        };

        // Comfortably past the default child cap, so the elision is exercised.
        for (var i = 0; i < 5412; i++)
        {
            quote.Legs.Add(new Leg
            {
                Id = i,
                Origin = i % 2 == 0 ? "KTEB" : "KLAS",
                Destination = i % 2 == 0 ? "KLAS" : "KTEB",
                Quote = quote,
            });
        }

        var mode = args.Length > 0 ? args[0] : "crash";
        if (mode == "wait")
        {
            Console.WriteLine($"ready: pid {Environment.ProcessId}");
            GC.KeepAlive(quote);
            Thread.Sleep(TimeSpan.FromMinutes(10));
            return;
        }

        Console.WriteLine($"built a quote with {quote.Legs.Count} legs; about to fail");
        Boom(quote);
    }

    private static void Boom(Quote quote)
    {
        // The customer is null, so this throws with `quote` still on the stack —
        // exactly the situation the inspector exists to explain.
        Console.WriteLine(quote.Customer!.Name.Length);
    }
}
