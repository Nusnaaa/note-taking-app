import org.stemnotebook.backend.NativeBridge;
import java.nio.file.*;
import java.security.MessageDigest;
import java.util.*;

/** Actual JNI over app-owned synthetic bytes, without an SDK or model licence claim. */
public final class EngineDiscardBoundarySmoke {
    private static void check(boolean value) {if(!value)throw new AssertionError("discard boundary contract failed");}
    private static void rejected(Runnable action) {try {action.run();throw new AssertionError("invalid discard accepted");}catch(IllegalStateException error){check(error.getMessage().equals("Native notebook operation failed"));}}
    private static String descriptor() throws Exception {
        String hash=HexFormat.of().formatHex(MessageDigest.getInstance("SHA-256").digest("model".getBytes(java.nio.charset.StandardCharsets.UTF_8)));
        return "{\"id\":\"synthetic-discard-jni\",\"version\":\"1\",\"adapter_version\":\"1\",\"family\":\"Recogniser\",\"input\":\"OrderedStrokesV1\",\"tasks\":[\"Prose\"],\"internals\":\"StrokeOnlyVerified\",\"offline\":true,\"content_egress\":false,\"personal_use_evidence\":\"app-owned synthetic fixture\",\"redistributable_resources\":false,\"resource_sha256\":\""+hash+"\",\"runtime_contract\":\"stem-processing-v1\"}";
    }
    private static String transfer(long host,Path models,boolean stage) throws Exception {
        try(var job=NativeBridge.engineImport(host,descriptor(),5,true,models.toString(),null,30000)) {
            String id=job.version;
            NativeBridge.EngineInputFactory input=control -> new NativeBridge.EngineInput() {
                int read;
                public int read(byte[] output,NativeBridge.EngineControl ctl) {
                    ctl.check();if(read++==0){byte[] bytes=(stage?"model":"mo").getBytes(java.nio.charset.StandardCharsets.UTF_8);System.arraycopy(bytes,0,output,0,bytes.length);return bytes.length;}
                    if(stage)return -1;throw new IllegalArgumentException("private interrupted source");
                }
                public void retire() {}
            };
            if(stage)job.copy(input);else rejected(() -> job.copy(input));
            return id;
        }
    }
    public static void main(String[] args) throws Exception {run();}
    public static void run() throws Exception {
        Path root=Files.createTempDirectory("stem-discard-jni"),models=root.resolve("models");
        long host=NativeBridge.open(root.resolve("library.db").toString());
        try {
            String version=transfer(host,models,false);Path part=models.resolve(version+".part");check(Files.size(part)==2);
            rejected(() -> NativeBridge.engineDiscard(host,version,false,null,30000));
            rejected(() -> NativeBridge.engineDiscard(host,version,true,null,0));
            String action;
            try(var job=NativeBridge.engineDiscard(host,version,true,null,30000)) {
                action=job.actionId;UUID.fromString(action);check(Files.exists(part));
                String receipt=job.discard(models.toString());
                check(receipt.contains(action) && receipt.contains("Completed") && receipt.contains("\"committed\":2") && !receipt.contains(models.toString()));
                rejected(() -> job.discard(models.toString()));
            }
            check(!Files.exists(part) && !Files.exists(models.resolve(version+".install.json")) && Files.exists(models.resolve(version+".owner.lock")));
            check(NativeBridge.engineResourceAction(host,action).contains("Completed"));
            check(NativeBridge.engineResourceActions(host,null,1).contains(action));
            rejected(() -> NativeBridge.engineResourceActions(host,null,129));
            rejected(() -> NativeBridge.engineResourceAction(host,"bad"));
            rejected(() -> NativeBridge.engineDiscard(host,version,true,action,30000));
            String staged=transfer(host,models,true);
            rejected(() -> NativeBridge.engineDiscard(host,staged,true,null,30000));
            check(Files.exists(models.resolve(staged+".resource")));
            System.out.println("PASS: explicit single-use discard, stable receipt before file work, durable metadata/history and published-version refusal (1)");

            String interrupted=transfer(host,models,false);Path kept=models.resolve(interrupted+".part");
            var cancelled=NativeBridge.engineDiscard(host,interrupted,true,null,30000);cancelled.close();rejected(() -> cancelled.discard(models.toString()));check(Files.size(kept)==2);
            try(var job=NativeBridge.engineDiscard(host,interrupted,true,null,30000)) {
                String pen=UUID.randomUUID().toString();NativeBridge.setCapturePriority(host,pen,true);NativeBridge.setCapturePriority(host,pen,false);
                rejected(() -> job.discard(models.toString()));
                rejected(() -> NativeBridge.engineResourceAction(host,job.actionId));
            }
            long foreign=NativeBridge.open(root.resolve("foreign.db").toString());
            try(var job=NativeBridge.engineDiscard(foreign,interrupted,true,null,30000)) {rejected(() -> job.discard(models.toString()));}
            finally {NativeBridge.close(foreign);}
            check(Files.size(kept)==2);
            Path outside=root.resolve("outside");Files.writeString(outside,"private outside bytes");Files.delete(kept);Files.createSymbolicLink(kept,outside);
            try(var job=NativeBridge.engineDiscard(host,interrupted,true,null,30000)) {rejected(() -> job.discard(models.toString()));rejected(() -> NativeBridge.engineResourceAction(host,job.actionId));}
            check(Files.isSymbolicLink(kept) && Files.readString(outside).equals("private outside bytes"));
            System.out.println("PASS: closed/capture-invalidated jobs, foreign library and symlink refusal preserve independent bytes without inventing audit completion (1)");

            NativeBridge.authenticate(host,"[\"App\"]",challenge -> true);
            check(NativeBridge.request(host,"{\"version\":1,\"command\":{\"op\":\"SetAppLock\",\"enabled\":true}}").contains("\"ok\":true"));
            NativeBridge.request(host,"{\"version\":1,\"command\":{\"op\":\"Relock\"}}");
            rejected(() -> NativeBridge.engineResourceAction(host,action));rejected(() -> NativeBridge.engineResourceActions(host,null,1));
            rejected(() -> NativeBridge.engineDiscard(host,interrupted,true,null,30000));
            System.out.println("PASS: fresh app authority gates discard plans and retained consent history (1)");
        } finally {NativeBridge.close(host);}
    }
}
