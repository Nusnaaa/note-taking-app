import org.stemnotebook.backend.NativeBridge;
import java.nio.file.*;
import java.security.MessageDigest;
import java.util.*;
import java.util.concurrent.*;
import java.util.concurrent.atomic.*;
import java.util.regex.Pattern;

/** App-owned synthetic bytes; actual JNI import, no real model/licence claim. */
public final class EngineImportBoundarySmoke {
    private static void check(boolean value) {if(!value) throw new AssertionError("engine import contract failed");}
    private static void rejected(Runnable action) {try {action.run();throw new AssertionError("invalid import accepted");}catch(IllegalStateException error){check(error.getMessage().equals("Native notebook operation failed"));}}
    private static String hash(byte[] bytes) throws Exception {return HexFormat.of().formatHex(MessageDigest.getInstance("SHA-256").digest(bytes));}
    private static String descriptor(byte[] bytes) throws Exception {return "{\"id\":\"synthetic-import-jni\",\"version\":\"1\",\"adapter_version\":\"1\",\"family\":\"Recogniser\",\"input\":\"OrderedStrokesV1\",\"tasks\":[\"Prose\"],\"internals\":\"StrokeOnlyVerified\",\"offline\":true,\"content_egress\":false,\"personal_use_evidence\":\"app-owned import fixture; no real model\",\"redistributable_resources\":false,\"resource_sha256\":\""+hash(bytes)+"\",\"runtime_contract\":\"stem-processing-v1\"}";}
    private static void zeros(byte[] bytes) {for(byte b:bytes) check(b==0);}
    private static class Input implements NativeBridge.EngineInput {
        final byte[] data;int offset,reads;final AtomicInteger retired=new AtomicInteger();final List<byte[]> buffers=new ArrayList<>();
        volatile NativeBridge.EngineControl control;
        Input(byte[] data,int offset) {this.data=data;this.offset=offset;}
        public int read(byte[] output,NativeBridge.EngineControl control) {control.check();this.control=control;buffers.add(output);reads++;if(offset==data.length)return -1;int n=Math.min(output.length,data.length-offset);System.arraycopy(data,offset,output,0,n);offset+=n;return n;}
        public void retire() {check(retired.incrementAndGet()==1);}
    }
    private static String field(String json,String name) {var m=Pattern.compile("\""+name+"\":\"([^\"]*)\"").matcher(json);check(m.find());return m.group(1);}
    public static void main(String[] args) throws Exception {run();}
    public static void run() throws Exception {
        Path root=Files.createTempDirectory("stem-jni-model-import"),db=root.resolve("library.db"),models=root.resolve("models");
        long host=NativeBridge.open(db.toString());ExecutorService workers=Executors.newCachedThreadPool();
        byte[] bytes=new byte[131089];for(int i=0;i<bytes.length;i++)bytes[i]=(byte)(i*71+137);String manifest=descriptor(bytes);
        try {
            rejected(() -> NativeBridge.engineImport(host,manifest,bytes.length,false,models.toString(),null,30000));check(!Files.exists(models));
            String version;Input input=new Input(bytes,0);AtomicReference<NativeBridge.EngineControl> opening=new AtomicReference<>();
            try(var job=NativeBridge.engineImport(host,manifest,bytes.length,true,models.toString(),null,30000)) {
                version=job.version;check(job.committedBytes==0);check(Files.exists(models.resolve(version+".install.json")));
                String staged=job.copy(control -> {control.check();opening.set(control);check(NativeBridge.request(host,"{\"version\":1,\"command\":{\"op\":\"Status\"}}").contains("\"ok\":true"));return input;});
                check(field(staged,"id").equals(version));check(staged.contains("\"state\":\"Staged\""));
            }
            check(input.retired.get()==1 && !opening.get().active() && !input.control.active());for(byte[] buffer:input.buffers)zeros(buffer);
            check(Arrays.equals(Files.readAllBytes(models.resolve(version+".resource")),bytes));
            AtomicReference<byte[]> resourceArray=new AtomicReference<>();AtomicInteger retired=new AtomicInteger();
            try(var activation=NativeBridge.engineActivation(host,version,bytes.length,30000)) {
                activation.activate((d,resource,control) -> {
                    check(Arrays.equals(resource,bytes));resourceArray.set(resource);byte[] owned=resource.clone();
                    return new NativeBridge.Recogniser() {
                        public void probe(NativeBridge.EngineControl ctl) {ctl.check();check(Arrays.equals(owned,bytes));}
                        public String recognise(String i,String t,NativeBridge.EngineControl ctl) {throw new AssertionError();}
                        public void retire() {Arrays.fill(owned,(byte)0);retired.incrementAndGet();}
                    };
                });
            }
            zeros(resourceArray.get());
            System.out.println("PASS: consented resource import, stable identity, callback reentry, exact binary handoff, source-buffer and factory-array clearing (1)");

            String partial;Input interrupted=new Input(bytes,0) {
                @Override public int read(byte[] output,NativeBridge.EngineControl control) {if(reads==1)throw new IllegalArgumentException("private source read diagnostic");return super.read(output,control);}
            };
            try(var job=NativeBridge.engineImport(host,manifest,bytes.length,true,models.toString(),null,30000)) {partial=job.version;rejected(() -> job.copy(c -> interrupted));}
            check(interrupted.retired.get()==1);Path part=models.resolve(partial+".part");long committed=Files.size(part);check(committed==65536);
            Files.write(part,new byte[]{1,2,3},StandardOpenOption.APPEND);long tail=Files.size(part);
            String legacy=UUID.randomUUID().toString(), corrupt=UUID.randomUUID().toString();
            String oldLedger=Files.readString(models.resolve(partial+".install.json"))
                .replace("\"id\":\""+partial+"\"","\"id\":\""+legacy+"\"")
                .replaceAll(",\"binding\":\\{[^}]*\\}","");
            Files.writeString(models.resolve(legacy+".install.json"),oldLedger);
            Files.writeString(models.resolve(corrupt+".install.json"),"private invalid ledger");
            try(var recovery=NativeBridge.engineRecovery(host,30000)) {
                String page=recovery.scan(models.toString(),null,128);
                check(page.contains(partial) && page.contains("\"committed\":65536") && page.contains("RecordedComplete"));
                check(page.contains("ReviewRequired") && page.contains("Invalid") && !page.contains("private invalid") && !page.contains(models.toString()));
                rejected(() -> recovery.scan(models.toString(),null,128));
            }
            check(Files.size(part)==tail);
            try(var recovery=NativeBridge.engineRecovery(host,30000)) {
                String page=recovery.scan(models.toString(),null,1);String cursor=field(page,"next");
                try(var next=NativeBridge.engineRecovery(host,30000)) {check(!next.scan(models.toString(),cursor,1).contains("\"id\":\""+cursor+"\""));}
            }
            var cancelledRecovery=NativeBridge.engineRecovery(host,30000);cancelledRecovery.close();
            rejected(() -> cancelledRecovery.scan(models.toString(),null,1));
            try(var recovery=NativeBridge.engineRecovery(host,30000)) {rejected(() -> recovery.scan(models.toString(),"bad",1));}
            try(var recovery=NativeBridge.engineRecovery(host,30000)) {rejected(() -> recovery.scan(models.toString(),null,129));}
            try(var recovery=NativeBridge.engineRecovery(host,30000)) {
                String pen=UUID.randomUUID().toString();NativeBridge.setCapturePriority(host,pen,true);NativeBridge.setCapturePriority(host,pen,false);
                rejected(() -> recovery.scan(models.toString(),null,1));
            }
            System.out.println("PASS: read-only recovery preserves unacknowledged tails, redacts legacy/corrupt metadata, pages and obeys single-use/cancel/capture authority (1)");
            long foreign=NativeBridge.open(root.resolve("foreign.db").toString());
            try {
                try(var recovery=NativeBridge.engineRecovery(foreign,30000)) {check(!recovery.scan(models.toString(),null,128).contains(partial));}
                rejected(() -> NativeBridge.engineImport(foreign,manifest,bytes.length,true,models.toString(),partial,30000));check(Files.size(part)==tail);
            }
            finally {NativeBridge.close(foreign);}
            try(var resumed=NativeBridge.engineImport(host,manifest,bytes.length,true,models.toString(),partial,30000)) {
                check(resumed.version.equals(partial) && resumed.committedBytes==committed);check(Files.size(part)==committed);
                Input remaining=new Input(bytes,(int)committed);check(field(resumed.copy(c -> remaining),"id").equals(partial));check(remaining.retired.get()==1);
            }
            System.out.println("PASS: failed input preserves acknowledged offset, foreign resume cannot truncate, exact resumed bytes stage under the same identity (1)");

            AtomicReference<NativeBridge.EngineControl> current=new AtomicReference<>();CountDownLatch entered=new CountDownLatch(1),release=new CountDownLatch(1);Input cancelled=new Input(bytes,0) {
                @Override public int read(byte[] output,NativeBridge.EngineControl control) {current.set(control);entered.countDown();try {check(release.await(10,TimeUnit.SECONDS));}catch(InterruptedException e){throw new AssertionError(e);}control.check();return super.read(output,control);}
            };
            try(var job=NativeBridge.engineImport(host,manifest,bytes.length,true,models.toString(),null,30000)) {
                Future<?> copying=workers.submit(() -> rejected(() -> job.copy(c -> cancelled)));check(entered.await(10,TimeUnit.SECONDS));String pen=UUID.randomUUID().toString();NativeBridge.setCapturePriority(host,pen,true);check(!current.get().active());NativeBridge.setCapturePriority(host,pen,false);release.countDown();copying.get(10,TimeUnit.SECONDS);check(cancelled.retired.get()==1);
            }
            System.out.println("PASS: native capture authority reaches a blocked Java model-input callback and prevents publication (1)");

            for(int invalid:new int[]{0,-2,65537}) {
                AtomicInteger disposal=new AtomicInteger();
                try(var job=NativeBridge.engineImport(host,manifest,bytes.length,true,models.toString(),null,30000)) {
                    rejected(() -> job.copy(c -> new NativeBridge.EngineInput() {public int read(byte[] b,NativeBridge.EngineControl ctl){return invalid;}public void retire(){disposal.incrementAndGet();}}));
                }
                check(disposal.get()==1);
            }
            AtomicInteger failedRetire=new AtomicInteger();
            try(var job=NativeBridge.engineImport(host,manifest,bytes.length,true,models.toString(),null,30000)) {
                rejected(() -> job.copy(c -> new Input(bytes,0) {@Override public void retire(){failedRetire.incrementAndGet();throw new IllegalArgumentException("private input retirement diagnostic");}}));
            }
            check(failedRetire.get()==1);
            System.out.println("PASS: input count violations and retirement errors are contained without private diagnostics or repeated cleanup (1)");

            CountDownLatch opened=new CountDownLatch(1),releaseOpen=new CountDownLatch(1),dropping=new CountDownLatch(1),releaseDrop=new CountDownLatch(1);AtomicReference<NativeBridge.EngineControl> openControl=new AtomicReference<>();
            Input finalInput=new Input(bytes,0) {@Override public void retire(){super.retire();dropping.countDown();try {check(releaseDrop.await(10,TimeUnit.SECONDS));}catch(InterruptedException e){throw new AssertionError(e);}}};
            var last=NativeBridge.engineImport(host,manifest,bytes.length,true,models.toString(),null,30000);
            Future<?> copying=workers.submit(() -> rejected(() -> last.copy(control -> {openControl.set(control);opened.countDown();check(releaseOpen.await(10,TimeUnit.SECONDS));return finalInput;})));
            check(opened.await(10,TimeUnit.SECONDS));Future<?> closing=workers.submit(() -> NativeBridge.close(host));
            long watchdog=System.nanoTime()+TimeUnit.SECONDS.toNanos(10);while(openControl.get().active()){check(System.nanoTime()<watchdog);Thread.onSpinWait();}
            rejected(() -> NativeBridge.open(db.toString()));check(!closing.isDone());releaseOpen.countDown();check(dropping.await(10,TimeUnit.SECONDS));rejected(() -> NativeBridge.open(db.toString()));check(!closing.isDone());releaseDrop.countDown();copying.get(10,TimeUnit.SECONDS);closing.get(10,TimeUnit.SECONDS);check(finalInput.retired.get()==1 && retired.get()==1);
            long reopened=NativeBridge.open(db.toString());NativeBridge.close(reopened);
            System.out.println("PASS: native close owns blocked input opening and actual cleanup until safe library reopen (1)");
        } finally {workers.shutdownNow();check(workers.awaitTermination(10,TimeUnit.SECONDS));try {NativeBridge.close(host);}catch(IllegalStateException closed){}}
    }
}
