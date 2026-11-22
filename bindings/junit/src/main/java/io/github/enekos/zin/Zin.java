package io.github.enekos.zin;

import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import org.junit.jupiter.api.DynamicTest;

import java.io.IOException;
import java.net.URISyntaxException;
import java.net.URL;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Collections;
import java.util.HexFormat;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Objects;
import java.util.TreeMap;
import java.util.function.Supplier;
import java.util.stream.Stream;

/**
 * Replays a Zin test plan as JUnit Jupiter dynamic tests, following the runner
 * contract in docs/plan-format.md.
 *
 * <pre>{@code
 * @TestFactory
 * Stream<DynamicTest> conforms() throws IOException {
 *     return Zin.load(Zin.resource(QueueTest.class, "queue.zin")).tests(QueueAdapter::new);
 * }
 * }</pre>
 */
public final class Zin {
    /** The plan file format this binding reads. */
    public static final String PLAN_FORMAT = "plan/1";
    // How many of the latest steps a report lists before eliding earlier ones.
    private static final int LISTED_STEPS = 8;
    private static final ObjectMapper JSON = new ObjectMapper();

    private Zin() {}

    /** Connects the implementation under test to the specification. */
    public interface Adapter extends AutoCloseable {
        /**
         * Performs one action. Return false for a refusal that changed nothing;
         * any exception fails the test with that exception.
         */
        boolean call(String action, Map<String, Object> arguments) throws Exception;

        /** Any subset of the specification's state fields, or null to compare nothing. */
        default Map<String, ?> observe() throws Exception {
            return null;
        }

        @Override
        default void close() throws Exception {}
    }

    /** The plan could not be loaded or derived. */
    public static final class PlanException extends IOException {
        PlanException(String message) {
            super(message);
        }

        PlanException(String message, Throwable cause) {
            super(message, cause);
        }
    }

    /** The implementation does not behave as the specification says. */
    public static final class Divergence extends AssertionError {
        private final String fingerprint;
        private final String scenario;

        Divergence(String report, String fingerprint, String scenario) {
            super(report);
            this.fingerprint = fingerprint;
            this.scenario = scenario;
        }

        /** The fingerprint of the specification the plan was derived from. */
        public String fingerprint() {
            return fingerprint;
        }

        /** The original failing steps as a Zin scenario, ready to paste. */
        public String scenario() {
            return scenario;
        }
    }

    /**
     * Planning options. Only the options set are compared with a committed
     * plan's recorded ones.
     */
    public static final class Options {
        private final Map<String, Object> planning = new LinkedHashMap<>();
        private Path plan;
        private String binary;

        /** transitions, states, actions, scenarios or pairs. */
        public Options cover(String cover) {
            planning.put("cover", Objects.requireNonNull(cover));
            return this;
        }

        /** Soft target for tour length ({@code zin plan --max-steps}). */
        public Options maxSteps(int steps) {
            return positive("max_steps", steps);
        }

        /** Hard limit on all emitted replay steps ({@code zin plan --step-budget}). */
        public Options stepBudget(int steps) {
            return positive("step_budget", steps);
        }

        /** With cover "pairs", how many adjacent-call pairs to sample. */
        public Options pairLimit(int pairs) {
            return positive("pair_limit", pairs);
        }

        /** Test one block of a composition in isolation. */
        public Options component(String name) {
            planning.put("component", Objects.requireNonNull(name));
            return this;
        }

        /** A committed plan to use instead of the sibling {@code <name>.plan.json}. */
        public Options plan(Path path) {
            this.plan = path;
            return this;
        }

        /** The zin binary; defaults to {@code $ZIN_BIN}, then {@code zin} on PATH. */
        public Options binary(String path) {
            this.binary = path;
            return this;
        }

        private Options positive(String key, int value) {
            if (value < 1) throw new IllegalArgumentException(key + " must be a positive integer, not " + value);
            planning.put(key, value);
            return this;
        }
    }

    /** A specification or plan copied to the test classpath, as a file path. */
    public static Path resource(Class<?> anchor, String name) {
        URL url = anchor.getResource("/" + name);
        if (url == null) throw new IllegalArgumentException(name + " is not on the test classpath");
        try {
            return Path.of(url.toURI());
        } catch (URISyntaxException error) {
            throw new IllegalArgumentException(error);
        }
    }

