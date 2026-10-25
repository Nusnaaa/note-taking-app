import java.nio.file.Files;
import java.nio.file.Path;
import org.stemnotebook.backend.NativeBridge;

/** Desktop JNI input-boundary fixture; no Android/device acceptance claim. */
public final class OpenBoundarySmoke {
    private static void rejected(String path) { rejected(path, false); }
    private static void rejected(String path, boolean existing) {
        try {
            long host = existing ? NativeBridge.openExisting(path) : NativeBridge.open(path);
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
            Path missing=directory.resolve("missing.db");
            rejected(missing.toString(),true);
            if(Files.exists(missing)) throw new AssertionError("Strict open created a file");
            Path empty=directory.resolve("empty.db");Files.write(empty,new byte[0]);
            rejected(empty.toString(),true);
            if(Files.size(empty)!=0) throw new AssertionError("Strict open initialized an empty file");
            Path invalid=directory.resolve("invalid.db");Files.write(invalid,new byte[]{1,2,3});
            rejected(invalid.toString(),true);
            if(!java.util.Arrays.equals(Files.readAllBytes(invalid),new byte[]{1,2,3})) throw new AssertionError("Invalid input changed");
            NativeBridge.close(NativeBridge.open(database.toString()));
            long owner=NativeBridge.openExisting(database.toString());
            try {
                rejected(database.toString(),true);
                String result=NativeBridge.request(owner,"{\"version\":1,\"command\":{\"op\":\"Create\",\"kind\":\"Note\",\"parent\":null,\"title\":\"locks retained\"}}");
                if(!result.contains("\"ok\":true")) throw new AssertionError("Failed duplicate disturbed writer locks");
                rejected(database.toString(),true);
            } finally {NativeBridge.close(owner);}
            NativeBridge.close(NativeBridge.openExisting(database.toString()));
            System.out.println("PASS: invalid paths rejected; valid library opens and reopens (1)");
        } finally {
            try (var files = Files.list(directory)) {
                for (Path file : files.toList()) Files.delete(file);
            }
            Files.delete(directory);
        }
    }
}
