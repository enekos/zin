package io.github.enekos.zin;

import com.fasterxml.jackson.core.type.TypeReference;
import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.node.ObjectNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import org.junit.jupiter.api.DynamicTest;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.TestFactory;
import org.junit.jupiter.api.condition.EnabledIfEnvironmentVariable;
import org.junit.jupiter.api.io.TempDir;

import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Map;
import java.util.concurrent.ArrayBlockingQueue;
import java.util.concurrent.atomic.AtomicInteger;
import java.util.stream.Stream;

import static org.junit.jupiter.api.Assertions.*;

class QueueTest {
    private static final Path SPEC = Path.of("src/test/resources/queue.zin");
    private static final Path PLAN = Path.of("src/test/resources/queue.plan.json");
    private static final ObjectMapper JSON = new ObjectMapper();

    private static class Queue implements Zin.Adapter {
        final ArrayBlockingQueue<Integer> queue;

        Queue(int capacity) { this.queue = new ArrayBlockingQueue<>(capacity); }
        public boolean call(String name, Map<String, Object> args) {
            return switch (name) {
                case "offer" -> queue.offer(1);
                case "poll" -> queue.poll() != null;
                default -> throw new IllegalArgumentException(name);
            };
        }
        public Map<String, ?> observe() { return Map.of("size", queue.size()); }
    }

    @TestFactory Stream<DynamicTest> boundedQueue() throws IOException {
        return Zin.read(SPEC, PLAN).tests(() -> new Queue(2));
    }

    @Test void wrongCapacityFailsInNativeTest() throws Exception {
        DynamicTest test = Zin.read(SPEC, PLAN).tests(() -> new Queue(3)).findFirst().orElseThrow();
        AssertionError e = assertThrows(AssertionError.class, test.getExecutable()::execute);
        assertTrue(e.getMessage().contains("offer was accepted, expected rejection"), e.getMessage());
        assertTrue(e.getMessage().contains("reject offer"), e.getMessage());
    }

    @Test void onlyMinimizesTheSameDivergence(@TempDir Path dir) throws Exception {
        ObjectNode data = JSON.readTree(PLAN.toFile()).deepCopy();
        var steps = data.putArray("tests").addObject().put("name", "return to empty").putArray("steps");
        // A removable offer/poll prefix before filling the queue.
        for (int[] step : new int[][] {{0, 1}, {1, 0}, {0, 1}, {0, 2}, {0, -1}})
            steps.addArray().add(step[0]).add(step[1]);
        Path path = dir.resolve("long.plan.json");
        JSON.writeValue(path.toFile(), data);
        var plan = Zin.read(SPEC, path);
        var same = plan.tests(() -> new Queue(3)).findFirst().orElseThrow();
        String message = assertThrows(AssertionError.class, same.getExecutable()::execute).getMessage();
        assertTrue(message.contains("spec " + data.path("fingerprint").asText()));
        assertTrue(message.endsWith("shortest reproduction:\nscenario \"Observed divergence\"\n  offer\n  offer\n  reject offer"));

        var different = plan.tests(() -> new Queue(3) {
            boolean polled;
            @Override public boolean call(String action, Map<String, Object> args) {
                if (action.equals("poll")) polled = true;
                return super.call(action, args);
            }
            @Override public Map<String, ?> observe() {
                return !polled && queue.size() == 2 ? Map.of("size", -1) : super.observe();
            }
        }).findFirst().orElseThrow();
        String original = assertThrows(AssertionError.class, different.getExecutable()::execute).getMessage();
        assertTrue(original.contains("step 5: offer was accepted, expected rejection"));
        assertFalse(original.contains("shortest reproduction:"));
    }

    @Test void unexpectedExceptionPropagatesAndClosesAdapter() throws Exception {
        var closed = new AtomicInteger();
        DynamicTest test = Zin.read(SPEC, PLAN).tests(() -> new Queue(2) {
            @Override public boolean call(String name, Map<String, Object> args) { throw new IllegalStateException("unavailable"); }
            @Override public void close() { closed.incrementAndGet(); }
        }).findFirst().orElseThrow();
        assertThrows(IllegalStateException.class, test.getExecutable()::execute);
        assertEquals(1, closed.get());
    }

    @Test void staleSourcesAreRejected(@TempDir Path dir) throws Exception {
        Path changed = Files.writeString(dir.resolve("q.zin"), Files.readString(SPEC) + "# edit\n");
        assertThrows(IOException.class, () -> Zin.read(changed, PLAN));
        ObjectNode stale = JSON.readTree(PLAN.toFile()).deepCopy();
        stale.putObject("sources").put("q.zin", "bad-hash");
        Path plan = dir.resolve("composed.plan.json");
        JSON.writeValue(plan.toFile(), stale);
        Path entry = Files.copy(SPEC, dir.resolve("entry.zin"));
        assertThrows(IOException.class, () -> Zin.read(entry, plan));
    }

    @TestFactory Stream<DynamicTest> sharedObservationContract(@TempDir Path dir) throws Exception {
        JsonNode cases = JSON.readTree(Path.of("../../tests/fixtures/observations.json").toFile());
        return cases.valueStream().map(example -> DynamicTest.dynamicTest(example.get("name").asText(), () -> {
            ObjectNode data = JSON.readTree(PLAN.toFile()).deepCopy();
            data.putArray("states").add(example.get("expected"));
            data.putArray("parents").addNull();
            var tour = data.putArray("tests").addObject();
            tour.put("name", "initial observation");
            tour.putArray("steps");
            Path plan = Files.createTempFile(dir, "observation-", ".json");
            JSON.writeValue(plan.toFile(), data);
            Map<String, Object> observation = JSON.convertValue(example.get("observed"), new TypeReference<>() {});
            var check = Zin.read(SPEC, plan).tests(() -> new Queue(2) {
                @Override public Map<String, ?> observe() { return observation; }
            }).findFirst().orElseThrow();
            if (example.get("matches").booleanValue()) check.getExecutable().execute();
            else assertThrows(AssertionError.class, check.getExecutable()::execute);
        }));
    }

    @Test @EnabledIfEnvironmentVariable(named = "ZIN_BIN", matches = ".+")
    void derivesAndRunsCurrentPlan() throws Throwable {
        var plan = Zin.plan(SPEC, "pairs", Map.of("pair-limit", "3"));
        var selected = plan.tests(() -> new Queue(2)).toList();
        assertFalse(selected.isEmpty());
        for (var test : selected) test.getExecutable().execute();
    }
}