    /** FNV-1a 64 of the specification bytes, as {@code zin plan} records it. */
    public static String fingerprint(byte[] source) {
        long hash = 0xcbf29ce484222325L;
        for (byte value : source) hash = (hash ^ (value & 0xff)) * 0x100000001b3L;
        return HexFormat.of().toHexDigits(hash);
    }

    public static Plan load(Path spec) throws IOException {
        return load(spec, new Options());
    }

    /**
     * The plan for {@code spec}: the explicit plan, else a sibling
     * {@code <name>.plan.json}, as long as it is fresh and was derived with
     * every option set; otherwise the output of {@code zin plan}. {@code spec}
     * may itself be a plan file.
     */
    public static Plan load(Path spec, Options options) throws IOException {
        String name = spec.getFileName().toString();
        List<String> args = new ArrayList<>(List.of("plan", spec.toString()));
        for (var option : options.planning.entrySet()) {
            args.add("--" + option.getKey().replace('_', '-'));
            args.add(String.valueOf(option.getValue()));
        }
        if (name.endsWith(".json")) {
            if (!options.planning.isEmpty() || options.plan != null) {
                throw new PlanException("planning options need a .zin specification, not a plan file");
            }
            JsonNode plan = readPlan(spec);
            if (name.endsWith(".plan.json")) {
                Path source = spec.resolveSibling(name.substring(0, name.length() - ".plan.json".length()) + ".zin");
                if (Files.exists(source) && !fresh(plan, source)) {
                    throw new PlanException(spec + " was derived from a different version of " + source.getFileName()
                            + "; regenerate it with: zin plan " + source + " --out " + spec);
                }
            }
            return new Plan(plan);
        }
        if (!Files.isRegularFile(spec)) throw new PlanException("cannot read specification " + spec);
        Path sibling = spec.resolveSibling(name.replaceFirst("\\.[^.]+$", "") + ".plan.json");
        Path cached = options.plan != null ? options.plan : sibling;
        List<String> regenerate = new ArrayList<>(args);
        regenerate.addAll(List.of("--out", cached.toString()));
        String command = "zin " + String.join(" ", regenerate);
        String reason = "";
        if (options.plan != null || Files.exists(cached)) {
            JsonNode plan = readPlan(cached);
            if (!fresh(plan, spec)) {
                reason = cached + " was derived from a different version of " + name;
            } else if (!compatible(plan, options.planning, options.plan != null)) {
                reason = cached + " was derived with different planning options";
            } else {
                return new Plan(plan);
            }
            if (options.plan != null) throw new PlanException(reason + "; regenerate it with: " + command);
        }
        String prefix = reason.isEmpty() ? "" : reason + "; ";
        String binary = options.binary != null ? options.binary : System.getenv().getOrDefault("ZIN_BIN", "zin");
        if (binary.isEmpty()) binary = "zin";
        List<String> invocation = new ArrayList<>(List.of(binary));
        invocation.addAll(args);
        Process process;
        try {
            process = new ProcessBuilder(invocation).start();
        } catch (IOException error) {
            throw new PlanException(prefix + "the zin binary was not found. Install zin-spec, set ZIN_BIN, "
                    + "or regenerate the plan: " + command, error);
        }
        try {
            process.getOutputStream().close();
            var stderr = new java.io.ByteArrayOutputStream();
            Thread drain = new Thread(() -> {
                try {
                    process.getErrorStream().transferTo(stderr);
                } catch (IOException ignored) {
                    // The exit status reports the failure.
                }
            });
            drain.start();
            byte[] stdout = process.getInputStream().readAllBytes();
            int status = process.waitFor();
            drain.join();
            if (status != 0) {
                throw new PlanException("zin plan " + spec + " failed:\n" + stderr.toString(StandardCharsets.UTF_8).trim());
            }
            JsonNode plan;
            try {
                plan = JSON.readTree(stdout);
            } catch (IOException error) {
                throw new PlanException(binary + " did not print a plan: " + error.getMessage(), error);
            }
            return new Plan(validate(plan, binary));
        } catch (InterruptedException error) {
            Thread.currentThread().interrupt();
            throw new PlanException("interrupted while running " + binary, error);
        } finally {
            process.destroy();
        }
    }

    private static JsonNode readPlan(Path path) throws IOException {
        JsonNode plan;
        try {
            plan = JSON.readTree(path.toFile());
        } catch (IOException error) {
            throw new PlanException("cannot read plan " + path + ": " + error.getMessage(), error);
        }
        return validate(plan, path);
    }

