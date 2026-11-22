using System.Diagnostics;
using System.Text.Json;

namespace Zin.Verify;

public interface IAdapter : IDisposable
{
    bool Call(string action, IReadOnlyDictionary<string, JsonElement> arguments);
    object? Observe() => null;
    void IDisposable.Dispose() { }
}

public sealed class Divergence(string message) : Exception(message);

public sealed class Plan
{
    private sealed record Call(string Name, IReadOnlyDictionary<string, JsonElement> Arguments, string Text);
    private readonly record struct Step(int CallId, int To);
    private sealed record Failure(int Index, int Before, string Detail);
    private readonly JsonElement parents;
    private readonly string fingerprint;
    private readonly JsonElement tests;
    private readonly JsonElement states;
    private readonly Call[] calls;

    private Plan(JsonElement document)
    {
        if (document.GetProperty("zin").GetString() != "plan/1")
            throw new FormatException("Expected a Zin plan/1 document");
        fingerprint = document.GetProperty("fingerprint").GetString()!;
        parents = document.GetProperty("parents");
        tests = document.GetProperty("tests");
        states = document.GetProperty("states");
        var parameters = document.GetProperty("actions").EnumerateArray().ToDictionary(
            action => action.GetProperty("name").GetString()!,
            action => action.GetProperty("params").EnumerateArray()
                .Select(parameter => parameter.GetProperty("name").GetString()!).ToArray());
        calls = document.GetProperty("calls").EnumerateArray().Select(call =>
        {
            var name = call.GetProperty("action").GetString()!;
            var args = parameters[name].Zip(call.GetProperty("args").EnumerateArray())
                .ToDictionary(pair => pair.First, pair => pair.Second);
            return new Call(name, args.AsReadOnly(), call.GetProperty("text").GetString()!);
        }).ToArray();
    }

    /// A .zin file derives a fresh plan. A JSON file explicitly selects an offline artifact.
    public static Plan Load(string path, params string[] options)
    {
        string text;
        if (Path.GetExtension(path).Equals(".json", StringComparison.OrdinalIgnoreCase))
        {
            if (options.Length != 0) throw new ArgumentException("Plan options require a .zin source");
            text = File.ReadAllText(path);
        }
        else
        {
            var start = new ProcessStartInfo(Environment.GetEnvironmentVariable("ZIN_BIN") ?? "zin")
            {
                UseShellExecute = false, RedirectStandardOutput = true, RedirectStandardError = true
            };
            start.ArgumentList.Add("plan");
            start.ArgumentList.Add(path);
            foreach (var argument in options) start.ArgumentList.Add(argument);
            using var process = Process.Start(start) ?? throw new IOException("Cannot start zin");
            var error = process.StandardError.ReadToEndAsync();
            text = process.StandardOutput.ReadToEnd();
            process.WaitForExit();
            var message = error.GetAwaiter().GetResult();
            if (process.ExitCode != 0) throw new ArgumentException(message);
        }
        using var document = JsonDocument.Parse(text);
        return new Plan(document.RootElement.Clone());
    }

    /// Primitive rows keep native xUnit theory discovery, naming and filtering available.
    public IEnumerable<object[]> Cases()
    {
        int index = 0;
        foreach (var test in tests.EnumerateArray())
            yield return [index++, test.GetProperty("name").GetString()!];
    }

    public void Run(int index, Func<IAdapter> factory)
    {
        var test = tests[index];
        var title = test.GetProperty("name").GetString();
        var steps = test.GetProperty("steps").EnumerateArray()
            .Select(step => new Step(step[0].GetInt32(), step[1].GetInt32())).ToArray();
        var failure = Replay(steps, factory);
        if (failure is null) return;
        var original = steps[..failure.Index];
        var message = $"{title}, step {failure.Index}: {failure.Detail}\nfingerprint: {fingerprint}\n{Scenario(original)}";
        if (failure.Index > 0)
        {
            var candidate = Shortest(failure.Before);
            candidate.Add(original[^1]);
            if (candidate.Count < original.Length)
            {
                try
                {
                    if (Replay(candidate, factory) == new Failure(candidate.Count, failure.Before, failure.Detail))
                        message += $"\nshortest reproduction:\n{Scenario(candidate)}";
                }
                catch (Exception error)
                {
                    message += $"\nshorter replay raised {error.GetType().Name}: {error.Message}";
                }
            }
        }
        throw new Divergence(message);
    }

    private Failure? Replay(IReadOnlyList<Step> steps, Func<IAdapter> factory)
    {
        using var adapter = factory();
        int state = 0;
        if (Observation(adapter, state) is { } initial) return new Failure(0, 0, initial);
        for (int index = 0; index < steps.Count; index++)
        {
            var step = steps[index];
            var before = state;
            var call = calls[step.CallId];
            var accepted = adapter.Call(call.Name, call.Arguments);
            if (accepted != (step.To >= 0))
            {
                var actual = accepted ? "accepted" : "rejected";
                var expected = step.To >= 0 ? "accepted" : "rejected";
                return new Failure(index + 1, before, $"{call.Text} was {actual}, expected {expected}");
            }
            if (step.To >= 0) state = step.To;
            if (Observation(adapter, state) is { } detail)
                return new Failure(index + 1, before, detail);
        }
        return null;
    }

    private string? Observation(IAdapter adapter, int state)
    {
        if (adapter.Observe() is not { } result) return null;
        var actual = JsonSerializer.SerializeToElement(result);
        return Matches(states[state], actual) ? null : $"observed {actual}, expected a subset of {states[state]}";
    }

    private List<Step> Shortest(int state)
    {
        var path = new List<Step>();
        while (state != 0)
        {
            var link = parents[state];
            var parent = link[0].GetInt32();
            if (parent < 0 || parent >= state) throw new FormatException("invalid shortest-path parent");
            path.Add(new Step(link[1].GetInt32(), state));
            state = parent;
        }
        path.Reverse();
        return path;
    }

    private string Scenario(IEnumerable<Step> steps) =>
        string.Join("\n", new[] { "scenario \"Reproduction\"" }.Concat(steps.Select(
            s => "  " + (s.To == -1 ? "reject " : "") + calls[s.CallId].Text)));

    private static bool Matches(JsonElement expected, JsonElement actual) =>
        expected.ValueKind == actual.ValueKind && expected.ValueKind switch
        {
            JsonValueKind.Object => actual.EnumerateObject().All(field =>
                expected.TryGetProperty(field.Name, out var wanted) && Matches(wanted, field.Value)),
            JsonValueKind.String => expected.GetString() == actual.GetString(),
            JsonValueKind.Number => expected.TryGetInt64(out var wanted) && actual.TryGetInt64(out var seen) && wanted == seen,
            JsonValueKind.True or JsonValueKind.False => true,
            _ => false
        };
}
