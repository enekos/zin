import io.github.enekos.zin.Zin;
import org.junit.jupiter.api.DynamicTest;
import org.junit.jupiter.api.TestFactory;

import java.nio.file.Path;
import java.util.Map;
import java.util.stream.Stream;

class Zin$MODELTest {
    static class Harness implements Zin.Adapter {
        public boolean call(String action, Map<String, Object> args) {
            // Return true for acceptance and false for a domain refusal.
            switch (action) {
$ACTIONS                default: throw new IllegalArgumentException(action);
            }
        }

        public Map<String, ?> observe() {
            // Return any observable subset of $FIELDS.
            return Map.of();
        }
    }

    @TestFactory Stream<DynamicTest> conformance() throws Exception {
        return Zin.plan(Path.of($SPEC)).tests(Harness::new);
    }
}