    /**
     * The fingerprint {@code zin plan} would record now, or null if a file is
     * unreadable. With included files, the entry file's hash and each source's
     * current hash, sorted by path, are hashed together.
     */
    private static String currentFingerprint(Path spec, JsonNode sources) {
        try {
            String entry = fingerprint(Files.readAllBytes(spec));
            if (sources == null || sources.isNull() || sources.isEmpty()) return entry;
            // Byte order of UTF-8 equals code point order, which String.compareTo
            // matches outside the surrogate range.
            var sorted = new TreeMap<String, String>();
            for (var source : sources.properties()) {
                Path file = spec.toAbsolutePath().getParent().resolve(source.getKey());
                sorted.put(source.getKey(), fingerprint(Files.readAllBytes(file)));
            }
            StringBuilder text = new StringBuilder(entry).append('\n');
            sorted.forEach((path, hash) -> text.append(path).append('\t').append(hash).append('\n'));
            return fingerprint(text.toString().getBytes(StandardCharsets.UTF_8));
        } catch (IOException error) {
            return null;
        }
    }

    private static boolean fresh(JsonNode plan, Path spec) {
        return Objects.equals(currentFingerprint(spec, plan.get("sources")), plan.path("fingerprint").asText(null));
    }

    // The plan keys that shape coverage; "component" is identity, not an option.
    private static final List<String> PLANNING_OPTIONS = List.of("cover", "max_steps", "step_budget", "pair_limit");

    // Passed options must match; the component is identity. A sibling picked
    // automatically must also match the plan's recorded defaults for every
    // option left out, so weaker coverage is never chosen silently. An explicit
    // plan is the opt-in to whatever it was derived with.
    private static boolean compatible(JsonNode plan, Map<String, Object> requested, boolean explicit) {
        JsonNode component = plan.get("component");
        String recorded = component == null || component.isNull() ? null : component.asText();
        if (!Objects.equals(recorded, requested.get("component"))) return false;
        JsonNode defaults = plan.get("defaults");
        for (String key : PLANNING_OPTIONS) {
            JsonNode value = plan.get(key);
            if (requested.containsKey(key)) {
                Object wanted = requested.get(key);
                boolean same = value != null && (wanted instanceof Integer number
                        ? value.isIntegralNumber() && value.longValue() == number
                        : value.isTextual() && value.textValue().equals(wanted));
                if (!same) return false;
            } else if (!explicit) {
                JsonNode fallback = defaults == null || !defaults.isObject() ? null : defaults.get(key);
                if (value == null || fallback == null || !value.equals(fallback)) return false;
            }
        }
        return true;
    }

    private static JsonNode validate(JsonNode plan, Object origin) throws PlanException {
        if (plan == null || !PLAN_FORMAT.equals(plan.path("zin").asText(null))) {
            throw new PlanException(origin + " is not a " + PLAN_FORMAT + " file");
        }
        Map<String, Integer> params = new LinkedHashMap<>();
        for (JsonNode action : array(plan, "actions", origin)) {
            params.put(action.path("name").asText(), array(action, "params", origin).size());
        }
        JsonNode calls = array(plan, "calls", origin);
        for (int index = 0; index < calls.size(); index++) {
            JsonNode call = calls.get(index);
            String action = call.path("action").asText();
            String text = call.path("text").asText();
            Integer wanted = params.get(action);
            if (wanted == null) {
                throw new PlanException("the plan is malformed: call " + index + " (" + text
                        + ") uses the unknown action " + action);
            }
            int given = array(call, "args", origin).size();
            if (given != wanted) {
                throw new PlanException("the plan is malformed: call " + index + " (" + text + ") has " + given
                        + " arguments, but " + action + " takes " + wanted);
            }
        }
        int states = array(plan, "states", origin).size();
        if (states == 0 || array(plan, "parents", origin).size() != states) {
            throw new PlanException("the plan is malformed: states and parents do not line up");
        }
        for (JsonNode test : array(plan, "tests", origin)) {
            for (JsonNode step : array(test, "steps", origin)) {
                if (!step.isArray() || step.size() != 2) {
                    throw new PlanException("the plan is malformed: step " + step + " is not [call, to]");
                }
                int call = step.get(0).asInt(-2);
                int to = step.get(1).asInt(-2);
                if (call < 0 || call >= calls.size() || to < -1 || to >= states) {
                    throw new PlanException("the plan is malformed: step " + step + " is out of range");
                }
            }
        }
        return plan;
    }

