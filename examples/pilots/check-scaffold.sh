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
        (cd "$repo/bindings/ruby" && gem build zin-verify.gemspec --output "$scratch/zin.gem")
        gem install --local --no-document --install-dir "$scratch/gems" "$scratch/zin.gem"
        GEM_PATH="$scratch/gems:$(ruby -e 'puts Gem.path.join(File::PATH_SEPARATOR)')" \
            rspec --dry-run --format documentation "$scratch/latch_spec.rb"
        ;;
    phpunit)
        "$ZIN_BIN" init phpunit "$spec" > "$scratch/ZinLatchTest.php"
        if command -v composer >/dev/null; then
            composer archive --working-dir="$repo/bindings/php" --format=tar --file=zin --dir="$scratch"
            mkdir "$scratch/package"
            tar -xf "$scratch/zin.tar" -C "$scratch/package"
            composer dump-autoload --working-dir="$scratch/package" --no-interaction
            phpunit --bootstrap "$scratch/package/vendor/autoload.php" --list-tests "$scratch/ZinLatchTest.php"
        else
            phpunit --bootstrap "$repo/bindings/php/src/Plan.php" --list-tests "$scratch/ZinLatchTest.php"
        fi
        ;;
    xunit)
        project="$scratch/bindings/dotnet/Tests"
        mkdir -p "$project" "$scratch/tests"
        dotnet pack "$repo/bindings/dotnet/Zin.Verify/Zin.Verify.csproj" -o "$scratch/feed"
        sed 's@<ProjectReference Include="[^"]*" */>@<PackageReference Include="Zin.Verify" Version="0.3.0" />@' \
            "$repo/bindings/dotnet/Tests/Tests.csproj" > "$project/Tests.csproj"
        ln -s "$repo/bindings/fixtures" "$scratch/bindings/fixtures"
        ln -s "$repo/tests/fixtures" "$scratch/tests/fixtures"
        "$ZIN_BIN" init xunit "$spec" > "$project/ZinLatchTests.cs"
        dotnet restore "$project/Tests.csproj" --source "$scratch/feed" --source https://api.nuget.org/v3/index.json --packages "$scratch/packages"
        dotnet test "$project/Tests.csproj" --no-restore --list-tests
        ;;
    *)
        echo "expected junit, rspec, phpunit, or xunit" >&2
        exit 2
        ;;
esac
