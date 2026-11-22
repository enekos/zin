package io.github.enekos.zin;

import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import org.junit.jupiter.api.DynamicTest;

import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Collections;
import java.util.HashMap;
import java.util.HexFormat;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.function.Supplier;
import java.util.stream.Stream;

/** Replays verified Zin plans as individual JUnit Jupiter dynamic tests. */
public final class Zin {
    private static final ObjectMapper JSON = new ObjectMapper();

    private Zin() {}

    public interface Adapter extends AutoCloseable {
        /** Return false only for a domain refusal. Unexpected exceptions reach JUnit. */
        boolean call(String action, Map<String, Object> arguments) throws Exception;

        default Map<String, ?> observe() throws Exception { return Map.of(); }
        @Override default void close() throws Exception {}
    }

    /** Derive the plan with the installed Zin binary. */
    public static Plan plan(Path spec) throws IOException, InterruptedException {
        return plan(spec, "transitions");
    }

    public static Plan plan(Path spec, String cover) throws IOException, InterruptedException {
        return plan(spec, cover, Map.of());
    }

    /** Additional options use CLI spellings, such as "pair-limit" or "component". */
    public static Plan plan(Path spec, String cover, Map<String, String> options) throws IOException, InterruptedException {
        var command = new ArrayList<>(List.of(System.getenv().getOrDefault("ZIN_BIN", "zin"),
                "plan", spec.toString(), "--cover", cover));
        var supported = Set.of("max-steps", "step-budget", "pair-limit", "component");
        for (var option : options.entrySet()) {
            if (!supported.contains(option.getKey())) throw new IllegalArgumentException("unknown plan option " + option.getKey());
            command.add("--" + option.getKey());
            command.add(option.getValue());
        }
        Path output = Files.createTempFile("zin-junit-", ".json");
        try {
            command.add("--out");
            command.add(output.toString());
            Process process = new ProcessBuilder(command).redirectErrorStream(true).start();
            try {
                String diagnostic = new String(process.getInputStream().readAllBytes(), StandardCharsets.UTF_8);
                if (process.waitFor() != 0) throw new IOException("zin plan failed:\n" + diagnostic);
                return read(spec, output);
            } finally {
                process.destroy();
            }
        } finally {
            Files.deleteIfExists(output);
        }
    }

    /** Load a committed plan after checking every source. Works without a Zin binary. */
    public static Plan read(Path spec, Path planFile) throws IOException {
        JsonNode data = JSON.readTree(planFile.toFile());
        if (data == null || !"plan/1".equals(data.path("zin").asText())) {
            throw new IOException("expected plan/1 in " + planFile);
        }
        String fingerprint = data.path("fingerprint").asText();
        checkFingerprint(spec, fingerprint);
        JsonNode sources = data.get("sources");
        if (sources != null) {
            if (!sources.isObject()) throw new IOException("expected a sources object in " + planFile);
            for (var source : sources.properties()) {
                checkFingerprint(spec.toAbsolutePath().getParent().resolve(source.getKey()), source.getValue().asText());
            }
        }
        return new Plan(data);
    }

    private static void checkFingerprint(Path source, String expected) throws IOException {
        long hash = 0xcbf29ce484222325L;
        for (byte value : Files.readAllBytes(source)) hash = (hash ^ (value & 0xff)) * 0x100000001b3L;
        if (!HexFormat.of().toHexDigits(hash).equals(expected)) {
            throw new IOException("stale plan for " + source + "; regenerate it with zin plan");
        }
    }

    private record Call(String name, String text, Map<String, Object> args) {}
    private record Step(int call, int to) {}
    private record Tour(String name, List<Step> steps) {}
    private record Failure(int at, String detail) {}

    public static final class Plan {
        private final String fingerprint;
        private final List<Call> calls = new ArrayList<>();
        private final List<Step> parents = new ArrayList<>();
        private final List<Tour> tours = new ArrayList<>();
        private final JsonNode states;

        private Plan(JsonNode data) throws IOException {
            fingerprint = data.path("fingerprint").asText();
            states = array(data, "states");
            if (states.isEmpty()) throw new IOException("plan has no initial state");
            Map<String, List<String>> params = new HashMap<>();
            for (JsonNode action : array(data, "actions")) {
                var names = new ArrayList<String>();
                for (JsonNode param : array(action, "params")) names.add(string(param, "name"));
                params.put(string(action, "name"), names);
            }
            for (JsonNode call : array(data, "calls")) {
                String name = string(call, "action");
                JsonNode args = array(call, "args");
                List<String> names = params.get(name);
                if (names == null || args.size() != names.size()) throw new IOException("invalid call to " + name);
                Map<String, Object> named = new LinkedHashMap<>();
                for (int i = 0; i < names.size(); i++) {
                    JsonNode value = args.get(i);
                    Object argument;
                    if (value.isTextual()) argument = value.textValue();
                    else if (value.isBoolean()) argument = value.booleanValue();
                    else if (value.isIntegralNumber() && value.canConvertToLong()) argument = value.longValue();
                    else throw new IOException("invalid argument for " + name);
                    named.put(names.get(i), argument);
                }
                calls.add(new Call(name, string(call, "text"), Collections.unmodifiableMap(named)));
            }
            JsonNode tree = array(data, "parents");
            if (tree.size() != states.size() || !tree.get(0).isNull()) throw new IOException("invalid parent tree");
            parents.add(null);
            for (int child = 1; child < tree.size(); child++) {
                JsonNode pair = tree.get(child);
                validatePair(pair);
                int from = pair.get(0).intValue(), call = pair.get(1).intValue();
                if (from < 0 || from >= child || call < 0 || call >= calls.size()) {
                    throw new IOException("invalid shortest path to state " + child);
                }
                parents.add(new Step(call, from));
            }
            for (JsonNode test : array(data, "tests")) {
                var steps = new ArrayList<Step>();
                for (JsonNode pair : array(test, "steps")) {
                    validatePair(pair);
                    int call = pair.get(0).intValue(), to = pair.get(1).intValue();
                    if (call < 0 || call >= calls.size() || to < -1 || to >= states.size()) {
                        throw new IOException("invalid test step " + pair);
                    }
                    steps.add(new Step(call, to));
                }
                tours.add(new Tour(string(test, "name"), List.copyOf(steps)));
            }
        }

