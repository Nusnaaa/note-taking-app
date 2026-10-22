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
        System.out.println("PASS: JNI Unicode, platform-verifier callback, relock, restart, malformed input and closed-handle checks (6)");
    }
}
