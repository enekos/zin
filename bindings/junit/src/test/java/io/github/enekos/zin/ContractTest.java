package io.github.enekos.zin;

import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import org.junit.jupiter.api.DynamicTest;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.TestFactory;

import java.io.IOException;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.LinkedHashMap;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.function.Supplier;
import java.util.stream.Stream;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

/** The runner contract shared by every binding: bindings/fixtures/contract. */
class ContractTest {
    private static final Path CONTRACT = Path.of("..", "fixtures", "contract");
    private static final ObjectMapper JSON = new ObjectMapper();

    private static JsonNode cases() throws IOException {
        return JSON.readTree(CONTRACT.resolve("cases.json").toFile());
    }

    @TestFactory
    Stream<DynamicTest> load() throws IOException {
        return cases().get("load").valueStream().map(item -> DynamicTest.dynamicTest(item.get("name").asText(), () -> {
            var options = new Zin.Options().binary(CONTRACT.resolve("no-such-zin").toString());
            if (item.has("plan")) options.plan(CONTRACT.resolve(item.get("plan").asText()));
            for (var option : item.path("options").properties()) {
                JsonNode value = option.getValue();
                switch (option.getKey()) {
                    case "cover" -> options.cover(value.asText());
                    case "component" -> options.component(value.asText());
                    case "max_steps" -> options.maxSteps(value.asInt());
                    case "step_budget" -> options.stepBudget(value.asInt());
                    case "pair_limit" -> options.pairLimit(value.asInt());
                    default -> throw new AssertionError("unknown option " + option.getKey());
                }
            }
            Path spec = CONTRACT.resolve(item.get("spec").asText());
            JsonNode expect = item.get("expect");
            if (expect.has("errors")) {
                var error = assertThrows(Zin.PlanException.class, () -> Zin.load(spec, options));
                for (JsonNode fragment : expect.get("errors")) {
                    assertTrue(error.getMessage().contains(fragment.asText()), error.getMessage());
                }
            } else {
                var plan = Zin.load(spec, options);
                assertEquals(expect.get("fingerprint").asText(), plan.fingerprint());
                assertEquals(expect.get("cover").asText(), plan.cover());
            }
        }));
    }

    /** The contract's table-driven adapter (see the contract README). */
    private static final class Scripted implements Zin.Adapter {
        private final JsonNode script;
        private final int instance;
        private final Map<String, Object> state;
        private int calls;

        Scripted(JsonNode script, int instance) {
            this.script = script;
            this.instance = instance;
            this.state = new LinkedHashMap<>(JSON.convertValue(script.get("initial"), Map.class));
        }

        @Override
        public boolean call(String action, Map<String, Object> arguments) {
            calls++;
            for (JsonNode override : script.path("overrides")) {
                if (override.path("instance").asInt(instance) == instance
                        && override.path("call").asInt(calls) == calls
                        && override.path("action").asText(action).equals(action)) {
                    String outcome = override.get("outcome").asText();
                    if (outcome.equals("raise")) throw new IllegalStateException(override.get("message").asText());
                    return outcome.equals("accept");
                }
            }
            JsonNode rule = script.get("rules").get(action);
            for (var requirement : rule.get("requires").properties()) {
                if (!JSON.convertValue(requirement.getValue(), Object.class).equals(state.get(requirement.getKey()))) {
                    return false;
                }
            }
            state.putAll(JSON.convertValue(rule.get("set"), Map.class));
            return true;
        }

        @Override
        public Map<String, ?> observe() {
            return script.get("observe").asBoolean() ? new HashMap<>(state) : null;
        }
    }

