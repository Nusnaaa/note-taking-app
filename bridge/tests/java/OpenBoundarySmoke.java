import java.nio.file.Files;
import java.nio.file.Path;
import org.stemnotebook.backend.NativeBridge;

/** Desktop JNI input-boundary fixture; no Android/device acceptance claim. */
public final class OpenBoundarySmoke {
    private static void rejected(String path) {
        try {
            long host = NativeBridge.open(path);
            NativeBridge.close(host);
            throw new AssertionError("Invalid database path accepted");
        } catch (IllegalStateException expected) {
            if (!"Native notebook operation failed".equals(expected.getMessage())) {
                throw new AssertionError("Unexpected boundary diagnostic", expected);
            }
        }
    }
    public static void main(String[] args) throws Exception {
        Path directory = Files.createTempDirectory("stem-open-boundary-");
        try {
            rejected(null);
            rejected("relative.db");
            rejected("/" + "a".repeat(4096));
            rejected("/" + "é".repeat(2048));
            rejected("/" + "a".repeat(1_000_000));
            try (var files = Files.list(directory)) {
                if (files.findAny().isPresent()) throw new AssertionError("Scratch directory changed");
            }
            Path database = directory.resolve("library.db");
            NativeBridge.close(NativeBridge.open(database.toString()));
            NativeBridge.close(NativeBridge.open(database.toString()));
            System.out.println("PASS: invalid paths rejected; valid library opens and reopens (1)");
        } finally {
            try (var files = Files.list(directory)) {
                for (Path file : files.toList()) Files.delete(file);
            }
            Files.delete(directory);
        }
    }
}
