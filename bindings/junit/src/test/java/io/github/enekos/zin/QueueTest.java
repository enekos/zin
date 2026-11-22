package io.github.enekos.zin;

import org.junit.jupiter.api.DynamicTest;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.TestFactory;
import org.junit.jupiter.api.condition.EnabledIfEnvironmentVariable;

import java.io.IOException;
import java.nio.file.Path;
import java.util.Map;
import java.util.concurrent.ArrayBlockingQueue;
import java.util.concurrent.atomic.AtomicInteger;
import java.util.stream.Stream;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

/** Drives a real ArrayBlockingQueue through its public offer and poll API. */
class QueueTest {
    private static final Path SPEC = Zin.resource(QueueTest.class, "queue.zin");

    private static class Queue implements Zin.Adapter {
        final ArrayBlockingQueue<Integer> queue;

        Queue(int capacity) {
            queue = new ArrayBlockingQueue<>(capacity);
        }

        @Override
        public boolean call(String action, Map<String, Object> arguments) {
            return switch (action) {
                case "offer" -> queue.offer(1);
                case "poll" -> queue.poll() != null;
                default -> throw new IllegalArgumentException(action);
            };
        }

        @Override
        public Map<String, ?> observe() {
            return Map.of("size", queue.size());
        }
    }

    @TestFactory
    Stream<DynamicTest> boundedQueueConforms() throws IOException {
        return Zin.load(SPEC).tests(() -> new Queue(2));
    }

    @Test
    void wrongCapacityFailsAsANativeAssertion() throws IOException {
        DynamicTest test = Zin.load(SPEC).tests(() -> new Queue(3)).findFirst().orElseThrow();
        var divergence = assertThrows(Zin.Divergence.class, test.getExecutable()::execute);
        assertTrue(divergence.getMessage().contains("the implementation accepted offer, but the specification rejects it"),
                divergence.getMessage());
        assertEquals("scenario \"Replay divergence\"\n  offer\n  offer\n  reject offer", divergence.scenario());
        assertEquals(Zin.load(SPEC).fingerprint(), divergence.fingerprint());
    }

    @Test
    void unexpectedExceptionsPropagateAndCloseTheAdapter() throws IOException {
        var closed = new AtomicInteger();
        DynamicTest test = Zin.load(SPEC).tests(() -> new Queue(2) {
            @Override
            public boolean call(String action, Map<String, Object> arguments) {
                throw new IllegalStateException("unavailable");
            }

            @Override
            public void close() {
                closed.incrementAndGet();
            }
        }).findFirst().orElseThrow();
        assertThrows(IllegalStateException.class, test.getExecutable()::execute);
        assertEquals(1, closed.get());
    }

    @Test
    @EnabledIfEnvironmentVariable(named = "ZIN_BIN", matches = ".+")
    void derivesAPairPlanWithTheBinary() throws Throwable {
        var plan = Zin.load(SPEC, new Zin.Options().cover("pairs").pairLimit(3));
        assertEquals("pairs", plan.cover());
        var tests = plan.tests(() -> new Queue(2)).toList();
        assertFalse(tests.isEmpty());
        for (var test : tests) test.getExecutable().execute();
    }
}