    private static JsonNode array(JsonNode node, String key, Object origin) throws PlanException {
        JsonNode value = node.get(key);
        if (value == null || !value.isArray()) {
            throw new PlanException("the plan is malformed: " + origin + ": expected an array " + key);
        }
        return value;
    }

    private record Call(String action, String text, Map<String, Object> args) {}

    private record Step(int call, int to) {}

    private record Test(String name, List<Step> steps) {}

    private record Found(int step, String message) {}

    /** A loaded, validated plan. */
    public static final class Plan {
        private final String system;
        private final String fingerprint;
        private final String cover;
        private final List<JsonNode> states = new ArrayList<>();
        private final List<Step> parents = new ArrayList<>();
        private final List<Call> calls = new ArrayList<>();
        private final List<Test> tests = new ArrayList<>();

        private Plan(JsonNode plan) {
            system = plan.path("system").asText();
            fingerprint = plan.path("fingerprint").asText("");
            cover = plan.path("cover").asText();
            plan.get("states").forEach(states::add);
            for (JsonNode parent : plan.get("parents")) {
                parents.add(parent.isNull() ? null : new Step(parent.get(1).asInt(), parent.get(0).asInt()));
            }
            Map<String, List<String>> params = new LinkedHashMap<>();
            for (JsonNode action : plan.get("actions")) {
                List<String> names = new ArrayList<>();
                action.get("params").forEach(param -> names.add(param.path("name").asText()));
                params.put(action.path("name").asText(), names);
            }
            for (JsonNode call : plan.get("calls")) {
                String action = call.path("action").asText();
                List<String> names = params.get(action);
                Map<String, Object> args = new LinkedHashMap<>();
                for (int i = 0; i < names.size(); i++) args.put(names.get(i), argument(call.get("args").get(i)));
                calls.add(new Call(action, call.path("text").asText(), Collections.unmodifiableMap(args)));
            }
            for (JsonNode test : plan.get("tests")) {
                List<Step> steps = new ArrayList<>();
                test.get("steps").forEach(step -> steps.add(new Step(step.get(0).asInt(), step.get(1).asInt())));
                tests.add(new Test(test.path("name").asText(), List.copyOf(steps)));
            }
        }

        private static Object argument(JsonNode value) {
            if (value.isBoolean()) return value.booleanValue();
            if (value.isIntegralNumber()) return value.longValue();
            return value.asText();
        }

        public String system() {
            return system;
        }

        public String fingerprint() {
            return fingerprint;
        }

        public String cover() {
            return cover;
        }

        /** One dynamic test per plan test, each from fresh adapters made by {@code factory}. */
        public Stream<DynamicTest> tests(Supplier<? extends Adapter> factory) {
            return tests.stream().map(test -> DynamicTest.dynamicTest(test.name(), () -> check(test, factory)));
        }

        private String text(Step step) {
            String text = calls.get(step.call()).text();
            return step.to() >= 0 ? text : "reject " + text;
        }

        private String listing(List<Step> steps) {
            int skipped = Math.max(0, steps.size() - LISTED_STEPS);
            List<String> lines = new ArrayList<>();
            if (skipped > 0) lines.add("  ... " + skipped + " earlier steps");
            for (int i = skipped; i < steps.size(); i++) lines.add("  " + (i + 1) + ". " + text(steps.get(i)));
            return String.join("\n", lines);
        }

        private void check(Test test, Supplier<? extends Adapter> factory) throws Exception {
            Found found = replay(factory, test.steps());
            if (found == null) return;
            List<Step> taken = test.steps().subList(0, found.step());
            List<String> lines = new ArrayList<>();
            for (Step step : taken) lines.add("  " + text(step));
            if (lines.isEmpty()) lines.add("  expect true");
            String scenario = "scenario \"Replay divergence\"\n" + String.join("\n", lines);
            String report = system + ": implementation diverges from the specification in test \"" + test.name()
                    + "\" at step " + found.step() + "\n" + found.message() + "\nsteps taken:\n" + listing(taken)
                    + "\nspec fingerprint: " + fingerprint + "\n" + scenario;
            if (found.step() > 0) {
                List<Step> shorter = shortest(test.steps(), found.step());
                if (shorter.size() < taken.size()) {
                    try {
                        Found again = replay(factory, shorter);
                        report += again != null && again.step() == shorter.size() && again.message().equals(found.message())
                                ? "\nshortest reproduction:\n" + listing(shorter)
                                : "\nthe shortest path to the same specification state does not reproduce the original mismatch";
                    } catch (Exception error) {
                        report += "\nthe shorter replay raised: " + error.getMessage();
                    }
                }
            }
            throw new Divergence(report, fingerprint, scenario);
        }

