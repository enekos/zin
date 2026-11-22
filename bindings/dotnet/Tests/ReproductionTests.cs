using System.Text.Json;
using Xunit;
using Zin.Verify;

public sealed class ReproductionTests
{
    private sealed class InconsistentLatch(bool alternate) : IAdapter
    {
        private bool locked;
        public bool Call(string name, IReadOnlyDictionary<string, JsonElement> arguments)
        {
            if (name == "unlock") return alternate;
            if (locked) return false;
            locked = true;
            return true;
        }
        public object Observe() => new { locked };
    }

    [Theory]
    [InlineData(true)]
    [InlineData(false)]
    public void OnlyReportsVerifiedShorterScenario(bool different)
    {
        var plan = Plan.Load(Path.Combine(AppContext.BaseDirectory, "latch.zin"));
        int factories = 0;
        var error = Assert.Throws<Divergence>(() =>
            plan.Run(0, () => new InconsistentLatch(++factories == 2 && different)));
        Assert.Matches("fingerprint: [0-9a-f]{16}", error.Message);
        Assert.Contains("unlock was rejected, expected accepted", error.Message);
        Assert.Contains("scenario \"Reproduction\"\n  lock\n  reject lock\n  unlock", error.Message);
        if (different) Assert.DoesNotContain("shortest reproduction:", error.Message);
        else Assert.EndsWith("shortest reproduction:\nscenario \"Reproduction\"\n  lock\n  unlock", error.Message);
        Assert.Equal(2, factories);
    }
}
