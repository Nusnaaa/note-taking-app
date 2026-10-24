import org.stemnotebook.backend.NativeBridge;
import java.nio.file.Files;
import java.util.Arrays;
import java.util.concurrent.*;
import java.util.concurrent.atomic.AtomicInteger;

/** Actual desktop JVM callbacks, arrays, exception containment and ownership. */
public final class BackupBoundarySmoke {
    private static final String STATUS="{\"version\":1,\"command\":{\"op\":\"Status\"}}";
    private static final char[] PASSWORD="Deliberate native backup test passphrase".toCharArray();
    private static void check(boolean value) { if(!value) throw new AssertionError("backup boundary contract failed"); }
    private static void rejected(Runnable action) {
        try { action.run(); throw new AssertionError("accepted invalid backup"); }
        catch(IllegalStateException expected) { check(!expected.getMessage().contains("private")); }
    }
    private static class Sink implements NativeBridge.BackupOutput {
        final java.io.ByteArrayOutputStream bytes=new java.io.ByteArrayOutputStream();
        final AtomicInteger retired=new AtomicInteger();
        int max;
        public void write(byte[] value) { max=Math.max(max,value.length);bytes.write(value,0,value.length); }
        public void flush() { }
        public void retire() { retired.incrementAndGet(); }
    }
    public static void run() throws Exception {
        String path=Files.createTempDirectory("stem-jni-backup").resolve("library.db").toString();
        long host=NativeBridge.open(path);
        try {
            var review=NativeBridge.reviewBackup(host,true,false);
            check(!review.includesOriginalInk && review.sourceAssets==0);
            try(var job=review.begin(120000)) {
                Sink sink=new Sink();
                char[] password=PASSWORD.clone();
                char[] key=job.run(password,c -> true,true,sink);
                check(key.length==64 && Arrays.equals(password,PASSWORD));
                Arrays.fill(key,'\0');Arrays.fill(password,'\0');
                byte[] bytes=sink.bytes.toByteArray();
                check(new String(bytes,0,8,java.nio.charset.StandardCharsets.US_ASCII).equals("STEMENC2"));
                check(sink.max<=65552 && sink.retired.get()==1);
                check(!new String(bytes,java.nio.charset.StandardCharsets.ISO_8859_1).contains("Deliberate native"));
                rejected(() -> job.run(PASSWORD,c -> true,true,new Sink()));
            }
            // Review/job handles are single use and cancellation retires idle plans.
            review.close();rejected(() -> review.begin(120000));
            try(var cancelled=NativeBridge.reviewBackup(host,true,false).begin(120000)) {
                cancelled.close();Sink sink=new Sink();
                rejected(() -> cancelled.run(PASSWORD,c -> true,true,sink));check(sink.retired.get()==1);
            }
            Sink declined=new Sink();
            try(var job=NativeBridge.reviewBackup(host,true,false).begin(120000)) {
                rejected(() -> job.run(PASSWORD,c -> true,false,declined));
                check(declined.bytes.size()==0 && declined.retired.get()==1);
            }
            Sink failing=new Sink() { public void write(byte[] b) { throw new IllegalArgumentException("private output diagnostic"); } };
            try(var job=NativeBridge.reviewBackup(host,true,false).begin(120000)) {
                rejected(() -> job.run(PASSWORD,c -> true,true,failing));check(failing.retired.get()==1);
            }
            Sink retirementFailure=new Sink() { public void retire() {super.retire();throw new IllegalArgumentException("private retirement diagnostic");} };
            try(var job=NativeBridge.reviewBackup(host,true,false).begin(120000)) {
                rejected(() -> job.run(PASSWORD,c -> true,true,retirementFailure));check(retirementFailure.retired.get()==1);
            }
            // Capture invalidation cannot revive a retained review or job.
            var stale=NativeBridge.reviewBackup(host,true,false);
            String contact=java.util.UUID.randomUUID().toString();
            NativeBridge.setCapturePriority(host,contact,true);
            NativeBridge.setCapturePriority(host,contact,false);
            rejected(() -> stale.begin(120000));
            try(var current=NativeBridge.reviewBackup(host,true,false)) { stale.close();check(!current.includesOriginalInk); }
            check(NativeBridge.request(host,STATUS).contains("\"ok\":true"));
        } finally { NativeBridge.close(host); }
        // A blocked retirement must retain native ownership without the global
        // registry or authoring writer being held by the callback.
        long blocked=NativeBridge.open(path);
        long other=NativeBridge.open(Files.createTempDirectory("stem-jni-backup-other").resolve("library.db").toString());
        var entered=new CountDownLatch(1);var release=new CountDownLatch(1);
        var pool=Executors.newFixedThreadPool(3);
        var job=NativeBridge.reviewBackup(blocked,true,false).begin(120000);
        Sink sink=new Sink() {public void retire() {
            super.retire();entered.countDown();
            try {check(release.await(30,TimeUnit.SECONDS));}catch(InterruptedException e){Thread.currentThread().interrupt();throw new IllegalStateException();}
        }};
        Future<?> work=pool.submit(() -> rejected(() -> job.run(PASSWORD,c -> true,true,sink)));
        Future<?> closing=null;
        try {
            check(entered.await(30,TimeUnit.SECONDS));
            check(pool.submit(() -> NativeBridge.request(blocked,STATUS)).get(5,TimeUnit.SECONDS).contains("\"ok\":true"));
            job.close();
            closing=pool.submit(() -> NativeBridge.close(blocked));
            try { closing.get(150,TimeUnit.MILLISECONDS);throw new AssertionError("premature native retirement"); }catch(TimeoutException expected) { }
            check(pool.submit(() -> NativeBridge.request(other,STATUS)).get(5,TimeUnit.SECONDS).contains("\"ok\":true"));
            rejected(() -> NativeBridge.open(path));
        } finally {
            release.countDown();work.get(30,TimeUnit.SECONDS);
            if(closing!=null)closing.get(10,TimeUnit.SECONDS);else NativeBridge.close(blocked);
            NativeBridge.close(other);pool.shutdownNow();
        }
        check(sink.retired.get()==1);
        long reopened=NativeBridge.open(path);NativeBridge.close(reopened);
        Arrays.fill(PASSWORD,'\0');
        System.out.println("PASS: JNI backup ciphertext/char-array key, single-use/cancel, explicit confirmation, callback exception, retirement failure, capture invalidation and close ownership (7)");
    }
}
