import java.io.ByteArrayOutputStream;
import java.nio.file.*;
import java.nio.file.attribute.PosixFilePermissions;
import java.util.Arrays;
import java.util.concurrent.*;
import org.stemnotebook.backend.NativeBridge;

/** Actual desktop JNI restore callbacks; no provider/device evidence. */
public final class RestoreBoundarySmoke {
    static char[] recoveryKey;
    static final char[] PASSWORD="Deliberate restore test passphrase".toCharArray();
    static void check(boolean value) { if(!value) throw new AssertionError(); }
    static void rejected(Runnable action) {
        try { action.run(); throw new AssertionError("Invalid restore accepted"); }
        catch(IllegalStateException error) { check(error.getMessage().equals("Native restore operation failed")); }
    }
    static class Input implements NativeBridge.RestoreInput {
        final byte[] bytes; int offset; int retired; int max;
        Input(byte[] bytes) { this.bytes=bytes; }
        public int read(byte[] target) {
            max=Math.max(max,target.length); check(target.length<=65552);
            if(offset==bytes.length) return -1;
            int count=Math.min(Math.min(target.length,97),bytes.length-offset);
            System.arraycopy(bytes,offset,target,0,count);offset+=count;return count;
        }
        public void retire() { retired++; }
    }
    static byte[] backup(long host) {
        ByteArrayOutputStream out=new ByteArrayOutputStream();
        try(var review=NativeBridge.reviewBackup(host,true,false);var job=review.begin(120000)) {
            char[] key=job.run(PASSWORD,c->true,true,new NativeBridge.BackupOutput() {
                public void write(byte[] bytes) { out.writeBytes(bytes); }
                public void flush() {}
                public void retire() {}
            });
            recoveryKey=key;
        }
        return out.toByteArray();
    }
    static String prepare(NativeBridge.RestoreJob job,Input input,Path stage) {
        return job.prepare(PASSWORD,false,input,stage.toString(),16*1024*1024);
    }
    public static void main(String[] args) throws Exception {
        Path root=Files.createTempDirectory("stem-restore-boundary-");
        Files.setPosixFilePermissions(root,PosixFilePermissions.fromString("rwx------"));
        final long host=NativeBridge.open(root.resolve("live.db").toString());
        boolean hostClosed=false;
        try {
            String created=NativeBridge.request(host,"{\"version\":1,\"command\":{\"op\":\"Create\",\"kind\":\"Note\",\"parent\":null,\"title\":\"restore fixture\"}}");
            check(created.contains("\"ok\":true"));
            byte[] bytes=backup(host);
            try(var job=NativeBridge.beginRestore(host,120000)) {
                rejected(()->NativeBridge.beginRestore(host,120000));
                Input input=new Input(bytes);
                String summary=prepare(job,input,root);
                check(summary.contains("\"entities\":1"));check(input.retired==1);
                // A foreign host must not consume or cancel this host's token.
                long foreign=NativeBridge.open(root.resolve("foreign.db").toString());
                try {
                    var field=NativeBridge.RestoreJob.class.getDeclaredField("token");field.setAccessible(true);
                    var method=NativeBridge.class.getDeclaredMethod("releaseRestore",long.class,long.class);method.setAccessible(true);
                    try {method.invoke(null,foreign,field.getLong(job));throw new AssertionError();}
                    catch(java.lang.reflect.InvocationTargetException error) {check(error.getCause() instanceof IllegalStateException);}
                } finally {NativeBridge.close(foreign);}
                // Ordinary authoring remains available while the private stage is reviewed.
                check(NativeBridge.request(host,"{\"version\":1,\"command\":{\"op\":\"List\",\"parent\":null}}").contains("restore fixture"));
                Path copy=root.resolve("recovered.db");
                check(job.publish(copy.toString(),c->true,true).equals("\"Published\""));
                long recovered=NativeBridge.open(copy.toString());
                try { check(NativeBridge.request(recovered,"{\"version\":1,\"command\":{\"op\":\"List\",\"parent\":null}}").contains("restore fixture")); }
                finally { NativeBridge.close(recovered); }
                rejected(()->job.publish(root.resolve("again.db").toString(),c->true,true));
            }
            try(var job=NativeBridge.beginRestore(host,120000)) {
                Input input=new Input(bytes);
                check(job.prepare(recoveryKey,true,input,root.toString(),16*1024*1024).contains("\"entities\":1"));
                check(input.retired==1);
                Path declined=root.resolve("declined.db");
                rejected(()->job.publish(declined.toString(),c->true,false));
                check(!Files.exists(declined));
            }
            Input malformed=new Input(bytes);
            try(var job=NativeBridge.beginRestore(host,120000)) {
                rejected(()->job.prepare(new char[]{'\ud800'},false,malformed,root.toString(),16*1024*1024));
            }
            check(malformed.retired==1);
            Input invalid=new Input(new byte[]{1,2,3});
            try(var job=NativeBridge.beginRestore(host,120000)) { rejected(()->prepare(job,invalid,root)); }
            check(invalid.retired==1);
            Input failingRetirement=new Input(bytes) { public void retire() {super.retire();throw new RuntimeException("private input diagnostic");} };
            try(var job=NativeBridge.beginRestore(host,120000)) { rejected(()->prepare(job,failingRetirement,root)); }
            check(failingRetirement.retired==1);
            Input zero=new Input(bytes) { public int read(byte[] target) {return 0;} };
            try(var job=NativeBridge.beginRestore(host,120000)) { rejected(()->prepare(job,zero,root)); }
            check(zero.retired==1);
            Input tooLarge=new Input(bytes) { public int read(byte[] target) {return target.length+1;} };
            try(var job=NativeBridge.beginRestore(host,120000)) { rejected(()->prepare(job,tooLarge,root)); }
            check(tooLarge.retired==1);
            // An unadmitted invalid credential still retires the caller's source once.
            Input unadmitted=new Input(bytes);
            try(var job=NativeBridge.beginRestore(host,120000)) {
                rejected(()->job.prepare(null,false,unadmitted,root.toString(),1024));
            }
            check(unadmitted.retired==1);
            // Running cancellation keeps the host's one-job admission until read retires.
            CountDownLatch entered=new CountDownLatch(1),release=new CountDownLatch(1);
            Input blocked=new Input(bytes) { public int read(byte[] target) {
                entered.countDown();try {check(release.await(10,TimeUnit.SECONDS));}catch(InterruptedException e){throw new RuntimeException(e);}
                return super.read(target);
            }};
            var worker=Executors.newSingleThreadExecutor();
            try(var job=NativeBridge.beginRestore(host,120000)) {
                Future<?> result=worker.submit(()->rejected(()->prepare(job,blocked,root)));
                check(entered.await(10,TimeUnit.SECONDS));job.close();
                rejected(()->NativeBridge.beginRestore(host,120000));
                release.countDown();result.get(30,TimeUnit.SECONDS);check(blocked.retired==1);
            } finally {release.countDown();worker.shutdownNow();}
            try(var job=NativeBridge.beginRestore(host,120000)) {
                Input input=new Input(bytes);prepare(job,input,root);
                NativeBridge.close(host);hostClosed=true;
                rejected(()->job.publish(root.resolve("closed.db").toString(),c->true,true));
            }
            System.out.println("PASS: restore publication, callback bounds/retirement, failure redaction, cancellation and close (7)");
        } finally {
            if(!hostClosed) NativeBridge.close(host);
            Arrays.fill(PASSWORD,'\0');
            if(recoveryKey!=null) Arrays.fill(recoveryKey,'\0');
            try(var files=Files.walk(root)) {
                for(Path file:files.sorted(java.util.Comparator.reverseOrder()).toList()) Files.delete(file);
            }
        }
    }
}
