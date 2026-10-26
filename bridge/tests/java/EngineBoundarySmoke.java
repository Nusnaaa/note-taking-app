import org.stemnotebook.backend.NativeBridge;
import java.nio.file.Files;
import java.util.UUID;
import java.util.concurrent.*;
import java.util.concurrent.atomic.AtomicInteger;
import java.util.regex.Pattern;

/** App-owned synthetic provider; establishes JNI behaviour, not real-model readiness. */
public final class EngineBoundarySmoke {
    private static final String STATUS="{\"op\":\"Status\"}";
    private static void check(boolean value) { if(!value) throw new AssertionError("engine boundary contract failed"); }
    private static void rejected(Runnable action) {
        try { action.run(); throw new AssertionError("invalid engine operation accepted"); }
        catch(IllegalStateException expected) { check(expected.getMessage().equals("Native notebook operation failed")); }
    }
    private static String request(long host, String command) { return NativeBridge.request(host,"{\"version\":1,\"command\":"+command+"}"); }
    private static String id(String json) { var m=Pattern.compile("\"id\":\"([^\"]+)\"").matcher(json);check(m.find());return m.group(1); }
    private static String descriptor(String version) {
        return "{\"id\":\"synthetic-jni\",\"version\":\""+version+"\",\"adapter_version\":\"1\",\"family\":\"Recogniser\",\"input\":\"OrderedStrokesV1\",\"tasks\":[\"Prose\"],\"internals\":\"StrokeOnlyVerified\",\"offline\":true,\"content_egress\":false,\"personal_use_evidence\":\"app-owned synthetic JNI fixture; no real model\",\"redistributable_resources\":false,\"resource_sha256\":null,\"runtime_contract\":\"stem-processing-v1\"}";
    }
    private static String block(long host) {
        String note=id(request(host,"{\"op\":\"Create\",\"kind\":\"Note\",\"parent\":null,\"title\":\"ordered ink fixture\"}"));
        String stroke="{\"id\":\""+UUID.randomUUID()+"\",\"capture_session\":\""+UUID.randomUUID()+"\",\"sequence\":0,\"pointer_id\":7,\"coordinate_space\":\"synthetic-units\",\"tool\":\"Pen\",\"end\":\"Completed\",\"points\":[{\"x\":1.25,\"y\":2.5,\"time_us\":10,\"pressure\":null,\"tilt_rad\":null,\"orientation_rad\":null,\"buttons\":null},{\"x\":2.5,\"y\":3.5,\"time_us\":20,\"pressure\":0.0,\"tilt_rad\":null,\"orientation_rad\":null,\"buttons\":0}]}";
        return id(request(host,"{\"op\":\"Capture\",\"ticket\":\""+UUID.randomUUID()+"\",\"entity\":\""+note+"\",\"strokes\":["+stroke+"]}"));
    }
    private static String source(long host,String block) { return request(host,"{\"op\":\"SourceInk\",\"block\":\""+block+"\"}").replaceAll("\"generation\":[0-9]+","\"generation\":0"); }
    private static class Model implements NativeBridge.Recogniser {
        final long host;
        final AtomicInteger retired=new AtomicInteger();
        volatile NativeBridge.EngineControl last;
        volatile CountDownLatch entered, release, retiring, releaseRetire;
        volatile String output="{\"candidates\":[{\"type\":\"Text\",\"value\":\"1 + 1 = 3\"}],\"raw_output\":\"1 + 1 = 3\",\"confidence\":null,\"diagnostics\":[]}";
        Model(long host) { this.host=host; }
        public void probe(NativeBridge.EngineControl control) { control.check();last=control;check(request(host,STATUS).contains("\"ok\":true")); }
        public String recognise(String ink,String task,NativeBridge.EngineControl control) throws Exception {
            control.check();last=control;
            check(task.equals("Prose") && ink.contains("\"strokes\":[") && ink.contains("\"pointer_id\":7") && ink.contains("\"pressure\":null") && ink.contains("\"pressure\":0.0") && ink.contains("\"time_us\":20"));
            if(entered!=null) { entered.countDown();check(release.await(10,TimeUnit.SECONDS)); }
            control.check();return output;
        }
        public void retire() throws Exception { check(retired.incrementAndGet()==1);if(retiring!=null) {retiring.countDown();check(releaseRetire.await(10,TimeUnit.SECONDS));} }
    }
    private static class Factory implements NativeBridge.RecogniserFactory {
        final Model model;
        volatile NativeBridge.EngineControl control;
        Factory(Model model) {this.model=model;}
        public NativeBridge.Recogniser create(String descriptor,byte[] resource,NativeBridge.EngineControl control) {
            this.control=control;control.check();check(resource==null && descriptor.contains("OrderedStrokesV1"));
            check(request(model.host,STATUS).contains("\"ok\":true"));return model;
        }
    }
    public static void main(String[] args) throws Exception { run(); }
    public static void run() throws Exception {
        String path=Files.createTempDirectory("stem-jni-engines").resolve("library.db").toString();
        long host=NativeBridge.open(path);
        ExecutorService workers=Executors.newCachedThreadPool();
        try {
            rejected(() -> NativeBridge.stageResourceFreeEngine(host,descriptor("1"),false));
            rejected(() -> NativeBridge.stageResourceFreeEngine(host,descriptor("1").replace("OrderedStrokesV1","MandatoryRaster"),true));
            rejected(() -> NativeBridge.stageResourceFreeEngine(host,descriptor("1").replace("\"content_egress\":false","\"content_egress\":true"),true));
            String version=id(NativeBridge.stageResourceFreeEngine(host,descriptor("1"),true));
            String block=block(host), original=source(host,block);
            rejected(() -> NativeBridge.engineRecognition(host,block,null,"Prose","synthetic-jni",30000));
            Model model=new Model(host);Factory factory=new Factory(model);
            try(var job=NativeBridge.engineActivation(host,version,0,30000)) {
                job.activate(factory);rejected(() -> job.activate(factory));
            }
            check(!factory.control.active() && !model.last.active());
            try(var job=NativeBridge.engineRecognition(host,block,null,"Prose","synthetic-jni",30000)) {
                String proposal=job.recognise();check(proposal.contains("\"state\":\"Pending\"") && proposal.contains("1 + 1 = 3"));rejected(job::recognise);
            }
            check(!model.last.active() && original.equals(source(host,block)));
            System.out.println("PASS: consented typed JNI provider, no implicit readiness, callback reentry, ordered ink, pending review and exact source preservation (1)");

            var cancelled=NativeBridge.engineRecognition(host,block,null,"Prose","synthetic-jni",30000);cancelled.close();rejected(cancelled::recognise);
            model.output="malformed private provider diagnostic";
            try(var job=NativeBridge.engineRecognition(host,block,null,"Prose","synthetic-jni",30000)) {rejected(job::recognise);}
            model.output="x".repeat(8*1024*1024+1);
            try(var job=NativeBridge.engineRecognition(host,block,null,"Prose","synthetic-jni",30000)) {rejected(job::recognise);}
            check(original.equals(source(host,block)));
            System.out.println("PASS: single-use and cancelled engine jobs, malformed and oversized private provider output containment (1)");

            model.entered=new CountDownLatch(1);model.release=new CountDownLatch(1);
            var running=NativeBridge.engineRecognition(host,block,null,"Prose","synthetic-jni",30000);
            Future<?> pending=workers.submit(() -> rejected(running::recognise));
            check(model.entered.await(10,TimeUnit.SECONDS));
            check(request(host,STATUS).contains("\"ok\":true"));
            String pen=UUID.randomUUID().toString();NativeBridge.setCapturePriority(host,pen,true);check(!model.last.active());
            NativeBridge.setCapturePriority(host,pen,false);model.release.countDown();pending.get(10,TimeUnit.SECONDS);
            check(original.equals(source(host,block)));
            model.entered=null;
            System.out.println("PASS: live ordered-ink callback cancellation through contact authority without blocking authoring or publishing stale output (1)");

            String replacement=id(NativeBridge.stageResourceFreeEngine(host,descriptor("2"),true));
            AtomicInteger failedRetire=new AtomicInteger();
            try(var job=NativeBridge.engineActivation(host,replacement,0,30000)) {
                rejected(() -> job.activate((d,r,c) -> new NativeBridge.Recogniser() {
                    public void probe(NativeBridge.EngineControl ctl) {throw new IllegalArgumentException("private SDK probe failure");}
                    public String recognise(String i,String t,NativeBridge.EngineControl ctl) {throw new AssertionError();}
                    public void retire() {failedRetire.incrementAndGet();}
                }));
            }
            check(failedRetire.get()==1 && model.retired.get()==0);
            model.output="{\"candidates\":[{\"type\":\"Text\",\"value\":\"old runtime still ready\"}],\"raw_output\":\"old runtime still ready\",\"confidence\":null,\"diagnostics\":[]}";
            try(var job=NativeBridge.engineRecognition(host,block,null,"Prose","synthetic-jni",30000)) {check(job.recognise().contains("old runtime still ready"));}
            System.out.println("PASS: throwing Java probe retires failed SDK once and preserves the old ready runtime (1)");

            model.entered=new CountDownLatch(1);model.release=new CountDownLatch(1);model.retiring=new CountDownLatch(1);model.releaseRetire=new CountDownLatch(1);
            var last=NativeBridge.engineRecognition(host,block,null,"Prose","synthetic-jni",30000);
            Future<?> active=workers.submit(() -> rejected(last::recognise));check(model.entered.await(10,TimeUnit.SECONDS));
            Future<?> closing=workers.submit(() -> NativeBridge.close(host));
            long watchdog=System.nanoTime()+TimeUnit.SECONDS.toNanos(10);
            while(model.last.active()) {check(System.nanoTime()<watchdog);Thread.onSpinWait();}
            rejected(() -> NativeBridge.open(path));check(!closing.isDone());
            model.release.countDown();check(model.retiring.await(10,TimeUnit.SECONDS));
            rejected(() -> NativeBridge.open(path));check(!closing.isDone());
            model.releaseRetire.countDown();active.get(10,TimeUnit.SECONDS);closing.get(10,TimeUnit.SECONDS);
            check(model.retired.get()==1);
            long reopened=NativeBridge.open(path);
            try {rejected(() -> NativeBridge.engineRecognition(reopened,block,null,"Prose","synthetic-jni",30000));check(original.equals(source(reopened,block)));}
            finally {NativeBridge.close(reopened);}
            System.out.println("PASS: JNI close waits for actual inference and SDK retirement before library reopen; installed metadata never recreates readiness (1)");

            String failingPath=Files.createTempDirectory("stem-jni-sdk-retire").resolve("library.db").toString();
            long failing=NativeBridge.open(failingPath);
            String staged=id(NativeBridge.stageResourceFreeEngine(failing,descriptor("retirement-failure"),true));
            AtomicInteger retireFailure=new AtomicInteger();
            try(var job=NativeBridge.engineActivation(failing,staged,0,30000)) {
                job.activate((d,r,c) -> new NativeBridge.Recogniser() {
                    public void probe(NativeBridge.EngineControl ctl) {ctl.check();}
                    public String recognise(String i,String t,NativeBridge.EngineControl ctl) {throw new AssertionError();}
                    public void retire() {retireFailure.incrementAndGet();throw new IllegalArgumentException("private SDK retirement diagnostic");}
                });
            }
            rejected(() -> NativeBridge.close(failing));check(retireFailure.get()==1);
            long afterFailure=NativeBridge.open(failingPath);NativeBridge.close(afterFailure);
            System.out.println("PASS: explicit SDK retirement error fails close without exposing Java diagnostics or repeating retirement (1)");
        } finally {
            workers.shutdownNow();check(workers.awaitTermination(10,TimeUnit.SECONDS));
            try {NativeBridge.close(host);} catch(IllegalStateException alreadyClosed) { }
        }
    }
}
