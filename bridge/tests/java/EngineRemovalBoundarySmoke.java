import org.stemnotebook.backend.NativeBridge;
import java.nio.file.*;
import java.util.UUID;
import java.util.concurrent.*;
import java.util.concurrent.atomic.AtomicInteger;
import java.util.regex.Pattern;

/** Synthetic compiled SDK only; logical removal does not reclaim model files. */
public final class EngineRemovalBoundarySmoke {
    private static void check(boolean value) {if(!value)throw new AssertionError("engine removal contract failed");}
    private static void rejected(Runnable action) {try {action.run();throw new AssertionError("invalid removal accepted");}catch(IllegalStateException error){check(error.getMessage().equals("Native notebook operation failed"));}}
    private static String request(long host,String command) {String reply=NativeBridge.request(host,"{\"version\":1,\"command\":"+command+"}");check(reply.contains("\"ok\":true"));return reply;}
    private static String id(String json) {var m=Pattern.compile("\"id\":\"([^\"]+)\"").matcher(json);check(m.find());return m.group(1);}
    private static String descriptor(String version) {return "{\"id\":\"synthetic-removal\",\"version\":\""+version+"\",\"adapter_version\":\"1\",\"family\":\"Recogniser\",\"input\":\"OrderedStrokesV1\",\"tasks\":[\"Prose\"],\"internals\":\"StrokeOnlyVerified\",\"offline\":true,\"content_egress\":false,\"personal_use_evidence\":\"app-owned removal fixture; no real model\",\"redistributable_resources\":false,\"resource_sha256\":null,\"runtime_contract\":\"stem-processing-v1\"}";}
    private static String block(long host) {
        String note=id(request(host,"{\"op\":\"Create\",\"kind\":\"Note\",\"parent\":null,\"title\":\"removal ink fixture\"}"));
        String stroke="{\"id\":\""+UUID.randomUUID()+"\",\"capture_session\":\""+UUID.randomUUID()+"\",\"sequence\":0,\"pointer_id\":7,\"coordinate_space\":\"synthetic\",\"tool\":\"Pen\",\"end\":\"Completed\",\"points\":[{\"x\":1,\"y\":2,\"time_us\":10,\"pressure\":null,\"tilt_rad\":null,\"orientation_rad\":null,\"buttons\":null},{\"x\":2,\"y\":3,\"time_us\":20,\"pressure\":0.0,\"tilt_rad\":null,\"orientation_rad\":null,\"buttons\":0}]}";
        return id(request(host,"{\"op\":\"Capture\",\"ticket\":\""+UUID.randomUUID()+"\",\"entity\":\""+note+"\",\"strokes\":["+stroke+"]}"));
    }
    private static String source(long host,String block) {return request(host,"{\"op\":\"SourceInk\",\"block\":\""+block+"\"}").replaceAll("\"generation\":[0-9]+","\"generation\":0");}
    private static class Model implements NativeBridge.Recogniser {
        final AtomicInteger retired=new AtomicInteger();
        CountDownLatch entered,release,retiring,releaseRetire;
        public void probe(NativeBridge.EngineControl control) {control.check();}
        public String recognise(String ink,String task,NativeBridge.EngineControl control) throws Exception {
            control.check();check(task.equals("Prose") && ink.contains("\"strokes\":["));
            if(entered!=null){entered.countDown();check(release.await(10,TimeUnit.SECONDS));}
            control.check();return "{\"candidates\":[{\"type\":\"Text\",\"value\":\"1 + 1 = 3\"}],\"raw_output\":\"1 + 1 = 3\",\"confidence\":null,\"diagnostics\":[]}";
        }
        public void retire() throws Exception {check(retired.incrementAndGet()==1);if(retiring!=null){retiring.countDown();check(releaseRetire.await(10,TimeUnit.SECONDS));}}
    }
    private static String install(long host,Model model) {
        String version=id(NativeBridge.stageResourceFreeEngine(host,descriptor("1"),true));
        try(var job=NativeBridge.engineActivation(host,version,0,30000)){job.activate((d,r,c)->{check(r==null);c.check();return model;});}
        NativeBridge.selectEngineDefault(host,"Prose","synthetic-removal");return version;
    }
    public static void main(String[] args) throws Exception {run();}
    public static void run() throws Exception {
        Path path=Files.createTempDirectory("stem-jni-removal").resolve("library.db");long host=NativeBridge.open(path.toString());
        try {
            Model model=new Model();String version=install(host,model),block=block(host);String raw=source(host,block);
            try(var recognition=NativeBridge.engineRecognition(host,block,null,"Prose",null,30000)) {
                String proposal=id(recognition.recognise());request(host,"{\"op\":\"AcceptProposal\",\"proposal\":\""+proposal+"\",\"candidate\":0}");
            }
            String accepted=request(host,"{\"op\":\"Block\",\"block\":\""+block+"\"}");check(accepted.contains("1 + 1 = 3"));
            String inactive=id(NativeBridge.stageResourceFreeEngine(host,descriptor("2"),true));
            try(var removal=NativeBridge.engineRemoval(host,inactive,30000)){removal.remove();rejected(removal::remove);}
            check(model.retired.get()==0 && NativeBridge.engineCatalogue(host,null,128).contains("\"runtime_ready\":true"));
            var cancelled=NativeBridge.engineRemoval(host,version,30000);cancelled.close();rejected(cancelled::remove);
            try(var stale=NativeBridge.engineRemoval(host,version,30000)){request(host,"{\"op\":\"Create\",\"kind\":\"Note\",\"parent\":null,\"title\":\"stale fence\"}");rejected(stale::remove);}
            try(var capture=NativeBridge.engineRemoval(host,version,30000)){String pen=UUID.randomUUID().toString();NativeBridge.setCapturePriority(host,pen,true);NativeBridge.setCapturePriority(host,pen,false);rejected(capture::remove);}
            try(var removal=NativeBridge.engineRemoval(host,version,30000)){removal.remove();rejected(removal::remove);}
            try(var repeated=NativeBridge.engineRemoval(host,version,30000)){repeated.remove();}
            check(raw.equals(source(host,block)));check(request(host,"{\"op\":\"Block\",\"block\":\""+block+"\"}").contains("1 + 1 = 3"));
            String page=NativeBridge.engineCatalogue(host,null,128);check(!page.contains("\"runtime_ready\":true") && page.contains("\"defaults\":[]"));
            rejected(()->NativeBridge.engineRecognition(host,block,null,"Prose",null,30000));
            System.out.println("PASS: JNI removal preserves raw ink and accepted content, clears active defaults, preserves inactive choices and rejects stale/cancelled/capture/replayed jobs (1)");
        } finally {NativeBridge.close(host);}
        long reopened=NativeBridge.open(path.toString());try{check(NativeBridge.engineCatalogue(reopened,null,128).contains("\"state\":\"Removed\""));}finally{NativeBridge.close(reopened);}

        for(boolean running:new boolean[]{false,true}) {
            Path db=Files.createTempDirectory("stem-jni-retiring-removal").resolve("library.db");long h=NativeBridge.open(db.toString());ExecutorService workers=Executors.newCachedThreadPool();
            Model model=new Model();model.retiring=new CountDownLatch(1);model.releaseRetire=new CountDownLatch(1);
            if(running){model.entered=new CountDownLatch(1);model.release=new CountDownLatch(1);}
            try {
                String version=install(h,model),block=block(h);Future<?> inference=null;
                if(running){inference=workers.submit(()->{try(var job=NativeBridge.engineRecognition(h,block,null,"Prose",null,30000)){rejected(job::recognise);}});check(model.entered.await(10,TimeUnit.SECONDS));}
                try(var removal=NativeBridge.engineRemoval(h,version,30000)){removal.remove();}
                if(running)check(model.retired.get()==0);else check(model.retiring.await(10,TimeUnit.SECONDS));
                request(h,"{\"op\":\"Create\",\"kind\":\"Note\",\"parent\":null,\"title\":\"authoring while SDK retires\"}");
                Future<?> closing=workers.submit(()->NativeBridge.close(h));
                long end=System.nanoTime()+TimeUnit.SECONDS.toNanos(10);
                while(true){try{NativeBridge.recognitionStatus(h);}catch(IllegalStateException closed){break;}check(System.nanoTime()<end);Thread.onSpinWait();}
                rejected(()->NativeBridge.open(db.toString()));check(!closing.isDone());
                if(running){model.release.countDown();check(model.retiring.await(10,TimeUnit.SECONDS));}
                rejected(()->NativeBridge.open(db.toString()));check(!closing.isDone());model.releaseRetire.countDown();
                if(inference!=null)inference.get(10,TimeUnit.SECONDS);closing.get(10,TimeUnit.SECONDS);check(model.retired.get()==1);
                long again=NativeBridge.open(db.toString());NativeBridge.close(again);
                System.out.println(running?"PASS: JNI removal invalidates in-flight output and retains actual SDK ownership through inference and cleanup before reopen (1)":"PASS: JNI logical removal acknowledges metadata while SDK retirement blocks, authoring stays available and close retains library ownership (1)");
            } finally {if(model.release!=null)model.release.countDown();model.releaseRetire.countDown();workers.shutdownNow();check(workers.awaitTermination(10,TimeUnit.SECONDS));try{NativeBridge.close(h);}catch(IllegalStateException closed){}}
        }
    }
}
