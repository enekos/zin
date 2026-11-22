# Zin for xUnit

Reference `Zin.Verify/Zin.Verify.csproj` from a .NET 8 test project, or pack the library with `dotnet pack` and install from the resulting local NuGet feed. The library uses the .NET standard library.

Load a source with `Plan.Load(path, "--cover", "pairs")`. It invokes `ZIN_BIN` or `zin` on PATH. To run offline, supply an explicitly generated JSON plan and regenerate it in CI when sources change. Return `plan.Cases()` from a static xUnit member-data provider. Its primitive rows keep each generated test individually discoverable. In the theory, call `plan.Run(index, () => new MyAdapter())`.

An adapter implements `IAdapter.Call`, returning true for an accepted call. Optional `Observe` returns an object with the fields or map entries to compare, and `Dispose` runs after each case. Unexpected exceptions propagate. See the [executable xUnit consumer](Tests/LatchTests.cs).
