#!/bin/sh
# Compile a generated JUnit test in a fresh Maven consumer of the installed binding.
set -eu
repo=$(cd "$(dirname "$0")/../.." && pwd)
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
spec="$repo/bindings/fixtures/contract/latch.zin"
mkdir -p "$scratch/src/test/java" "$scratch/src/test/resources"
cp "$repo/examples/pilots/junit/pom.xml" "$scratch/"
cp "$spec" "$scratch/src/test/resources/"
"$ZIN_BIN" init junit "$spec" > "$scratch/src/test/java/ZinLatchTest.java"
mvn -B -q -f "$scratch/pom.xml" test-compile
