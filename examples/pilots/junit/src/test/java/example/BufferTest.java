package example;

import io.github.enekos.zin.Zin;
import org.junit.jupiter.api.DynamicTest;
import org.junit.jupiter.api.TestFactory;

import java.nio.ByteBuffer;
import java.nio.InvalidMarkException;
import java.nio.file.Path;
import java.util.Map;
import java.util.stream.Stream;

import static org.junit.jupiter.api.Assertions.assertFalse;

class BufferTest {
    private static class BufferAdapter implements Zin.Adapter {
        private final ByteBuffer buffer = ByteBuffer.allocate(2);

        public boolean call(String action, Map<String, Object> args) {
            switch (action) {
                case "advance" -> {
                    if (!buffer.hasRemaining()) return false;
                    buffer.get();
                }
                case "mark" -> buffer.mark();
                case "rewind" -> buffer.rewind();
                case "reset" -> {
                    try { buffer.reset(); }
                    catch (InvalidMarkException refusal) { return false; }
                }
                default -> throw new IllegalArgumentException(action);
            }
            return true;
        }

        public Map<String, ?> observe() { return Map.of("position", buffer.position()); }
    }

    @TestFactory Stream<DynamicTest> byteBuffer() throws Exception {
        var plan = Zin.plan(Path.of("buffer.zin"));
        String filter = System.getProperty("zin.case", "");
        var selected = plan.tests(BufferAdapter::new)
                .filter(test -> test.getDisplayName().contains(filter)).toList();
        assertFalse(selected.isEmpty(), "no case matches " + filter);
        return selected.stream();
    }
}
