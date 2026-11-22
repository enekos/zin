using System.Text.Json;
using Xunit;
using Zin.Verify;

public sealed class LatchTests
{
    private static readonly Plan LatchPlan = Plan.Load(Path.Combine(AppContext.BaseDirectory, "latch.zin"));
    public static IEnumerable<object[]> Cases() => LatchPlan.Cases();

    private sealed class Latch(bool faulty = false) : IAdapter
    {
        private bool locked;
        public bool Call(string name, IReadOnlyDictionary<string, JsonElement> args)
        {
            var target = name == "lock";
            if (locked == target) return faulty;
            locked = target;
            return true;
        }
        public object Observe() => new { locked };
    }

    [Theory]
    [MemberData(nameof(Cases))]
    public void Conforms(int index, string name)
    {
        Assert.NotEmpty(name);
        LatchPlan.Run(index, () => new Latch());
    }

    [Fact]
    public void BrokenRefusalHasTheReproduction()
    {
        var error = Assert.Throws<Divergence>(() => LatchPlan.Run(0, () => new Latch(true)));
        Assert.Contains("reject lock", error.Message);
    }
}
