using System.Text.Json;
using Xunit;
using Zin.Verify;

public sealed class ObservationsTests
{
    public static IEnumerable<object[]> Cases()
    {
        using var data = JsonDocument.Parse(File.ReadAllText(Path.Combine(AppContext.BaseDirectory, "observations.json")));
        foreach (var row in data.RootElement.EnumerateArray())
            yield return [row.GetProperty("name").GetString()!, row.GetRawText()];
    }

    private sealed class Observer(JsonElement observed) : IAdapter
    {
        public bool Call(string action, IReadOnlyDictionary<string, JsonElement> args) =>
            throw new InvalidOperationException("expected an initial observation only");
        public object Observe() => observed;
    }

    [Theory]
    [MemberData(nameof(Cases))]
    public void MatchesContract(string name, string row)
    {
        using var fixture = JsonDocument.Parse(row);
        var data = fixture.RootElement;
        var path = Path.Combine(Path.GetTempPath(), $"zin-observation-{Guid.NewGuid():N}.json");
        try
        {
            File.WriteAllText(path, JsonSerializer.Serialize(new {
                zin = "plan/1", system = name, states = new[] { data.GetProperty("expected") },
                actions = Array.Empty<object>(), calls = Array.Empty<object>(),
                tests = new[] { new { name, steps = Array.Empty<object>() } }
            }));
            var plan = Plan.Load(path);
            void Run() => plan.Run(0, () => new Observer(data.GetProperty("observed")));
            if (data.GetProperty("matches").GetBoolean()) Run();
            else Assert.Throws<Divergence>(Run);
        }
        finally { File.Delete(path); }
    }
}
