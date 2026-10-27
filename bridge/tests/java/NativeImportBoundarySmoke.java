import org.stemnotebook.backend.NativeBridge;
import java.nio.file.*;
import java.nio.charset.StandardCharsets;
import java.util.*;
import java.util.regex.Pattern;

/** Actual local JVM import and durable reconciliation, not provider/device acceptance. */
public final class NativeImportBoundarySmoke {
    static void check(boolean value){if(!value)throw new AssertionError("native import boundary contract failed");}
    static void rejected(Runnable action){try{action.run();throw new AssertionError("invalid import accepted");}catch(IllegalStateException expected){check(!expected.getMessage().contains("private diagnostic"));}}
    static String call(long host,String command){return NativeExportBoundarySmoke.call(host,command);}
    static String id(String json){return NativeExportBoundarySmoke.id(json);}
    static String entity(String receipt){var m=Pattern.compile("\"entities\":\\[\"([^\"]+)\"").matcher(receipt);check(m.find());return m.group(1);}
    public static void run() throws Exception {
        Path path=Files.createTempDirectory("stem-native-import").resolve("library.db");
        long source=NativeBridge.open(Files.createTempDirectory("stem-import-source").resolve("library.db").toString());
        long host=NativeBridge.open(path.toString());
        byte[] bytes=null;
        try {
            String note=id(call(source,"{\"op\":\"Create\",\"kind\":\"Note\",\"parent\":null,\"title\":\"import λ\"}"));
            call(source,"{\"op\":\"Append\",\"entity\":\""+note+"\",\"content\":{\"type\":\"Text\",\"value\":\"001 + 1 = 3\"}}");
            try(var export=NativeBridge.reviewNativeExport(source,NativeExportBoundarySmoke.select(note),true);var data=export.finish(null,c->true,true,true,30000)){bytes=data.bytes.clone();}
            final byte[] plain=bytes;
            String operation,receipt;
            try(var job=NativeBridge.beginNativeImport(host)) {
                operation=job.operation;check(UUID.fromString(operation).toString().equals(operation));
                check(NativeBridge.nativeImportReceipt(host,operation).equals("null"));
                check(job.prepare(bytes,null,false,30000).contains("\"entities\":1"));
                final long current=host;
                rejected(()->NativeBridge.beginNativeImport(current));
                receipt=job.publish(c->true,true,true);
                check(!entity(receipt).equals(note));
                rejected(()->job.publish(c->true,true,true));
                check(NativeBridge.nativeImportReceipt(host,operation).equals(receipt));
                check(call(host,"{\"op\":\"Blocks\",\"entity\":\""+entity(receipt)+"\",\"offset\":0,\"limit\":10}").contains("001 + 1 = 3"));
            }
            NativeBridge.close(host);host=NativeBridge.openExisting(path.toString());
            check(NativeBridge.nativeImportReceipt(host,operation).equals(receipt));
            System.out.println("PASS: JNI native import independent copy, exact authored text, operation identity before commit and durable lost-response reconciliation after reopen (1)");
            try(var job=NativeBridge.beginNativeImport(host)){job.prepare(bytes,null,false,30000);rejected(()->job.publish(c->true,false,true));check(NativeBridge.nativeImportReceipt(host,job.operation).equals("null"));}
            try(var job=NativeBridge.beginNativeImport(host)){rejected(()->job.prepare(new byte[]{1,2,3},null,false,30000));}
            try(var job=NativeBridge.beginNativeImport(host)){rejected(()->job.prepare(plain,new char[]{(char)0xd800},false,30000));}
            try(var job=NativeBridge.beginNativeImport(host)){
                job.prepare(bytes,null,false,30000);
                call(host,"{\"op\":\"Create\",\"kind\":\"Note\",\"parent\":null,\"title\":\"generation change\"}");
                rejected(()->job.publish(c->true,true,true));check(NativeBridge.nativeImportReceipt(host,job.operation).equals("null"));
            }
            try(var job=NativeBridge.beginNativeImport(host)){job.prepare(bytes,null,false,30000);job.close();rejected(()->job.publish(c->true,true,true));}
            System.out.println("PASS: JNI native import confirmation, malformed input/UTF16, single use, shared admission, cancellation and stale-generation rejection without receipts (1)");
            NativeBridge.authenticate(source,"[{\"Entity\":\""+note+"\"}]",c->true);
            call(source,"{\"op\":\"SetLock\",\"entity\":\""+note+"\",\"revision\":"+NativeExportBoundarySmoke.revision(source,note)+",\"locked\":true}");
            char[] password="native import explicit encrypted credential passphrase".toCharArray();
            try(var export=NativeBridge.reviewNativeExport(source,NativeExportBoundarySmoke.select(note),true);var data=export.finish(password,c->true,false,true,120000)){
                try(var job=NativeBridge.beginNativeImport(host)){rejected(()->job.prepare(data.bytes,"wrong".toCharArray(),false,120000));}
                try(var job=NativeBridge.beginNativeImport(host)){
                    check(job.prepare(data.bytes,data.recoveryKey,true,120000).contains("\"protected_objects\":1"));
                    String copy=entity(job.publish(c->true,true,true));
                    check(NativeBridge.request(host,"{\"version\":1,\"command\":{\"op\":\"Read\",\"id\":\""+copy+"\"}}").contains("\"ok\":false"));
                }
            } finally{Arrays.fill(password,'\0');}
            System.out.println("PASS: JNI native import explicit encrypted credential/recovery, wrong-secret refusal and retained locks without automatic grants (1)");
            NativeBridge.authenticate(host,"[\"App\"]",c->true);
            call(host,"{\"op\":\"SetAppLock\",\"enabled\":true}");
            try(var job=NativeBridge.beginNativeImport(host)){job.prepare(bytes,null,false,30000);rejected(()->job.publish(c->false,true,true));}
            final long current=host;
            try(var job=NativeBridge.beginNativeImport(host)){job.prepare(bytes,null,false,30000);check(job.publish(c->{check(NativeBridge.captureLibraryIdentity(current).length()==36);return true;},true,true).contains(job.operation));}
            try(var job=NativeBridge.beginNativeImport(host)){job.prepare(bytes,null,false,30000);rejected(()->job.publish(c->{job.close();return true;},true,true));check(NativeBridge.nativeImportReceipt(host,job.operation).equals("null"));}
            System.out.println("PASS: JNI native import fresh app proof off writer and reentrant callback cancellation before publication (1)");
        } finally {if(bytes!=null)Arrays.fill(bytes,(byte)0);NativeBridge.close(host);NativeBridge.close(source);}
    }
}
