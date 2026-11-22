#!/bin/sh
# Compile or collect a generated test from a fresh consumer directory.
set -eu
repo=$(cd "$(dirname "$0")/../.." && pwd)
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
spec="$repo/bindings/fixtures/latch.zin"
cp "$spec" "$scratch/latch.zin"

case "$1" in
    junit)
        mkdir -p "$scratch/src/test/java"
        cp "$repo/examples/pilots/junit/pom.xml" "$scratch/"
        "$ZIN_BIN" init junit "$spec" > "$scratch/src/test/java/ZinLatchTest.java"
        mvn -B -f "$scratch/pom.xml" test-compile
        ;;
    rspec)
        "$ZIN_BIN" init rspec "$spec" > "$scratch/latch_spec.rb"
        rspec -I "$repo/bindings/ruby/lib" --dry-run --format documentation "$scratch/latch_spec.rb"
        ;;
    phpunit)
        "$ZIN_BIN" init phpunit "$spec" > "$scratch/ZinLatchTest.php"
        phpunit --bootstrap "$repo/bindings/php/src/Plan.php" --list-tests "$scratch/ZinLatchTest.php"
        ;;
    xunit)
        project="$scratch/bindings/dotnet/Tests"
        mkdir -p "$project" "$scratch/tests"
        cp "$repo/bindings/dotnet/Tests/Tests.csproj" "$project/"
        library="$scratch/bindings/dotnet/Zin.Verify"
        mkdir -p "$library"
        cp "$repo"/bindings/dotnet/Zin.Verify/*.cs "$repo"/bindings/dotnet/Zin.Verify/*.csproj "$library/"
        ln -s "$repo/bindings/fixtures" "$scratch/bindings/fixtures"
        ln -s "$repo/tests/fixtures" "$scratch/tests/fixtures"
        "$ZIN_BIN" init xunit "$spec" > "$project/ZinLatchTests.cs"
        dotnet test "$project/Tests.csproj" --list-tests
        ;;
    *)
        echo "expected junit, rspec, phpunit, or xunit" >&2
        exit 2
        ;;
esac
