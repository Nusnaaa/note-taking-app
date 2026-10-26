import org.stemnotebook.backend.NativeBridge;
import java.nio.file.*;
import java.security.MessageDigest;
import java.util.*;
import java.util.concurrent.*;

/** Actual JNI lifetime/review checks using app-owned synthetic model bytes. */
public final class EngineReclamationBoundarySmoke {
    private static void check(boolean value){if(!value)throw new AssertionError("reclamation boundary contract failed");}
    private static void rejected(Runnable run){try{run.run();throw new AssertionError("invalid reclamation accepted");}catch(IllegalStateException error){check(error.getMessage().equals("Native notebook operation failed"));}}
    private static String descriptor() throws Exception {
        String hash=HexFormat.of().formatHex(MessageDigest.getInstance("SHA-256").digest("model".getBytes(java.nio.charset.StandardCharsets.UTF_8)));
        return "{\"id\":\"synthetic-reclamation-jni\",\"version\":\"1\",\"adapter_version\":\"1\",\"family\":\"Recogniser\",\"input\":\"OrderedStrokesV1\",\"tasks\":[\"Prose\"],\"internals\":\"StrokeOnlyVerified\",\"offline\":true,\"content_egress\":false,\"personal_use_evidence\":\"app-owned synthetic fixture\",\"redistributable_resources\":false,\"resource_sha256\":\""+hash+"\",\"runtime_contract\":\"stem-processing-v1\"}";
    }
    private static String stage(long host,Path models) throws Exception {
        try(var job=NativeBridge.engineImport(host,descriptor(),5,true,models.toString(),null,30000)) {
            String id=job.version;job.copy(c -> new NativeBridge.EngineInput(){int reads;public int read(byte[] buffer,NativeBridge.EngineControl ctl){ctl.check();if(reads++>0)return -1;System.arraycopy("model".getBytes(java.nio.charset.StandardCharsets.UTF_8),0,buffer,0,5);return 5;}public void retire(){}});return id;
        }
    }
    private static void remove(long host,String id){try(var job=NativeBridge.engineRemoval(host,id,30000)){job.remove();}}
    private static void idle(long host){long end=System.nanoTime()+TimeUnit.SECONDS.toNanos(10);while(!NativeBridge.recognitionStatus(host).contains("\"reserved\":0")){check(System.nanoTime()<end);Thread.onSpinWait();}}
    public static void main(String[] args)throws Exception{run();}
    public static void run()throws Exception{
        Path root=Files.createTempDirectory("stem-reclamation-jni"),models=root.resolve("models"),db=root.resolve("library.db");long host=NativeBridge.open(db.toString());
        try {
            String version=stage(host,models);rejected(()->NativeBridge.engineReclamation(host,version,null,30000));remove(host,version);
            try(var job=NativeBridge.engineReclamation(host,version,null,30000)){
                UUID.fromString(job.actionId);String summary=job.review(models.toString());check(summary.contains(job.actionId)&&summary.contains("BoundLedger")&&!summary.contains(models.toString()));check(Files.exists(models.resolve(version+".resource")));
                rejected(()->job.review(models.toString()));
                String receipt=job.reclaim(true,false,false,false);check(receipt.contains("Completed")&&receipt.contains(job.actionId));check(NativeBridge.engineReclamation(host,job.actionId).contains("Completed"));rejected(()->job.reclaim(true,true,true,true));
            }
            check(!Files.exists(models.resolve(version+".resource"))&&!Files.exists(models.resolve(version+".part"))&&!Files.exists(models.resolve(version+".install.json")));
            check(NativeBridge.engineReclamations(host,null,1).contains("Completed"));rejected(()->NativeBridge.engineReclamations(host,null,129));
            System.out.println("PASS: scoped two-stage reclamation review, explicit consent, stable action identity, durable metadata and single-use history boundary (1)");

            String busy=stage(host,models);CountDownLatch entered=new CountDownLatch(1),release=new CountDownLatch(1);
            try {
                try(var activation=NativeBridge.engineActivation(host,busy,5,30000)){activation.activate((d,r,c)->new NativeBridge.Recogniser(){public void probe(NativeBridge.EngineControl ctl){ctl.check();}public String recognise(String i,String t,NativeBridge.EngineControl ctl){throw new AssertionError();}public void retire()throws Exception{entered.countDown();check(release.await(10,TimeUnit.SECONDS));}});}
                remove(host,busy);check(entered.await(10,TimeUnit.SECONDS));
                try(var job=NativeBridge.engineReclamation(host,busy,null,30000)){job.review(models.toString());rejected(()->job.reclaim(true,true,true,true));rejected(()->NativeBridge.engineReclamation(host,job.actionId));}
                check(Files.exists(models.resolve(busy+".resource")));
            } finally {release.countDown();}
            idle(host);
            try(var job=NativeBridge.engineReclamation(host,busy,null,30000)){job.review(models.toString());job.reclaim(true,false,false,false);}
            System.out.println("PASS: every review flag still refuses a live SDK retirement pin; actual optional retirement permits later explicit reclamation (1)");

            String cancelled=stage(host,models);remove(host,cancelled);
            var closed=NativeBridge.engineReclamation(host,cancelled,null,30000);closed.review(models.toString());closed.close();rejected(()->closed.reclaim(true,true,true,true));
            try(var job=NativeBridge.engineReclamation(host,cancelled,null,30000)){job.review(models.toString());String pen=UUID.randomUUID().toString();NativeBridge.setCapturePriority(host,pen,true);NativeBridge.setCapturePriority(host,pen,false);rejected(()->job.reclaim(true,true,true,true));}
            check(Files.exists(models.resolve(cancelled+".resource")));
            Path outside=root.resolve("outside");Files.writeString(outside,"independent bytes");Files.delete(models.resolve(cancelled+".resource"));Files.createSymbolicLink(models.resolve(cancelled+".resource"),outside);
            try(var job=NativeBridge.engineReclamation(host,cancelled,null,30000)){check(job.review(models.toString()).contains("Symlink"));rejected(()->job.reclaim(true,false,false,false));}
            try(var job=NativeBridge.engineReclamation(host,cancelled,null,30000)){job.review(models.toString());job.reclaim(true,false,false,true);}
            check(Files.readString(outside).equals("independent bytes"));
            NativeBridge.authenticate(host,"[\"App\"]",challenge->true);check(NativeBridge.request(host,"{\"version\":1,\"command\":{\"op\":\"SetAppLock\",\"enabled\":true}}").contains("\"ok\":true"));NativeBridge.request(host,"{\"version\":1,\"command\":{\"op\":\"Relock\"}}");rejected(()->NativeBridge.engineReclamations(host,null,1));
            System.out.println("PASS: closed/capture-invalidated reviews, explicit symlink-only removal and fresh app-history authority preserve unrelated bytes (1)");
        }finally{NativeBridge.close(host);}
    }
}