    @TestFactory
    Stream<DynamicTest> replay() throws IOException {
        JsonNode cases = cases();
        return cases.get("replay").valueStream().map(item -> DynamicTest.dynamicTest(item.get("name").asText(), () -> {
            var script = cases.get("adapter").deepCopy();
            item.get("adapter").properties().forEach(entry -> ((com.fasterxml.jackson.databind.node.ObjectNode) script)
                    .set(entry.getKey(), entry.getValue()));
            var plan = Zin.load(CONTRACT.resolve(item.get("plan").asText()));
            int[] made = {0};
            Supplier<Zin.Adapter> factory = () -> new Scripted(script, ++made[0]);
            List<DynamicTest> tests = plan.tests(factory).toList();
            JsonNode expect = item.get("expect");
            for (JsonNode index : item.get("tests")) {
                var executable = tests.get(index.asInt()).getExecutable();
                if (expect.path("passes").asBoolean()) {
                    executable.execute();
                } else if (expect.has("raises")) {
                    var error = assertThrows(IllegalStateException.class, executable::execute);
                    assertEquals(expect.get("raises").asText(), error.getMessage());
                } else {
                    var divergence = assertThrows(Zin.Divergence.class, executable::execute);
                    assertEquals(expect.get("report").asText(), divergence.getMessage());
                }
            }
            if (expect.has("factories")) assertEquals(expect.get("factories").asInt(), made[0]);
        }));
    }

    enum Token { open, closed, ann, ben, idle, busy }

    private static Object nativeValue(JsonNode value) {
        if (value.has("$enum")) return Token.valueOf(value.get("$enum").asText());
        if (value.has("$set")) {
            Set<Object> set = new LinkedHashSet<>();
            for (JsonNode item : value.get("$set")) set.add(nativeValue(item));
            return set;
        }
        if (value.has("$record")) return nativeValue(value.get("$record"));
        if (value.has("$map")) {
            Map<Object, Object> map = new LinkedHashMap<>();
            for (JsonNode entry : value.get("$map")) map.put(nativeValue(entry.get(0)), nativeValue(entry.get(1)));
            return map;
        }
        if (value.isObject()) {
            Map<String, Object> map = new LinkedHashMap<>();
            value.properties().forEach(entry -> map.put(entry.getKey(), nativeValue(entry.getValue())));
            return map;
        }
        return JSON.convertValue(value, Object.class);
    }

    @TestFactory
    Stream<DynamicTest> observations() throws IOException {
        List<JsonNode> all = new ArrayList<>();
        for (String name : List.of("observations.json", "native-observations.json")) {
            JSON.readTree(CONTRACT.resolve(name).toFile()).forEach(all::add);
        }
        return all.stream().map(item -> DynamicTest.dynamicTest(item.get("name").asText(), () -> {
            var plan = JSON.createObjectNode();
            plan.put("zin", "plan/1").put("system", "Observation").put("fingerprint", "0000000000000000");
            plan.putArray("actions");
            plan.putArray("calls");
            plan.putArray("states").add(item.get("expected"));
            plan.putArray("parents").addNull();
            plan.putArray("tests").addObject().put("name", "t").putArray("steps");
            Path file = java.nio.file.Files.createTempFile("observation-", ".json");
            JSON.writeValue(file.toFile(), plan);
            @SuppressWarnings("unchecked")
            Map<String, Object> observed = (Map<String, Object>) nativeValue(item.get("observed"));
            var test = Zin.load(file).tests(() -> new Zin.Adapter() {
                @Override
                public boolean call(String action, Map<String, Object> arguments) {
                    return true;
                }

                @Override
                public Map<String, ?> observe() {
                    return observed;
                }
            }).findFirst().orElseThrow();
            if (item.get("matches").asBoolean()) test.getExecutable().execute();
            else assertThrows(Zin.Divergence.class, test.getExecutable()::execute);
        }));
    }

    @Test
    void plansHaveEveryKeyTheSchemaRequires() throws IOException {
        JsonNode schema = JSON.readTree(Path.of("..", "..", "docs", "plan.schema.json").toFile());
        for (String name : List.of("latch.plan.json", "composed/main.plan.json")) {
            JsonNode plan = JSON.readTree(CONTRACT.resolve(name).toFile());
            schema.get("required").forEach(key -> assertTrue(plan.has(key.asText()), name + ": " + key));
            schema.at("/properties/summary/required")
                    .forEach(key -> assertTrue(plan.get("summary").has(key.asText()), name + ": summary." + key));
            assertFalse(plan.isEmpty());
        }
    }
}
