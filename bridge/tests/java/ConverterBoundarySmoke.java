import org.stemnotebook.backend.NativeBridge;
import java.nio.file.*;
import java.security.MessageDigest;
import java.util.*;
import java.util.concurrent.*;
import java.util.concurrent.atomic.*;
import java.util.regex.Pattern;

/** Synthetic editable-content JNI checks; no real converter or device claim. */
public final class ConverterBoundarySmoke {
    private static void check(boolean value) {if(!value)throw new AssertionError("converter boundary contract failed");}
    private static void rejected(Runnable action) {try {action.run();throw new AssertionError("invalid conversion accepted");}catch(IllegalStateException e){check(e.getMessage().equals("Native notebook operation failed"));}}
    private static String request(long host,String command) {return NativeBridge.request(host,"{\"version\":1,\"command\":"+command+"}");}
    private static String id(String json) {var m=Pattern.compile("\"id\":\"([^\"]+)\"").matcher(json);check(m.find());return m.group(1);}
    private static String descriptor(String hash) {
        return "{\"id\":\"synthetic-converter\",\"version\":\"1\",\"adapter_version\":\"1\",\"family\":\"Converter\",\"input\":\"EditableV1\",\"tasks\":[],\"internals\":\"StrokeOnlyVerified\",\"offline\":true,\"content_egress\":false,\"personal_use_evidence\":\"app-owned synthetic converter fixture\",\"redistributable_resources\":false,\"resource_sha256\":"+(hash==null?"null":"\""+hash+"\"")+",\"runtime_contract\":\"stem-processing-v1\"}";
    }
    private static class Model implements NativeBridge.Converter {
        final AtomicInteger retired=new AtomicInteger();
        volatile NativeBridge.EngineControl control;
        volatile String output="{\"text\":\"\\t001\\r\\n1+1=3\",\"fidelity\":{\"complete\":true,\"lexical_change\":false,\"layout_change\":false,\"unsupported\":[],\"semantic_equivalence_asserted\":false}}";
        volatile String declared="[\"PlainText\"]";
        volatile CountDownLatch entered,release;
        public void probe(NativeBridge.EngineControl c) {c.check();control=c;}
        public String formats(NativeBridge.EngineControl c) {c.check();return declared;}
        public String convert(String content,String target,NativeBridge.EngineControl c) throws Exception {
            c.check();control=c;check(target.equals("PlainText") && content.contains("\\t001\\r\\n1+1=3"));
            if(entered!=null){entered.countDown();check(release.await(10,TimeUnit.SECONDS));}
            return output; // deliberately leave cancellation rejection to core too
        }
        public void retire() {check(retired.incrementAndGet()==1);}
    }
    private static String block(long host) {
        String note=id(request(host,"{\"op\":\"Create\",\"kind\":\"Note\",\"parent\":null,\"title\":\"converter source\"}"));
        return id(request(host,"{\"op\":\"Append\",\"entity\":\""+note+"\",\"content\":{\"type\":\"Code\",\"value\":{\"text\":\"\\t001\\r\\n1+1=3\",\"language\":null}}}"));
    }
    public static void main(String[] args) throws Exception {run();}
    public static void run() throws Exception {
        Path root=Files.createTempDirectory("stem-jni-converter");long host=NativeBridge.open(root.resolve("library.db").toString());
        ExecutorService workers=Executors.newCachedThreadPool();Model model=new Model();
        try {
            String block=block(host);
            String version=id(NativeBridge.stageResourceFreeEngine(host,descriptor(null),true));
            try(var job=NativeBridge.engineActivation(host,version,0,30000)) {job.activateConverter((d,r,c)->{check(r==null);c.check();return model;});}
            check(!model.control.active());
            var once=NativeBridge.engineConversion(host,block,null,"synthetic-converter","PlainText",30000);
            String result=once.convert();check(result.contains("\"block\":\""+block+"\"") && result.contains("\"revision\":0") && result.contains("\\t001\\r\\n1+1=3") && result.contains("\"semantic_equivalence_asserted\":false"));
            rejected(once::convert);
            try(var job=NativeBridge.engineConversion(host,block,null,"synthetic-converter","Latex",30000)){rejected(job::convert);}
            model.output="private malformed converter diagnostic";
            try(var job=NativeBridge.engineConversion(host,block,null,"synthetic-converter","PlainText",30000)){rejected(job::convert);}
            model.output="x".repeat(8*1024*1024+1);
            try(var job=NativeBridge.engineConversion(host,block,null,"synthetic-converter","PlainText",30000)){rejected(job::convert);}
            System.out.println("PASS: typed converter activation, source-bound immutable fidelity output, single use, unsupported and malformed/oversized containment (1)");

            model.entered=new CountDownLatch(1);model.release=new CountDownLatch(1);
            var job=NativeBridge.engineConversion(host,block,null,"synthetic-converter","PlainText",30000);
            Future<?> running=workers.submit(()->rejected(job::convert));check(model.entered.await(10,TimeUnit.SECONDS));
            check(request(host,"{\"op\":\"Status\"}").contains("\"ok\":true"));
            String pen=UUID.randomUUID().toString();NativeBridge.setCapturePriority(host,pen,true);check(!model.control.active());
            NativeBridge.setCapturePriority(host,pen,false);model.release.countDown();running.get(10,TimeUnit.SECONDS);
            var cancelled=NativeBridge.engineConversion(host,block,null,"synthetic-converter","PlainText",30000);cancelled.close();rejected(cancelled::convert);
            check(request(host,"{\"op\":\"Block\",\"block\":\""+block+"\"}").contains("\\t001\\r\\n1+1=3"));
            System.out.println("PASS: converter callback off writer, capture cancellation, closed job refusal and preserved source (1)");
        } finally {NativeBridge.close(host);workers.shutdownNow();}
        check(model.retired.get()==1);

        host=NativeBridge.open(root.resolve("resource-library.db").toString());final long ownedHost=host;
        byte[] bytes=new byte[65543];for(int i=0;i<bytes.length;i++)bytes[i]=(byte)(i*37);
        String hash=HexFormat.of().formatHex(MessageDigest.getInstance("SHA-256").digest(bytes));
        AtomicReference<byte[]> passed=new AtomicReference<>();Model resourceModel=new Model();
        try {
            String version;
            try(var transfer=NativeBridge.engineImport(host,descriptor(hash),bytes.length,true,root.resolve("models").toString(),null,30000)) {
                version=transfer.version;
                transfer.copy(c->new NativeBridge.EngineInput(){int offset;public int read(byte[] b,NativeBridge.EngineControl control){control.check();if(offset==bytes.length)return -1;int n=Math.min(b.length,bytes.length-offset);System.arraycopy(bytes,offset,b,0,n);offset+=n;return n;}public void retire(){}});
            }
            try(var activation=NativeBridge.engineActivation(host,version,bytes.length,30000)) {activation.activateConverter((d,r,c)->{check(Arrays.equals(bytes,r));passed.set(r);return resourceModel;});}
            for(byte b:passed.get())check(b==0);
            Model bad=new Model();bad.declared="[\"PlainText\",\"PlainText\"]";
            String invalid=id(NativeBridge.stageResourceFreeEngine(host,descriptor(null),true));
            try(var activation=NativeBridge.engineActivation(host,invalid,0,30000)){rejected(()->activation.activateConverter((d,r,c)->bad));}
            check(bad.retired.get()==1);
            check(NativeBridge.engineCatalogue(ownedHost,null,128).contains("\"runtime_ready\":true"));
            System.out.println("PASS: converter verified-resource array wiping, malformed format declaration retirement and previous runtime preservation (1)");
        } finally {NativeBridge.close(host);}
        check(resourceModel.retired.get()==1);
    }
}