        /** Use in a @TestFactory. Tests are independent and safe to run concurrently. */
        public Stream<DynamicTest> tests(Supplier<? extends Adapter> factory) {
            return tours.stream().map(tour -> DynamicTest.dynamicTest(tour.name(), () -> check(tour, factory)));
        }

        private void check(Tour tour, Supplier<? extends Adapter> factory) throws Exception {
            Failure failure = replay(tour.steps(), factory);
            if (failure == null) return;
            List<Step> prefix = tour.steps().subList(0, failure.at());
            String report = tour.name() + ", step " + failure.at() + ": " + failure.detail()
                    + "\nspec " + fingerprint + "\n" + scenario(prefix);
            if (!prefix.isEmpty()) {
                List<Step> shorter = shortest(prefix);
                if (shorter.size() < prefix.size()) {
                    Failure again = replay(shorter, factory);
                    if (again != null && again.at() == shorter.size() && again.detail().equals(failure.detail())) {
                        report += "\nshortest reproduction:\n" + scenario(shorter);
                    }
                }
            }
            throw new AssertionError(report);
        }

        private Failure replay(List<Step> steps, Supplier<? extends Adapter> factory) throws Exception {
            try (Adapter adapter = factory.get()) {
                int state = 0;
                String problem = compare(states.get(0), JSON.valueToTree(adapter.observe()), "state");
                if (problem != null) return new Failure(0, problem);
                for (int i = 0; i < steps.size(); i++) {
                    Step step = steps.get(i);
                    Call call = calls.get(step.call());
                    boolean accepted = adapter.call(call.name(), call.args());
                    if (accepted != (step.to() >= 0)) {
                        return new Failure(i + 1, call.text() + (accepted ? " was accepted, expected rejection"
                                : " was rejected, expected acceptance"));
                    }
                    if (accepted) state = step.to();
                    problem = compare(states.get(state), JSON.valueToTree(adapter.observe()), "state");
                    if (problem != null) return new Failure(i + 1, "after " + call.text() + ", " + problem);
                }
                return null;
            }
        }

        private List<Step> shortest(List<Step> prefix) {
            int state = 0;
            for (Step step : prefix.subList(0, prefix.size() - 1)) if (step.to() >= 0) state = step.to();
            List<Step> path = new ArrayList<>();
            while (state != 0) {
                Step parent = parents.get(state);
                path.add(new Step(parent.call(), state));
                state = parent.to();
            }
            Collections.reverse(path);
            path.add(prefix.get(prefix.size() - 1));
            return path;
        }

        private String scenario(List<Step> steps) {
            StringBuilder text = new StringBuilder("scenario \"Observed divergence\"");
            if (steps.isEmpty()) text.append("\n  expect true");
            for (Step step : steps) {
                text.append("\n  ");
                if (step.to() < 0) text.append("reject ");
                text.append(calls.get(step.call()).text());
            }
            return text.toString();
        }
    }

    private static String compare(JsonNode expected, JsonNode observed, String path) {
        if (expected == null) return "unknown observed field " + path;
        if (expected.isObject() && observed.isObject()) {
            for (var entry : observed.properties()) {
                String problem = compare(expected.get(entry.getKey()), entry.getValue(), path + "." + entry.getKey());
                if (problem != null) return problem;
            }
            return null;
        }
        boolean equal = expected.isIntegralNumber() && observed.isIntegralNumber()
                ? expected.bigIntegerValue().equals(observed.bigIntegerValue()) : expected.equals(observed);
        return equal ? null : path + " was " + observed + ", expected " + expected;
    }

    private static String string(JsonNode node, String key) throws IOException {
        if (!node.path(key).isTextual()) throw new IOException("expected string " + key + " in plan");
        return node.get(key).textValue();
    }

    private static JsonNode array(JsonNode node, String key) throws IOException {
        JsonNode value = node.path(key);
        if (!value.isArray()) throw new IOException("expected array " + key + " in plan");
        return value;
    }

    private static void validatePair(JsonNode pair) throws IOException {
        if (!pair.isArray() || pair.size() != 2 || !pair.get(0).isIntegralNumber()
                || !pair.get(0).canConvertToInt() || !pair.get(1).isIntegralNumber() || !pair.get(1).canConvertToInt()) {
            throw new IOException("expected a pair of integers in plan");
        }
    }
}
