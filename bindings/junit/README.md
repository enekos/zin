# Zin for JUnit 5

Generate native JUnit Jupiter dynamic tests from a finite-state specification. Each tour or scenario constructs a fresh adapter. The adapter returns the domain's acceptance decision, exposes any observable subset of fields and closes even when a test fails. Exceptions propagate to JUnit. Requires a JDK 17+ and Maven.

Install from this checkout with `mvn -f bindings/junit/pom.xml install`. Add `io.github.enekos:zin-junit:0.3.0` as a test dependency in the consuming project alongside the Jupiter test engine.

```java
import io.github.enekos.zin.Zin;
import org.junit.jupiter.api.DynamicTest;
import org.junit.jupiter.api.TestFactory;
import java.nio.file.Path;
import java.util.stream.Stream;

@TestFactory
Stream<DynamicTest> queueConforms() throws Exception {
    return Zin.plan(Path.of("specs/queue.zin")).tests(QueueAdapter::new);
}
```

The binary comes from `ZIN_BIN` or `PATH`. Pass a coverage name as the second argument to `Zin.plan`. An optional third argument accepts plan options, for example `Zin.plan(spec, "pairs", Map.of("pair-limit", "50"))` or `Map.of("component", "Server")` for a component adapter. For binary-free tests, generate a plan with `zin plan specs/queue.zin --out specs/queue.plan.json` and call `Zin.read(specPath, planPath)`. The loader verifies source fingerprints. Filter the returned stream by `DynamicTest.getDisplayName()` to replay an individual scenario. A failure includes the fingerprint, the original sequence of calls and any shorter sequence that reproduces the same mismatch.

Implement `Zin.Adapter.call(String, Map<String,Object>)`. Arguments are named and use Java strings, booleans and longs. `observe()` may return partial maps. `close()` can release an implementation or test fixture. The suite drives a real `ArrayBlockingQueue` through its public `offer` and `poll` API and catches a wrong capacity. [Executable adapter and tests](src/test/java/io/github/enekos/zin/QueueTest.java)
