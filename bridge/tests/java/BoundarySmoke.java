import org.stemnotebook.backend.NativeBridge;
import java.nio.file.Files;
import java.util.regex.Pattern;
public final class BoundarySmoke {
    private static void check(boolean condition) { if (!condition) throw new AssertionError("boundary contract failed"); }
    public static void main(String[] args) throws Exception {
        String path=Files.createTempDirectory("stem-jni").resolve("notebook.db").toString();
        long handle=NativeBridge.open(path);
        String request="{\"version\":1,\"command\":{\"op\":\"Create\",\"kind\":\"Note\",\"parent\":null,\"title\":\"λ 日本 😀\"}}";
        String created=NativeBridge.request(handle,request);
        check(created.contains("\"ok\":true") && created.contains("λ 日本 😀"));
        var matcher=Pattern.compile("\"id\":\"([^\"]+)\"").matcher(created);check(matcher.find());String id=matcher.group(1);
        NativeBridge.authenticate(handle,"[{\"Entity\":\""+id+"\"}]",challenge -> !challenge.isEmpty());
        String locked=NativeBridge.request(handle,"{\"version\":1,\"command\":{\"op\":\"SetLock\",\"entity\":\""+id+"\",\"revision\":0,\"locked\":true}}");
        check(locked.contains("\"ok\":true"));
        NativeBridge.request(handle,"{\"version\":1,\"command\":{\"op\":\"Relock\"}}");
        String read="{\"version\":1,\"command\":{\"op\":\"Read\",\"id\":\""+id+"\"}}";
        check(NativeBridge.request(handle,read).contains("authentication required"));
        NativeBridge.close(handle);handle=NativeBridge.open(path);
        check(NativeBridge.request(handle,read).contains("authentication required"));
        try { NativeBridge.request(handle,"malformed private payload");throw new AssertionError(); } catch(IllegalStateException expected) {check(!expected.getMessage().contains("private"));}
        NativeBridge.close(handle);
        try {NativeBridge.request(handle,read);throw new AssertionError();}catch(IllegalStateException expected) { }
        // A verifier waiting on one library must not hold the global registry.
        long blockedHandle=NativeBridge.open(Files.createTempDirectory("stem-jni-blocked").resolve("notebook.db").toString());
        long otherHandle=NativeBridge.open(Files.createTempDirectory("stem-jni-other").resolve("notebook.db").toString());
        var entered=new java.util.concurrent.CountDownLatch(1);
        var release=new java.util.concurrent.CountDownLatch(1);
        var pool=java.util.concurrent.Executors.newFixedThreadPool(2);
        try {
            var authentication=pool.submit(() -> NativeBridge.authenticate(blockedHandle,"[\"App\"]",challenge -> {
                entered.countDown();
                try { return release.await(10,java.util.concurrent.TimeUnit.SECONDS); }
                catch(InterruptedException interrupted) { Thread.currentThread().interrupt(); return false; }
            }));
            check(entered.await(5,java.util.concurrent.TimeUnit.SECONDS));
            var independent=pool.submit(() -> NativeBridge.request(otherHandle,"{\"version\":1,\"command\":{\"op\":\"Status\"}}"));
            check(independent.get(5,java.util.concurrent.TimeUnit.SECONDS).contains("\"ok\":true"));
            release.countDown();
            authentication.get(5,java.util.concurrent.TimeUnit.SECONDS);
        } finally {
            release.countDown();
            pool.shutdownNow();
            NativeBridge.close(blockedHandle);
            NativeBridge.close(otherHandle);
        }
        System.out.println("PASS: JNI Unicode, platform-verifier callback, relock, restart, malformed input, closed handles and independent registry access (7)");
    }
}
