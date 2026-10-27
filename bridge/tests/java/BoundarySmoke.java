import org.stemnotebook.backend.NativeBridge;
import java.nio.file.Files;
import java.util.regex.Pattern;
public final class BoundarySmoke {
    private static void check(boolean condition) { if (!condition) throw new AssertionError("boundary contract failed"); }
    private static final class Proof implements NativeBridge.Verification {
        public boolean verify(String challenge) { return !challenge.isEmpty(); }
    }
    public static void main(String[] args) throws Exception {
        check(NativeBridge.help("BackupStatus").contains("Publication history records past"));
        String guidance=NativeBridge.help("RestoreCleanup");
        check(guidance.contains("cannot be undone") && guidance.contains("Quarantine preserves"));
        for(String invalid:new String[]{null,"unknown private topic","x".repeat(10000)}) {
            try {NativeBridge.help(invalid);throw new AssertionError("Invalid help topic accepted");}
            catch(IllegalStateException expected) {check(expected.getMessage().equals("Native notebook operation failed"));}
        }
        System.out.println("PASS: bounded offline recovery Help without a library handle (1)");
        String path=Files.createTempDirectory("stem-jni").resolve("notebook.db").toString();
        long handle=NativeBridge.open(path);
        String identity=NativeBridge.captureLibraryIdentity(handle);check(!identity.isEmpty());
        check(NativeBridge.backupGeneration(handle).matches("[0-9]{1,20}"));
        String request="{\"version\":1,\"command\":{\"op\":\"Create\",\"kind\":\"Note\",\"parent\":null,\"title\":\"λ 日本 😀\"}}";
        String created=NativeBridge.request(handle,request);
        check(created.contains("\"ok\":true") && created.contains("λ 日本 😀"));
        var matcher=Pattern.compile("\"id\":\"([^\"]+)\"").matcher(created);check(matcher.find());String id=matcher.group(1);
        NativeBridge.authenticate(handle,"[{\"Entity\":\""+id+"\"}]",new Proof());
        String locked=NativeBridge.request(handle,"{\"version\":1,\"command\":{\"op\":\"SetLock\",\"entity\":\""+id+"\",\"revision\":0,\"locked\":true}}");
        check(locked.contains("\"ok\":true"));
        NativeBridge.request(handle,"{\"version\":1,\"command\":{\"op\":\"Relock\"}}");
        String read="{\"version\":1,\"command\":{\"op\":\"Read\",\"id\":\""+id+"\"}}";
        check(NativeBridge.request(handle,read).contains("authentication required"));
        NativeBridge.close(handle);handle=NativeBridge.open(path);check(identity.equals(NativeBridge.captureLibraryIdentity(handle)));
        check(NativeBridge.request(handle,read).contains("authentication required"));
        try { NativeBridge.request(handle,"malformed private payload");throw new AssertionError(); } catch(IllegalStateException expected) {check(!expected.getMessage().contains("private"));}
        NativeBridge.close(handle);
        try {NativeBridge.request(handle,read);throw new AssertionError();}catch(IllegalStateException expected) { }
        try {NativeBridge.backupGeneration(handle);throw new AssertionError();}catch(IllegalStateException expected) { }
        // A verifier waiting on one library must not hold the global registry.
        String blockedPath=Files.createTempDirectory("stem-jni-blocked").resolve("notebook.db").toString();
        long blockedHandle=NativeBridge.open(blockedPath);
        long otherHandle=NativeBridge.open(Files.createTempDirectory("stem-jni-other").resolve("notebook.db").toString());
        var entered=new java.util.concurrent.CountDownLatch(1);
        var release=new java.util.concurrent.CountDownLatch(1);
        var pool=java.util.concurrent.Executors.newFixedThreadPool(3);
        boolean closedBlocked=false;
        try {
            var authentication=pool.submit(() -> NativeBridge.authenticate(blockedHandle,"[\"App\"]",challenge -> {
                entered.countDown();
                try { return release.await(10,java.util.concurrent.TimeUnit.SECONDS); }
                catch(InterruptedException interrupted) { Thread.currentThread().interrupt(); return false; }
            }));
            check(entered.await(5,java.util.concurrent.TimeUnit.SECONDS));
            var independent=pool.submit(() -> NativeBridge.request(otherHandle,"{\"version\":1,\"command\":{\"op\":\"Status\"}}"));
            check(independent.get(5,java.util.concurrent.TimeUnit.SECONDS).contains("\"ok\":true"));
            var closing=pool.submit(() -> NativeBridge.close(blockedHandle));
            try { closing.get(100,java.util.concurrent.TimeUnit.MILLISECONDS);throw new AssertionError("Close returned before native drain"); }
            catch(java.util.concurrent.TimeoutException expected) { }
            check(pool.submit(() -> NativeBridge.request(otherHandle,"{\"version\":1,\"command\":{\"op\":\"Status\"}}")).get(5,java.util.concurrent.TimeUnit.SECONDS).contains("\"ok\":true"));
            release.countDown();
            authentication.get(5,java.util.concurrent.TimeUnit.SECONDS);
            closing.get(5,java.util.concurrent.TimeUnit.SECONDS);closedBlocked=true;
            long reopened=NativeBridge.open(blockedPath);NativeBridge.close(reopened);
        } finally {
            release.countDown();
            pool.shutdownNow();
            if (!closedBlocked) NativeBridge.close(blockedHandle);
            NativeBridge.close(otherHandle);
        }
        long selectionHandle=NativeBridge.open(Files.createTempDirectory("stem-jni-source-selection").resolve("notebook.db").toString());
        try {
            String note=NativeBridge.request(selectionHandle,request);
            var sourceOwner=Pattern.compile("\"id\":\"([^\"]+)\"").matcher(note);check(sourceOwner.find());
            String target="{\"kind\":\"Link\",\"context\":\""+sourceOwner.group(1)+"\"}";
            String uri="content://synthetic/selected";String revision="a".repeat(64);
            var verified=new java.util.concurrent.atomic.AtomicInteger();
            NativeBridge.PdfGrant proof=new NativeBridge.PdfGrant() {
                public boolean verify(String challenge,String requested,String locator,String hash) {
                    check(!challenge.isEmpty() && requested.equals(target) && locator.equals(uri) && hash.equals(revision));verified.incrementAndGet();return true;
                }
            };
            String prepared=NativeBridge.preparePdfSelection(selectionHandle,target,uri,revision,proof);
            var review=Pattern.compile("\"review\":\"([^\"]+)\"").matcher(prepared);check(review.find());
            String accepted=NativeBridge.finishPdfSelection(selectionHandle,review.group(1),true,proof);
            check(accepted.contains(uri) && verified.get()==2);
            try { NativeBridge.finishPdfSelection(selectionHandle,review.group(1),true,proof);throw new AssertionError(); } catch(IllegalStateException expected) { }
            try { NativeBridge.preparePdfSelection(selectionHandle,target,uri,revision,(c,t,l,r) -> {throw new IllegalArgumentException("private provider diagnostic");});throw new AssertionError(); } catch(IllegalStateException expected) {check(!expected.getMessage().contains("private"));}
        } finally {NativeBridge.close(selectionHandle);}
        EngineImportBoundarySmoke.run();
        EngineBoundarySmoke.run();
        ConverterBoundarySmoke.run();
        EngineRemovalBoundarySmoke.run();
        EngineDiscardBoundarySmoke.run();
        EngineReclamationBoundarySmoke.run();
        BackupBoundarySmoke.run();
        NativeExportBoundarySmoke.run();
        DerivativeExportBoundarySmoke.run();
        NativeImportBoundarySmoke.run();
        OpenBoundarySmoke.main(new String[0]);
        RestoreBoundarySmoke.main(new String[0]);
        System.out.println("PASS: JNI Unicode, platform-verifier callback, relock, restart, malformed input, closed handles, independent registry access fresh source-selection proof and concurrent close drain and recovery identity/generation (10)");

    }
}
