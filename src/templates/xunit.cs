using System;
using System.Collections.Generic;
using System.IO;
using System.Text.Json;
using Xunit;
using Zin.Verify;

public sealed class Zin$MODELTests
{
    // Copy the spec (and any included files) to the test output directory.
    private static readonly Plan Model = Plan.Load(Path.Combine(AppContext.BaseDirectory, $SPEC));
    public static IEnumerable<object[]> Cases() => Model.Cases();

    private sealed class Harness : IAdapter
    {
        public bool Call(string action, IReadOnlyDictionary<string, JsonElement> args)
        {
            // Return true for acceptance and false for a domain refusal.
            switch (action)
            {
$ACTIONS                default: throw new ArgumentException(action);
            }
        }

        // Return any observable subset of $FIELDS.
        public object Observe() => new Dictionary<string, object>();
    }

    [Theory]
    [MemberData(nameof(Cases))]
    public void Conforms(int index, string name)
    {
        Assert.NotEmpty(name);
        Model.Run(index, () => new Harness());
    }
}
