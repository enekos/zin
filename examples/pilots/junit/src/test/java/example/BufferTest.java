package example;

import io.github.enekos.zin.Zin;
import org.junit.jupiter.api.DynamicTest;
import org.junit.jupiter.api.TestFactory;

import java.io.IOException;
import java.nio.ByteBuffer;
import java.nio.InvalidMarkException;
import java.util.Map;
import java.util.stream.Stream;

import static org.junit.jupiter.api.Assertions.assertFalse;

/** Drives the JDK's ByteBuffer: the position, and mark validity through reset. */
class BufferTest {
    private static class BufferAdapter implements Zin.Adapter {
        private final ByteBuffer buffer = ByteBuffer.allocate(2);

        @Override
        public boolean call(String action, Map<String, Object> args) {
            switch (action) {
                case "advance" -> {
                    if (!buffer.hasRemaining()) return false;
                    buffer.get();
                }
                case "mark" -> buffer.mark();
                case "rewind" -> buffer.rewind();
                case "reset" -> {
                    try {
                        buffer.reset();
                    } catch (InvalidMarkException refusal) {
                        return false;
                    }
                }
                default -> throw new IllegalArgumentException(action);
            }
            return true;
        }

        @Override
        public Map<String, ?> observe() {
            return Map.of("position", buffer.position());
        }
    }

    @TestFactory
    Stream<DynamicTest> byteBuffer() throws IOException {
        var plan = Zin.load(Zin.resource(BufferTest.class, "buffer.zin"));
        String filter = System.getProperty("zin.case", "");
        var selected = plan.tests(BufferAdapter::new).filter(test -> test.getDisplayName().contains(filter)).toList();
        assertFalse(selected.isEmpty(), "no case matches " + filter);
        return selected.stream();
    }
}
