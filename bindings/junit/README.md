# Zin for JUnit 5

Replay a Zin specification's test plan as JUnit Jupiter dynamic tests. Each
plan test builds a fresh adapter and closes it, even when the test fails.
Requires JDK 17+.

Install from a checkout with `mvn -f bindings/junit/pom.xml install` (the
package is not on Maven Central yet), then add the test dependencies:

```xml
<dependency>
  <groupId>io.github.enekos</groupId>
  <artifactId>zin-junit</artifactId>
  <version>0.6.0</version>
  <scope>test</scope>
</dependency>
<dependency>
  <groupId>org.junit.jupiter</groupId>
  <artifactId>junit-jupiter-engine</artifactId>
  <version>5.12.2</version>
  <scope>test</scope>
</dependency>
```

```java
@TestFactory
Stream<DynamicTest> queueConforms() throws IOException {
    return Zin.load(Zin.resource(QueueTest.class, "queue.zin")).tests(QueueAdapter::new);
}
```

`zin init junit queue.zin` prints a skeleton. Keep the specification, and a
committed `queue.plan.json` if you want test runs without the binary, in
`src/test/resources`; `Zin.resource` finds them on the test classpath.

Implement `Zin.Adapter`: `call(action, arguments)` performs one action, with
arguments by parameter name as `Long`, `Boolean` or `String`, and returns
`false` for a refusal; any exception fails the test with that exception.
`observe()` returns any subset of the state fields (enum constants compare by
`toString()`, map keys by their text), or `null` to compare nothing. `close()`
releases the implementation.

`Zin.load(spec, new Zin.Options().cover("pairs").pairLimit(50))` passes
planning options (`cover`, `maxSteps`, `stepBudget`, `pairLimit`,
`component`); `.plan(path)` and `.binary(path)` choose the plan file and the
`zin` binary (default `$ZIN_BIN`, then `zin` on PATH). A committed plan is used
only when it is fresh and was derived with every option you set; see
[the runner contract](../../docs/plan-format.md#what-a-binding-must-do), whose
shared cases this binding runs.

A failure is a `Zin.Divergence` (an `AssertionError`) with the same report as
the other runners, plus `fingerprint()` and a pasteable `scenario()`. Filter
the stream by `DynamicTest.getDisplayName()` to replay one scenario.
`examples/pilots/junit` drives the JDK's `ByteBuffer`.