        private Found replay(Supplier<? extends Adapter> factory, List<Step> steps) throws Exception {
            try (Adapter adapter = factory.get()) {
                String message = compare(adapter, 0, "before any action");
                if (message != null) return new Found(0, message);
                int state = 0;
                for (int i = 0; i < steps.size(); i++) {
                    Step step = steps.get(i);
                    Call call = calls.get(step.call());
                    boolean accepted = adapter.call(call.action(), call.args());
                    if (step.to() >= 0 && accepted) {
                        state = step.to();
                        message = compare(adapter, state, "after " + call.text());
                    } else if (step.to() < 0 && !accepted) {
                        message = compare(adapter, state, "after rejecting " + call.text());
                    } else if (accepted) {
                        message = "the implementation accepted " + call.text() + ", but the specification rejects it";
                    } else {
                        message = "the implementation rejected " + call.text() + ", but the specification allows it";
                    }
                    if (message != null) return new Found(i + 1, message);
                }
                return null;
            }
        }

        private String compare(Adapter adapter, int state, String after) throws Exception {
            Map<String, ?> observed = adapter.observe();
            if (observed == null) return null;
            JsonNode expected = states.get(state);
            for (var entry : new TreeMap<String, Object>(observed).entrySet()) {
                JsonNode wanted = expected.get(entry.getKey());
                if (wanted == null) {
                    return "observe() returned '" + entry.getKey() + "', which is not a state field of the specification";
                }
                Object actual = plain(entry.getValue());
                if (!same(wanted, actual)) {
                    return after + ", " + entry.getKey() + " is " + show(actual) + ", but the specification says "
                            + show(plain(JSON.convertValue(wanted, Object.class)));
                }
            }
            return null;
        }

        private List<Step> shortest(List<Step> steps, int failed) {
            int state = 0;
            for (Step step : steps.subList(0, failed - 1)) if (step.to() >= 0) state = step.to();
            List<Step> path = new ArrayList<>();
            while (parents.get(state) != null) {
                Step parent = parents.get(state);
                path.add(new Step(parent.call(), state));
                state = parent.to();
            }
            Collections.reverse(path);
            path.add(steps.get(failed - 1));
            return path;
        }
    }

    /** An observed value in the plan's vocabulary: Long, Boolean, String or a map of them. */
    static Object plain(Object value) {
        if (value instanceof Enum<?> member) return member.toString();
        if (value instanceof Boolean || value instanceof String || value == null) return value;
        if (value instanceof Byte || value instanceof Short || value instanceof Integer || value instanceof Long) {
            return ((Number) value).longValue();
        }
        if (value instanceof Map<?, ?> map) {
            Map<String, Object> out = new LinkedHashMap<>();
            map.forEach((key, item) -> out.put(String.valueOf(plain(key)), plain(item)));
            return out;
        }
        return value;
    }

    private static boolean same(JsonNode expected, Object observed) {
        if (expected.isObject()) {
            if (!(observed instanceof Map<?, ?> map)) return false;
            for (var entry : map.entrySet()) {
                JsonNode wanted = expected.get((String) entry.getKey());
                if (wanted == null || !same(wanted, entry.getValue())) return false;
            }
            return true;
        }
        if (expected.isBoolean()) return observed instanceof Boolean flag && flag == expected.booleanValue();
        if (expected.isIntegralNumber()) return observed instanceof Long number && number == expected.longValue();
        if (expected.isTextual()) return expected.textValue().equals(observed);
        return false;
    }

    private static String show(Object value) {
        if (value instanceof Map<?, ?> map) {
            List<String> parts = new ArrayList<>();
            map.forEach((key, item) -> parts.add(key + ": " + show(item)));
            return "{" + String.join(", ", parts) + "}";
        }
        return String.valueOf(value);
    }
}
